//! A file name and an independently chosen device.name, with standard parameter controls.
#[allow(dead_code)]
mod common;
use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut graph = DeviceGraph::new("org.example.custom-panel", "Velvet Volume", DeviceRole::Effect);
    let mut gain = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    gain.control = ParameterControl::Knob;
    graph.parameters.push(gain);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    graph.ui = Some(common::explanation(&[
        "File: custom-panel.sd",
        "Device name: Velvet Volume",
        "The title comes from device.name.",
        "Gain adjusts the volume.",
    ]));
    common::finish(graph)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "custom-panel")
}
