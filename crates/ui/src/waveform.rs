//! Waveforms of a project's audio files: the lowest and highest sample of every short stretch,
//! computed once when the files are decoded, so drawing a clip at any zoom reads only a few
//! numbers per pixel.

use std::collections::HashMap;
use std::sync::Arc;

use loopwerk_engine::{AudioData, AudioPool};

/// Frames summarised by one bucket.
pub const FRAMES_PER_BUCKET: usize = 256;

/// One file's waveform: the range of its samples (all channels together) per bucket.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Peaks {
    pub sample_rate: u32,
    /// (lowest, highest) sample in each bucket, -1 … 1 for a file that does not clip.
    pub buckets: Vec<(f32, f32)>,
}

/// Waveforms by the `file` string of the project's audio regions, as the audio pool keys them.
pub type Waveforms = HashMap<String, Arc<Peaks>>;

/// The waveforms of every file in `pool`.
pub fn from_pool(pool: &AudioPool) -> Waveforms {
    pool.files.iter().map(|(file, data)| (file.clone(), Arc::new(peaks(data)))).collect()
}

/// Summarises one decoded file.
pub fn peaks(data: &AudioData) -> Peaks {
    let channels = usize::from(data.channels.max(1));
    let buckets = data
        .samples
        .chunks(FRAMES_PER_BUCKET * channels)
        .map(|chunk| {
            chunk.iter().filter(|s| s.is_finite()).fold((0.0f32, 0.0f32), |(low, high), s| (low.min(*s), high.max(*s)))
        })
        .collect();
    Peaks { sample_rate: data.sample_rate, buckets }
}

impl Peaks {
    /// The range of the samples between two file positions in seconds; (0, 0) outside the file.
    pub fn range(&self, from: f64, to: f64) -> (f32, f32) {
        if self.sample_rate == 0 || self.buckets.is_empty() || !(from.is_finite() && to.is_finite()) {
            return (0.0, 0.0);
        }
        let bucket = |seconds: f64| seconds * f64::from(self.sample_rate) / FRAMES_PER_BUCKET as f64;
        let (a, b) = (bucket(from.min(to)), bucket(from.max(to)));
        if b < 0.0 || a >= self.buckets.len() as f64 {
            return (0.0, 0.0);
        }
        let first = a.max(0.0).floor() as usize;
        let last = (b.ceil() as usize).clamp(first + 1, self.buckets.len());
        self.buckets
            .get(first..last)
            .unwrap_or_default()
            .iter()
            .fold((0.0f32, 0.0f32), |(low, high), (l, h)| (low.min(*l), high.max(*h)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone() -> AudioData {
        // One second of a stereo square wave at full scale on the left, half on the right,
        // then one second of silence.
        let rate = 1000u32;
        let mut samples = Vec::new();
        for frame in 0..rate * 2 {
            let on = frame < rate;
            let sign = if (frame / 10) % 2 == 0 { 1.0 } else { -1.0 };
            samples.push(if on { sign } else { 0.0 });
            samples.push(if on { sign * 0.5 } else { 0.0 });
        }
        AudioData { sample_rate: rate, channels: 2, samples }
    }

    #[test]
    fn peaks_summarise_every_channel_per_bucket() {
        let peaks = peaks(&tone());
        assert_eq!(peaks.buckets.len(), 2000 / FRAMES_PER_BUCKET + 1);
        assert_eq!(peaks.range(0.0, 0.5), (-1.0, 1.0));
        assert_eq!(peaks.range(1.5, 1.9), (0.0, 0.0));
        assert_eq!(peaks.range(-5.0, -1.0), (0.0, 0.0));
        assert_eq!(peaks.range(10.0, 11.0), (0.0, 0.0));
        // A stretch shorter than a bucket reads the bucket it is in.
        assert_eq!(peaks.range(0.1, 0.1001), (-1.0, 1.0));
    }

    #[test]
    fn a_pool_gives_a_waveform_per_file() {
        let mut pool = AudioPool::new();
        pool.files.insert("loop.wav".into(), Arc::new(tone()));
        let waveforms = from_pool(&pool);
        assert_eq!(waveforms.len(), 1);
        assert!(waveforms.get("loop.wav").is_some_and(|p| p.sample_rate == 1000));
    }
}
