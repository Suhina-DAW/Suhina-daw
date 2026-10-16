// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[allow(dead_code)]
use std::path::PathBuf;

use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind, Parameter, ParameterControl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("midi-transpose.sd"));
    let mut graph = DeviceGraph::new("org.example.midi-transpose", "MIDI Transpose", DeviceRole::MidiEffect);
    let mut semitones = Parameter::new("semitones", "Semitones", (-24.0, 24.0), 12.0);
    semitones.control = ParameterControl::Integer;
    style_parameter(&mut semitones);
    graph.parameters.push(semitones);
    graph.nodes = vec![
        Node::new("midi", NodeKind::MidiInput),
        Node::new("transpose", NodeKind::MidiTransposeParameter { parameter: "semitones".into() }),
        Node::new("out", NodeKind::MidiOutput),
    ];
    for node in &mut graph.nodes {
        node.visible = false;
    }
    graph.connections = vec![Connection::new("midi", "transpose"), Connection::new("transpose", "out")];
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
