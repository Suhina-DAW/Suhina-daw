//! A file name and an independently chosen device.name, with standard parameter controls.
#[allow(dead_code)]
use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut graph = DeviceGraph::new("org.example.custom-panel", "Velvet Volume", DeviceRole::Effect);
    let mut gain = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    gain.control = ParameterControl::Knob;
    graph.parameters.push(gain);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    graph.ui = Some(explanation(&[
        "File: custom-panel.sd",
        "Device name: Velvet Volume",
        "The title comes from device.name.",
        "Gain adjusts the volume.",
    ]));
    finish(graph)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    save(build()?, "custom-panel")
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
