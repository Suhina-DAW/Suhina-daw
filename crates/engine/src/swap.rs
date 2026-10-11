//! Edits reaching a playing renderer (ADR 0005).
//!
//! [`Controller::update`] builds a new [`Graph`] for the edited project on the calling thread,
//! picks a frame a little ahead of the playhead, moves the graph's stretch voices there and sends
//! the graph to the audio thread. [`Renderer::process`] splits its block at that frame, carries
//! its state over to the new graph and sends the old one back, so nothing is allocated or freed
//! on the audio thread.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use loopwerk_model::Project;
use rtrb::{Consumer, Producer, RingBuffer};

use crate::ahead::StretchThread;
use crate::audio::AudioPool;
use crate::launcher::Controller;
use crate::{Graph, MAX_AUDIO_VOICES, Renderer};

/// How far ahead of the playhead a new graph takes over, in seconds: time to move its stretch
/// voices and fill their rings before the audio thread reaches it.
const SWAP_AHEAD_SECONDS: f64 = 0.15;
/// Graphs that can wait in either direction.
const QUEUE: usize = 4;

/// A graph and the renderer frame it takes over at.
pub(crate) struct Swap {
    graph: Box<Graph>,
    at: u64,
    /// The arrangement frame the graph's stretch voices were moved to for `at`, or `None` if the
    /// transport was stopped (they are at the start).
    arrangement: Option<u64>,
}

/// The renderer's end of the swap queues.
#[derive(Default)]
pub(crate) struct SwapState {
    incoming: Option<Consumer<Swap>>,
    retired: Option<Producer<Box<Graph>>>,
    pending: Option<Swap>,
    /// An old graph the controller has not taken back yet; no new graph is taken until it has.
    held: Option<Box<Graph>>,
}

/// The controller's end: builds graphs and takes old ones back to free them.
pub(crate) struct Updater {
    swaps: Producer<Swap>,
    retired: Consumer<Box<Graph>>,
    audio: Arc<AudioPool>,
    sample_rate: u32,
    stretch: Option<StretchThread>,
}

impl Updater {
    fn collect(&mut self) {
        while let Ok(graph) = self.retired.pop() {
            drop(graph);
        }
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.collect();
    }
}

impl Renderer {
    /// Connects a new pair of swap queues; the controller gets the other ends.
    pub(crate) fn updater(&mut self) -> Updater {
        let (swaps, incoming) = RingBuffer::new(QUEUE);
        let (retired_producer, retired) = RingBuffer::new(QUEUE);
        self.swap.incoming = Some(incoming);
        self.swap.retired = Some(retired_producer);
        Updater { swaps, retired, audio: Arc::clone(&self.audio), sample_rate: self.sample_rate, stretch: None }
    }

    /// Takes the next graph from the queue, once any old graph has gone back. Never blocks.
    pub(crate) fn receive_swap(&mut self) {
        if let Some(old) = self.swap.held.take()
            && let Some(retired) = self.swap.retired.as_mut()
            && let Err(rtrb::PushError::Full(old)) = retired.push(old)
        {
            self.swap.held = Some(old);
        }
        if self.swap.held.is_none()
            && self.swap.pending.is_none()
            && let Some(incoming) = self.swap.incoming.as_mut()
        {
            self.swap.pending = incoming.pop().ok();
        }
    }

    /// The frame the pending graph takes over at, if one is waiting.
    pub(crate) fn swap_at(&self) -> Option<u64> {
        self.swap.pending.as_ref().map(|s| s.at)
    }

    /// Swaps in the pending graph and carries the state over (ADR 0005). Never allocates.
    pub(crate) fn apply_swap(&mut self) {
        let Some(swap) = self.swap.pending.take() else { return };
        let old_beat_frames = self.graph.frames_per_beat;
        let old_time = self.time();
        let mut old = std::mem::replace(&mut self.graph, swap.graph);
        // The playhead stays on the same beat when the tempo changed.
        if self.running && old_beat_frames != self.graph.frames_per_beat {
            self.time_at_origin = rescale(self.time(), old_beat_frames, self.graph.frames_per_beat);
            self.origin = self.position;
        }
        self.graph.launcher.adopt(&mut old.launcher);
        let time = self.time();
        // Cursors into the new lists: what has started by now has been handled.
        self.next_event = self.graph.events.partition_point(|e| e.start < time);
        self.next_audio = self.graph.audio_events.partition_point(|a| a.event.start < time);
        self.active_audio_count = 0;
        if self.running {
            for (index, arranged) in self.graph.audio_events.iter_mut().enumerate().take(self.next_audio) {
                if arranged.event.end > time && self.active_audio_count < MAX_AUDIO_VOICES {
                    if let Some(slot) = self.active_audio.get_mut(self.active_audio_count) {
                        *slot = index;
                    }
                    self.active_audio_count += 1;
                    // An unwarped clip plays at its own speed, not the beat's: it carries on from
                    // the file position it was at, whatever the tempo now.
                    if !arranged.event.warped()
                        && let Some(before) = old
                            .audio_events
                            .iter()
                            .find(|a| a.key == arranged.key && a.event.start <= old_time && old_time < a.event.end)
                    {
                        arranged.event.continue_from(time, before.event.file_seconds(old_time));
                    }
                }
            }
        }
        // The new stretch voices were moved to where the playhead was expected.
        self.underruns_before += old.ahead.underruns();
        if self.running {
            if swap.arrangement != Some(time) {
                self.graph.ahead.move_to(time);
            }
            self.stretch_fresh = false;
        } else {
            if swap.arrangement.is_some() {
                self.graph.ahead.rewind();
            }
            self.stretch_fresh = true;
        }
        self.swap.held = Some(old);
        self.receive_swap();
    }

    /// Publishes the playhead for the controller.
    pub(crate) fn publish_position(&self) {
        let monitor = &self.graph.launcher.monitor;
        monitor.position.store(self.position, Ordering::Relaxed);
        monitor.arrangement.store(self.time(), Ordering::Relaxed);
        monitor.stretch_underruns.store(self.stretch_underruns(), Ordering::Relaxed);
        monitor.frames_per_beat.store(self.graph.frames_per_beat.to_bits(), Ordering::Relaxed);
    }
}

/// Arrangement frame `time` at `from` frames per beat, moved to the same beat at `to` frames per
/// beat. The controller and the audio thread both use this, so they agree to the frame.
fn rescale(time: u64, from: f64, to: f64) -> u64 {
    if from > 0.0 && to > 0.0 && from != to { (time as f64 / from * to).round() as u64 } else { time }
}

impl Controller {
    /// Makes the renderer play `project` from now on, keeping what is playing (ADR 0005): builds
    /// it on this thread and swaps it in about [`SWAP_AHEAD_SECONDS`] from now. Fails if the
    /// project cannot be rendered or the renderer has not taken the previous updates yet.
    pub fn update(&mut self, project: &Project) -> Result<(), String> {
        let updater = &mut self.updater;
        updater.collect();
        let mut graph = Graph::build(project, &updater.audio, updater.sample_rate).map_err(|e| e.to_string())?;
        let monitor = &self.monitor;
        let (at, arrangement) = if monitor.running() {
            let ahead = (SWAP_AHEAD_SECONDS * f64::from(updater.sample_rate)) as u64;
            let position = monitor.position.load(Ordering::Relaxed);
            let arrangement = monitor.arrangement.load(Ordering::Relaxed);
            // Arrangement frames are in the playing graph's tempo; the swap keeps the beat, so
            // the new graph's voices go to the same beat in its own tempo.
            let playing = f64::from_bits(monitor.frames_per_beat.load(Ordering::Relaxed));
            (position + ahead, Some(rescale(arrangement + ahead, playing, graph.frames_per_beat)))
        } else {
            (0, None)
        };
        if let Some(arrangement) = arrangement {
            graph.ahead.move_to(arrangement);
        }
        if let Some(thread) = updater.stretch.as_mut()
            && let Some(worker) = graph.ahead.take_worker()
        {
            thread.add(worker)?;
        }
        updater
            .swaps
            .push(Swap { graph, at, arrangement })
            .map_err(|_| "the audio engine has not taken the previous edits yet".to_string())
    }

    /// Runs the renderer's stretch worker on a thread of its own (ADR 0003), for this graph and
    /// every graph [`Controller::update`] sends later. Call it before the renderer goes to the
    /// audio thread; the thread stops when the controller is dropped.
    pub fn run_stretch_worker(&mut self, renderer: &mut Renderer) -> std::io::Result<()> {
        let mut thread = StretchThread::spawn()?;
        if let Some(worker) = renderer.take_stretch_worker() {
            thread.add(worker).map_err(std::io::Error::other)?;
        }
        self.updater.stretch = Some(thread);
        Ok(())
    }

    /// Frames of stretched clips that were not ready in time and played as silence.
    pub fn stretch_underruns(&self) -> u64 {
        self.monitor.stretch_underruns.load(Ordering::Relaxed)
    }
}
