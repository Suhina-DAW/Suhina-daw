use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};
pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    use ParameterControl::*;
    let mut graph = DeviceGraph::new("org.example.ui-controls", "UI Controls", DeviceRole::Effect);
    graph.nodes.push(Node::audio_input("in"));
    let mut previous = String::from("in");
    for (index, control) in
        [Slider, LogSlider, Integer, Toggle, Knob, LogKnob, Number, VerticalSlider, Button, Choice, Radio]
            .into_iter()
            .enumerate()
    {
        let id = format!("{control:?}");
        let mut p = Parameter::new(&id, &id, (0.0, 2.0), 1.0);
        p.control = control;
        p.ui.tooltip = "Gain: 0 is silent, 1 is unity".into();
        if matches!(control, LogSlider | LogKnob) {
            p.min = 0.01;
        }
        if matches!(control, Toggle | Button) {
            p.max = 1.0;
        }
        if matches!(control, Choice | Radio) {
            p.ui.options = vec!["Off".into(), "Unity".into(), "Boost".into()];
        }
        p.ui.position = Some([0.0, index as f32 * 104.0]);
        p.ui.size = Some([220.0, 96.0]);
        p.ui.label_position = suhina_device_sdk::LabelPosition::Top;
        p.ui.accent = Some([103, 227, 189]);
        p.ui.background = Some([34, 44, 56]);
        p.ui.panel_color = Some([22, 29, 38]);
        p.ui.text_color = Some([220, 230, 238]);
        p.ui.font_size = 13.0;
        p.ui.monospace = false;
        p.ui.shape = suhina_device_sdk::ControlShape::Rounded;
        p.ui.corner_radius = 8.0;
        graph.parameters.push(p);
        let mut node = Node::gain(&id, &id);
        node.visible = false;
        graph.nodes.push(node);
        graph.connections.push(Connection::new(&previous, &id));
        previous = id;
    }
    let mut output = Node::audio_output("out");
    output.visible = false;
    graph.nodes[0].visible = false;
    graph.nodes.push(output);
    graph.connections.push(Connection::new(previous, "out"));
    graph.validate()?;
    Ok(graph)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).unwrap_or_else(|| "target/sdk-examples/ui-controls.sd".into());
    build()?.write_file(std::path::Path::new(&path))?;
    Ok(())
}
