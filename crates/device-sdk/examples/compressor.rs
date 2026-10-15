//! Build the compressor effect as a self-contained .sd file.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::Connection as C;
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.compressor", "compressor", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    parameter(&mut g, "threshold", (-60.0, 0.0), -18.0, "dB", false);
    parameter(&mut g, "ratio", (1.0, 20.0), 4.0, "", false);
    parameter(&mut g, "attack", (0.0001, 1.0), 0.01, "seconds", true);
    parameter(&mut g, "release", (0.001, 2.0), 0.1, "seconds", true);
    g.nodes.extend([
        N::new("detector", K::Envelope { rms: false }),
        N::new("linked", K::Max),
        N::new("level", K::GainToDb),
        N::new("above", K::Subtract),
        N::new("positive", K::Max),
        N::new("zero", K::Constant { value: 0.0 }),
        N::new("one", K::Constant { value: 1.0 }),
        N::new("inverse", K::Divide),
        N::new("slope", K::Subtract),
        N::new("reduction", K::Multiply),
        N::new("gain", K::DbToGain),
        N::new("smooth", K::Smooth),
        N::new("apply", K::Multiply),
    ]);
    wire(&mut g, "in", "detector", "in");
    wire(&mut g, "attack", "detector", "attack");
    wire(&mut g, "release", "detector", "release");
    g.connections.extend([C::ports("detector", "left", "linked", "a"), C::ports("detector", "right", "linked", "b")]);
    wire(&mut g, "linked", "level", "in");
    wire(&mut g, "level", "above", "a");
    wire(&mut g, "threshold", "above", "b");
    wire(&mut g, "above", "positive", "a");
    wire(&mut g, "zero", "positive", "b");
    wire(&mut g, "one", "inverse", "a");
    wire(&mut g, "ratio", "inverse", "b");
    wire(&mut g, "inverse", "slope", "a");
    wire(&mut g, "one", "slope", "b");
    wire(&mut g, "positive", "reduction", "a");
    wire(&mut g, "slope", "reduction", "b");
    wire(&mut g, "reduction", "gain", "in");
    wire(&mut g, "gain", "smooth", "in");
    wire(&mut g, "in", "apply", "a");
    wire(&mut g, "smooth", "apply", "b");
    wire(&mut g, "apply", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "compressor")
}
