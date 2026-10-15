#[allow(dead_code)]
use std::path::PathBuf;

use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("gain.sd"));
    let mut graph = DeviceGraph::new("org.example.gain", "Example Gain", DeviceRole::Effect);
    let mut gain = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    gain.control = ParameterControl::Knob;
    style_parameter(&mut gain);
    graph.parameters.push(gain);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    for node in &mut graph.nodes {
        node.visible = false;
    }
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    graph.write_file(&path)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn style_parameter(parameter: &mut suhina_device_sdk::Parameter) {
    parameter.ui.font_size = 11.0;
    parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
    let knob = matches!(
        parameter.control,
        suhina_device_sdk::ParameterControl::Knob | suhina_device_sdk::ParameterControl::LogKnob
    );
    parameter.ui.shape =
        if knob { suhina_device_sdk::ControlShape::Rounded } else { suhina_device_sdk::ControlShape::Square };
    parameter.ui.corner_radius = if knob { 5.0 } else { 0.0 };
}
