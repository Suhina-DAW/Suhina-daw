// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build the stereo-utility effect as a self-contained .sd file.
use suhina_device_sdk::Connection as C;
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.stereo-utility", "stereo-utility", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    g.parameters.push(Parameter::new("width", "Width", (0.0, 2.0), 1.0));
    g.nodes.extend([
        N::new("encode", K::MidSideEncode),
        N::gain("width", "width"),
        N::new("stereo", K::Stereo),
        N::new("decode", K::MidSideDecode),
    ]);
    wire(&mut g, "in", "encode", "in");
    wire(&mut g, "encode", "width", "in");
    g.connections.push(C::ports("encode", "left", "stereo", "left"));
    g.connections.push(C::ports("width", "right", "stereo", "right"));
    wire(&mut g, "stereo", "decode", "in");
    wire(&mut g, "decode", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    save(build()?, "stereo-utility")
}

fn wire(graph: &mut suhina_device_sdk::DeviceGraph, from: &str, to: &str, port: &str) {
    graph.connections.push(suhina_device_sdk::Connection::ports(from, "out", to, port));
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

fn finish(mut g: suhina_device_sdk::DeviceGraph) -> Result<suhina_device_sdk::DeviceGraph, Box<dyn std::error::Error>> {
    let mut visible_parameter = 0usize;
    // The examples teach their controls; their implementation graph does not need panel space.
    for node in &mut g.nodes {
        node.visible = false;
    }
    for p in &mut g.parameters {
        p.control = match p.id.as_str() {
            "cutoff" | "rate" | "time" => suhina_device_sdk::ParameterControl::LogKnob,
            "drive" | "feedback" | "width" => suhina_device_sdk::ParameterControl::Knob,
            "ratio" | "attack" | "release" => suhina_device_sdk::ParameterControl::Number,
            _ => p.control,
        };
        style_parameter(p);
        p.ui.tooltip = format!("{} ({})", p.name, p.unit);
        if !g.ui.as_ref().is_some_and(|ui| ui.hidden_parameters.contains(&p.id)) {
            p.ui.position = Some([(visible_parameter % 2) as f32 * 104.0, (visible_parameter / 2) as f32 * 84.0]);
            p.ui.size = Some([100.0, 80.0]);
            visible_parameter += 1;
        }
    }
    g.validate()?;
    Ok(g)
}

#[cfg(not(test))]
fn save(graph: suhina_device_sdk::DeviceGraph, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(format!("target/sdk-examples/{name}.sd")));
    graph.write_file(&path)?;
    Ok(())
}
