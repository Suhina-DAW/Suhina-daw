//! Checks every device must pass, whatever it does. Each device crate calls [`check`] from a
//! test; the checks cover the [`Device`] contract, not the device's sound. Instruments also go
//! through note checks: a fixed performance played in blocks of every size, `reset` while notes
//! sound, hostile events and every sample rate.

use crate::{Device, NoteEvent, ParamKind, ProcessContext, Role, controller};

/// Frames of test signal per check.
const FRAMES: usize = 8_192;
/// Largest difference two renders of the same input may have and still count as equal.
const TOLERANCE: f32 = 1e-6;

/// Runs every check on devices made by `make` and returns what failed (empty when all pass).
pub fn check(make: &dyn Fn() -> Box<dyn Device>) -> Vec<String> {
    let mut failures = Vec::new();
    check_info(make().as_ref(), &mut failures);
    check_params(make().as_mut(), &mut failures);
    check_hostile_values(make, &mut failures);
    check_block_sizes(make, &mut failures);
    check_reset(make, &mut failures);
    check_sample_rates(make, &mut failures);
    if make().info().role == Role::Instrument {
        check_notes(make, &mut failures);
    }
    failures
}

/// Interleaved stereo test signal: noise plus a low sine, different on each side, with a DC
/// offset, so filters, stereo processing and DC removal all have something to do.
pub fn test_signal(frames: usize) -> Vec<f32> {
    let mut seed = 0x2545_f491_u32;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let sine = (std::f32::consts::TAU * 110.0 * n as f32 / 48_000.0).sin();
        for side in [0.3f32, -0.2] {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            out.push(0.4 * sine * side.signum() + 0.3 * noise + 0.05 + side * 0.1);
        }
    }
    out
}

fn check_info(device: &dyn Device, failures: &mut Vec<String>) {
    let info = device.info();
    if info.id.is_empty() || info.name.is_empty() {
        failures.push("device id and name must not be empty".into());
    }
    for (i, p) in info.params.iter().enumerate() {
        if p.id.is_empty() || info.params.iter().take(i).any(|q| q.id == p.id) {
            failures.push(format!("parameter {i}: id {:?} is empty or used twice", p.id));
        }
        if p.min.partial_cmp(&p.max).is_none_or(|o| o.is_gt()) || p.clamp(p.default) != p.default {
            failures.push(format!("parameter {:?}: default {} is outside {}..{}", p.id, p.default, p.min, p.max));
        }
        if let ParamKind::Choice(options) = p.kind
            && options.is_empty()
        {
            failures.push(format!("parameter {:?}: a choice needs options", p.id));
        }
    }
}

fn check_params(device: &mut dyn Device, failures: &mut Vec<String>) {
    let params = device.info().params;
    for (i, p) in params.iter().enumerate() {
        if device.param(i) != Some(p.default) {
            failures.push(format!("parameter {:?} starts at {:?}, not its default", p.id, device.param(i)));
        }
        for value in [f64::NAN, f64::INFINITY, -1e300, 1e300, p.min, p.max, (p.min + p.max) / 2.0] {
            device.set_param(i, value);
            if device.param(i) != Some(p.clamp(value)) {
                failures.push(format!("parameter {:?} set to {value} reads back {:?}", p.id, device.param(i)));
            }
        }
    }
    device.set_param(params.len(), 1.0);
    device.set_param(usize::MAX, 1.0);
    if device.param(params.len()).is_some() {
        failures.push("an unknown parameter index returns a value".into());
    }
}

/// Settings that push every parameter to its ends, in turn, while the device runs.
fn extreme_settings(device: &mut dyn Device, step: usize) {
    for (i, p) in device.info().params.iter().enumerate() {
        let value = match (step + i) % 4 {
            0 => p.min,
            1 => p.max,
            2 => p.default,
            _ => f64::NAN,
        };
        device.set_param(i, value);
    }
}

fn check_hostile_values(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let mut device = make();
    let mut signal = test_signal(FRAMES);
    for (step, block) in signal.chunks_mut(512).enumerate() {
        extreme_settings(device.as_mut(), step);
        device.process(&ProcessContext::default(), block);
    }
    if let Some(n) = signal.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("extreme parameters produce {:?} at sample {n}", signal.get(n)));
    }
    let mut silly = [f32::NAN, f32::INFINITY, 1e30, -1e30, 0.5];
    device.process(&ProcessContext { tempo: f64::NAN, beat: -1e300, playing: true }, &mut silly);
    let mut after = test_signal(FRAMES);
    device.process(&ProcessContext::default(), &mut after);
    if after.iter().skip(FRAMES).any(|s| !s.is_finite()) {
        failures.push("non-finite input leaves the device producing non-finite output".into());
    }
    let mut odd = [0.1f32, 0.2, 0.7];
    device.reset();
    device.process(&ProcessContext::default(), &mut odd);
    if odd.get(2) != Some(&0.7) {
        failures.push("a trailing odd sample was changed".into());
    }
}

/// Renders the test signal through a fresh device in blocks of `sizes` (repeated), changing
/// parameters at fixed frames.
fn render(make: &dyn Fn() -> Box<dyn Device>, sizes: &[usize]) -> Vec<f32> {
    let mut device = make();
    let mut signal = test_signal(FRAMES);
    let tempo = 120.0;
    let mut frame = 0usize;
    for &size in sizes.iter().cycle() {
        if frame >= FRAMES || size == 0 {
            break;
        }
        let size = size.min(FRAMES - frame);
        // Every 1024 frames the parameters change; blocks are split there, as the host does.
        let size = size.min(1_024 - frame % 1_024);
        if frame.is_multiple_of(1_024) {
            extreme_settings(device.as_mut(), frame / 1_024);
        }
        let beat = frame as f64 / 48_000.0 * tempo / 60.0;
        if let Some(block) = signal.get_mut(frame * 2..(frame + size) * 2) {
            device.process(&ProcessContext { tempo, beat, playing: true }, block);
        }
        frame += size;
    }
    signal
}

fn largest_difference(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn check_block_sizes(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let reference = render(make, &[512]);
    for sizes in [&[1][..], &[7, 64, 333], &[1_024], &[3, 1, 250, 900]] {
        let other = render(make, sizes);
        let diff = largest_difference(&reference, &other);
        if diff > TOLERANCE {
            failures.push(format!("blocks of {sizes:?} differ from blocks of 512 by {diff}"));
        }
    }
}

fn check_reset(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let fresh = {
        let mut device = make();
        let mut signal = test_signal(FRAMES);
        device.process(&ProcessContext::default(), &mut signal);
        signal
    };
    let mut device = make();
    let mut warmup = test_signal(FRAMES);
    warmup.reverse();
    device.process(&ProcessContext::default(), &mut warmup);
    device.reset();
    let mut signal = test_signal(FRAMES);
    device.process(&ProcessContext::default(), &mut signal);
    let diff = largest_difference(&fresh, &signal);
    if diff > TOLERANCE {
        failures.push(format!("after reset the output differs from a fresh device by {diff}"));
    }
}

fn check_sample_rates(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let mut device = make();
    for sr in [8_000.0, 22_050.0, 44_100.0, 96_000.0, 192_000.0, 384_000.0] {
        if let Err(e) = device.prepare(sr) {
            failures.push(format!("prepare({sr}) failed: {e}"));
        }
        let mut signal = test_signal(1_024);
        device.process(&ProcessContext::default(), &mut signal);
        if signal.iter().any(|s| !s.is_finite()) {
            failures.push(format!("non-finite output at {sr} Hz"));
        }
    }
    for bad in [0.0, -1.0, f64::NAN, 1e12] {
        if device.prepare(bad).is_ok() {
            failures.push(format!("prepare({bad}) succeeded"));
        }
    }
    let mut signal = test_signal(1_024);
    device.process(&ProcessContext::default(), &mut signal);
    if signal.iter().any(|s| !s.is_finite()) {
        failures.push("a rejected sample rate left the device broken".into());
    }
}

/// A short performance for instruments, as (frame, event): overlapping notes on and off frames
/// that fall inside blocks, the sustain pedal, the bend wheel, the modulation wheel and pressure.
pub fn test_notes() -> Vec<(usize, NoteEvent)> {
    use NoteEvent::*;
    vec![
        (0, On { key: 60, velocity: 0.8 }),
        (100, On { key: 64, velocity: 0.5 }),
        (517, On { key: 67, velocity: 1.0 }),
        (1_000, PitchBend(0.3)),
        (1_500, Controller { number: controller::MODULATION, value: 0.7 }),
        (2_000, Off { key: 60, velocity: 0.5 }),
        (2_600, Controller { number: controller::SUSTAIN, value: 1.0 }),
        (2_700, Off { key: 64, velocity: 0.5 }),
        (3_000, Pressure(0.6)),
        (3_500, Off { key: 67, velocity: 0.2 }),
        (4_100, Controller { number: controller::SUSTAIN, value: 0.0 }),
        (4_500, On { key: 48, velocity: 0.9 }),
        (4_501, On { key: 48, velocity: 0.9 }),
        (5_000, PitchBend(-1.0)),
        (6_000, AllNotesOff),
    ]
}

/// Plays `notes` on `device` into `frames` frames of silence, in blocks of `sizes` (repeated),
/// split where an event falls, as the host does.
fn play(device: &mut dyn Device, notes: &[(usize, NoteEvent)], sizes: &[usize], frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    let mut frame = 0usize;
    let mut next = 0usize;
    for &size in sizes.iter().cycle() {
        if frame >= frames || size == 0 {
            break;
        }
        while let Some((at, event)) = notes.get(next)
            && *at <= frame
        {
            device.event(*event);
            next += 1;
        }
        let until = notes.get(next).map_or(frames, |(at, _)| (*at).min(frames));
        let size = size.min(until - frame).max(1).min(frames - frame);
        if let Some(block) = out.get_mut(frame * 2..(frame + size) * 2) {
            device.process(&ProcessContext::default(), block);
        }
        frame += size;
    }
    out
}

fn check_notes(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let notes = test_notes();
    let reference = play(make().as_mut(), &notes, &[512], FRAMES);
    if !reference.iter().any(|s| s.abs() > 1e-3) {
        failures.push("the test notes make no sound".into());
    }
    if let Some(n) = reference.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("the test notes produce {:?} at sample {n}", reference.get(n)));
    }
    for sizes in [&[1][..], &[7, 64, 333], &[1_024], &[3, 1, 250, 900]] {
        let other = play(make().as_mut(), &notes, sizes, FRAMES);
        let diff = largest_difference(&reference, &other);
        if diff > TOLERANCE {
            failures.push(format!("notes in blocks of {sizes:?} differ from blocks of 512 by {diff}"));
        }
    }

    // Reset while notes sound, with the pedal down and the wheels moved, then play again.
    let mut device = make();
    let held = [
        (0, NoteEvent::Controller { number: controller::SUSTAIN, value: 1.0 }),
        (0, NoteEvent::PitchBend(0.8)),
        (0, NoteEvent::On { key: 30, velocity: 1.0 }),
        (10, NoteEvent::On { key: 90, velocity: 1.0 }),
    ];
    play(device.as_mut(), &held, &[256], 4_096);
    device.reset();
    let after = play(device.as_mut(), &notes, &[512], FRAMES);
    let diff = largest_difference(&reference, &after);
    if diff > TOLERANCE {
        failures.push(format!("after reset with notes sounding the output differs from a fresh device by {diff}"));
    }

    // Hostile events: keys out of range, numbers that are not finite, far more notes than
    // any polyphony.
    let mut hostile = Vec::new();
    for (i, value) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 5.0, 1e30, 0.5].into_iter().enumerate() {
        hostile.push((i, NoteEvent::On { key: 200, velocity: value }));
        hostile.push((i, NoteEvent::On { key: 61, velocity: value }));
        hostile.push((i, NoteEvent::Off { key: 255, velocity: value }));
        hostile.push((i, NoteEvent::PitchBend(value)));
        hostile.push((i, NoteEvent::Pressure(value)));
        for number in [0, controller::MODULATION, controller::SUSTAIN, controller::PORTAMENTO_TIME, 127, 255] {
            hostile.push((i, NoteEvent::Controller { number, value }));
        }
    }
    for round in 0..3 {
        for key in 0..=127u8 {
            hostile.push((10 + round * 200 + usize::from(key), NoteEvent::On { key, velocity: 1.0 }));
        }
    }
    let mut device = make();
    let loud = play(device.as_mut(), &hostile, &[512], FRAMES);
    if let Some(n) = loud.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("hostile events produce {:?} at sample {n}", loud.get(n)));
    }

    // At the default settings, every note ends after all notes are released.
    let mut device = make();
    play(device.as_mut(), &notes, &[512], FRAMES);
    device.event(NoteEvent::AllNotesOff);
    let mut released = vec![0.0f32; 480_000 * 2];
    for block in released.chunks_mut(1_024) {
        device.process(&ProcessContext::default(), block);
    }
    let end = released.len().saturating_sub(2_048);
    if released.iter().skip(end).any(|s| s.abs() > 1e-4) {
        failures.push("notes still sound ten seconds after all notes are released".into());
    }

    let mut device = make();
    for sr in [8_000.0, 22_050.0, 44_100.0, 96_000.0, 192_000.0, 384_000.0] {
        if device.prepare(sr).is_err() {
            continue;
        }
        let out = play(device.as_mut(), &notes, &[512], 2_048);
        if out.iter().any(|s| !s.is_finite() || s.abs() > 1e4) {
            failures.push(format!("notes at {sr} Hz produce non-finite or runaway output"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceError, DeviceInfo, ParamInfo, Role, Scale, Unit};

    const PARAMS: [ParamInfo; 1] = [ParamInfo::continuous("gain", "Gain", (0.0, 2.0), 1.0, Unit::None, Scale::Linear)];
    const INFO: DeviceInfo = DeviceInfo { id: "broken", name: "Broken", role: Role::Effect, params: &PARAMS };

    /// Breaks the contract in two ways: it applies a gain change once per block (so the output
    /// depends on the block size) and keeps state across `reset`.
    struct Broken {
        gain: f64,
        applied: f32,
        counter: f32,
    }

    impl Device for Broken {
        fn info(&self) -> &'static DeviceInfo {
            &INFO
        }
        fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
            crate::check_sample_rate(sample_rate).map(|_| ())
        }
        fn reset(&mut self) {}
        fn set_param(&mut self, index: usize, value: f64) {
            if index == 0 {
                self.gain = PARAMS[0].clamp(value);
            }
        }
        fn param(&self, index: usize) -> Option<f64> {
            (index == 0).then_some(self.gain)
        }
        fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
            self.applied += (self.gain as f32 - self.applied) * 0.5;
            for s in buffer.as_chunks_mut::<2>().0.iter_mut().flatten() {
                self.counter += 1e-3;
                *s *= self.applied + self.counter.sin() * 0.01;
            }
        }
    }

    #[test]
    fn catches_a_device_that_breaks_the_contract() {
        let failures = check(&|| Box::new(Broken { gain: 1.0, applied: 1.0, counter: 0.0 }));
        assert!(failures.iter().any(|f| f.contains("blocks of")), "{failures:#?}");
        assert!(failures.iter().any(|f| f.contains("after reset")), "{failures:#?}");
    }
}
