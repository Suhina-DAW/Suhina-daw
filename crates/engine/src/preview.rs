//! The preview voice ([ADR 0011](../../../docs/decisions/0011-preview-and-added-audio.md)): plays a
//! sample at its own speed, or a MIDI file's notes through a track's instrument or a fallback
//! instrument, into the output after the track chains, whether the transport runs or not.
//!
//! The controller sends what to play over a lock-free queue; whatever the voice lets go of goes
//! back over a second queue to be freed on the controller's thread. Nothing here allocates,
//! locks or frees on the audio thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};
use suhina_device_api::{Device, NoteEvent, ProcessContext};
use suhina_model::TrackId;

use crate::AudioData;
use crate::audio::CLICK_GUARD_SECONDS;
use crate::chain::{Chain, MAX_BLOCK};

/// Commands waiting for the audio thread.
const QUEUE: usize = 16;
/// Events the fallback instrument keeps for one block.
const MAX_EVENTS: usize = 256;
const NOT_HELD: u64 = u64::MAX;

/// One note of a MIDI preview, in frames from the preview's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewNote {
    pub start: u64,
    pub length: u64,
    pub key: u8,
    pub velocity: f32,
}

/// A MIDI preview, prepared on the controller's thread.
pub struct PreviewNotes {
    /// Sorted by start.
    pub notes: Vec<PreviewNote>,
    /// The track whose instrument plays them, when its chain starts with one.
    pub track: Option<TrackId>,
    /// Plays them when the track has no instrument (or there is no track); prepared at the
    /// engine's rate.
    pub fallback: Option<Box<dyn Device>>,
    /// Where the preview ends, in frames (the last note's end plus a short tail).
    pub length: u64,
}

/// What the controller asks the preview voice to do.
pub(crate) enum PreviewCommand {
    Sample {
        data: Arc<AudioData>,
        from: f64,
        gain: f32,
    },
    Notes {
        notes: Box<PreviewNotes>,
        gain: f32,
    },
    /// A new level for what plays now and what is previewed next; glides there over a few
    /// milliseconds so it does not click.
    Gain(f32),
    Stop,
}

/// What the voice lets go of, to be freed on the controller's thread.
pub(crate) enum Retired {
    Sample(Arc<AudioData>),
    Notes(Box<PreviewNotes>),
}

/// Whether a preview plays and where, written by the audio thread.
#[derive(Debug, Default)]
pub struct PreviewMonitor {
    playing: AtomicBool,
    /// Seconds from the start of what plays (f64 bits).
    seconds: AtomicU64,
}

impl PreviewMonitor {
    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Seconds into the sample or MIDI file being previewed.
    pub fn seconds(&self) -> f64 {
        f64::from_bits(self.seconds.load(Ordering::Relaxed))
    }
}

/// The controller's end.
pub(crate) struct PreviewSender {
    commands: Producer<PreviewCommand>,
    retired: Consumer<Retired>,
}

impl PreviewSender {
    pub(crate) fn send(&mut self, command: PreviewCommand) -> Result<(), String> {
        self.collect();
        self.commands.push(command).map_err(|_| "the audio engine has not taken the previous preview yet".to_string())
    }

    pub(crate) fn collect(&mut self) {
        while let Ok(retired) = self.retired.pop() {
            match retired {
                Retired::Sample(data) => drop(data),
                Retired::Notes(notes) => drop(notes),
            }
        }
    }
}

/// A sample being played.
struct SamplePlay {
    data: Arc<AudioData>,
    /// Position in the file, in its frames.
    position: f64,
    /// File frames per engine frame.
    step: f64,
    /// Engine frames played, for the click guard at the start.
    played: u64,
    gain: f32,
    /// The level `gain` glides to.
    target: f32,
}

/// Notes being played.
struct NotesPlay {
    notes: Box<PreviewNotes>,
    /// Frames since the preview started.
    frame: u64,
    next: usize,
    /// Per key, the preview frame its note ends at, or `NOT_HELD`.
    held: [u64; 128],
    gain: f32,
    /// The level `gain` glides to.
    target: f32,
    /// The index of the track's chain in the playing graph, found again each block.
    chain: Option<usize>,
}

/// The renderer's end.
pub(crate) struct Preview {
    commands: Option<Consumer<PreviewCommand>>,
    retired: Option<Producer<Retired>>,
    /// A command taken but not applied because the retired queue had no room for what it
    /// replaces.
    waiting: Option<PreviewCommand>,
    sample: Option<SamplePlay>,
    notes: Option<NotesPlay>,
    /// The fallback instrument's events in the current block, and its output.
    events: Vec<(usize, NoteEvent)>,
    buffer: Vec<f32>,
    sample_rate: u32,
    /// How far a level glides towards its target each frame.
    glide: f32,
    pub(crate) monitor: Arc<PreviewMonitor>,
}

/// About how long a change of level takes.
const GLIDE_SECONDS: f64 = 0.01;

/// `gain` one frame closer to `target`.
fn glided(gain: f32, target: f32, glide: f32) -> f32 {
    if (target - gain).abs() < 1e-6 { target } else { gain + (target - gain) * glide }
}

impl Preview {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            commands: None,
            retired: None,
            waiting: None,
            sample: None,
            notes: None,
            events: Vec::with_capacity(MAX_EVENTS),
            buffer: vec![0.0; MAX_BLOCK * 2],
            sample_rate,
            glide: (1.0 - (-1.0 / (GLIDE_SECONDS * f64::from(sample_rate.max(1)))).exp()) as f32,
            monitor: Arc::new(PreviewMonitor::default()),
        }
    }

    /// Connects a new pair of queues; the controller gets the other ends.
    pub(crate) fn sender(&mut self) -> PreviewSender {
        let (commands, incoming) = RingBuffer::new(QUEUE);
        let (outgoing, retired) = RingBuffer::new(QUEUE * 2);
        self.commands = Some(incoming);
        self.retired = Some(outgoing);
        PreviewSender { commands, retired }
    }

    pub(crate) fn playing(&self) -> bool {
        self.sample.is_some() || self.notes.is_some()
    }

    /// Applies the commands that have arrived. A command waits while the retired queue has no
    /// room for the two things it may replace.
    pub(crate) fn receive(&mut self, chains: &mut [Chain]) {
        loop {
            let room = self.retired.as_ref().is_some_and(|r| r.slots() >= 2);
            if !room {
                return;
            }
            let command = match self.waiting.take() {
                Some(command) => command,
                None => match self.commands.as_mut().and_then(|c| c.pop().ok()) {
                    Some(command) => command,
                    None => return,
                },
            };
            if let PreviewCommand::Gain(gain) = command {
                if let Some(play) = self.sample.as_mut() {
                    play.target = gain;
                }
                if let Some(play) = self.notes.as_mut() {
                    play.target = gain;
                }
                continue;
            }
            self.stop(chains, 0);
            match command {
                PreviewCommand::Sample { data, from, gain } => {
                    let rate = f64::from(data.sample_rate.max(1));
                    let position = (from.max(0.0) * rate).min(data.frames() as f64);
                    let step = rate / f64::from(self.sample_rate.max(1));
                    self.sample = Some(SamplePlay { data, position, step, played: 0, gain, target: gain });
                }
                PreviewCommand::Notes { notes, gain } => {
                    self.notes = Some(NotesPlay {
                        notes,
                        frame: 0,
                        next: 0,
                        held: [NOT_HELD; 128],
                        gain,
                        target: gain,
                        chain: None,
                    });
                }
                PreviewCommand::Gain(_) | PreviewCommand::Stop => {}
            }
        }
    }

    /// Stops whatever plays, releasing its held notes at `frame` of the block, and sends it
    /// back. Only called when the retired queue has room for two.
    fn stop(&mut self, chains: &mut [Chain], frame: usize) {
        if let Some(sample) = self.sample.take()
            && let Some(retired) = self.retired.as_mut()
        {
            let _full = retired.push(Retired::Sample(sample.data));
        }
        if let Some(mut play) = self.notes.take() {
            if let Some(chain) = play.chain.and_then(|i| chains.get_mut(i)) {
                for key in 0..=127u8 {
                    if play.held.get(usize::from(key)).is_some_and(|h| *h != NOT_HELD) {
                        chain.release_key(frame, key);
                    }
                }
            }
            if let Some(device) = play.notes.fallback.as_mut() {
                device.event(NoteEvent::AllNotesOff);
            }
            if let Some(retired) = self.retired.as_mut() {
                let _full = retired.push(Retired::Notes(play.notes));
            }
        }
        self.monitor.playing.store(false, Ordering::Relaxed);
    }

    /// Finds the chain of the track a MIDI preview plays through, at the start of a block (the
    /// graph may have been swapped).
    pub(crate) fn begin(&mut self, chains: &[Chain]) {
        self.events.clear();
        if let Some(play) = self.notes.as_mut() {
            play.chain = play.notes.track.and_then(|t| chains.iter().position(|c| c.track == t && c.instrument));
        }
    }

    /// Starts and releases the MIDI preview's notes due at `frame` of the block (renderer frame
    /// `position`).
    pub(crate) fn frame(&mut self, chains: &mut [Chain], frame: usize, position: u64) {
        let Some(play) = self.notes.as_mut() else { return };
        let now = play.frame;
        let chain = play.chain.and_then(|i| chains.get_mut(i));
        match chain {
            Some(chain) => {
                while let Some(note) = play.notes.notes.get(play.next).filter(|n| n.start <= now) {
                    let end = position + note.length.max(1);
                    chain.note_on(frame, position, note.key, note.velocity, end);
                    if let Some(held) = play.held.get_mut(usize::from(note.key)) {
                        *held = now + note.length.max(1);
                    }
                    play.next += 1;
                }
                // The chain releases them itself; this only tracks which are still held.
                for held in play.held.iter_mut() {
                    if *held != NOT_HELD && *held <= now {
                        *held = NOT_HELD;
                    }
                }
            }
            None => {
                for key in 0..=127u8 {
                    if let Some(held) = play.held.get_mut(usize::from(key))
                        && *held != NOT_HELD
                        && *held <= now
                    {
                        *held = NOT_HELD;
                        if self.events.len() < MAX_EVENTS {
                            self.events.push((frame, NoteEvent::Off { key, velocity: 0.5 }));
                        }
                    }
                }
                while let Some(note) = play.notes.notes.get(play.next).filter(|n| n.start <= now) {
                    if self.events.len() >= MAX_EVENTS {
                        break;
                    }
                    let key = note.key;
                    if let Some(held) = play.held.get_mut(usize::from(key)) {
                        if *held != NOT_HELD && self.events.len() < MAX_EVENTS {
                            self.events.push((frame, NoteEvent::Off { key, velocity: 0.5 }));
                        }
                        *held = now + note.length.max(1);
                    }
                    let velocity = note.velocity.clamp(1.0 / 127.0, 1.0);
                    if self.events.len() < MAX_EVENTS {
                        self.events.push((frame, NoteEvent::On { key, velocity }));
                    }
                    play.next += 1;
                }
            }
        }
        play.frame += 1;
    }

    /// Adds the preview to a block of `frames` frames after the track chains have run.
    pub(crate) fn finish(&mut self, out: &mut [f32], frames: usize, chains: &mut [Chain]) {
        let mut ended = false;
        if let Some(play) = self.notes.as_mut() {
            if play.chain.is_none()
                && let Some(device) = play.notes.fallback.as_mut()
                && let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2)
            {
                buffer.fill(0.0);
                let context = ProcessContext::default();
                let mut done = 0;
                for &(at, event) in &self.events {
                    let at = at.clamp(done, frames);
                    if let Some(span) = buffer.get_mut(done * 2..at * 2)
                        && !span.is_empty()
                    {
                        device.process(&context, span);
                    }
                    done = at;
                    device.event(event);
                }
                if let Some(span) = buffer.get_mut(done * 2..frames * 2)
                    && !span.is_empty()
                {
                    device.process(&context, span);
                }
                for ([l, r], [pl, pr]) in out.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0) {
                    play.gain = glided(play.gain, play.target, self.glide);
                    *l += pl * play.gain;
                    *r += pr * play.gain;
                }
            }
            let seconds = play.frame as f64 / f64::from(self.sample_rate.max(1));
            self.monitor.seconds.store(seconds.to_bits(), Ordering::Relaxed);
            ended = play.frame >= play.notes.length;
        }
        if let Some(play) = self.sample.as_mut() {
            let frames_in_file = play.data.frames() as f64;
            let guard = (CLICK_GUARD_SECONDS * f64::from(self.sample_rate)).max(1.0);
            let left_in_file = |position: f64| (frames_in_file - position) / play.step.max(f64::MIN_POSITIVE);
            for [l, r] in out.as_chunks_mut::<2>().0.iter_mut().take(frames) {
                if play.position >= frames_in_file {
                    break;
                }
                let (left, right) = play.data.frame_at(play.position);
                let fade_in = ((play.played as f64 + 1.0) / guard).min(1.0);
                let fade_out = (left_in_file(play.position) / guard).min(1.0);
                play.gain = glided(play.gain, play.target, self.glide);
                let gain = play.gain * (fade_in * fade_out) as f32;
                *l += left * gain;
                *r += right * gain;
                play.position += play.step;
                play.played += 1;
            }
            let seconds = play.position / f64::from(play.data.sample_rate.max(1));
            self.monitor.seconds.store(seconds.to_bits(), Ordering::Relaxed);
            ended = ended || play.position >= frames_in_file;
        }
        if ended {
            if self.retired.as_ref().is_some_and(|r| r.slots() >= 2) {
                self.stop(chains, frames.saturating_sub(1));
            }
        } else {
            self.monitor.playing.store(self.playing(), Ordering::Relaxed);
        }
    }
}

/// Turns notes in beats into a MIDI preview at `bpm` and `sample_rate`: `(time, duration, key,
/// velocity)` per note.
pub fn preview_notes(
    notes: impl IntoIterator<Item = (f64, f64, u8, f32)>,
    bpm: f64,
    sample_rate: u32,
    track: Option<TrackId>,
    fallback: Option<Box<dyn Device>>,
) -> PreviewNotes {
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm.clamp(1.0, 1000.0) } else { 120.0 };
    let frames_per_beat = 60.0 / bpm * f64::from(sample_rate);
    let frames = |beats: f64| if beats.is_finite() { (beats.max(0.0) * frames_per_beat).round() as u64 } else { 0 };
    let mut list: Vec<PreviewNote> = notes
        .into_iter()
        .map(|(time, duration, key, velocity)| PreviewNote {
            start: frames(time),
            length: frames(duration).max(1),
            key: key.min(127),
            velocity: if velocity.is_finite() { velocity.clamp(0.0, 1.0) } else { 0.5 },
        })
        .collect();
    list.sort_by_key(|n| n.start);
    let tail = (0.5 * f64::from(sample_rate)) as u64;
    let length = list.iter().map(|n| n.start.saturating_add(n.length)).max().unwrap_or(0).saturating_add(tail);
    PreviewNotes { notes: list, track, fallback, length }
}
