//! What the host can run. Until plug-in formats land, only test processors, named `test:…`:
//!
//! - `test:gain`: multiplies by a gain (0.5 at first); its state is the gain as 4 bytes,
//!   little-endian `f32`.
//! - `test:crash-after:<n>`: passes audio through, then exits the process in the middle of
//!   block `n` (counting from 0) without answering.
//! - `test:hang-after:<n>`: passes audio through, then stops answering in block `n`.
//! - `test:slow-load:<ms>`: `test:gain` that takes `ms` milliseconds (at most 60 s) to load, like
//!   a large plug-in.
//! - `test:slow-start:<n>`: `test:gain` whose first `n` blocks take 30 ms each, like a plug-in
//!   that has just started (or been restarted) and is still settling.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Something that processes blocks. Shared between the control thread (state) and the audio
/// thread (process), so its state lives in atomics.
pub(crate) trait Processor: Send + Sync {
    fn process(&self, input: &[f32], output: &mut [f32]);
    fn latency(&self) -> u32 {
        0
    }
    fn state(&self) -> Vec<u8>;
    fn set_state(&self, data: &[u8]) -> Result<(), String>;
}

/// A test processor as an instance's audio processing.
pub(crate) struct TestBlock(pub(crate) std::sync::Arc<dyn Processor>);

impl crate::audio::Block for TestBlock {
    fn process(&mut self, input: &[f32], _events: &[suhina_ipc::Event], output: &mut [f32]) {
        self.0.process(input, output);
    }
}

pub(crate) fn create(plugin: &str) -> Result<std::sync::Arc<dyn Processor>, String> {
    let after = |prefix: &str| plugin.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok());
    if plugin == "test:gain" {
        Ok(std::sync::Arc::new(Gain(AtomicU32::new(0.5f32.to_bits()))))
    } else if let Some(n) = after("test:crash-after:") {
        Ok(std::sync::Arc::new(Failing { after: n, blocks: AtomicU32::new(0), hang: false }))
    } else if let Some(ms) = after("test:slow-load:") {
        std::thread::sleep(Duration::from_millis(u64::from(ms.min(60_000))));
        Ok(std::sync::Arc::new(Gain(AtomicU32::new(0.5f32.to_bits()))))
    } else if let Some(n) = after("test:slow-start:") {
        Ok(std::sync::Arc::new(SlowStart { slow: n, blocks: AtomicU32::new(0) }))
    } else if let Some(n) = after("test:hang-after:") {
        Ok(std::sync::Arc::new(Failing { after: n, blocks: AtomicU32::new(0), hang: true }))
    } else {
        Err(format!("unknown plug-in {plugin:?}"))
    }
}

struct Gain(AtomicU32);

impl Processor for Gain {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        let gain = f32::from_bits(self.0.load(Ordering::Relaxed));
        for (o, i) in output.iter_mut().zip(input) {
            *o = i * gain;
        }
    }

    fn state(&self) -> Vec<u8> {
        f32::from_bits(self.0.load(Ordering::Relaxed)).to_le_bytes().to_vec()
    }

    fn set_state(&self, data: &[u8]) -> Result<(), String> {
        let bytes: [u8; 4] = data.try_into().map_err(|_| format!("gain state is 4 bytes, not {}", data.len()))?;
        let gain = f32::from_le_bytes(bytes);
        if !gain.is_finite() {
            return Err("gain is not a finite number".into());
        }
        self.0.store(gain.to_bits(), Ordering::Relaxed);
        Ok(())
    }
}

/// The time each of a `test:slow-start` plug-in's first blocks takes.
const SLOW_BLOCK: Duration = Duration::from_millis(30);

struct SlowStart {
    slow: u32,
    blocks: AtomicU32,
}

impl Processor for SlowStart {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        if self.blocks.fetch_add(1, Ordering::Relaxed) < self.slow {
            std::thread::sleep(SLOW_BLOCK);
        }
        for (o, i) in output.iter_mut().zip(input) {
            *o = i * 0.5;
        }
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

struct Failing {
    after: u32,
    blocks: AtomicU32,
    hang: bool,
}

impl Processor for Failing {
    fn process(&self, input: &[f32], output: &mut [f32]) {
        if self.blocks.fetch_add(1, Ordering::Relaxed) >= self.after {
            if !self.hang {
                std::process::exit(3);
            }
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        output.copy_from_slice(input);
    }

    fn state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn set_state(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}
