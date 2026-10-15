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
    let knob = matches!(parameter.control, ParameterControl::Knob | ParameterControl::LogKnob);
    parameter.ui.shape =
        if knob { suhina_device_sdk::ControlShape::Rounded } else { suhina_device_sdk::ControlShape::Square };
    parameter.ui.corner_radius = if knob { 5.0 } else { 0.0 };
}

pub fn finish(mut g: DeviceGraph) -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    // Hide implementation details so the generated panel has room for the user controls.
    for node in &mut g.nodes {
        node.visible =
            matches!(node.id.as_str(), "encode" | "decode" | "pan" | "filter" | "shape" | "delay" | "detector");
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
