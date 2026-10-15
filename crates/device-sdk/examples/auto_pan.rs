//! Build the auto-pan effect as a self-contained .sd file.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K, Waveform};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.auto-pan", "auto-pan", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    parameter(&mut g, "rate", (0.01, 20.0), 1.0, "Hz", true);
    g.nodes.extend([N::new("lfo", K::Oscillator { shape: Waveform::Sine }), N::new("pan", K::Pan)]);
    wire(&mut g, "rate", "lfo", "frequency");
    wire(&mut g, "lfo", "pan", "balance");
    wire(&mut g, "in", "pan", "in");
    wire(&mut g, "pan", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "auto-pan")
}
