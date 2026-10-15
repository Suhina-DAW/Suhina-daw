//! Filter and stereo controls composed from the SDK's built-in widgets.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::{DeviceGraph, DeviceRole, FilterMode, Node as N, NodeKind as K, ParameterControl};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.custom-panel", "Orbit Filter", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];
    parameter(&mut g, "cutoff", (20.0, 20_000.0), 2400.0, "Hz", true);
    parameter(&mut g, "balance", (-1.0, 1.0), 0.0, "", false);
    parameter(&mut g, "level", (0.0, 2.0), 1.0, "", false);
    if let Some(p) = g.parameters.iter_mut().find(|p| p.id == "balance") {
        p.name = "Stereo position".into();
        p.control = ParameterControl::Knob;
    }
    if let Some(p) = g.parameters.iter_mut().find(|p| p.id == "level") {
        p.name = "Output".into();
        p.control = ParameterControl::Knob;
    }
    g.parameters[0].name = "Cutoff".into();
    g.nodes.extend([
        N::new("filter", K::Filter { mode: FilterMode::Lowpass }),
        N::new("pan", K::Pan),
        N::gain("gain", "level"),
    ]);
    wire(&mut g, "in", "filter", "in");
    wire(&mut g, "cutoff", "filter", "frequency");
    wire(&mut g, "filter", "pan", "in");
    wire(&mut g, "balance", "pan", "balance");
    wire(&mut g, "pan", "gain", "in");
    wire(&mut g, "gain", "out", "in");
    g.nodes.retain(|node| node.id != "level");
    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "custom-panel")
}
