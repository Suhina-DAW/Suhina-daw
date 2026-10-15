//! A gain graph with authored node positions, port anchors and wire bends.
#[allow(dead_code)]
mod common;
use suhina_device_sdk::{
    Connection, DeviceGraph, DeviceRole, Node, Parameter,
    editor::{Editor, GraphLayout, NodeLayout, WireLayout},
};

pub fn build() -> DeviceGraph {
    let mut graph = DeviceGraph::new("org.example.wire-layout", "Wire Layout", DeviceRole::Effect);
    let mut gain = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
    gain.control = suhina_device_sdk::ParameterControl::Knob;
    common::style_parameter(&mut gain);
    graph.parameters.push(gain);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
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
