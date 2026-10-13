//! Plug-in devices in a track's chain (ADR 0006).
//!
//! The engine never loads a plug-in and does not know how one is hosted: a plug-in device's slot
//! in a [`Chain`](crate::chain::Chain) holds a [`PluginProcessor`], the audio half of a plug-in
//! running in a host process of its own, which the session starts off the audio thread and
//! hands over. A slot runs one round trip per block with the block's notes at their frames, and
//! waits only until the block's deadline: a plug-in that misses it, or whose host has crashed,
//! leaves the chain's sound as it was (an effect) or silent (an instrument) for that block.
//!
//! Plug-in devices are told apart by a [`plugin_key`], so a processor finds its slot again in
//! every graph built after an edit, and one that arrives while no graph has its slot waits for
//! one.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use suhina_device_api::NoteEvent;
use suhina_model::{ChannelId, DeviceKind, Project, TrackId};

/// How long a slot waits for a block when rendering offline, where there is no deadline: long
/// enough for any plug-in that still works.
pub const OFFLINE_TIMEOUT: Duration = Duration::from_secs(10);
/// Share of a block's playing time the plug-ins of a block may take together before the engine
/// stops waiting for them; the rest is for the engine itself and the audio device.
pub(crate) const DEADLINE_SHARE: f64 = 0.6;

/// The audio half of a running plug-in, as the engine sees it. Implemented outside the engine
/// (the session, over `suhina-plugins`); test doubles implement it too.
pub trait PluginProcessor: Send {
    /// Runs `buffer`, interleaved stereo (the chain's sound so far), through the plug-in in
    /// place, with `events` at frames counted from the start of `buffer`, waiting at most
    /// `timeout`. Returns false when the plug-in did not answer in time or is gone; `buffer` is
    /// then left as it was. Never allocates, locks or does I/O beyond the block's round trip.
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], timeout: Duration) -> bool;
}

/// What the engine needs to know about a plug-in device to place it in a chain: whether it
/// plays notes and the delay it adds, in frames (0 until it has loaded and said so).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginSlotInfo {
    pub instrument: bool,
    pub latency: u32,
}

/// A plug-in device in a project: where it is and the key its processor goes by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDevice {
    pub key: u64,
    pub channel: ChannelId,
    /// The device's index among the channel's devices.
    pub position: usize,
    /// The track whose channel it is on, if any.
    pub track: Option<TrackId>,
}

/// The key of the `nth` plug-in device of `kind` (counting from 0) on `channel`: it stays the
/// same while devices are added, removed or moved around it, so a running plug-in keeps its
/// slot through edits. `None` for devices that are not plug-ins.
pub fn plugin_key(channel: ChannelId, kind: &DeviceKind, nth: u32) -> Option<u64> {
    let (format, id): (u8, String) = match kind {
        DeviceKind::Clap { id } => (1, id.clone()),
        DeviceKind::Vst3 { id } => (2, id.clone()),
        DeviceKind::Vst2 { id } => (3, id.to_string()),
        DeviceKind::Builtin(_) | DeviceKind::Foreign { .. } => return None,
    };
    // FNV-1a: stable across runs and builds, unlike the standard library's hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(&channel.0.to_le_bytes());
    eat(&[format]);
    eat(id.as_bytes());
    eat(&[0]);
    eat(&nth.to_le_bytes());
    Some(hash)
}

/// Every plug-in device in `project`, channel by channel, in order.
pub fn plugin_devices(project: &Project) -> Vec<PluginDevice> {
    let mut list = Vec::new();
    for channel in &project.channels {
        let track = project.tracks.iter().find(|t| t.channel == Some(channel.id)).map(|t| t.id);
        let mut seen: Vec<(u64, u32)> = Vec::new();
        for (position, device) in channel.devices.iter().enumerate() {
            let Some(first) = plugin_key(channel.id, &device.kind, 0) else { continue };
            let nth = match seen.iter_mut().find(|(k, _)| *k == first) {
                Some((_, count)) => {
                    *count += 1;
                    *count
                }
                None => {
                    seen.push((first, 0));
                    0
                }
            };
            if let Some(key) = plugin_key(channel.id, &device.kind, nth) {
                list.push(PluginDevice { key, channel: channel.id, position, track });
            }
        }
    }
    list
}

/// The plug-ins a renderer is built with: what is known about each plug-in device (devices
/// without an entry, such as missing plug-ins, have no slot and pass the sound through) and the
/// processors of the ones that run.
#[derive(Default)]
pub struct Plugins {
    pub(crate) info: HashMap<u64, PluginSlotInfo>,
    pub(crate) processors: HashMap<u64, Box<dyn PluginProcessor>>,
}

impl Plugins {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gives the plug-in device `key` a slot, described by `info`; it passes the sound through
    /// (or is silent, for an instrument) until a processor arrives.
    pub fn set_info(&mut self, key: u64, info: PluginSlotInfo) {
        self.info.insert(key, info);
    }

    /// The plug-in device `key`, running in `processor`.
    pub fn insert(&mut self, key: u64, info: PluginSlotInfo, processor: Box<dyn PluginProcessor>) {
        self.info.insert(key, info);
        self.processors.insert(key, processor);
    }

    pub fn info(&self) -> &HashMap<u64, PluginSlotInfo> {
        &self.info
    }
}

/// A plug-in device's place in a chain.
pub(crate) struct PluginSlot {
    pub(crate) key: u64,
    pub(crate) processor: Option<Box<dyn PluginProcessor>>,
    pub(crate) instrument: bool,
    pub(crate) latency: u32,
    pub(crate) enabled: bool,
    /// Blocks it did not answer in time, for tests and diagnostics.
    pub(crate) missed: u64,
}

impl PluginSlot {
    /// Runs `span` through the plug-in with `events` (frames counted from the start of the
    /// span) until `deadline`, or [`OFFLINE_TIMEOUT`] without one. A disabled slot passes the
    /// sound through; one that misses passes it through too, or is silent for an instrument.
    pub(crate) fn run(&mut self, span: &mut [f32], events: &[(usize, NoteEvent)], deadline: Option<Instant>) {
        if !self.enabled || span.is_empty() {
            return;
        }
        let timeout = deadline.map_or(OFFLINE_TIMEOUT, |d| d.saturating_duration_since(Instant::now()));
        let answered = match self.processor.as_mut() {
            Some(processor) => processor.process(span, events, timeout),
            None => false,
        };
        if !answered {
            if self.processor.is_some() {
                self.missed += 1;
            }
            if self.instrument {
                span.fill(0.0);
            }
        }
    }
}

/// A delay line for latency compensation: interleaved stereo, `frames` long, allocated when it
/// is made.
pub(crate) struct Delay {
    buffer: Vec<f32>,
    at: usize,
}

impl Delay {
    pub(crate) fn new(frames: usize) -> Self {
        Self { buffer: vec![0.0; frames * 2], at: 0 }
    }

    /// Delays `samples` (interleaved stereo) in place. Never allocates.
    pub(crate) fn run(&mut self, samples: &mut [f32]) {
        if self.buffer.is_empty() {
            return;
        }
        for frame in samples.as_chunks_mut::<2>().0 {
            if let Some(stored) =
                self.buffer.get_mut(self.at..self.at + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
            {
                std::mem::swap(stored, frame);
            }
            self.at += 2;
            if self.at >= self.buffer.len() {
                self.at = 0;
            }
        }
    }

    /// Takes over what `old` holds when it is as long, so a graph swap that keeps the delay
    /// does not drop the sound in it. Never allocates.
    pub(crate) fn adopt(&mut self, old: &mut Delay) {
        if old.buffer.len() == self.buffer.len() {
            std::mem::swap(&mut self.buffer, &mut old.buffer);
            self.at = old.at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{Device, Extensions};

    fn plug(kind: DeviceKind) -> Device {
        Device {
            name: "P".into(),
            kind,
            enabled: true,
            state: None,
            params: Vec::new(),
            extensions: Extensions::default(),
        }
    }

    #[test]
    fn keys_follow_the_plug_in_not_its_position() {
        let vst3 = DeviceKind::Vst3 { id: "ABC".into() };
        let a = plugin_key(ChannelId(1), &vst3, 0);
        assert!(a.is_some());
        assert_eq!(a, plugin_key(ChannelId(1), &vst3, 0));
        assert_ne!(a, plugin_key(ChannelId(1), &vst3, 1));
        assert_ne!(a, plugin_key(ChannelId(2), &vst3, 0));
        assert_ne!(a, plugin_key(ChannelId(1), &DeviceKind::Clap { id: "ABC".into() }, 0));
        assert_eq!(plugin_key(ChannelId(1), &DeviceKind::Builtin("utility".into()), 0), None);

        let mut project = crate::chain_tests::empty_project();
        project.channels[0].devices = vec![
            plug(vst3.clone()),
            plug(DeviceKind::Builtin("utility".into())),
            plug(vst3.clone()),
            plug(DeviceKind::Vst2 { id: 7 }),
        ];
        let found = plugin_devices(&project);
        assert_eq!(found.iter().map(|d| d.position).collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!(found[0].key, a.unwrap_or_default());
        assert_eq!(Some(found[1].key), plugin_key(ChannelId(1), &vst3, 1));
        // A device added in front of them changes no key.
        project.channels[0].devices.insert(0, plug(DeviceKind::Builtin("utility".into())));
        let after: Vec<u64> = plugin_devices(&project).iter().map(|d| d.key).collect();
        assert_eq!(after, found.iter().map(|d| d.key).collect::<Vec<_>>());
    }

    #[test]
    fn a_delay_line_delays_and_carries_over() {
        let mut delay = Delay::new(2);
        let mut block = [1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        delay.run(&mut block);
        assert_eq!(block, [0.0, 0.0, 0.0, 0.0, 1.0, -1.0]);
        let mut fresh = Delay::new(2);
        fresh.adopt(&mut delay);
        let mut next = [0.0; 4];
        fresh.run(&mut next);
        assert_eq!(next, [2.0, -2.0, 3.0, -3.0]);
        let mut none = Delay::new(0);
        let mut same = [5.0, 6.0];
        none.run(&mut same);
        assert_eq!(same, [5.0, 6.0]);
    }
}
