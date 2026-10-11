//! The metronome: a short click on every beat while the transport runs, higher on the first beat
//! of a bar. It follows the launcher's beat clock, so it keeps time through tempo changes, scene
//! tempos and graph swaps. It is off until [`crate::LaunchCommand::SetMetronome`] turns it on.

/// Click pitch on an ordinary beat and on the first beat of a bar, in Hz.
const BEAT_HZ: f64 = 1_000.0;
const BAR_HZ: f64 = 1_600.0;
/// The click decays by this time constant and is cut after `LENGTH_SECONDS`.
const DECAY_SECONDS: f64 = 0.012;
const LENGTH_SECONDS: f64 = 0.05;
/// Fade-in, so the click starts without a step.
const ATTACK_SECONDS: f64 = 0.000_5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Metronome {
    on: bool,
    gain: f32,
    /// The beat whose click last started; `None` before the first one.
    last_beat: Option<i64>,
    /// Frames into the sounding click, and its phase step; `None` when silent.
    sounding: Option<(f64, f64)>,
    sample_rate: f64,
}

impl Metronome {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self { on: false, gain: 0.5, last_beat: None, sounding: None, sample_rate: f64::from(sample_rate) }
    }

    /// Turns the click on or off, at `gain` (linear; non-finite or negative is silence), or at
    /// the level it had.
    pub(crate) fn set(&mut self, on: bool, gain: Option<f32>) {
        self.on = on;
        if let Some(gain) = gain {
            self.gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
        }
    }

    pub(crate) fn on(&self) -> bool {
        self.on
    }

    /// Forgets the beat counted so far, so the next beat clicks (the transport started or stopped).
    pub(crate) fn reset(&mut self) {
        self.last_beat = None;
        self.sounding = None;
    }

    /// The click's sample for a frame at launcher beat `beat` (`bar_beats` beats to a bar).
    pub(crate) fn frame(&mut self, running: bool, beat: f64, bar_beats: f64) -> f32 {
        if running && beat.is_finite() {
            let index = beat.floor() as i64;
            if self.last_beat != Some(index) {
                self.last_beat = Some(index);
                if self.on {
                    let bar_start = bar_beats > 0.0 && (index as f64 / bar_beats).fract().abs() < 1e-9;
                    let hz = if bar_start { BAR_HZ } else { BEAT_HZ };
                    self.sounding = Some((0.0, hz / self.sample_rate));
                }
            }
        }
        let Some((age, step)) = self.sounding.as_mut() else { return 0.0 };
        let seconds = *age / self.sample_rate;
        if seconds >= LENGTH_SECONDS || !self.on {
            self.sounding = None;
            return 0.0;
        }
        let envelope = (seconds / ATTACK_SECONDS).min(1.0) * (-seconds / DECAY_SECONDS).exp();
        let sample = (std::f64::consts::TAU * *step * *age).sin() * envelope;
        *age += 1.0;
        sample as f32 * self.gain
    }
}
