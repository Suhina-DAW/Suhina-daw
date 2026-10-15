use std::path::PathBuf;

use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("midi-transpose.sd"));
    let mut graph = DeviceGraph::new("org.example.midi-transpose", "Octave Up", DeviceRole::MidiEffect);
    graph.nodes = vec![
        Node::new("midi", NodeKind::MidiInput),
        Node::new("transpose", NodeKind::MidiTranspose { semitones: 12 }),
        Node::new("out", NodeKind::MidiOutput),
    ];
    graph.connections = vec![Connection::new("midi", "transpose"), Connection::new("transpose", "out")];
    graph.write_file(&path)?;
    println!("Wrote {}", path.display());
    Ok(())
}
