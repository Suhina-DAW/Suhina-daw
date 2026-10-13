//! Stretched clips rendered ahead of the audio thread (ADR 0003).
//!
//! A stretch voice costs little on average but unevenly: a restart before an onset or a stretcher
//! settling can take a large share of one audio block. So the voices of arrangement clips live on
//! a worker ([`StretchWorker`]) that keeps a lock-free ring per voice filled about
//! [`AHEAD_SECONDS`] ahead, and the audio thread ([`AheadReader`]) only copies finished frames.
//!
//! Timing never slips: the reader asks for a voice's frames by their position in the clip. If the
//! worker has fallen behind, the missing frames play as silence and are counted, and frames that
//! arrive late are dropped, so the clip carries on in time.
//!
//! Moving the voices (the transport stopping rewinds them; a new graph can start them anywhere)
//! is a lock-free handshake: the audio side requests a new generation with the arrangement frame
//! to move to, the worker stops pushing and says so, the audio side empties the rings, and the
//! worker moves its voices there and fills the rings again.
//!
//! Without a worker thread (offline rendering) the reader fills the rings itself just before it
//! reads them, through the same voices and rings, so offline renders stay the test oracle for
//! real-time playback (AGENTS.md rule 4).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::stretch::StretchVoice;

/// Seconds of stretched audio each voice keeps ready.
const AHEAD_SECONDS: f64 = 0.5;
/// Project frames a voice renders per call.
const BLOCK_FRAMES: usize = 1024;
/// How long the worker thread sleeps when every ring is full.
const IDLE: Duration = Duration::from_millis(1);

/// State both sides share.
#[derive(Default)]
struct Shared {
    /// Rewind generations: `requested` by the audio side, `paused` once the worker has stopped
    /// pushing for it, `drained` once the audio side has emptied every ring for it.
    requested: AtomicU64,
    paused: AtomicU64,
    drained: AtomicU64,
    /// The arrangement frame the voices move to for the requested generation.
    seek_to: AtomicU64,
    /// Frames the audio thread needed that were not ready.
    underruns: AtomicU64,
}

/// One voice on the worker side.
struct Writer {
    voice: StretchVoice,
    producer: Producer<f32>,
    /// The clip's first arrangement frame.
    start: u64,
    /// Frames of the clip pushed so far, and the clip's length in frames.
    produced: u64,
    length: u64,
}

/// One voice on the audio side.
struct Reader {
    consumer: Consumer<f32>,
    start: u64,
    length: u64,
    /// The clip frame at the head of the ring.
    next: u64,
    /// Rewound, and nothing has arrived since: the silence until the worker catches up is the
    /// jump's, not an underrun.
    rewound: bool,
}

/// Renders stretched clips ahead into the rings the [`AheadReader`] reads. A
/// [`crate::Controller`] runs it on its own thread (`Controller::run_stretch_worker`).
pub struct StretchWorker {
    writers: Vec<Writer>,
    shared: Arc<Shared>,
    generation: u64,
    scratch: Vec<f32>,
}

impl StretchWorker {
    /// Does one round of work: follows a rewind request, or renders one block for every voice
    /// whose ring has room. Returns whether it did anything.
    pub fn work(&mut self) -> bool {
        let requested = self.shared.requested.load(Ordering::Acquire);
        if requested != self.generation {
            // Stop pushing, wait for the audio side to empty the rings, then rewind.
            self.shared.paused.store(requested, Ordering::Release);
            if self.shared.drained.load(Ordering::Acquire) != requested {
                return false;
            }
            self.move_to(self.shared.seek_to.load(Ordering::Acquire));
            self.generation = requested;
            return true;
        }
        let mut did = false;
        for writer in &mut self.writers {
            let left = writer.length.saturating_sub(writer.produced);
            let frames = (BLOCK_FRAMES as u64).min(left) as usize;
            if frames == 0 || writer.producer.slots() < frames * 2 {
                continue;
            }
            let Some(block) = self.scratch.get_mut(..frames * 2) else { continue };
            writer.voice.render(block);
            if let Ok(mut chunk) = writer.producer.write_chunk(frames * 2) {
                let (first, second) = chunk.as_mut_slices();
                let (a, b) = block.split_at(first.len());
                first.copy_from_slice(a);
                second.copy_from_slice(b);
                chunk.commit_all();
            }
            writer.produced += frames as u64;
            did = true;
        }
        did
    }

    /// Works until every ring is full.
    pub fn fill(&mut self) {
        while self.work() {}
    }

    /// Moves every voice to arrangement frame `to`: the clips playing there to that point, the
    /// later ones to their start.
    fn move_to(&mut self, to: u64) {
        for writer in &mut self.writers {
            let local = to.saturating_sub(writer.start).min(writer.length);
            if local < writer.length {
                writer.voice.seek(local);
            }
            writer.produced = local;
        }
    }
}

/// Workers that can wait to join the thread.
const INBOX: usize = 8;

/// A thread running [`StretchWorker`]s, one per graph (ADR 0005): a worker joins through
/// [`StretchThread::add`] and leaves once its graph has been dropped. Dropping the handle stops
/// the thread.
pub(crate) struct StretchThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    inbox: Producer<StretchWorker>,
}

impl StretchThread {
    pub(crate) fn spawn() -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let (inbox, mut incoming) = RingBuffer::<StretchWorker>::new(INBOX);
        let handle = std::thread::Builder::new().name("loopwerk-stretch".into()).spawn(move || {
            let mut workers: Vec<StretchWorker> = Vec::new();
            while !stop_flag.load(Ordering::Relaxed) {
                while let Ok(worker) = incoming.pop() {
                    workers.push(worker);
                }
                // A worker whose reader (its graph) is gone has nobody to render for.
                workers.retain(|w| Arc::strong_count(&w.shared) > 1);
                let mut did = false;
                for worker in &mut workers {
                    did |= worker.work();
                }
                if !did {
                    std::thread::sleep(IDLE);
                }
            }
        })?;
        Ok(Self { stop, handle: Some(handle), inbox })
    }

    /// Hands a graph's worker to the thread.
    pub(crate) fn add(&mut self, worker: StretchWorker) -> Result<(), String> {
        self.inbox.push(worker).map_err(|_| "the stretch worker thread is not taking new work".to_string())
    }
}

impl Drop for StretchThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // A worker that panicked has nothing left to clean up.
            let _ = handle.join();
        }
    }
}

/// The audio thread's side: reads each voice's frames by their position in the clip.
pub(crate) struct AheadReader {
    readers: Vec<Reader>,
    shared: Arc<Shared>,
    /// The worker, while no thread has taken it: the reader then fills the rings itself.
    inline: Option<StretchWorker>,
}

impl AheadReader {
    /// Splits `voices` (each with its clip's start and length in arrangement frames) into a reader
    /// and its worker, with a ring per voice holding [`AHEAD_SECONDS`] at `sample_rate`.
    pub(crate) fn new(voices: Vec<(StretchVoice, u64, u64)>, sample_rate: u32) -> Self {
        let frames = ((AHEAD_SECONDS * f64::from(sample_rate)) as usize).max(2 * BLOCK_FRAMES);
        let shared = Arc::new(Shared::default());
        let mut readers = Vec::with_capacity(voices.len());
        let mut writers = Vec::with_capacity(voices.len());
        for (voice, start, length) in voices {
            let (producer, consumer) = RingBuffer::new(frames * 2);
            readers.push(Reader { consumer, start, length, next: 0, rewound: false });
            writers.push(Writer { voice, producer, start, produced: 0, length });
        }
        let worker =
            StretchWorker { writers, shared: Arc::clone(&shared), generation: 0, scratch: vec![0.0; BLOCK_FRAMES * 2] };
        Self { readers, shared, inline: Some(worker) }
    }

    /// Whether there are no stretched clips to read.
    pub(crate) fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }

    /// Hands the worker to a thread of its own, with every ring already full. `None` once taken.
    pub(crate) fn take_worker(&mut self) -> Option<StretchWorker> {
        let mut worker = self.inline.take()?;
        worker.fill();
        Some(worker)
    }

    /// Asks for the voices to continue from arrangement frame `to`; until the worker has moved
    /// them, reads are silence.
    pub(crate) fn move_to(&mut self, to: u64) {
        self.shared.seek_to.store(to, Ordering::Release);
        self.shared.requested.fetch_add(1, Ordering::AcqRel);
        self.service();
    }

    /// Moves a pending rewind along; call it before reading. Never blocks.
    pub(crate) fn service(&mut self) {
        let requested = self.shared.requested.load(Ordering::Acquire);
        if self.shared.drained.load(Ordering::Acquire) == requested {
            return;
        }
        if let Some(worker) = self.inline.as_mut() {
            // Nothing else pushes: the reader stands in for the worker's side of the handshake.
            worker.shared.paused.store(requested, Ordering::Release);
        }
        if self.shared.paused.load(Ordering::Acquire) != requested {
            return;
        }
        let to = self.shared.seek_to.load(Ordering::Acquire);
        for reader in &mut self.readers {
            let queued = reader.consumer.slots();
            if let Ok(chunk) = reader.consumer.read_chunk(queued) {
                chunk.commit_all();
            }
            reader.next = to.saturating_sub(reader.start).min(reader.length);
            reader.rewound = true;
        }
        self.shared.drained.store(requested, Ordering::Release);
        if let Some(worker) = self.inline.as_mut() {
            worker.work();
        }
    }

    /// Fills `out` (interleaved stereo) with voice `index`'s frames from clip frame `from` on.
    /// Frames that are not ready are silence and are counted as underruns, except while a rewind
    /// is under way or has delivered nothing yet (that silence is the jump's); never blocks or
    /// allocates.
    pub(crate) fn read(&mut self, index: usize, from: u64, out: &mut [f32]) {
        out.fill(0.0);
        let wanted = (out.len() / 2) as u64;
        if let Some(worker) = self.inline.as_mut() {
            worker.fill();
        }
        let ready = self.shared.drained.load(Ordering::Acquire) == self.shared.requested.load(Ordering::Acquire);
        let Some(reader) = self.readers.get_mut(index) else { return };
        // A rewind under way, or done with nothing rendered since: silence the transport asked
        // for, not frames the worker failed to deliver.
        if !ready {
            return;
        }
        if reader.rewound {
            if reader.consumer.slots() == 0 {
                return;
            }
            reader.rewound = false;
        }
        // Drop what was due before `from` (frames that arrived too late to play).
        if reader.next < from {
            let skip = ((from - reader.next) as usize).min(reader.consumer.slots() / 2);
            if let Ok(chunk) = reader.consumer.read_chunk(skip * 2) {
                chunk.commit_all();
            }
            reader.next += skip as u64;
        }
        if reader.next != from {
            self.shared.underruns.fetch_add(wanted, Ordering::Relaxed);
            return;
        }
        let frames = (wanted as usize).min(reader.consumer.slots() / 2);
        if let Ok(chunk) = reader.consumer.read_chunk(frames * 2) {
            let (first, second) = chunk.as_slices();
            if let Some(dst) = out.get_mut(..first.len()) {
                dst.copy_from_slice(first);
            }
            if let Some(dst) = out.get_mut(first.len()..first.len() + second.len()) {
                dst.copy_from_slice(second);
            }
            chunk.commit_all();
        }
        reader.next += frames as u64;
        if (frames as u64) < wanted {
            self.shared.underruns.fetch_add(wanted - frames as u64, Ordering::Relaxed);
        }
    }

    /// Frames needed that were not ready, since the renderer was built.
    pub(crate) fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }
}
