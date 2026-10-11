//! Parameter smoothing. A parameter that jumps from one block to the next makes an audible step
//! ("zipper noise"); a [`Smoother`] ramps it linearly to each new target instead.

/// Ramps a value linearly to its target over a fixed number of samples. The ramp ends exactly
/// on the target, so a settled smoother returns the target unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoother {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp: u32,
}

impl Smoother {
    /// A smoother that starts settled at `value` and ramps over `ramp` samples (at least one).
    pub fn new(value: f32, ramp: u32) -> Self {
        let value = if value.is_finite() { value } else { 0.0 };
        Self { current: value, target: value, step: 0.0, remaining: 0, ramp: ramp.max(1) }
    }

    /// Ramp length for `seconds` at `sample_rate`, in samples (at least one).
    pub fn ramp_frames(seconds: f64, sample_rate: f64) -> u32 {
        let frames = seconds * sample_rate;
        if frames.is_finite() && frames >= 1.0 { frames.min(f64::from(u32::MAX)) as u32 } else { 1 }
    }

    /// Changes the ramp length used by later targets.
    pub fn set_ramp(&mut self, ramp: u32) {
        self.ramp = ramp.max(1);
    }

    /// Starts a ramp from the current value to `target`. Non-finite targets are ignored.
    pub fn set_target(&mut self, target: f32) {
        if !target.is_finite() || target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp;
        self.step = (target - self.current) / self.ramp as f32;
    }

    /// Jumps to `value` without a ramp (after a reset, or before the first block).
    pub fn reset(&mut self, value: f32) {
        if value.is_finite() {
            *self = Self { current: value, target: value, step: 0.0, remaining: 0, ramp: self.ramp };
        }
    }

    /// Advances one sample and returns the value for it.
    #[inline]
    pub fn next_value(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 { self.target } else { self.current + self.step };
        }
        self.current
    }

    /// The value the last [`next_value`](Self::next_value) returned.
    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether the value has reached its target.
    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_linearly_and_lands_on_the_target() {
        let mut s = Smoother::new(0.0, 4);
        s.set_target(1.0);
        let values: Vec<f32> = (0..6).map(|_| s.next_value()).collect();
        assert_eq!(values, [0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
        assert!(s.is_settled());
    }

    #[test]
    fn a_new_target_mid_ramp_starts_from_where_it_is() {
        let mut s = Smoother::new(0.0, 4);
        s.set_target(1.0);
        s.next_value();
        s.next_value();
        s.set_target(0.0);
        let values: Vec<f32> = (0..4).map(|_| s.next_value()).collect();
        assert_eq!(values, [0.375, 0.25, 0.125, 0.0]);
    }

    #[test]
    fn ignores_hostile_values() {
        let mut s = Smoother::new(f32::NAN, 0);
        assert_eq!(s.next_value(), 0.0);
        s.set_target(f32::INFINITY);
        assert!(s.is_settled());
        s.reset(f32::NAN);
        assert_eq!(s.current(), 0.0);
        assert_eq!(Smoother::ramp_frames(f64::NAN, 48_000.0), 1);
        assert_eq!(Smoother::ramp_frames(0.02, 48_000.0), 960);
        assert_eq!(Smoother::ramp_frames(1e300, 48_000.0), u32::MAX);
    }
}
