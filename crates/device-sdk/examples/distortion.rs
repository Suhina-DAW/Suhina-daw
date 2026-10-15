//! Build the distortion effect as a self-contained .sd file.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.distortion", "distortion", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    g.parameters.push(Parameter::new("drive", "Drive", (1.0, 20.0), 2.0));
    parameter(&mut g, "wet", (0.0, 1.0), 0.5, "", false);
    g.nodes.extend([N::gain("drive", "drive"), N::new("shape", K::Tanh), N::new("mix", K::Mix)]);
    wire(&mut g, "in", "drive", "in");
    wire(&mut g, "drive", "shape", "in");
    wire(&mut g, "in", "mix", "a");
    wire(&mut g, "shape", "mix", "b");
    wire(&mut g, "wet", "mix", "amount");
    wire(&mut g, "mix", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "distortion")
}
