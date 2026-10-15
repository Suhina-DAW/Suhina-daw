#[allow(dead_code)]
mod common;
use std::path::PathBuf;

use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind, Parameter, ParameterControl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("midi-transpose.sd"));
    let mut graph = DeviceGraph::new("org.example.midi-transpose", "MIDI Transpose", DeviceRole::MidiEffect);
    let mut semitones = Parameter::new("semitones", "Semitones", (-24.0, 24.0), 12.0);
    semitones.control = ParameterControl::Integer;
    common::style_parameter(&mut semitones);
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
