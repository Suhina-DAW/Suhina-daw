use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};
#[allow(dead_code, clippy::duplicate_mod)]
mod common;
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
        if control == Radio {
            p.max = 1.0;
        }
        if matches!(control, Choice | Radio) {
            p.ui.options = if control == Radio {
                vec!["Off".into(), "On".into()]
            } else {
                vec!["Off".into(), "Unity".into(), "Boost".into()]
            };
        }
        p.ui.position = Some([index as f32 * 104.0, 0.0]);
        p.ui.size = Some([100.0, 128.0]);
        common::style_parameter(&mut p);
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
