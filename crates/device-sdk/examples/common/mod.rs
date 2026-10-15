use suhina_device_sdk::{Connection as C, DeviceGraph, Node as N, NodeKind as K, Parameter, ParameterControl};

#[allow(dead_code)]
pub fn parameter(graph: &mut DeviceGraph, id: &str, range: (f64, f64), default: f64, unit: &str, logarithmic: bool) {
    let mut p = Parameter::new(id, id, range, default);
    p.unit = unit.into();
    if logarithmic {
        p.control = ParameterControl::LogSlider;
    }
    graph.parameters.push(p);
    graph.nodes.push(N::new(id, K::Parameter { parameter: id.into() }));
}
pub fn wire(graph: &mut DeviceGraph, from: &str, to: &str, port: &str) {
    graph.connections.push(C::ports(from, "out", to, port));
}

pub fn style_parameter(parameter: &mut Parameter) {
    parameter.ui.font_size = 11.0;
    parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
    let knob = matches!(parameter.control, ParameterControl::Knob | ParameterControl::LogKnob);
    parameter.ui.shape =
        if knob { suhina_device_sdk::ControlShape::Rounded } else { suhina_device_sdk::ControlShape::Square };
    parameter.ui.corner_radius = if knob { 5.0 } else { 0.0 };
}

#[allow(dead_code)]
pub fn explanation(lines: &[&str]) -> suhina_device_sdk::ui::DeviceUi {
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

pub fn finish(mut g: DeviceGraph) -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut visible_parameter = 0usize;
    // The examples teach their controls; their implementation graph does not need panel space.
    for node in &mut g.nodes {
        node.visible = false;
    }
    for p in &mut g.parameters {
        p.control = match p.id.as_str() {
            "cutoff" | "rate" | "time" => ParameterControl::LogKnob,
            "drive" | "feedback" | "width" => ParameterControl::Knob,
            "ratio" | "attack" | "release" => ParameterControl::Number,
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
pub fn save(graph: DeviceGraph, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(format!("target/sdk-examples/{name}.sd")));
    graph.write_file(&path)?;
    Ok(())
}
