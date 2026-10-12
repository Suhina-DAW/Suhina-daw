//! Device chains on tracks (ADR 0004 › the engine side, LOOP-35).
//!
//! A track whose channel holds built-in devices gets a [`Chain`]: everything the track plays in
//! a block (its sine voices, audio clips, launched clips) is summed into the chain's buffer
//! instead of the output, the devices run over it in order, and only then do the track's pan
//! and volume apply. When the first device is an instrument, the track's notes go to it as
//! [`NoteEvent`]s instead of starting sine voices; the chain runs in pieces split at each event,
//! so a note lands on its frame. A track without devices is mixed straight into the output, as
//! before.

use loopwerk_device_api::{Device, NoteEvent, ProcessContext, Role};
use loopwerk_model::{DeviceKind, Project, TrackId};

/// The most frames one block of a chain holds; the renderer splits longer blocks.
pub(crate) const MAX_BLOCK: usize = 1024;
/// Events a chain keeps for one block before it runs up to the frame it has reached.
const MAX_PENDING: usize = 256;
const NOT_HELD: u64 = u64::MAX;
/// Gain of a centred track at unity volume, which the sources of a chain track are mixed at
/// (see `track_gains`); the track's real gains apply after the chain, divided by this.
pub(crate) const CHAIN_INPUT_GAIN: f32 = 0.25;

pub(crate) struct Chain {
    pub(crate) track: TrackId,
    /// The launcher column of the track, or `NO_COLUMN`.
    pub(crate) column: u32,
    devices: Vec<Box<dyn Device>>,
    /// Each device's index among the channel's devices in the project (disabled and unknown
    /// devices have no place in the chain).
    positions: Vec<usize>,
    /// The project's device ids in order, so a graph swap can tell whether a new chain holds
    /// the same devices as an old one.
    ids: Vec<String>,
    /// Every parameter's value from the project (its default when the project has none), per
    /// device.
    params: Vec<Vec<f64>>,
    pub(crate) instrument: bool,
    /// The track's gains after the chain.
    post: (f32, f32),
    buffer: Vec<f32>,
    pending: Vec<(usize, NoteEvent)>,
    /// Frames of the current block the devices have run over.
    done: usize,
    /// Per key, the renderer frame its note ends at, or `NOT_HELD`.
    held: [u64; 128],
    next_off: u64,
}

impl Chain {
    /// The chain for `track` at `sample_rate`, or `None` when its channel holds no built-in
    /// device Loopwerk knows (plug-ins and other DAWs' devices pass the sound through for now).
    /// `post` is the track's gains. Allocates: call it off the audio thread.
    pub(crate) fn build(
        project: &Project,
        track: TrackId,
        column: u32,
        post: (f32, f32),
        sample_rate: u32,
    ) -> Option<Self> {
        let channel = project.tracks.iter().find(|t| t.id == track)?.channel?;
        let channel = project.channels.iter().find(|c| c.id == channel)?;
        let mut devices: Vec<Box<dyn Device>> = Vec::new();
        let mut positions = Vec::new();
        let mut ids = Vec::new();
        let mut params = Vec::new();
        for (position, stored) in channel.devices.iter().enumerate().filter(|(_, d)| d.enabled) {
            let DeviceKind::Builtin(id) = &stored.kind else { continue };
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
            devices.push(device);
            positions.push(position);
            ids.push(id.clone());
            params.push(values);
        }
        let instrument = devices.first().is_some_and(|d| d.info().role == Role::Instrument);
        (!devices.is_empty()).then(|| Self {
            track,
            column,
            devices,
            positions,
            ids,
            params,
            instrument,
            post: (post.0 / CHAIN_INPUT_GAIN, post.1 / CHAIN_INPUT_GAIN),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::with_capacity(MAX_PENDING),
            done: 0,
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        })
    }

    /// Clears the buffer for a block of `frames` frames. Events queued since the last block
    /// stay; they are at frame 0.
    pub(crate) fn begin(&mut self, frames: usize) {
        if let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) {
            buffer.fill(0.0);
        }
        self.done = 0;
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

    /// Releases every note now, at `frame` of the block.
    pub(crate) fn release_all(&mut self, frame: usize) {
        if !self.instrument {
            return;
        }
        self.held = [NOT_HELD; 128];
        self.next_off = NOT_HELD;
        self.push(frame, NoteEvent::AllNotesOff);
    }

    fn push(&mut self, frame: usize, event: NoteEvent) {
        if self.pending.len() >= MAX_PENDING {
            // Too many events in one block: run the devices up to here first.
            self.run(frame, &ProcessContext::default());
        }
        if self.pending.len() < MAX_PENDING {
            self.pending.push((frame.max(self.done), event));
        }
    }

    /// Runs the devices over the block up to `frame`, handing each queued event over at its
    /// frame.
    fn run(&mut self, frame: usize, context: &ProcessContext) {
        let to = frame.min(MAX_BLOCK);
        let Self { devices, buffer, pending, done, .. } = self;
        for &(at, event) in pending.iter() {
            let at = at.clamp(*done, to);
            process(devices, buffer, *done, at, context);
            *done = at;
            for device in devices.iter_mut() {
                device.event(event);
            }
        }
        pending.clear();
        process(devices, buffer, *done, to, context);
        *done = to;
    }

    /// Runs the devices over the rest of a block of `frames` frames and adds the result, at
    /// the track's gains, to `out`.
    pub(crate) fn finish(&mut self, out: &mut [f32], frames: usize, context: &ProcessContext) {
        self.run(frames, context);
        let (gain_left, gain_right) = self.post;
        let Some(buffer) = self.buffer.get(..frames.min(MAX_BLOCK) * 2) else { return };
        for ([l, r], [cl, cr]) in out.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0) {
            *l += cl * gain_left;
            *r += cr * gain_right;
        }
    }

    /// Sets parameter `param` of the device at `position` among the channel's devices, as a
    /// parameter message does (ADR 0005, amendment). Never allocates.
    pub(crate) fn set_param(&mut self, position: usize, param: usize, value: f64) {
        let Some(index) = self.positions.iter().position(|&p| p == position) else { return };
        if let Some(device) = self.devices.get_mut(index) {
            device.set_param(param, value);
            if let Some(slot) = self.params.get_mut(index).and_then(|values| values.get_mut(param)) {
                *slot = device.param(param).unwrap_or(value);
            }
        }
    }

    /// Takes the running devices of `old` if it holds the same devices for the same track,
    /// with this chain's parameter values and `old`'s held notes. Never allocates.
    pub(crate) fn adopt(&mut self, old: &mut Chain) -> bool {
        if old.track != self.track || old.ids != self.ids {
            return false;
        }
        std::mem::swap(&mut self.devices, &mut old.devices);
        for (device, values) in self.devices.iter_mut().zip(&self.params) {
            for (index, &value) in values.iter().enumerate() {
                device.set_param(index, value);
            }
        }
        self.held = old.held;
        self.next_off = old.next_off;
        true
    }
}

fn process(devices: &mut [Box<dyn Device>], buffer: &mut [f32], from: usize, to: usize, context: &ProcessContext) {
    if to <= from {
        return;
    }
    let Some(span) = buffer.get_mut(from * 2..to * 2) else { return };
    for device in devices.iter_mut() {
        device.process(context, span);
    }
}
