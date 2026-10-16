// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stretches a WAV file, for listening tests:
//! `cargo run --release -p suhina-stretch --example stretch_wav -- in.wav out.wav <ratio> [semitones]`
//! A ratio of 1.2 makes it 20 % longer (120 bpm material played at 100 bpm).
#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: stretch_wav <in.wav> <out.wav> <length ratio> [semitones]";
    let (input, output) = (args.get(1).expect(usage), args.get(2).expect(usage));
    let ratio: f32 = args.get(3).expect(usage).parse().expect("ratio must be a number");
    let semitones: f32 = args.get(4).map_or(0.0, |s| s.parse().expect("semitones must be a number"));

    let mut reader = hound::WavReader::open(input).unwrap();
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
        }
    };
    let out_frames = (samples.len() / channels) as f32 * ratio;
    let mut out = vec![0.0f32; out_frames as usize * channels];
    let started = std::time::Instant::now();
    let mut stretch = suhina_stretch::Stretch::new(channels, spec.sample_rate as f32);
    stretch.set_transpose_semitones(semitones, 0.0);
    stretch.exact(&samples, &mut out);
    let seconds = out.len() as f64 / channels as f64 / f64::from(spec.sample_rate);
    println!("{seconds:.1} s stretched in {:.2} s", started.elapsed().as_secs_f64());

    let out_spec = hound::WavSpec { bits_per_sample: 32, sample_format: hound::SampleFormat::Float, ..spec };
    let mut writer = hound::WavWriter::create(output, out_spec).unwrap();
    for s in out {
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
}
