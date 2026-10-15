//! Device chains on tracks (ADR 0004 › 9).
//!
//! A track whose channel holds built-in effects and instruments gets a [`Chain`]: everything the track plays in
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
//! for built-in effects and instruments, which only ever see their own input.
//!
//! A group track whose channel holds devices is a bus (see `buses`): the chains of the tracks
//! that play into it add their output to its buffer instead of the output, and its devices run
//! over the sum.

use std::time::Instant;

use suhina_device_api::{Device, NoteEvent, ProcessContext, Role};
use suhina_model::{DeviceKind, Project, TrackId};

use crate::automation::{DeviceLanes, MixLanes};
use crate::plugin::{Delay, PluginProcessor, PluginSlot, Plugins, Sent, plugin_devices};
use crate::plugin_params::SlotParams;
use crate::sends::SendOut;

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
    /// The lanes of its built-in devices' parameters.
    device_lanes: DeviceLanes,
    /// The lanes of the track's mixer, which scale `post` frame by frame (#395).
    pub(crate) mix: Option<MixLanes>,
    /// The index of the bus chain this chain's output goes into, or `None` for the output. Bus
    /// chains come after the chains that feed them, so they run after them in a block.
    pub(crate) target: Option<usize>,
    /// Some chain plays into this one (a group bus): it runs after them, in order, rather than
    /// in the stages the other chains run their plug-ins in together. Set with `target`.
    pub(crate) is_bus: bool,
    /// The track's cue is on: its output after the chain is also added to the cue bus, at
    /// these gains (its own channel's volume and pan, divided by `CHAIN_INPUT_GAIN`).
    pub(crate) cue: Option<(f32, f32)>,
    /// The returns this chain sends to (see `sends`), each after its devices.
    pub(crate) sends: Vec<SendOut>,
    /// What a send reads through its delay line; empty without sends.
    pub(crate) scratch: Vec<f32>,
    pub(crate) buffer: Vec<f32>,
    pending: Vec<(usize, NoteEvent)>,
    /// The pending events with their frames counted from the start of the span a plug-in runs.
    span_events: Vec<(usize, NoteEvent)>,
    /// The current block's: when plug-ins must have answered (`None` offline), and the
    /// transport.
    deadline: Option<Instant>,
    context: ProcessContext,
    /// Frames of the current block the devices have run over.
    done: usize,
    /// When the block runs in stages with other chains ([`Chain::advance`]): the next slot to
    /// run, and the plug-in slot whose block is out, if one is.
    stage: usize,
    waiting: Option<usize>,
    /// Per key, the renderer frame its note ends at, or `NOT_HELD`.
    held: [u64; 128],
    next_off: u64,
}

impl Chain {
    /// The chain for `track` at `sample_rate`, or `None` when its channel holds no device
    /// Suhina can run (other DAWs' devices and plug-ins that are not installed pass the sound
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
        let frames_per_beat = 60.0 / project.transport.tempo_bpm * f64::from(sample_rate);
        let mut slots: Vec<Slot> = Vec::new();
        let mut positions = Vec::new();
        let mut ids = Vec::new();
        let mut params = Vec::new();
        let mut latency = 0u32;
        let mut device_lanes = DeviceLanes::none();
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
                        params: SlotParams::new(stored, frames_per_beat),
                    }));
                    positions.push(position);
                    ids.push(format!("plugin:{key:016x}"));
                    params.push(Vec::new());
                    continue;
                }
                _ => continue,
            };
            let Some(mut device) = suhina_device_registry::create(id) else { continue };
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
            device_lanes.add(slots.len(), device.as_ref(), &stored.automation, frames_per_beat);
            for (index, &value) in values.iter().enumerate() {
                // A parameter that follows a lane starts where the lane starts.
                device.set_param(index, device_lanes.initial(slots.len(), index).unwrap_or(value));
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
            device_lanes,
            mix: None,
            target: None,
            is_bus: false,
            cue: None,
            sends: Vec::new(),
            scratch: Vec::new(),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::with_capacity(MAX_PENDING),
            span_events: Vec::with_capacity(MAX_PENDING),
            deadline: None,
            context: ProcessContext::default(),
            done: 0,
            stage: 0,
            waiting: None,
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        })
    }

    /// A chain without devices for a track that plays into a bus, so its sound reaches the bus
    /// at its gains and with its share of latency compensation. Allocates.
    pub(crate) fn passthrough(track: TrackId, column: u32) -> Self {
        Self {
            track,
            column,
            slots: Vec::new(),
            positions: Vec::new(),
            ids: Vec::new(),
            params: Vec::new(),
            instrument: false,
            latency: 0,
            delay: Delay::new(0),
            post: (1.0, 1.0),
            device_lanes: DeviceLanes::none(),
            mix: None,
            target: None,
            is_bus: false,
            cue: None,
            sends: Vec::new(),
            scratch: Vec::new(),
            buffer: vec![0.0; MAX_BLOCK * 2],
            pending: Vec::new(),
            span_events: Vec::new(),
            deadline: None,
            context: ProcessContext::default(),
            done: 0,
            stage: 0,
            waiting: None,
            held: [NOT_HELD; 128],
            next_off: NOT_HELD,
        }
    }

    /// Sets the gains applied after the devices: the track's gains up to where its sound goes.
    pub(crate) fn set_post(&mut self, post: (f32, f32)) {
        self.post = (post.0 / CHAIN_INPUT_GAIN, post.1 / CHAIN_INPUT_GAIN);
    }

    /// Sets the gains the chain's output is added to the cue bus at, or `None` for no cue.
    pub(crate) fn set_cue(&mut self, gains: Option<(f32, f32)>) {
        self.cue = gains.map(|(left, right)| (left / CHAIN_INPUT_GAIN, right / CHAIN_INPUT_GAIN));
    }

    /// Whether it runs devices (a passthrough chain has none).
    pub(crate) fn has_devices(&self) -> bool {
        !self.slots.is_empty()
    }

    /// The first `frames` frames of the buffer, for the chains that play into this bus to add
    /// their output to.
    pub(crate) fn input(&mut self, frames: usize) -> &mut [f32] {
        let end = (frames.min(MAX_BLOCK) * 2).min(self.buffer.len());
        self.buffer.get_mut(..end).unwrap_or_default()
    }

    /// Clears the buffer for a block of `frames` frames, whose plug-ins must answer by
    /// `deadline` (`None` offline). Events queued since the last block stay; they are at frame
    /// 0.
    pub(crate) fn begin(&mut self, frames: usize, deadline: Option<Instant>) {
        if let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) {
            buffer.fill(0.0);
        }
        self.done = 0;
        self.stage = 0;
        self.waiting = None;
        self.deadline = deadline;
    }

    /// Plans the parameter events of its plug-ins for a block of `frames` frames starting at
    /// arrangement frame `time` (`None` while the transport is stopped): call it after
    /// [`Chain::begin`], before any of the block runs. Never allocates.
    pub(crate) fn automate(&mut self, time: Option<u64>, frames: usize) {
        let frames = frames.min(MAX_BLOCK);
        for slot in self.plugin_slots() {
            slot.params.plan(time, frames);
        }
        self.device_lanes.plan(time, frames);
    }

    /// Fills the track's mixer gains for a block of `frames` frames whose first frame is at
    /// arrangement frame `time` (the play start marker while stopped, with `running` false).
    /// Never allocates.
    pub(crate) fn automate_mix(&mut self, time: u64, frames: usize, running: bool) {
        if let Some(mix) = self.mix.as_mut() {
            mix.plan(time, frames.min(MAX_BLOCK), running);
        }
    }

    /// Runs the devices over the rest of the block one stage further, for chains that run
    /// their plug-ins at the same time: built-in effects and instruments run here, with the block's `context`;
    /// at the next plug-in that takes the block without waiting, the block is handed over and
    /// this returns true, for [`Chain::collect`] to wait for it once every chain has handed its
    /// block over. Returns false once every device has run (a plug-in that cannot take a block
    /// that way runs as in [`Chain::finish`]).
    pub(crate) fn advance(&mut self, frames: usize, context: &ProcessContext) -> bool {
        self.context = *context;
        if self.waiting.is_some() {
            return true;
        }
        let to = frames.min(MAX_BLOCK);
        let Self { slots, buffer, pending, span_events, done, deadline, context, stage, waiting, device_lanes, .. } =
            self;
        let from = (*done).min(to);
        while let Some(slot) = slots.get_mut(*stage) {
            match slot {
                Slot::Device(device) => run_device(device, *stage, buffer, from, to, pending, device_lanes, context),
                Slot::Plugin(plugin) => {
                    let last = (to - from).saturating_sub(1);
                    span_events.clear();
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    plugin.automate(from, to);
                    if let Some(span) = buffer.get_mut(from * 2..to * 2) {
                        match plugin.send(span, span_events, *deadline) {
                            Sent::Out => {
                                *waiting = Some(*stage);
                                return true;
                            }
                            // A miss already counted: running it again would count it twice.
                            Sent::Refused => {}
                            Sent::Unsupported => plugin.run(span, span_events, *deadline),
                        }
                    }
                }
            }
            *stage += 1;
        }
        pending.clear();
        *done = to;
        false
    }

    /// Waits for the block [`Chain::advance`] handed over, if one is out, and takes its result.
    pub(crate) fn collect(&mut self, frames: usize) {
        let Some(index) = self.waiting.take() else { return };
        let to = frames.min(MAX_BLOCK);
        let from = self.done.min(to);
        if let (Some(Slot::Plugin(plugin)), Some(span)) =
            (self.slots.get_mut(index), self.buffer.get_mut(from * 2..to * 2))
        {
            plugin.finish(span, self.deadline);
        }
        self.stage = index + 1;
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

    /// Holds `key`, started by [`Chain::note_on`] or about to start, until it is released by
    /// hand: the piano roll's key column (`preview.noteOn`). `end` is the frame the note would
    /// otherwise end at; `u64::MAX` holds it for good.
    pub(crate) fn hold_key(&mut self, key: u8, end: u64) {
        if let Some(held) = self.held.get_mut(usize::from(key)) {
            *held = end;
        }
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
        let Self { slots, buffer, pending, span_events, done, deadline, context, device_lanes, .. } = self;
        let from = (*done).min(to);
        for (index, slot) in slots.iter_mut().enumerate() {
            match slot {
                Slot::Device(device) => run_device(device, index, buffer, from, to, pending, device_lanes, context),
                Slot::Plugin(plugin) => {
                    let last = (to - from).saturating_sub(1);
                    span_events.clear();
                    span_events.extend(pending.iter().map(|&(at, event)| (at.saturating_sub(from).min(last), event)));
                    plugin.automate(from, to);
                    if let Some(span) = buffer.get_mut(from * 2..to * 2) {
                        plugin.run(span, span_events, *deadline);
                    }
                }
            }
        }
        pending.clear();
        *done = to;
    }

    /// Runs the devices over the rest of a block of `frames` frames; [`Chain::mix`] then adds
    /// the result to where the track plays into, and the sends read it before that.
    pub(crate) fn complete(&mut self, frames: usize, context: &ProcessContext) {
        self.context = *context;
        self.run(frames);
    }

    /// Adds what the devices made of the block, through the latency compensation and at the
    /// track's gains, to `out`, and, when its cue is on, at its cue gains to `cue`.
    pub(crate) fn mix(&mut self, out: &mut [f32], cue: &mut [f32], frames: usize) {
        let (gain_left, gain_right) = self.post;
        let Some(buffer) = self.buffer.get_mut(..frames.min(MAX_BLOCK) * 2) else { return };
        self.delay.run(buffer);
        let pairs = out.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0);
        match &self.mix {
            None => {
                for ([l, r], [cl, cr]) in pairs {
                    *l += cl * gain_left;
                    *r += cr * gain_right;
                }
            }
            Some(mix) => {
                for (frame, ([l, r], [cl, cr])) in pairs.enumerate() {
                    let (left, right) = mix.gains(frame);
                    *l += cl * gain_left * left;
                    *r += cr * gain_right * right;
                }
            }
        }
        if let Some((cue_left, cue_right)) = self.cue {
            for ([l, r], [cl, cr]) in cue.as_chunks_mut::<2>().0.iter_mut().zip(buffer.as_chunks::<2>().0) {
                *l += cl * cue_left;
                *r += cr * cue_right;
            }
        }
    }

    /// The first `frames` frames of the block's output, before the track's gains, once
    /// [`Chain::complete`] has run.
    pub(crate) fn output(&self, frames: usize) -> &[f32] {
        let end = (frames.min(MAX_BLOCK) * 2).min(self.buffer.len());
        self.buffer.get(..end).unwrap_or_default()
    }

    /// Sets parameter `param` of the device at `position` among the channel's devices, as a
    /// parameter message does (ADR 0005, amendment). Never allocates.
    /// For a plug-in, `param` is its parameter id, and the value goes at the start of the next
    /// block.
    pub(crate) fn set_param(&mut self, position: usize, param: usize, value: f64) {
        let Some(index) = self.positions.iter().position(|&p| p == position) else { return };
        match self.slots.get_mut(index) {
            Some(Slot::Device(device)) => {
                device.set_param(param, value);
                if let Some(slot) = self.params.get_mut(index).and_then(|values| values.get_mut(param)) {
                    *slot = device.param(param).unwrap_or(value);
                }
            }
            Some(Slot::Plugin(plugin)) => {
                if let Ok(id) = u32::try_from(param) {
                    plugin.params.set(id, value);
                }
            }
            None => {}
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
        let lanes = &self.device_lanes;
        for (position, ((slot, values), fresh)) in
            self.slots.iter_mut().zip(&self.params).zip(old.slots.iter_mut()).enumerate()
        {
            match (slot, fresh) {
                (Slot::Device(device), _) => {
                    // A parameter that follows a lane keeps its running value until the lane
                    // sets it in the next block.
                    for (index, &value) in
                        values.iter().enumerate().filter(|&(index, _)| !lanes.covers(position, index))
                    {
                        device.set_param(index, value);
                    }
                }
                (Slot::Plugin(running), Slot::Plugin(fresh)) => {
                    running.enabled = fresh.enabled;
                    running.instrument = fresh.instrument;
                    running.latency = fresh.latency;
                    running.params.adopt(&mut fresh.params);
                    // A processor that reached the new graph's slot (a restart) wins, and gets
                    // the set values again.
                    if fresh.processor.is_some() {
                        std::mem::swap(&mut running.processor, &mut fresh.processor);
                        running.params.restart();
                    }
                }
                (Slot::Plugin(_), Slot::Device(_)) => {}
            }
        }
        self.held = old.held;
        self.next_off = old.next_off;
        self.delay.adopt(&mut old.delay);
        self.adopt_sends(old);
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
            Some(slot) => {
                slot.params.restart();
                Placed::Replaced(slot.processor.replace(processor))
            }
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

/// Runs a built-in device (slot `slot` of its chain) over the block's frames `from..to`,
/// split where a note event or a change of a parameter that follows a lane falls; at one frame
/// the parameter changes before the note.
#[allow(clippy::too_many_arguments)]
fn run_device(
    device: &mut Box<dyn Device>,
    slot: usize,
    buffer: &mut [f32],
    from: usize,
    to: usize,
    pending: &[(usize, NoteEvent)],
    lanes: &DeviceLanes,
    context: &ProcessContext,
) {
    let mut at_done = from;
    let mut changes = lanes.changes(slot, from, to).peekable();
    for &(at, event) in pending {
        let at = at.clamp(at_done, to);
        while let Some(change) = changes.next_if(|c| c.frame <= at) {
            process(device, buffer, at_done, change.frame.max(at_done), context);
            at_done = change.frame.max(at_done);
            device.set_param(change.param, change.value);
        }
        process(device, buffer, at_done, at, context);
        at_done = at;
        device.event(event);
    }
    for change in changes {
        process(device, buffer, at_done, change.frame.max(at_done), context);
        at_done = change.frame.max(at_done);
        device.set_param(change.param, change.value);
    }
    process(device, buffer, at_done, to, context);
}

fn process(device: &mut Box<dyn Device>, buffer: &mut [f32], from: usize, to: usize, context: &ProcessContext) {
    if to <= from {
        return;
    }
    let Some(span) = buffer.get_mut(from * 2..to * 2) else { return };
    device.process(context, span);
}
