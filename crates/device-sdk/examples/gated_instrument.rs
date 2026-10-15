#[allow(dead_code)]
mod common;
use std::path::PathBuf;

use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind, Parameter, ParameterControl, Waveform};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("gated-instrument.sd"));
    let mut graph = DeviceGraph::new("org.example.gated-instrument", "MIDI Sine", DeviceRole::Instrument);
    let mut volume = Parameter::new("volume", "Output", (0.0, 1.0), 0.8);
    volume.control = ParameterControl::Knob;
    common::style_parameter(&mut volume);
    graph.parameters.push(volume);
    graph.nodes = vec![
        Node::new("key", NodeKind::MidiKey),
        Node::new("velocity", NodeKind::MidiVelocity),
        Node::new("gate", NodeKind::MidiGate),
        Node::new("zero_key", NodeKind::Constant { value: 69.0 }),
        Node::new("semitones", NodeKind::Subtract),
        Node::new("twelve", NodeKind::Constant { value: 12.0 }),
        Node::new("octaves", NodeKind::Divide),
        Node::new("two", NodeKind::Constant { value: 2.0 }),
        Node::new("ratio", NodeKind::Power),
        Node::new("concert_a", NodeKind::Constant { value: 440.0 }),
        Node::new("frequency", NodeKind::Multiply),
        Node::new("osc", NodeKind::Oscillator { shape: Waveform::Sine }),
        Node::new("gated", NodeKind::Multiply),
        Node::new("level", NodeKind::Multiply),
        Node::new("volume", NodeKind::Parameter { parameter: "volume".into() }),
        Node::new("output", NodeKind::Multiply),
        Node::audio_output("out"),
    ];
    graph.connections = vec![
        Connection::ports("key", "out", "semitones", "a"),
        Connection::ports("zero_key", "out", "semitones", "b"),
        Connection::ports("semitones", "out", "octaves", "a"),
        Connection::ports("twelve", "out", "octaves", "b"),
        Connection::ports("two", "out", "ratio", "a"),
        Connection::ports("octaves", "out", "ratio", "b"),
        Connection::ports("concert_a", "out", "frequency", "a"),
        Connection::ports("ratio", "out", "frequency", "b"),
        Connection::ports("frequency", "out", "osc", "frequency"),
        Connection::ports("osc", "out", "gated", "a"),
        Connection::ports("gate", "out", "gated", "b"),
        Connection::ports("gated", "out", "level", "a"),
        Connection::ports("velocity", "out", "level", "b"),
        Connection::ports("level", "out", "output", "a"),
        Connection::ports("volume", "out", "output", "b"),
        Connection::new("output", "out"),
    ];
    graph.write_file(&path)?;
    println!("Wrote {}", path.display());
    Ok(())
}
