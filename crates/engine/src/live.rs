// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live input ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)): notes played on a
//! MIDI controller, heard through the instrument of every armed track, whether the transport
//! runs or not.
//!
//! The MIDI driver's thread sends notes over a lock-free queue, each with where it came from (a
//! port and a channel, [`LiveSource`]); the render thread takes them at the start of each block
//! and plays them at its first frame on the chains of the armed tracks whose input
//! ([`LiveFilter`]) takes that source, the way the preview voice plays its notes (ADR 0011).
//! Which tracks are armed, and what each hears, comes from the controller
//! ([`crate::LaunchCommand::SetArmed`]) and outlives graph swaps. The keys held and the sustain
//! pedal are kept per source, so one controller's pedal does not hold another's notes. Nothing
//! here allocates, locks or frees on the render thread.

use rtrb::{Consumer, Producer, RingBuffer};
use suhina_model::TrackId;

use crate::chain::Chain;

/// Notes waiting for the render thread: more than a hand plays within one block of any size.
pub const LIVE_QUEUE: usize = 1024;
/// Tracks that can be armed at once; arming more leaves the rest unarmed.
pub const MAX_ARMED: usize = 64;
/// Sources (a port and a channel) whose keys and pedal are followed at once; past that, the one
/// with nothing sounding is reused.
pub const MAX_SOURCES: usize = 16;
/// A live note's end: it sounds until its key is released.
const UNTIL_RELEASED: u64 = u64::MAX - 1;

/// What a MIDI controller played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LiveEvent {
    /// A key went down; `velocity` is from 0 to 1 (a note on of velocity 0 is a [`LiveEvent::NoteOff`]).
    NoteOn {
        key: u8,
        velocity: f32,
    },
    NoteOff {
        key: u8,
    },
    /// The sustain pedal went down or up: while it is down, released keys keep sounding until
    /// it comes up.
    Sustain {
        on: bool,
    },
    /// Every key the controller holds goes up (a closed port, an all-notes-off message).
    AllNotesOff,
}

/// Where a live note came from: a port, by [`port_id`] of its name (0 when unknown), and a
/// MIDI channel, 0 to 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveSource {
    pub port: u64,
    pub channel: u8,
}

/// The id of the MIDI port called `name` in a [`LiveSource`] and a [`LiveFilter`]: the same
/// name gives the same id in the MIDI layer and in the session, with no table to share. Never 0.
pub fn port_id(name: &str) -> u64 {
    // FNV-1a, 64 bits.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    hash.max(1)
}

/// Which ports a track hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LivePorts {
    /// Every port.
    #[default]
    All,
    /// The port with this [`port_id`].
    One(u64),
    /// None: the track plays nothing from a controller.
    Nothing,
}

/// What an armed track hears: its ports and its channel (0 to 15, or every one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveFilter {
    pub ports: LivePorts,
    pub channel: Option<u8>,
}

impl LiveFilter {
    /// Whether a note from `source` plays on a track with this input.
    pub fn takes(&self, source: LiveSource) -> bool {
        let port = match self.ports {
            LivePorts::All => true,
            LivePorts::One(id) => id == source.port,
            LivePorts::Nothing => false,
        };
        port && self.channel.is_none_or(|c| c == source.channel)
    }
}

/// The sending end of the live-note queue, for the thread a MIDI driver calls back on.
pub struct LiveSender {
    producer: Producer<(LiveSource, LiveEvent)>,
}

impl LiveSender {
    /// Queues `event`, from an unknown port on channel 1, for the start of the next block: only
    /// tracks that hear every port play it. Returns false when it was dropped (see
    /// [`LiveSender::send_from`]).
    pub fn send(&mut self, event: LiveEvent) -> bool {
        self.send_from(LiveSource::default(), event)
    }

    /// Queues `event` from `source` for the start of the next block. Returns false when it was
    /// dropped: the queue is full, or the renderer it fed is gone.
    pub fn send_from(&mut self, source: LiveSource, event: LiveEvent) -> bool {
        !self.producer.is_abandoned() && self.producer.push((source, event)).is_ok()
    }

    /// The renderer still takes notes from this sender.
    pub fn is_connected(&self) -> bool {
        !self.producer.is_abandoned()
    }
}

impl std::fmt::Debug for LiveSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSender").field("connected", &self.is_connected()).finish()
    }
}

/// One source's keys and pedal.
#[derive(Clone, Copy)]
struct Keys {
    source: LiveSource,
    /// The keys held down now.
    held: [bool; 128],
    /// The sustain pedal is down.
    sustain: bool,
    /// Keys released while the pedal is down, sounding until it comes up.
    sustained: [bool; 128],
}

impl Keys {
    const EMPTY: Self = Self {
        source: LiveSource { port: 0, channel: 0 },
        held: [false; 128],
        sustain: false,
        sustained: [false; 128],
    };

    /// `key` sounds because of this source: held down, or sustained by the pedal.
    fn sounding(&self, key: u8) -> bool {
        let at = usize::from(key);
        self.held.get(at).copied().unwrap_or(false) || self.sustained.get(at).copied().unwrap_or(false)
    }

    fn silent(&self) -> bool {
        !self.held.contains(&true) && !self.sustained.contains(&true) && !self.sustain
    }
}

/// The render thread's end: the queue, the armed tracks with their inputs, and each source's
/// keys.
pub(crate) struct Live {
    consumer: Option<Consumer<(LiveSource, LiveEvent)>>,
    armed: [(TrackId, LiveFilter); MAX_ARMED],
    armed_count: usize,
    keys: [Keys; MAX_SOURCES],
    keys_count: usize,
}

impl Live {
    pub(crate) fn new() -> Self {
        Self {
            consumer: None,
            armed: [(TrackId(0), LiveFilter::default()); MAX_ARMED],
            armed_count: 0,
            keys: [Keys::EMPTY; MAX_SOURCES],
            keys_count: 0,
        }
    }

    /// A new queue to the renderer; the previous sender is cut off.
    pub(crate) fn sender(&mut self) -> LiveSender {
        let (producer, consumer) = RingBuffer::new(LIVE_QUEUE);
        self.consumer = Some(consumer);
        LiveSender { producer }
    }

    fn armed(&self) -> &[(TrackId, LiveFilter)] {
        self.armed.get(..self.armed_count).unwrap_or_default()
    }

    /// Whether `track` is armed.
    pub(crate) fn is_armed(&self, track: TrackId) -> bool {
        self.armed().iter().any(|(t, _)| *t == track)
    }

    /// Whether any track is armed: the output then keeps a shallower queue (ADR 0017).
    pub(crate) fn any_armed(&self) -> bool {
        self.armed_count > 0
    }

    /// Arms `track` to hear what `input` takes, or disarms it. Arming an armed track changes
    /// what it hears. A track that is disarmed, or stops hearing a source, lets go of that
    /// source's keys at `frame` of the block; a newly armed one hears the next key pressed, not
    /// the ones already down.
    pub(crate) fn set_armed(
        &mut self,
        chains: &mut [Chain],
        track: TrackId,
        armed: bool,
        input: LiveFilter,
        frame: usize,
    ) {
        let index = self.armed().iter().position(|(t, _)| *t == track);
        let before = index.and_then(|i| self.armed.get(i)).map(|(_, f)| *f);
        match (armed, index) {
            (true, None) => {
                if let Some(slot) = self.armed.get_mut(self.armed_count) {
                    *slot = (track, input);
                    self.armed_count += 1;
                }
            }
            (true, Some(index)) => {
                if let Some(slot) = self.armed.get_mut(index) {
                    slot.1 = input;
                }
            }
            (false, Some(index)) => {
                let last = self.armed_count - 1;
                self.armed.swap(index, last);
                self.armed_count = last;
            }
            (false, None) => {}
        }
        // Let go of what the track heard and no longer does.
        let Some(before) = before else { return };
        let keys = self.keys.get(..self.keys_count).unwrap_or_default();
        for chain in chains.iter_mut().filter(|c| c.instrument && c.track == track) {
            for source in keys.iter().filter(|k| before.takes(k.source) && !(armed && input.takes(k.source))) {
                for key in 0..=127u8 {
                    if source.sounding(key) {
                        chain.release_key(frame, key);
                    }
                }
            }
        }
    }

    /// The keys of `source`, a slot found or taken for it.
    fn keys_of(&mut self, source: LiveSource) -> Option<usize> {
        let count = self.keys_count;
        if let Some(found) = self.keys.get(..count).unwrap_or_default().iter().position(|k| k.source == source) {
            return Some(found);
        }
        let slot = if count < MAX_SOURCES {
            self.keys_count += 1;
            count
        } else {
            // Every slot is taken: reuse one with nothing sounding, else the last.
            self.keys.iter().position(Keys::silent).unwrap_or(MAX_SOURCES - 1)
        };
        let keys = self.keys.get_mut(slot)?;
        *keys = Keys { source, ..Keys::EMPTY };
        Some(slot)
    }

    /// Plays what the controllers sent since the last block, at the block's first frame, on the
    /// instrument chains of the armed tracks that hear each note's source. Call it once the
    /// chains have begun the block.
    pub(crate) fn receive(&mut self, chains: &mut [Chain], position: u64) {
        // Bounded: what arrives while this runs waits for the next block.
        for _ in 0..LIVE_QUEUE {
            let Some(Ok((source, event))) = self.consumer.as_mut().map(Consumer::pop) else { break };
            let Some(slot) = self.keys_of(source) else { continue };
            let Self { armed, armed_count, keys, .. } = self;
            let armed = armed.get(..*armed_count).unwrap_or_default();
            let Some(keys) = keys.get_mut(slot) else { continue };
            let targets = chains
                .iter_mut()
                .filter(|c| c.instrument && armed.iter().any(|(t, f)| *t == c.track && f.takes(source)));
            match event {
                LiveEvent::NoteOn { key, velocity } => {
                    let Some(held) = keys.held.get_mut(usize::from(key)) else { continue };
                    *held = true;
                    if let Some(sustained) = keys.sustained.get_mut(usize::from(key)) {
                        *sustained = false;
                    }
                    let velocity = if velocity.is_finite() { velocity } else { 1.0 };
                    for chain in targets {
                        chain.note_on(0, position, key, velocity, UNTIL_RELEASED);
                    }
                }
                LiveEvent::NoteOff { key } => {
                    let Some(held) = keys.held.get_mut(usize::from(key)) else { continue };
                    if !*held {
                        continue;
                    }
                    *held = false;
                    if keys.sustain {
                        if let Some(sustained) = keys.sustained.get_mut(usize::from(key)) {
                            *sustained = true;
                        }
                        continue;
                    }
                    for chain in targets {
                        chain.release_key(0, key);
                    }
                }
                LiveEvent::Sustain { on } => {
                    keys.sustain = on;
                    if on {
                        continue;
                    }
                    for chain in targets {
                        for key in 0..=127u8 {
                            if keys.sustained.get(usize::from(key)).copied().unwrap_or(false) {
                                chain.release_key(0, key);
                            }
                        }
                    }
                    keys.sustained = [false; 128];
                }
                LiveEvent::AllNotesOff => {
                    for chain in targets {
                        for key in 0..=127u8 {
                            if keys.sounding(key) {
                                chain.release_key(0, key);
                            }
                        }
                    }
                    *keys = Keys { source, ..Keys::EMPTY };
                }
            }
        }
    }
}
