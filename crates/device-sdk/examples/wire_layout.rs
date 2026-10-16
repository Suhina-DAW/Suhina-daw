// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A gain graph with authored node positions, port anchors and wire bends.
#[allow(dead_code)]
use suhina_device_sdk::{
    Connection, DeviceGraph, DeviceRole, Node, Parameter,
    editor::{Editor, GraphLayout, NodeLayout, WireLayout},
};

pub fn build() -> DeviceGraph {
    let mut graph = DeviceGraph::new("org.example.wire-layout", "Wire Layout", DeviceRole::Effect);
    let mut gain = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    gain.control = suhina_device_sdk::ParameterControl::Knob;
    style_parameter(&mut gain);
    graph.parameters.push(gain);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    graph.ui = Some(explanation(&[
        "Volume: 0 silent, 1 unchanged, 2 louder.",
        "Demonstrates saved node positions",
        "and wire bends for graph editors.",
        "This panel shows the signal order.",
    ]));
    graph.editor = Some(Editor {
        graph: GraphLayout {
            nodes: ["in", "gain", "out"]
                .into_iter()
                .zip([0.0, 240.0, 480.0])
                .map(|(node, x)| NodeLayout { node: node.into(), position: [x, 0.0], size: [120.0, 80.0] })
                .collect(),
            wires: vec![
                WireLayout {
                    connection: Connection::new("in", "gain"),
                    points: vec![[180.0, 40.0], [180.0, 60.0]],
                    from_anchor: Some([120.0, 40.0]),
                    to_anchor: Some([0.0, 40.0]),
                },
                WireLayout::new(Connection::new("gain", "out")),
            ],
        },
    });
    graph
}

#[allow(dead_code)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).unwrap_or_else(|| "wire-layout.sd".into());
    build().write_file(std::path::Path::new(&path))?;
    println!("Wrote {}", std::path::Path::new(&path).display());
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

fn explanation(lines: &[&str]) -> suhina_device_sdk::ui::DeviceUi {
    use suhina_device_sdk::ui::{DeviceUi, Draw, TextAlign, TextFormat};
    DeviceUi {
        size: [208.0, lines.len() as f32 * 16.0 + 8.0],
        background: [0, 0, 0, 0],
        layers: lines
            .iter()
            .enumerate()
            .map(|(index, line)| Draw::Text {
                position: [4.0.into(), (index as f32 * 16.0 + 4.0).into()],
                text: (*line).into(),
                color: [140, 140, 140, 255],
                size: 11.0,
                parameter: None,
                decimals: 0,
                align: TextAlign::Left,
                format: TextFormat::Number,
            })
            .collect(),
        ..DeviceUi::default()
    }
}
