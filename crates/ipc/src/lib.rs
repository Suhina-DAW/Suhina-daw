//! Audio blocks between the engine and a plug-in host process (ADR 0006).
//!
//! The engine creates a [`Link`] (a shared-memory region and two doorbells) and starts the host
//! process with its [`Link::address`]; the host opens it with [`Served::open`]. Then, block by
//! block, [`Link::process`] copies the input into shared memory, rings the host and waits for
//! the output until a deadline; [`Served::serve`] waits for a block, lets the plug-ins fill the
//! output and rings back.
//!
//! One block is in flight at a time. If the host misses the deadline the engine gets
//! [`Outcome::Missed`], and until the host answers that block, [`Outcome::Busy`] at once,
//! without sending or waiting, so neither side touches the buffers while the other one owns
//! them. A host that crashes or hangs costs the engine the output of the blocks it misses,
//! never a crash or a wait past the deadline.
//!
//! This crate is one of the two allowed to use `unsafe` (ADR 0006): memory shared with another
//! process and waiting on it need it. Everything it exports is safe to use.

mod doorbell;
mod region;

use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use doorbell::Doorbell;
use region::Region;

/// Largest number of interleaved channels a link carries.
pub const MAX_CHANNELS: u32 = 64;
/// Largest block, in frames.
pub const MAX_FRAMES: u32 = 65_536;

#[derive(Debug)]
pub enum IpcError {
    /// Creating, opening or mapping the shared memory or a doorbell failed.
    Io(std::io::Error),
    /// The address does not name a link made by this version of Loopwerk.
    BadLink(&'static str),
    /// Channel count or block size out of range.
    BadSize { channels: u32, max_frames: u32 },
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "shared memory: {e}"),
            Self::BadLink(why) => write!(f, "not a usable link: {why}"),
            Self::BadSize { channels, max_frames } => {
                write!(f, "{channels} channels of {max_frames} frames is not supported")
            }
        }
    }
}

impl std::error::Error for IpcError {}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What happened to one block on the engine side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The host answered in time; the output holds its result.
    Done,
    /// The host did not answer before the deadline. The output is untouched.
    Missed,
    /// The host has not answered an earlier block yet, so this one was not sent. The output is
    /// untouched.
    Busy,
}

/// The engine's end of a link.
pub struct Link {
    region: Region,
    to_host: Doorbell,
    to_engine: Doorbell,
    /// The last block sent.
    sequence: u32,
}

impl Link {
    /// Creates a link for blocks of up to `max_frames` frames of `channels` interleaved samples.
    pub fn create(channels: u32, max_frames: u32) -> Result<Self, IpcError> {
        if !(1..=MAX_CHANNELS).contains(&channels) || !(1..=MAX_FRAMES).contains(&max_frames) {
            return Err(IpcError::BadSize { channels, max_frames });
        }
        let region = Region::create(channels, max_frames)?;
        let to_host = Doorbell::create(region.name(), "host")?;
        let to_engine = Doorbell::create(region.name(), "engine")?;
        Ok(Self { region, to_host, to_engine, sequence: 0 })
    }

    /// What the host process needs to open this link; pass it on its command line.
    pub fn address(&self) -> String {
        self.region.path().display().to_string()
    }

    pub fn channels(&self) -> u32 {
        self.region.channels()
    }

    pub fn max_frames(&self) -> u32 {
        self.region.max_frames()
    }

    /// Sends one block of interleaved `input` to the host and waits until `timeout` has passed
    /// for its `output`. The block is the shorter of the two, cut to whole frames and to the
    /// link's block size. Real-time safe: it does not allocate, and it returns within `timeout`.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], timeout: Duration) -> Outcome {
        let deadline = Instant::now() + timeout;
        if self.region.response() != self.sequence {
            return Outcome::Busy;
        }
        let channels = self.region.channels() as usize;
        let samples = input.len().min(output.len()).min(self.region.samples());
        let samples = samples - samples % channels;
        self.region.write_input(input.get(..samples).unwrap_or_default());
        self.region.set_frames((samples / channels) as u32);
        self.sequence = self.sequence.wrapping_add(1);
        self.region.set_request(self.sequence);
        self.to_host.ring(self.region.request_word());
        let sequence = self.sequence;
        if self.to_engine.wait(self.region.response_word(), |v| v == sequence, deadline) {
            self.region.read_output(output.get_mut(..samples).unwrap_or_default());
            Outcome::Done
        } else {
            Outcome::Missed
        }
    }
}

/// The host's end of a link.
pub struct Served {
    region: Region,
    to_host: Doorbell,
    to_engine: Doorbell,
    /// The last block answered.
    seen: u32,
}

impl Served {
    /// Opens the link at `address` ([`Link::address`]).
    pub fn open(address: &str) -> Result<Self, IpcError> {
        let region = Region::open(Path::new(address))?;
        let to_host = Doorbell::open(region.name(), "host")?;
        let to_engine = Doorbell::open(region.name(), "engine")?;
        let seen = region.response();
        Ok(Self { region, to_host, to_engine, seen })
    }

    pub fn channels(&self) -> u32 {
        self.region.channels()
    }

    pub fn max_frames(&self) -> u32 {
        self.region.max_frames()
    }

    /// Waits until `timeout` has passed for a block, runs `process(input, output)` on it
    /// (interleaved, the same length, the output zeroed) and answers. Returns whether a block
    /// was served.
    pub fn serve(&mut self, timeout: Duration, process: impl FnOnce(&[f32], &mut [f32])) -> bool {
        let deadline = Instant::now() + timeout;
        let seen = self.seen;
        if !self.to_host.wait(self.region.request_word(), |v| v != seen, deadline) {
            return false;
        }
        let request = self.region.request();
        let samples = self.region.frames() as usize * self.region.channels() as usize;
        self.region.with_block(samples, process);
        self.seen = request;
        self.region.set_response(request);
        self.to_engine.ring(self.region.response_word());
        true
    }
}

#[cfg(test)]
mod tests;
