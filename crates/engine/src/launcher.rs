//! The clip launcher: one column of slots per track, at most one playing clip per column, and
//! launches that wait for the next quantisation boundary. Behaviour: `docs/spec/clip-launching.md`.
//!
//! Every launch clip is prepared when the [`Renderer`] is built; launching only changes indices
//! and counters, so [`Renderer::handle`] and the per-frame work never allocate. Other threads
//! send [`LaunchCommand`]s through a [`Controller`] (a lock-free queue) and read the state back
//! from a [`LauncherMonitor`] (atomics).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use loopwerk_model::{LaneData, Project, TimeSignature};

use crate::audio::{AudioEvent, AudioPool, Timing};
use crate::chain::Chain;
use crate::stretch::StretchVoice;
use crate::{AUDIO_HEADROOM, NoteStart, Renderer, bus_chain, route, start_note};

/// Commands that can wait in the queue between two audio blocks.
const QUEUE_CAPACITY: usize = 256;
/// Fade-out of a stopped or replaced audio clip, in seconds.
const STOP_FADE_SECONDS: f64 = 0.005;
/// Length given to looping audio clips, in beats: long enough to be endless in practice.
const ENDLESS_BEATS: f64 = 10_000_000.0;
/// Length given to looping time-stretched clips, in beats (over an hour at 120 bpm); their
/// playlist is prepared up front, so it cannot be endless. After it they fall back to re-pitch.
const STRETCH_LOOP_BEATS: f64 = 8_192.0;
/// Monitor value for "no slot".
const NONE: u32 = u32::MAX;
/// Monitor value for "a stop is queued".
const STOP: u32 = u32::MAX - 1;

/// A grid that launches wait for. Beats are quarter notes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quantization {
    None,
    Bars(u32),
    /// A note value, in beats: 1/16 is 0.25, a 1/8 triplet 1/3.
    Beats(f64),
}

impl Quantization {
    /// The grid step in beats; 0 means none.
    fn step(self, bar_beats: f64) -> f64 {
        let step = match self {
            Self::None => 0.0,
            Self::Bars(bars) => f64::from(bars) * bar_beats,
            Self::Beats(beats) => beats,
        };
        if step.is_finite() && step > 0.0 { step } else { 0.0 }
    }
}

/// An action for the launcher. Columns, slots and scenes are 0-based indices into
/// `Project::launcher`; out-of-range indices are ignored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaunchCommand {
    /// Start the transport from beat 0; nothing happens if it is running.
    Play,
    /// Stop the transport, every clip and the arrangement at once.
    Stop,
    LaunchClip {
        column: usize,
        slot: usize,
    },
    LaunchScene {
        scene: usize,
    },
    StopColumn {
        column: usize,
    },
    StopAll,
    SetQuantization(Quantization),
    /// Every track plays its arrangement clips again.
    BackToArrangement,
    /// Turns the metronome on or off; `gain` is its level (linear, 0.5 is -6 dB), or `None` to
    /// keep the level it has.
    SetMetronome {
        on: bool,
        gain: Option<f32>,
    },
}

/// What a column is waiting to do, as reported by [`LauncherMonitor::column`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queued {
    Launch(usize),
    Stop,
}

/// One column's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnStatus {
    /// The slot whose clip is playing.
    pub playing: Option<usize>,
    pub queued: Option<Queued>,
    /// The track plays launched clips instead of its arrangement clips.
    pub overrides_arrangement: bool,
}

/// The launcher's state, written by the audio thread and readable from any thread.
#[derive(Debug)]
pub struct LauncherMonitor {
    pub(crate) running: AtomicBool,
    beat: AtomicU64,
    tempo: AtomicU64,
    playing: Vec<AtomicU32>,
    queued: Vec<AtomicU32>,
    overrides: Vec<AtomicBool>,
    /// Renderer frames played so far, and the arrangement frame playing (ADR 0005).
    pub(crate) position: AtomicU64,
    pub(crate) arrangement: AtomicU64,
    pub(crate) stretch_underruns: AtomicU64,
    /// Arrangement frames per beat of the playing graph (f64 bits), so an update can find the
    /// playhead's beat.
    pub(crate) frames_per_beat: AtomicU64,
    metronome: AtomicBool,
}

impl LauncherMonitor {
    fn new(columns: usize, tempo: f64) -> Self {
        Self {
            running: AtomicBool::new(false),
            beat: AtomicU64::new(0f64.to_bits()),
            tempo: AtomicU64::new(tempo.to_bits()),
            playing: (0..columns).map(|_| AtomicU32::new(NONE)).collect(),
            queued: (0..columns).map(|_| AtomicU32::new(NONE)).collect(),
            overrides: (0..columns).map(|_| AtomicBool::new(false)).collect(),
            position: AtomicU64::new(0),
            arrangement: AtomicU64::new(0),
            stretch_underruns: AtomicU64::new(0),
            frames_per_beat: AtomicU64::new(0),
            metronome: AtomicBool::new(false),
        }
    }

    /// Whether the transport is running.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Beats since the transport started.
    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat.load(Ordering::Relaxed))
    }

    pub fn tempo_bpm(&self) -> f64 {
        f64::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    /// Whether the metronome clicks while the transport runs.
    pub fn metronome(&self) -> bool {
        self.metronome.load(Ordering::Relaxed)
    }

    pub fn columns(&self) -> usize {
        self.playing.len()
    }

    pub fn column(&self, column: usize) -> Option<ColumnStatus> {
        let slot = |v: u32| (v < STOP).then_some(v as usize);
        let playing = slot(self.playing.get(column)?.load(Ordering::Relaxed));
        let queued = match self.queued.get(column)?.load(Ordering::Relaxed) {
            NONE => None,
            STOP => Some(Queued::Stop),
            v => Some(Queued::Launch(v as usize)),
        };
        let overrides_arrangement = self.overrides.get(column)?.load(Ordering::Relaxed);
        Some(ColumnStatus { playing, queued, overrides_arrangement })
    }
}

/// Sends commands to a renderer that may be running on the audio thread.
pub struct Controller {
    producer: rtrb::Producer<LaunchCommand>,
    pub(crate) monitor: Arc<LauncherMonitor>,
    pub(crate) updater: crate::swap::Updater,
}

impl Controller {
    /// Queues a command for the start of the next audio block. Fails only if the queue is full,
    /// which means the renderer is not running.
    pub fn send(&mut self, command: LaunchCommand) -> Result<(), String> {
        self.producer.push(command).map_err(|_| "the audio engine is not taking commands (queue full)".to_string())
    }

    pub fn monitor(&self) -> &LauncherMonitor {
        &self.monitor
    }
}

/// A note of a launch clip, in content beats, with its mix resolved.
#[derive(Debug, Clone, Copy)]
struct PlanNote {
    time: f64,
    duration: f64,
    frequency: f64,
    key: u8,
    velocity: f32,
    track: loopwerk_model::TrackId,
    gain_left: f32,
    gain_right: f32,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
}

struct PlanAudio {
    /// Placed at frame 0 at the project tempo; reads the file directly (re-pitch).
    event: AudioEvent,
    /// Warped audio follows the launcher's tempo; unwarped audio plays at file speed.
    follows_tempo: bool,
    /// The time-stretched voice of a warped clip, used while the tempo is the project tempo it
    /// was prepared for (ADR 0003).
    stretch: Option<StretchVoice>,
    /// Beats the stretch voice covers; after that the clip is re-pitched.
    stretch_beats: f64,
    stretch_gains: (f32, f32),
    /// Identifies what the stretch voice plays (region, placement, tempo, sample rate), so a
    /// graph swap can keep a playing voice whose audio has not changed (ADR 0005).
    fingerprint: u64,
    /// 0 for the output, or 1 + the index of the track's chain.
    bus: u32,
}

/// A slot's clip, ready to play.
struct ClipPlan {
    offset: f64,
    length: f64,
    loop_range: Option<(f64, f64)>,
    /// Sorted by time.
    notes: Vec<PlanNote>,
    audio: Vec<PlanAudio>,
}

impl ClipPlan {
    /// The content beat reached `elapsed` beats after launch.
    fn clip_beat(&self, elapsed: f64) -> f64 {
        let beat = self.offset + elapsed;
        match self.loop_range {
            Some((start, end)) if beat >= end => start + (beat - end).rem_euclid(end - start),
            _ => beat,
        }
    }

    /// Notes end here: the loop end, or the end marker.
    fn note_limit(&self) -> f64 {
        self.loop_range.map_or(self.offset + self.length, |(_, end)| end)
    }

    fn first_note_from(&self, beat: f64) -> usize {
        self.notes.partition_point(|n| n.time < beat)
    }
}

struct SlotPlan {
    clip: Option<ClipPlan>,
    has_stop: bool,
}

/// A clip that is playing or fading out.
#[derive(Debug, Clone, Copy)]
struct Playing {
    slot: usize,
    elapsed_beats: f64,
    elapsed_frames: f64,
    last_clip_beat: f64,
    next_note: usize,
    /// Frames of fade-out left, while stopping.
    fade_left: f64,
    /// Whether this playback uses the clip's stretch voices; it stops using them for good when
    /// the tempo changes.
    stretched: bool,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    at_beat: f64,
    action: Queued,
}

struct Column {
    slots: Vec<SlotPlan>,
    playing: Option<Playing>,
    fading: Option<Playing>,
    pending: Option<Pending>,
    /// The frame from which the track plays launched clips instead of its arrangement.
    overrides_since: Option<u64>,
}

pub(crate) struct Launcher {
    columns: Vec<Column>,
    scene_tempos: Vec<Option<f64>>,
    quantization: Quantization,
    bar_beats: f64,
    beat: f64,
    tempo_bpm: f64,
    /// The project tempo, which stretch voices are prepared for.
    project_tempo_bpm: f64,
    pending_tempo: Option<(f64, f64)>,
    frames_per_beat_at_build: f64,
    stop_fade_frames: f64,
    pub(crate) monitor: Arc<LauncherMonitor>,
    control: Option<rtrb::Consumer<LaunchCommand>>,
}

impl Launcher {
    pub(crate) fn new(project: &Project, audio: &AudioPool, sample_rate: u32, chains: &[Chain]) -> Self {
        let tempo = project.transport.tempo_bpm;
        let frames_per_beat = 60.0 / tempo * f64::from(sample_rate);
        let columns: Vec<Column> = project
            .launcher
            .columns
            .iter()
            .map(|column| Column {
                slots: column
                    .slots
                    .iter()
                    .map(|slot| SlotPlan {
                        clip: slot.clip.as_ref().filter(|c| c.enabled).and_then(|c| {
                            plan_clip(project, audio, chains, c, column.track, frames_per_beat, sample_rate)
                        }),
                        has_stop: slot.has_stop,
                    })
                    .collect(),
                playing: None,
                fading: None,
                pending: None,
                overrides_since: None,
            })
            .collect();
        Self {
            monitor: Arc::new(LauncherMonitor::new(columns.len(), tempo)),
            columns,
            scene_tempos: project
                .launcher
                .scenes
                .iter()
                .map(|s| s.tempo_bpm.filter(|bpm| bpm.is_finite() && (1.0..=999.0).contains(bpm)))
                .collect(),
            quantization: Quantization::Bars(1),
            bar_beats: bar_beats(project.transport.time_signature),
            beat: 0.0,
            tempo_bpm: tempo,
            project_tempo_bpm: tempo,
            pending_tempo: None,
            frames_per_beat_at_build: frames_per_beat,
            stop_fade_frames: (STOP_FADE_SECONDS * f64::from(sample_rate)).max(1.0),
            control: None,
        }
    }

    /// Takes over `old`'s runtime state at a graph swap (ADR 0005): what each column plays, fades
    /// and has queued, the beat, tempo and quantisation, and the control queue and monitor (the
    /// new graph's own go back with the old graph). A playing clip whose stretched audio has not
    /// changed keeps its stretch voice; any other playing stretch voice is moved to where its clip
    /// is. A column or slot that no longer exists is dropped. Never allocates.
    pub(crate) fn adopt(&mut self, old: &mut Launcher) {
        std::mem::swap(&mut self.monitor, &mut old.monitor);
        std::mem::swap(&mut self.control, &mut old.control);
        self.quantization = old.quantization;
        self.beat = old.beat;
        self.pending_tempo = old.pending_tempo;
        // An edited project tempo replaces the running one; otherwise a scene's tempo stays.
        self.tempo_bpm =
            if self.project_tempo_bpm == old.project_tempo_bpm { old.tempo_bpm } else { self.project_tempo_bpm };
        let frames_per_beat = self.frames_per_beat_at_build;
        for (new, old) in self.columns.iter_mut().zip(old.columns.iter_mut()) {
            new.overrides_since = old.overrides_since;
            new.pending = old.pending.filter(|p| match p.action {
                Queued::Launch(slot) => new.slots.get(slot).is_some_and(|s| s.clip.is_some()),
                Queued::Stop => true,
            });
            new.playing = carry(&mut new.slots, &mut old.slots, old.playing, frames_per_beat);
            new.fading = carry(&mut new.slots, &mut old.slots, old.fading, frames_per_beat);
        }
    }

    /// Whether the track behind `column` plays launched clips instead of its arrangement.
    pub(crate) fn overrides(&self, column: u32) -> bool {
        self.columns.get(column as usize).is_some_and(|c| c.overrides_since.is_some())
    }

    /// Like [`Launcher::overrides`], at frame `position` of the block just rendered.
    pub(crate) fn overrides_at(&self, column: u32, position: u64) -> bool {
        self.columns.get(column as usize).and_then(|c| c.overrides_since).is_some_and(|since| since <= position)
    }

    /// The tempo playing now.
    pub(crate) fn tempo_bpm(&self) -> f64 {
        self.tempo_bpm
    }

    /// The beat playing now and the beats in a bar, for the metronome.
    pub(crate) fn beat_and_bar(&self) -> (f64, f64) {
        (self.beat, self.bar_beats)
    }

    /// The beat a launch issued now happens at.
    fn launch_beat(&self) -> f64 {
        let step = self.quantization.step(self.bar_beats);
        if step == 0.0 {
            return self.beat;
        }
        // A launch exactly on a boundary (within rounding) happens there.
        let index = ((self.beat - 1e-9) / step).ceil().max(0.0);
        index * step
    }

    fn queue(&mut self, column: usize, action: Queued) {
        let at_beat = self.launch_beat();
        if let Some(c) = self.columns.get_mut(column) {
            c.pending = Some(Pending { at_beat, action });
        }
    }

    fn publish(&self) {
        let m = &self.monitor;
        m.beat.store(self.beat.to_bits(), Ordering::Relaxed);
        m.tempo.store(self.tempo_bpm.to_bits(), Ordering::Relaxed);
        for (i, column) in self.columns.iter().enumerate() {
            let playing = column.playing.map_or(NONE, |p| u32::try_from(p.slot).unwrap_or(NONE));
            let queued = match column.pending.map(|p| p.action) {
                None => NONE,
                Some(Queued::Stop) => STOP,
                Some(Queued::Launch(slot)) => u32::try_from(slot).unwrap_or(NONE).min(STOP - 1),
            };
            if let Some(a) = m.playing.get(i) {
                a.store(playing, Ordering::Relaxed);
            }
            if let Some(a) = m.queued.get(i) {
                a.store(queued, Ordering::Relaxed);
            }
            if let Some(a) = m.overrides.get(i) {
                a.store(column.overrides_since.is_some(), Ordering::Relaxed);
            }
        }
    }
}

/// A playing or fading clip carried from `old_slots` to the same slot in `new_slots`, or `None`
/// if that slot has no clip now.
fn carry(
    new_slots: &mut [SlotPlan],
    old_slots: &mut [SlotPlan],
    state: Option<Playing>,
    frames_per_beat: f64,
) -> Option<Playing> {
    let mut playing = state?;
    let new_plan = new_slots.get_mut(playing.slot)?.clip.as_mut()?;
    let old_plan = old_slots.get_mut(playing.slot).and_then(|s| s.clip.as_mut());
    // Notes: the same place in the new list. Notes at exactly the last beat may have played.
    let last = playing.last_clip_beat;
    let played_at_last =
        old_plan.as_ref().map_or(0, |plan| playing.next_note.saturating_sub(plan.first_note_from(last)));
    let at_last = new_plan.notes.partition_point(|n| n.time <= last) - new_plan.first_note_from(last);
    playing.next_note = new_plan.first_note_from(last) + played_at_last.min(at_last);
    // Stretch voices: unchanged audio keeps its voice; changed audio starts where the clip is.
    let unchanged = old_plan.as_ref().is_some_and(|plan| {
        plan.audio.len() == new_plan.audio.len()
            && plan.audio.iter().zip(&new_plan.audio).all(|(a, b)| a.fingerprint == b.fingerprint)
    });
    match old_plan {
        Some(old_plan) if unchanged => {
            for (new, old) in new_plan.audio.iter_mut().zip(old_plan.audio.iter_mut()) {
                std::mem::swap(&mut new.stretch, &mut old.stretch);
            }
        }
        _ if playing.stretched => {
            let frames = (playing.elapsed_beats * frames_per_beat).round().max(0.0) as u64;
            for voice in new_plan.audio.iter_mut().filter_map(|a| a.stretch.as_mut()) {
                voice.seek(frames);
            }
        }
        _ => {}
    }
    Some(playing)
}

/// Beats in one bar: a 4/4 bar has 4, a 6/8 bar 3.
fn bar_beats(ts: TimeSignature) -> f64 {
    if ts.numerator == 0 || ts.denominator == 0 {
        return 4.0;
    }
    f64::from(ts.numerator) * 4.0 / f64::from(ts.denominator)
}

fn plan_clip(
    project: &Project,
    audio: &AudioPool,
    chains: &[Chain],
    clip: &loopwerk_model::Clip,
    track: loopwerk_model::TrackId,
    frames_per_beat: f64,
    sample_rate: u32,
) -> Option<ClipPlan> {
    let content = project.content(clip.content)?;
    if !(clip.length.is_finite() && clip.length > 0.0 && clip.offset.is_finite()) {
        return None;
    }
    let loop_range = clip
        .loop_range
        .filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start)
        .map(|r| (r.start, r.end));
    let mut notes = Vec::new();
    let mut audio_plans = Vec::new();
    for lane in &content.lanes {
        let lane_track = lane.target.unwrap_or(track);
        let Some((bus, (gain_left, gain_right))) = route(project, chains, lane_track) else {
            continue;
        };
        match &lane.data {
            LaneData::Notes(lane_notes) => {
                for note in lane_notes.iter().filter(|n| n.time.is_finite() && n.duration.is_finite()) {
                    let velocity = note.velocity.clamp(0.0, 1.0);
                    notes.push(PlanNote {
                        time: note.time,
                        duration: note.duration.max(0.0),
                        frequency: 440.0 * 2f64.powf((f64::from(note.key) - 69.0) / 12.0),
                        key: note.key.min(127),
                        velocity,
                        track: lane_track,
                        gain_left: gain_left * velocity,
                        gain_right: gain_right * velocity,
                        bus,
                    });
                }
            }
            LaneData::Audio(region) => {
                let Some(data) = audio.files.get(&region.file) else { continue };
                // Placed at frame 0; a looping clip is given an endless length.
                let placed = loopwerk_model::Clip {
                    start: 0.0,
                    length: if loop_range.is_some() { ENDLESS_BEATS } else { clip.length },
                    ..clip.clone()
                };
                let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                let tempo = project.transport.tempo_bpm;
                let timing = Timing { frames_per_beat, tempo_bpm: tempo, sample_rate };
                let Some(event) = AudioEvent::new(&placed, region, data.clone(), timing, gains) else { continue };
                // Warped clips are time-stretched like arrangement clips; the stretch voice gets
                // the region gain, the direct reader has it already.
                let stretch_beats = if loop_range.is_some() { STRETCH_LOOP_BEATS } else { clip.length };
                let stretch = (region.warped && region.stretch != loopwerk_model::StretchMode::Repitch)
                    .then(|| {
                        let stretched = loopwerk_model::Clip { length: stretch_beats, ..placed.clone() };
                        StretchVoice::new(&stretched, region, data.clone(), tempo, sample_rate)
                    })
                    .flatten();
                let region_gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
                let fingerprint = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::hash::DefaultHasher::new();
                    format!("{region:?}|{placed:?}|{stretch_beats}|{tempo}|{sample_rate}").hash(&mut hasher);
                    hasher.finish()
                };
                audio_plans.push(PlanAudio {
                    event,
                    follows_tempo: region.warped,
                    stretch,
                    stretch_beats,
                    stretch_gains: (gains.0 * region_gain, gains.1 * region_gain),
                    fingerprint,
                    bus,
                });
            }
        }
    }
    notes.sort_by(|a, b| a.time.total_cmp(&b.time));
    Some(ClipPlan { offset: clip.offset, length: clip.length, loop_range, notes, audio: audio_plans })
}

impl Renderer {
    /// A controller for sending launch commands from another thread. The renderer applies them
    /// at the start of each [`Renderer::process`] call. Calling this again replaces the queue.
    pub fn controller(&mut self) -> Controller {
        let (producer, consumer) = rtrb::RingBuffer::new(QUEUE_CAPACITY);
        self.graph.launcher.control = Some(consumer);
        Controller { producer, monitor: Arc::clone(&self.graph.launcher.monitor), updater: self.updater() }
    }

    /// The launcher state, as other threads see it.
    pub fn monitor(&self) -> Arc<LauncherMonitor> {
        Arc::clone(&self.graph.launcher.monitor)
    }

    /// Applies one launch command now. Never allocates.
    pub fn handle(&mut self, command: LaunchCommand) {
        match command {
            LaunchCommand::Play => {
                if !self.running {
                    self.start_transport();
                }
            }
            LaunchCommand::Stop => self.stop_transport(),
            LaunchCommand::SetMetronome { on, gain } => {
                self.metronome.set(on, gain);
                self.graph.launcher.monitor.metronome.store(self.metronome.on(), Ordering::Relaxed);
            }
            LaunchCommand::LaunchClip { column, slot } => {
                let Some(target) = self.graph.launcher.columns.get(column).and_then(|c| c.slots.get(slot)) else {
                    return;
                };
                let action = match (&target.clip, target.has_stop) {
                    (Some(_), _) => Queued::Launch(slot),
                    (None, true) => Queued::Stop,
                    (None, false) => return,
                };
                if matches!(action, Queued::Stop) && !self.running {
                    return;
                }
                self.ensure_running();
                self.graph.launcher.queue(column, action);
            }
            LaunchCommand::LaunchScene { scene } => {
                if scene >= self.graph.launcher.scene_tempos.len() {
                    return;
                }
                self.ensure_running();
                for column in 0..self.graph.launcher.columns.len() {
                    let action = match self.graph.launcher.columns.get(column).and_then(|c| c.slots.get(scene)) {
                        Some(SlotPlan { clip: Some(_), .. }) => Queued::Launch(scene),
                        Some(SlotPlan { clip: None, has_stop: true }) => Queued::Stop,
                        _ => continue,
                    };
                    self.graph.launcher.queue(column, action);
                }
                if let Some(Some(bpm)) = self.graph.launcher.scene_tempos.get(scene) {
                    self.graph.launcher.pending_tempo = Some((self.graph.launcher.launch_beat(), *bpm));
                }
            }
            LaunchCommand::StopColumn { column } => {
                if self.running {
                    self.graph.launcher.queue(column, Queued::Stop);
                }
            }
            LaunchCommand::StopAll => {
                if self.running {
                    for column in 0..self.graph.launcher.columns.len() {
                        self.graph.launcher.queue(column, Queued::Stop);
                    }
                }
            }
            LaunchCommand::SetQuantization(q) => self.graph.launcher.quantization = q,
            LaunchCommand::BackToArrangement => {
                for column in &mut self.graph.launcher.columns {
                    column.overrides_since = None;
                }
            }
        }
        self.graph.launcher.publish();
    }

    fn ensure_running(&mut self) {
        if !self.running {
            self.start_transport();
        }
    }

    fn start_transport(&mut self) {
        self.running = true;
        self.origin = self.position;
        self.time_at_origin = 0;
        self.metronome.reset();
        self.next_event = 0;
        self.next_audio = 0;
        self.active_audio_count = 0;
        // The stretch voices are rewound when the transport stops, so the worker has them ready
        // by the next start; only a start without a stop before it rewinds here.
        if !self.stretch_fresh {
            self.graph.ahead.rewind();
            self.stretch_fresh = true;
        }
        self.graph.launcher.beat = 0.0;
        self.graph.launcher.monitor.running.store(true, Ordering::Relaxed);
    }

    fn stop_transport(&mut self) {
        self.running = false;
        self.metronome.reset();
        self.active_audio_count = 0;
        if !self.stretch_fresh {
            self.graph.ahead.rewind();
            self.stretch_fresh = true;
        }
        for voice in self.voices.iter_mut().filter(|v| v.active) {
            voice.end = voice.end.min(self.position);
        }
        let frame = self.block_frame;
        for chain in &mut self.graph.chains {
            chain.release_all(frame);
        }
        let launcher = &mut self.graph.launcher;
        for column in &mut launcher.columns {
            column.pending = None;
            if let Some(mut playing) = column.playing.take() {
                playing.fade_left = launcher.stop_fade_frames;
                column.fading = Some(playing);
            }
        }
        launcher.pending_tempo = None;
        launcher.beat = 0.0;
        launcher.monitor.running.store(false, Ordering::Relaxed);
    }

    /// Applies commands queued by a [`Controller`]. Called at the start of each block.
    pub(crate) fn drain_commands(&mut self) {
        let mut handled = 0;
        while handled < QUEUE_CAPACITY {
            let Some(command) = self.graph.launcher.control.as_mut().and_then(|c| c.pop().ok()) else { break };
            self.handle(command);
            handled += 1;
        }
        let monitor = &self.graph.launcher.monitor;
        monitor.beat.store(self.graph.launcher.beat.to_bits(), Ordering::Relaxed);
        monitor.tempo.store(self.graph.launcher.tempo_bpm.to_bits(), Ordering::Relaxed);
    }

    /// Advances the launcher by one frame: applies launches that are due, starts notes, mixes
    /// audio and fades. Returns the launcher's stereo output for this frame.
    pub(crate) fn launcher_frame(&mut self) -> (f32, f32) {
        let mut out = (0.0f32, 0.0f32);
        let frame = self.block_frame;
        if self.running {
            self.apply_due();
        }
        let beats_per_frame = self.graph.launcher.tempo_bpm / 60.0 / f64::from(self.sample_rate);
        for index in 0..self.graph.launcher.columns.len() {
            if self.running {
                self.play_notes(index, beats_per_frame);
            }
            let launcher = &mut self.graph.launcher;
            let chains = &mut self.graph.chains;
            let fpb = launcher.frames_per_beat_at_build;
            let fade_total = launcher.stop_fade_frames;
            let fade_in = self.fade_frames;
            let (tempo, project_tempo) = (launcher.tempo_bpm, launcher.project_tempo_bpm);
            let Some(column) = launcher.columns.get_mut(index) else { continue };
            for state in [&mut column.playing, &mut column.fading] {
                let Some(p) = state.as_mut() else { continue };
                let Some(plan) = column.slots.get_mut(p.slot).and_then(|s| s.clip.as_mut()) else {
                    *state = None;
                    continue;
                };
                let gain = if p.fade_left > 0.0 { (p.fade_left / fade_total) as f32 } else { 1.0 };
                if p.stretched && tempo != project_tempo {
                    p.stretched = false;
                }
                for a in &mut plan.audio {
                    let (l, r) = match a.stretch.as_mut() {
                        Some(voice) if p.stretched && p.elapsed_beats < a.stretch_beats => {
                            let mut frame = [0.0f32; 2];
                            voice.render(&mut frame);
                            // The same fades as the direct reader: in at the start, out at a
                            // clip end.
                            let until_end = a.event.end as f64 - p.elapsed_frames;
                            let fade = (p.elapsed_frames / fade_in).min(until_end / fade_in).clamp(0.0, 1.0) as f32;
                            (frame[0] * a.stretch_gains.0 * fade, frame[1] * a.stretch_gains.1 * fade)
                        }
                        _ => {
                            let at = if a.follows_tempo { p.elapsed_beats * fpb } else { p.elapsed_frames };
                            a.event.sample_elapsed(at)
                        }
                    };
                    match bus_chain(chains, a.bus) {
                        Some(chain) => chain.add(frame, l * gain, r * gain),
                        None => {
                            out.0 += l * gain;
                            out.1 += r * gain;
                        }
                    }
                }
                p.elapsed_beats += beats_per_frame;
                p.elapsed_frames += 1.0;
                if p.fade_left > 0.0 {
                    p.fade_left -= 1.0;
                    if p.fade_left <= 0.0 {
                        *state = None;
                    }
                }
            }
            // A clip without a loop stops at its end marker.
            if let Some(p) = column.playing
                && let Some(plan) = column.slots.get(p.slot).and_then(|s| s.clip.as_ref())
                && plan.loop_range.is_none()
                && p.elapsed_beats >= plan.length
            {
                column.playing = None;
                self.graph.launcher.publish();
            }
        }
        if self.running {
            self.graph.launcher.beat += beats_per_frame;
        }
        out
    }

    /// Applies queued launches, stops and tempo changes whose beat has come.
    fn apply_due(&mut self) {
        let beat = self.graph.launcher.beat;
        if let Some((at, bpm)) = self.graph.launcher.pending_tempo
            && at <= beat
        {
            self.graph.launcher.tempo_bpm = bpm;
            self.graph.launcher.pending_tempo = None;
        }
        let mut changed = false;
        for index in 0..self.graph.launcher.columns.len() {
            let Some(column) = self.graph.launcher.columns.get_mut(index) else { continue };
            let Some(pending) = column.pending.filter(|p| p.at_beat <= beat) else { continue };
            column.pending = None;
            changed = true;
            if let Some(mut old) = column.playing.take() {
                old.fade_left = self.graph.launcher.stop_fade_frames;
                column.fading = Some(old);
            }
            if let Queued::Launch(slot) = pending.action
                && let Some(plan) = column.slots.get_mut(slot).and_then(|s| s.clip.as_mut())
            {
                // A stretch voice can play one launch at a time: a relaunch of the clip that
                // is fading out cuts its fade short.
                if column.fading.is_some_and(|f| f.slot == slot) {
                    column.fading = None;
                }
                for a in &mut plan.audio {
                    if let Some(voice) = a.stretch.as_mut() {
                        voice.restart();
                    }
                }
                column.playing = Some(Playing {
                    slot,
                    elapsed_beats: 0.0,
                    elapsed_frames: 0.0,
                    last_clip_beat: plan.offset,
                    next_note: plan.first_note_from(plan.offset),
                    fade_left: 0.0,
                    stretched: self.graph.launcher.tempo_bpm == self.graph.launcher.project_tempo_bpm,
                });
                column.overrides_since = Some(self.position);
            }
            self.release_column(index);
        }
        if changed {
            self.graph.launcher.publish();
        }
    }

    /// Releases the notes a column is sounding, launched or from the arrangement.
    fn release_column(&mut self, column: usize) {
        let Ok(column) = u32::try_from(column) else { return };
        let position = self.position;
        for voice in self.voices.iter_mut().filter(|v| v.active && v.column == column) {
            voice.end = voice.end.min(position);
        }
        let frame = self.block_frame;
        for chain in self.graph.chains.iter_mut().filter(|c| c.column == column) {
            chain.release_all(frame);
        }
        // Arrangement audio on this track goes silent through the override check.
    }

    /// Starts the notes of a column's playing clip that are due at this frame.
    fn play_notes(&mut self, index: usize, beats_per_frame: f64) {
        let Self { graph, voices, position, sample_rate, block_frame, .. } = self;
        let frame = *block_frame;
        let launcher = &mut graph.launcher;
        let chains = &mut graph.chains;
        let Ok(column_id) = u32::try_from(index) else { return };
        let Some(column) = launcher.columns.get_mut(index) else { return };
        let Some(p) = column.playing.as_mut() else { return };
        let Some(plan) = column.slots.get(p.slot).and_then(|s| s.clip.as_ref()) else { return };
        let limit = plan.note_limit();
        let beat = plan.clip_beat(p.elapsed_beats);
        let mut start = |note: &PlanNote| {
            let beats = note.duration.min(limit - note.time).max(0.0);
            let end = position.saturating_add((beats / beats_per_frame).round() as u64);
            let start = NoteStart {
                frequency: note.frequency,
                key: note.key,
                velocity: note.velocity,
                track: note.track,
                gains: (note.gain_left, note.gain_right),
                column: column_id,
                bus: note.bus,
            };
            start_note(voices, chains, frame, *position, *sample_rate, end, start);
        };
        if let Some((loop_start, loop_end)) = plan.loop_range
            && beat < p.last_clip_beat
        {
            // Wrapped: finish the notes before the loop end, then start the loop again.
            while let Some(note) = plan.notes.get(p.next_note).filter(|n| n.time < loop_end) {
                start(note);
                p.next_note += 1;
            }
            p.next_note = plan.first_note_from(loop_start);
        }
        while let Some(note) = plan.notes.get(p.next_note).filter(|n| n.time <= beat && n.time < limit) {
            start(note);
            p.next_note += 1;
        }
        p.last_clip_beat = beat;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use loopwerk_model::*;

    use super::*;
    use crate::{AudioData, render_launches};

    const SR: u32 = 48_000;
    /// Frames per beat at 120 bpm.
    const BEAT: usize = SR as usize / 2;
    const BAR: usize = 4 * BEAT;

    fn note(time: f64, key: u8) -> Note {
        Note { time, duration: 0.25, key, velocity: 1.0, release_velocity: 0.5, channel: 0 }
    }

    /// A clip of `length` beats that loops over its whole length, or plays once.
    fn clip(content: u32, length: f64, looping: bool) -> Clip {
        Clip {
            content: ContentId(content),
            start: 0.0,
            length,
            offset: 0.0,
            loop_range: looping.then_some(LoopRange { start: 0.0, end: length }),
            enabled: true,
            extensions: Extensions::default(),
        }
    }

    fn notes(id: u32, notes: Vec<Note>) -> ClipContent {
        ClipContent {
            id: ContentId(id),
            name: format!("c{id}"),
            length: 4.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
            extensions: Extensions::default(),
        }
    }

    /// Tracks 1..=n with one launcher column each; `slots[column][scene]`.
    fn project(contents: Vec<ClipContent>, slots: Vec<Vec<Slot>>) -> Project {
        let n = u32::try_from(slots.len()).unwrap();
        let scenes = slots.iter().map(Vec::len).max().unwrap_or(0);
        Project {
            tracks: (1..=n)
                .map(|i| Track {
                    id: TrackId(i),
                    name: format!("T{i}"),
                    color: None,
                    content: ContentKinds { notes: true, audio: true },
                    channel: Some(ChannelId(i)),
                    parent: None,
                    extensions: Extensions::default(),
                })
                .collect(),
            channels: (1..=n)
                .map(|i| Channel {
                    id: ChannelId(i),
                    name: format!("C{i}"),
                    role: ChannelRole::Regular,
                    volume: 1.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    output: None,
                    sends: Vec::new(),
                    devices: Vec::new(),
                    extensions: Extensions::default(),
                })
                .collect(),
            contents,
            launcher: loopwerk_model::Launcher {
                scenes: vec![Scene::default(); scenes],
                columns: slots
                    .into_iter()
                    .zip(1..)
                    .map(|(slots, track)| LauncherColumn { track: TrackId(track), slots })
                    .collect(),
            },
            ..Project::default()
        }
    }

    fn with(clip: Clip) -> Slot {
        Slot { clip: Some(clip), has_stop: true }
    }

    /// Frames where sound starts after more than 100 silent frames.
    fn onsets(out: &[f32]) -> Vec<usize> {
        let mut result = Vec::new();
        let mut silent_run = usize::MAX;
        for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
            let loud = frame.iter().any(|s| s.abs() > 1e-6);
            if loud && silent_run > 100 {
                result.push(i);
            }
            silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
        }
        result
    }

    fn assert_onsets(out: &[f32], expected: &[usize]) {
        let actual = onsets(out);
        assert_eq!(actual.len(), expected.len(), "onsets {actual:?}, expected {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(a.abs_diff(*e) <= 2, "onsets {actual:?}, expected {expected:?}");
        }
    }

    fn render(project: &Project, commands: &[(f64, LaunchCommand)], seconds: f64) -> Vec<f32> {
        render_launches(project, &AudioPool::new(), SR, commands, seconds).unwrap()
    }

    /// One column: scene 1 loops a note on every beat, scene 2 a note on every off-beat.
    fn two_loops() -> Project {
        project(
            vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
            vec![vec![with(clip(1, 1.0, true)), with(clip(2, 1.0, true))]],
        )
    }

    #[test]
    fn nothing_plays_until_something_is_launched() {
        assert!(render(&two_loops(), &[], 2.0).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn launching_while_stopped_starts_at_once_and_the_clip_loops() {
        let out = render(&two_loops(), &[(0.1, LaunchCommand::LaunchClip { column: 0, slot: 0 })], 2.1);
        let start = SR as usize / 10;
        assert_onsets(&out, &[start, start + BEAT, start + 2 * BEAT, start + 3 * BEAT]);
    }

    #[test]
    fn a_launch_waits_for_the_next_bar_and_replaces_the_playing_clip() {
        let commands = [
            (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
            (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
        ];
        let out = render(&two_loops(), &commands, 3.0);
        // Clip 1 on beats 0-3, then clip 2 from bar 2 on its off-beats.
        let off = BEAT / 2;
        assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off]);
    }

    #[test]
    fn quantisation_can_be_a_note_value_or_off() {
        let mut commands = vec![
            (0.0, LaunchCommand::SetQuantization(Quantization::Beats(1.0))),
            (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
            (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
        ];
        let out = render(&two_loops(), &commands, 1.5);
        // The second launch waits for beat 2 (0.5 s at 120 bpm) instead of the bar.
        assert_onsets(&out, &[0, BEAT + BEAT / 2, 2 * BEAT + BEAT / 2]);

        commands[0].1 = LaunchCommand::SetQuantization(Quantization::None);
        commands[2].0 = 0.1;
        let out = render(&two_loops(), &commands, 1.0);
        // Clip 2 starts at 0.1 s from its beginning: its off-beat note follows a quarter second later.
        let at = SR as usize / 10;
        assert_onsets(&out, &[0, at + BEAT / 2, at + BEAT + BEAT / 2]);
    }

    #[test]
    fn the_bar_follows_the_time_signature() {
        let mut p = two_loops();
        p.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
        let commands = [
            (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
            (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
        ];
        let out = render(&p, &commands, 2.0);
        assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT + BEAT / 2]);
    }

    #[test]
    fn stops_are_quantised_and_transport_stop_is_immediate() {
        let out = render(
            &two_loops(),
            &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }), (0.3, LaunchCommand::StopColumn { column: 0 })],
            3.0,
        );
        assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT]);

        let out = render(
            &two_loops(),
            &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }), (0.6, LaunchCommand::Stop)],
            3.0,
        );
        assert_onsets(&out, &[0, BEAT]);
        // Silent soon after the stop: only the 50 ms release tail is left.
        let after = (0.6 + 0.06) * f64::from(SR);
        assert!(out.iter().skip(after as usize * 2).all(|s| *s == 0.0));
    }

    #[test]
    fn scenes_launch_their_row_and_stop_buttons_stop_columns() {
        // Column 1: scene 1 clip, scene 2 empty with a stop button.
        // Column 2: scene 1 clip, scene 2 empty without one, so it keeps playing.
        let p = project(
            vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
            vec![
                vec![with(clip(1, 1.0, true)), Slot { clip: None, has_stop: true }],
                vec![with(clip(2, 1.0, true)), Slot { clip: None, has_stop: false }],
            ],
        );
        let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
        renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
        let monitor = renderer.monitor();
        assert!(monitor.running());
        let mut block = vec![0.0f32; 2 * 256];
        renderer.process(&mut block);
        assert_eq!(monitor.column(0).unwrap().playing, Some(0));
        assert_eq!(monitor.column(1).unwrap().playing, Some(0));

        renderer.handle(LaunchCommand::LaunchScene { scene: 1 });
        assert_eq!(monitor.column(0).unwrap().queued, Some(Queued::Stop));
        assert_eq!(monitor.column(1).unwrap().queued, None);
        let mut bar = vec![0.0f32; 2 * BAR];
        renderer.process(&mut bar);
        assert_eq!(monitor.column(0).unwrap().playing, None);
        assert_eq!(monitor.column(1).unwrap().playing, Some(0));
        assert!(monitor.column(0).unwrap().overrides_arrangement);
    }

    #[test]
    fn a_scene_tempo_changes_the_tempo_at_the_launch() {
        let mut p = two_loops();
        p.launcher.scenes[1].tempo_bpm = Some(60.0);
        let commands = [(0.0, LaunchCommand::LaunchScene { scene: 0 }), (0.3, LaunchCommand::LaunchScene { scene: 1 })];
        let out = render(&p, &commands, 5.0);
        // From bar 2 a beat lasts a second: off-beats at 2.5 s and 3.5 s.
        let second = SR as usize;
        let off = second / 2;
        assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + second + off, BAR + 2 * second + off]);
    }

    #[test]
    fn a_clip_without_a_loop_plays_once_and_the_column_stops() {
        let p = project(vec![notes(1, vec![note(0.0, 60), note(1.0, 64)])], vec![vec![with(clip(1, 2.0, false))]]);
        let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
        let monitor = renderer.monitor();
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
        let mut out = vec![0.0f32; 2 * 3 * BEAT];
        renderer.process(&mut out);
        assert_onsets(&out, &[0, BEAT]);
        assert_eq!(monitor.column(0).unwrap().playing, None);
        // The transport keeps running.
        assert!(monitor.running());
    }

    #[test]
    fn notes_are_cut_at_the_loop_end() {
        let long = Note { duration: 4.0, ..note(0.0, 60) };
        let p = project(vec![notes(1, vec![long])], vec![vec![with(clip(1, 1.0, true))]]);
        let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
        let mut out = vec![0.0f32; 2 * (BEAT + 100)];
        renderer.process(&mut out);
        // Two voices: the first pass releasing and the second pass sounding.
        let sounding = renderer.voices.iter().filter(|v| v.active).count();
        assert_eq!(sounding, 2);
        assert!(renderer.voices.iter().filter(|v| v.active).any(|v| v.end <= BEAT as u64 + 1));
    }

    #[test]
    fn a_launched_clip_takes_its_track_over_from_the_arrangement() {
        let mut p = two_loops();
        let looped = Clip { length: 32.0, ..clip(1, 1.0, true) };
        p.arrangement.lanes.push(ArrangementLane { name: String::new(), track: Some(TrackId(1)), clips: vec![looped] });
        let mut renderer = Renderer::with_audio(&p, &AudioPool::new(), SR).unwrap();
        // The arrangement plays every beat of bar 1, then the off-beat clip takes over at bar 2.
        let mut out = vec![0.0f32; 2 * (BAR + 2 * BEAT)];
        let (first, rest) = out.split_at_mut(2 * (BEAT / 4));
        renderer.process(first);
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
        renderer.process(rest);
        let off = BEAT / 2;
        assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off]);

        // Stopping the clip (at the next bar) leaves the track silent until it goes back to the
        // arrangement.
        renderer.handle(LaunchCommand::StopColumn { column: 0 });
        let mut out = vec![0.0f32; 2 * 2 * BAR];
        renderer.process(&mut out);
        assert_eq!(onsets(&out).len(), 2, "{:?}", onsets(&out));
        renderer.handle(LaunchCommand::BackToArrangement);
        let mut out = vec![0.0f32; 2 * BAR];
        renderer.process(&mut out);
        assert_eq!(onsets(&out).len(), 4);
    }

    #[test]
    fn commands_from_a_controller_take_the_same_path() {
        let p = two_loops();
        let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
        let mut controller = renderer.controller();
        controller.send(LaunchCommand::LaunchClip { column: 0, slot: 0 }).unwrap();
        let mut out = vec![0.0f32; 2 * 2 * BEAT];
        for block in out.chunks_mut(2 * 128) {
            renderer.process(block);
        }
        assert_onsets(&out, &[0, BEAT]);
        let monitor = controller.monitor();
        assert!(monitor.running());
        assert!((monitor.beat() - 2.0).abs() < 0.01, "{}", monitor.beat());
        assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    }

    #[test]
    fn launched_audio_loops_from_its_start_marker() {
        // A one-second ramp: the sample value tells the file position in seconds.
        let samples = (0..SR).map(|i| i as f32 / SR as f32).collect();
        let mut audio = AudioPool::new();
        audio.files.insert("ramp.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
        // Warped at the file's own speed, so it is read directly rather than stretched.
        let region = AudioRegion {
            file: "ramp.wav".into(),
            absolute_file: None,
            sample_rate: Some(SR),
            channels: Some(1),
            duration_seconds: Some(1.0),
            warped: true,
            stretch: StretchMode::Smooth,
            warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 2.0, audio_time: 1.0 }],
            gain: 1.0,
            transpose: 0.0,
        };
        let content = ClipContent {
            id: ContentId(1),
            name: "ramp".into(),
            length: 2.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Extensions::default(),
        };
        // Loop beats 0..1 (0..0.5 s of the file), starting at beat 0.5.
        let c = Clip { offset: 0.5, ..clip(1, 1.0, true) };
        let p = project(vec![content], vec![vec![with(c)]]);
        let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];
        let out = render_launches(&p, &audio, SR, &launch, 1.0).unwrap();
        // A centred track at unity gain: the left sample is the ramp value.
        let left = |frame: usize| out[2 * frame];
        assert!((left(BEAT / 4) - 0.375).abs() < 0.01, "{}", left(BEAT / 4));
        // After half a beat the loop wraps to its start.
        assert!((left(BEAT / 2 + BEAT / 4) - 0.125).abs() < 0.01, "{}", left(BEAT / 2 + BEAT / 4));
        assert!((left(BEAT + BEAT / 2 + BEAT / 4) - 0.125).abs() < 0.01);
    }

    /// A project whose only slot loops a 440 Hz sine warped to play at 0.9 times its speed.
    fn stretched_sine() -> (Project, AudioPool) {
        let samples = (0..2 * SR as usize)
            .map(|i| (std::f64::consts::TAU * 440.0 * i as f64 / f64::from(SR)).sin() as f32 * 0.5)
            .collect();
        let mut audio = AudioPool::new();
        audio.files.insert("sine.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
        let region = AudioRegion {
            file: "sine.wav".into(),
            absolute_file: None,
            sample_rate: Some(SR),
            channels: Some(1),
            duration_seconds: Some(2.0),
            warped: true,
            stretch: StretchMode::Smooth,
            // Two beats (1 s at 120 bpm) play 0.9 s of the file.
            warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 2.0, audio_time: 0.9 }],
            gain: 1.0,
            transpose: 0.0,
        };
        let content = ClipContent {
            id: ContentId(1),
            name: "sine".into(),
            length: 2.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Extensions::default(),
        };
        (project(vec![content], vec![vec![with(clip(1, 2.0, true))]]), audio)
    }

    /// Upward zero crossings per second in the left channel of `out[from..to]` (frames).
    fn hz(out: &[f32], from: usize, to: usize) -> f64 {
        let left: Vec<f32> = out[2 * from..2 * to].iter().step_by(2).copied().collect();
        let crossings = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f64 * f64::from(SR) / (to - from) as f64
    }

    #[test]
    fn launched_warped_clips_are_stretched_and_keep_their_pitch() {
        let (p, audio) = stretched_sine();
        let launches = [
            (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
            (1.0, LaunchCommand::Stop),
            (1.5, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        ];
        let out = render_launches(&p, &audio, SR, &launches, 2.5).unwrap();
        // Read directly at 0.9 times its speed the sine would sound at 396 Hz.
        let first = hz(&out, SR as usize / 5, 4 * SR as usize / 5);
        assert!((first - 440.0).abs() < 8.0, "{first} Hz");
        // Relaunched after a stop, the rewound voice plays again from the start.
        let again = hz(&out, 17 * SR as usize / 10, 23 * SR as usize / 10);
        assert!((again - 440.0).abs() < 8.0, "{again} Hz");
        // Between the stop and the relaunch: silence once the fade is over.
        assert!(out[2 * (11 * SR as usize / 10)..2 * (14 * SR as usize / 10)].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn after_a_tempo_change_warped_clips_follow_by_re_pitching() {
        let (mut p, audio) = stretched_sine();
        p.launcher.scenes[0].tempo_bpm = Some(60.0);
        let launches = [(0.0, LaunchCommand::LaunchScene { scene: 0 })];
        let out = render_launches(&p, &audio, SR, &launches, 2.0).unwrap();
        // At half the tempo the direct reader plays the file at 0.9 x 0.5 = 0.45 times its speed:
        // 198 Hz. (Keeping the pitch at another tempo needs real-time retargeting, LOOP-4.)
        let pitch = hz(&out, SR as usize / 5, 9 * SR as usize / 5);
        assert!((pitch - 198.0).abs() < 6.0, "{pitch} Hz");
    }

    #[test]
    fn stretched_arrangement_clips_rewind_with_the_transport_and_give_way_to_launches() {
        let (mut p, audio) = stretched_sine();
        let looped = p.launcher.columns[0].slots[0].clip.clone().unwrap();
        p.arrangement.lanes.push(ArrangementLane {
            name: String::new(),
            track: Some(TrackId(1)),
            clips: vec![Clip { length: 16.0, ..looped }],
        });
        // A second scene with a silent clip, to take the track over without sound of its own.
        p.contents.push(notes(2, Vec::new()));
        p.launcher.scenes.push(Scene::default());
        p.launcher.columns[0].slots.push(with(clip(2, 1.0, true)));
        let mut renderer = Renderer::with_audio(&p, &audio, SR).unwrap();
        let half = SR as usize / 2;
        let mut first = vec![0.0f32; 2 * half];
        renderer.process(&mut first);
        assert!((hz(&first, half / 4, half) - 440.0).abs() < 10.0);

        // Stop and play again: the stretched clip starts over, sample for sample.
        renderer.handle(LaunchCommand::Stop);
        let mut gap = vec![0.0f32; 2 * 4800];
        renderer.process(&mut gap);
        renderer.handle(LaunchCommand::Play);
        let mut again = vec![0.0f32; 2 * half];
        renderer.process(&mut again);
        let diff = first.iter().zip(&again).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(diff < 1e-4, "restart differs by {diff}");

        // Launching the silent clip at the next bar takes the track over: silence from there.
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
        let mut rest = vec![0.0f32; 2 * 2 * SR as usize];
        renderer.process(&mut rest);
        let bar = BAR - half;
        assert!(rest[..2 * (bar - 10)].iter().any(|s| s.abs() > 0.1));
        assert!(rest[2 * bar..].iter().all(|s| *s == 0.0), "the arrangement clip still sounds");
    }

    #[test]
    fn hostile_commands_are_ignored() {
        let p = two_loops();
        let commands = [
            (f64::NAN, LaunchCommand::StopAll),
            (0.0, LaunchCommand::LaunchClip { column: 99, slot: 0 }),
            (0.0, LaunchCommand::LaunchClip { column: 0, slot: usize::MAX }),
            (0.0, LaunchCommand::LaunchScene { scene: 7 }),
            (0.0, LaunchCommand::StopColumn { column: usize::MAX }),
            (0.0, LaunchCommand::SetQuantization(Quantization::Beats(f64::NAN))),
            (0.1, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        ];
        let out = render(&p, &commands, 0.5);
        assert_eq!(onsets(&out).len(), 1);
        assert!(render_launches(&p, &AudioPool::new(), SR, &[], f64::INFINITY).is_err());
    }
}
