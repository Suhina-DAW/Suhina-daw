//! Build the filter effect as a self-contained .sd file.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::{DeviceGraph, DeviceRole, FilterMode, Node as N, NodeKind as K};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.filter", "filter", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    parameter(&mut g, "cutoff", (20.0, 20_000.0), 1000.0, "Hz", true);
    g.nodes.push(N::new("filter", K::Filter { mode: FilterMode::Lowpass }));
    wire(&mut g, "in", "filter", "in");
    wire(&mut g, "cutoff", "filter", "frequency");
    wire(&mut g, "filter", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "filter")
}
