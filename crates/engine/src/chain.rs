//! Device chains on tracks (ADR 0004 › the engine side, LOOP-35).
//!
//! A track whose channel holds built-in devices gets a [`Chain`]: everything the track plays in
//! a block (its sine voices, audio clips, launched clips) is summed into the chain's buffer
//! instead of the output, the devices run over it in order, and only then do the track's pan
//! and volume apply. When the first device is an instrument, the track's notes go to it as
//! [`NoteEvent`]s instead of starting sine voices; the chain runs in pieces split at each event,
//! so a note lands on its frame. A track without devices is mixed straight into the output, as
//! before.
//!
//! A plug-in device takes a [`PluginSlot`] in the chain (ADR 0006): it gets the block's events
//! with their frames and runs once per block, so the devices run one after another over the
//! whole block, each split at the events, rather than piece by piece. The output is the same
//! for built-in devices, which only ever see their own input.

use std::time::Instant;

use loopwerk_device_api::{Device, NoteEvent, ProcessContext, Role};
use loopwerk_model::{DeviceKind, Project, TrackId};

use crate::plugin::{Delay, PluginProcessor, PluginSlot, Plugins, plugin_devices};

/// The most frames one block of a chain holds; the renderer splits longer blocks.
pub(crate) const MAX_BLOCK: usize = 1024;
/// Events a chain keeps for one block before it runs up to the frame it has reached.
const MAX_PENDING: usize = 256;
const NOT_HELD: u64 = u64::MAX;
/// Gain of a centred track at unity volume, which the sources of a chain track are mixed at
/// (see `track_gains`); the track's real gains apply after the chain, divided by this.
pub(crate) const CHAIN_INPUT_GAIN: f32 = 0.25;

/// One device of a chain: built in, or a plug-in in a host process.
pub(crate) enum Slot {
    Device(Box<dyn Device>),
    Plugin(PluginSlot),
}

pub(crate) struct Chain {
    pub(crate) track: TrackId,
    /// The launcher column of the track, or `NO_COLUMN`.
    pub(crate) column: u32,
    slots: Vec<Slot>,
    /// Each device's index among the channel's devices in the project (disabled built-in,
    /// unknown and missing devices have no place in the chain).
    positions: Vec<usize>,
    /// The project's device ids in order (a plug-in's by its key), so a graph swap can tell
    /// whether a new chain holds the same devices as an old one.
    ids: Vec<String>,
    /// Every parameter's value from the project (its default when the project has none), per
    /// device.
    params: Vec<Vec<f64>>,
    pub(crate) instrument: bool,
    /// The delay its devices add together, in frames.
    pub(crate) latency: u32,
    /// Latency compensation: delays the chain's output so that it lines up with the chain with
    /// the most latency (set by the graph once every chain is built).
    pub(crate) delay: Delay,
    /// The track's gains after the chain.
    post: (f32, f32),
    buffer: Vec<f32>,
    pending: Vec<(usize, NoteEvent)>,
    /// The pending events with their frames counted from the start of the span a plug-in runs.
    span_events: Vec<(usize, NoteEvent)>,
    /// The current block's: when plug-ins must have answered (`None` offline), and the
    /// transport.
    deadline: Option<Instant>,
    context: ProcessContext,
    /// Frames of the current block the devices have run over.
    done: usize,
    /// Per key, the renderer frame its note ends at, or `NOT_HELD`.
    held: [u64; 128],
    next_off: u64,
}

impl Chain {
    /// The chain for `track` at `sample_rate`, or `None` when its channel holds no device
    /// Loopwerk can run (other DAWs' devices and plug-ins that are not installed pass the sound
    /// through). Plug-in devices take their processors from `plugins` (one without a processor
    /// yet gets a slot that waits for it). `post` is the track's gains. Allocates: call it off
    /// the audio thread.
    pub(crate) fn build(
        project: &Project,
        track: TrackId,
        column: u32,
        post: (f32, f32),
        sample_rate: u32,
        plugins: &mut Plugins,
    ) -> Option<Self> {
        let channel = project.tracks.iter().find(|t| t.id == track)?.channel?;
        let channel = project.channels.iter().find(|c| c.id == channel)?;
        let keys = plugin_devices(project);
        let mut slots: Vec<Slot> = Vec::new();
        let mut positions = Vec::new();
        let mut ids = Vec::new();
        let mut params = Vec::new();
        let mut latency = 0u32;
        for (position, stored) in channel.devices.iter().enumerate() {
            let id = match &stored.kind {
                DeviceKind::Builtin(id) if stored.enabled => id,
                DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => {
                    let Some(key) =
                        keys.iter().find(|d| d.channel == channel.id && d.position == position).map(|d| d.key)
                    else {
                        continue;
                    };
                    let Some(&info) = plugins.info.get(&key) else { continue };
                    if stored.enabled {
                        latency = latency.saturating_add(info.latency);
                    }
                    slots.push(Slot::Plugin(PluginSlot {
                        key,
                        processor: plugins.processors.remove(&key),
                        instrument: info.instrument,
                        latency: info.latency,
                        enabled: stored.enabled,
                        missed: 0,
                    }));
                    positions.push(position);
                    ids.push(format!("plugin:{key:016x}"));
                    params.push(Vec::new());
                    continue;
                }
                _ => continue,
            };
            let Some(mut device) = loopwerk_device_registry::create(id) else { continue };
            if device.prepare(f64::from(sample_rate)).is_err() {
                continue;
            }
            let info = device.info();
            let mut values: Vec<f64> = info.params.iter().map(|p| p.default).collect();
            for p in &stored.params {
                if let Some(slot) = info.param_index(&p.id).and_then(|i| values.get_mut(i)) {
                    *slot = p.value;
                }
            }
            for (index, &value) in values.iter().enumerate() {
                device.set_param(index, value);
            }
            device.reset();
            latency = latency.saturating_add(device.latency());
            slots.push(Slot::Device(device));
            positions.push(position);
            ids.push(id.clone());
            params.push(values);
        }
        let instrument = match slots.first() {
            Some(Slot::Device(device)) => device.info().role == Role::Instrument,
            Some(Slot::Plugin(slot)) => slot.instrument,
            None => false,
        };
        (!slots.is_empty()).then(|| Self {
            track,
            column,
            slots,
            positions,
            ids,
            params,
            instrument,
            latency,
            delay: Delay::new(0),
            post: (post.0 / CHAIN_INPUT_GAIN, post.1 / CHAIN_INPUT_GAIN),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::with_capacity(MAX_PENDING),
            span_events: Vec::with_capacity(MAX_PENDING),
            deadline: None,
            context: ProcessContext::default(),
            done: 0,
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        })
    }

    /// Clears the buffer for a block of `frames` frames, whose plug-ins must answer by
    /// `deadline` (`None` offline). Events queued since the last block stay; they are at frame
    /// 0.
    pub(crate) fn begin(&mut self, frames: usize, deadline: Option<Instant>) {
        if let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) {
            buffer.fill(0.0);
        }
        self.done = 0;
        self.deadline = deadline;
    }

    /// Adds a frame of the track's sound at `frame` of the block.
    pub(crate) fn add(&mut self, frame: usize, left: f32, right: f32) {
        if let Some([l, r]) =
            self.buffer.get_mut(frame * 2..frame * 2 + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
        {
            *l += left;
            *r += right;
        }
    }

    /// Plays `key` from `frame` of the block (renderer frame `position`) until renderer frame
    /// `end`. A key that is already held is released first.
    pub(crate) fn note_on(&mut self, frame: usize, position: u64, key: u8, velocity: f32, end: u64) {
        let Some(held) = self.held.get_mut(usize::from(key)) else { return };
        if *held != NOT_HELD {
            *held = NOT_HELD;
            self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
        }
        // A note of velocity 0 would be a release.
        let velocity = velocity.clamp(1.0 / 127.0, 1.0);
        self.push(frame, NoteEvent::On { key, velocity });
        if let Some(held) = self.held.get_mut(usize::from(key)) {
            *held = end.max(position + 1);
        }
        self.next_off = self.next_off.min(end.max(position + 1));
    }

    /// Releases the notes that end by renderer frame `position`, at `frame` of the block.
    pub(crate) fn release_due(&mut self, frame: usize, position: u64) {
        if position < self.next_off {
            return;
        }
        let mut next = NOT_HELD;
        for key in 0..=127u8 {
            let end = self.held.get(usize::from(key)).copied().unwrap_or(NOT_HELD);
            if end == NOT_HELD {
                continue;
            }
            if end <= position {
                if let Some(held) = self.held.get_mut(usize::from(key)) {
                    *held = NOT_HELD;
                }
                self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
            } else {
                next = next.min(end);
            }
        }
        self.next_off = next;
    }

    /// Releases `key` now, at `frame` of the block, if it is held.
    pub(crate) fn release_key(&mut self, frame: usize, key: u8) {
        if let Some(held) = self.held.get_mut(usize::from(key))
            && *held != NOT_HELD
        {
            *held = NOT_HELD;
            self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
        }
    }

    /// Releases every note now, at `frame` of the block.
    /// Each held key is released on its own, then everything: an instrument that ignores an
    /// all-notes-off (many plug-ins do) still lets go of its notes.
    pub(crate) fn release_all(&mut self, frame: usize) {
        if !self.instrument {
            return;
        }
        for key in 0..=127u8 {
            if self.held.get(usize::from(key)).is_some_and(|&end| end != NOT_HELD) {
                self.push(frame, NoteEvent::Off { key, velocity: 0.5 });
            }
        }
        self.held = [NOT_HELD; 128];
        self.next_off = NOT_HELD;
        self.push(frame, NoteEvent::AllNotesOff);
    }

    fn push(&mut self, frame: usize, event: NoteEvent) {
        if self.pending.len() >= MAX_PENDING {
            // Too many events in one block: run the devices up to here first.
            self.run(frame);
        }
        if self.pending.len() < MAX_PENDING {
            self.pending.push((frame.max(self.done), event));
        }
    }

    /// Runs the devices over the block up to `frame`, one after another, handing each queued
    /// event over at its frame.
    fn run(&mut self, frame: usize) {
        let to = frame.min(MAX_BLOCK);
        let Self { slots, buffer, pending, span_events, done, deadline, context, .. } = self;
        let from = (*done).min(to);
        for slot in slots.iter_mut() {
            match slot {
                Slot::Device(device) => {
                    let mut at_done = from;
                    for &(at, event) in pending.iter() {
                        let at = at.clamp(at_done, to);
                        process(device, buffer, at_done, at, context);
                        at_done = at;
                        device.event(event);
                    }
                    process(device, buffer, at_done, to, context);
                }
                Slot::Plugin(plugin) => {
                    let last = (to - from).saturating_sub(1);
                    span_events.clear();
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    if let Some(span) = buffer.get_mut(from * 2..to * 2) {
                        plugin.run(span, span_events, *deadline);
                    }
                }
            }
        }
        pending.clear();
        *done = to;
    }

    /// Runs the devices over the rest of a block of `frames` frames and adds the result, at
    /// the track's gains, to `out`.
    pub(crate) fn finish(&mut self, out: &mut [f32], frames: usize, context: &ProcessContext) {
        self.context = *context;
        self.run(frames);
        let (gain_left, gain_right) = self.post;
        let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) else { return };
        self.delay.run(buffer);
        for ([l, r], [cl, cr]) in out.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0) {
            *l += cl * gain_left;
            *r += cr * gain_right;
        }
    }

    /// Sets parameter `param` of the device at `position` among the channel's devices, as a
    /// parameter message does (ADR 0005, amendment). Never allocates.
    pub(crate) fn set_param(&mut self, position: usize, param: usize, value: f64) {
        let Some(index) = self.positions.iter().position(|&p| p == position) else { return };
        if let Some(Slot::Device(device)) = self.slots.get_mut(index) {
            device.set_param(param, value);
            if let Some(slot) = self.params.get_mut(index).and_then(|values| values.get_mut(param)) {
                *slot = device.param(param).unwrap_or(value);
            }
        }
    }

    /// Takes the running devices of `old` if it holds the same devices for the same track,
    /// with this chain's parameter values and plug-in settings and `old`'s held notes. Never
    /// allocates.
    pub(crate) fn adopt(&mut self, old: &mut Chain) -> bool {
        if old.track != self.track || old.ids != self.ids {
            return false;
        }
        std::mem::swap(&mut self.slots, &mut old.slots);
        for ((slot, values), fresh) in self.slots.iter_mut().zip(&self.params).zip(old.slots.iter_mut()) {
            match (slot, fresh) {
                (Slot::Device(device), _) => {
                    for (index, &value) in values.iter().enumerate() {
                        device.set_param(index, value);
                    }
                }
                (Slot::Plugin(running), Slot::Plugin(fresh)) => {
                    running.enabled = fresh.enabled;
                    running.instrument = fresh.instrument;
                    running.latency = fresh.latency;
                    // A processor that reached the new graph's slot (a restart) wins.
                    if fresh.processor.is_some() {
                        std::mem::swap(&mut running.processor, &mut fresh.processor);
                    }
                }
                (Slot::Plugin(_), Slot::Device(_)) => {}
            }
        }
        self.held = old.held;
        self.next_off = old.next_off;
        self.delay.adopt(&mut old.delay);
        true
    }

    /// The plug-in slots, for processors to find theirs.
    pub(crate) fn plugin_slots(&mut self) -> impl Iterator<Item = &mut PluginSlot> {
        self.slots.iter_mut().filter_map(|slot| match slot {
            Slot::Plugin(plugin) => Some(plugin),
            Slot::Device(_) => None,
        })
    }

    /// Puts `processor` into the slot of plug-in `key`, if this chain has it, and returns the
    /// one it replaces; gives `processor` back when there is no such slot. Never allocates.
    pub(crate) fn place(&mut self, key: u64, processor: Box<dyn PluginProcessor>) -> Placed {
        match self.plugin_slots().find(|slot| slot.key == key) {
            Some(slot) => Placed::Replaced(slot.processor.replace(processor)),
            None => Placed::NoSlot(processor),
        }
    }

    /// Blocks its plug-ins did not answer in time, for tests.
    #[cfg(test)]
    pub(crate) fn missed(&mut self) -> u64 {
        self.plugin_slots().map(|slot| slot.missed).sum()
    }
}

/// What [`Chain::place`] did with a processor.
pub(crate) enum Placed {
    /// It went into its slot; the processor that was there, if one was.
    Replaced(Option<Box<dyn PluginProcessor>>),
    /// The chain has no slot for it; here it is back.
    NoSlot(Box<dyn PluginProcessor>),
}

fn process(device: &mut Box<dyn Device>, buffer: &mut [f32], from: usize, to: usize, context: &ProcessContext) {
    if to <= from {
        return;
    }
    let Some(span) = buffer.get_mut(from * 2..to * 2) else { return };
    device.process(context, span);
}
