use super::{NodeInfo, PortInfo};

pub const TEMPO: NodeInfo = NodeInfo {
    kind: "tempo",
    name: "Tempo",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts current transport tempo in beats/minute, limited to 1..1000.",
    configuration: "none",
    example: "{\"kind\":\"tempo\"}",
    rust_variant: "Tempo",
};

pub const BEAT: NodeInfo = NodeInfo {
    kind: "beat",
    name: "Beat",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts transport beat, advanced each frame while playing.",
    configuration: "none",
    example: "{\"kind\":\"beat\"}",
    rust_variant: "Beat",
};

pub const PLAYING: NodeInfo = NodeInfo {
    kind: "playing",
    name: "Playing",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts transport running flag as 0 or 1.",
    configuration: "none",
    example: "{\"kind\":\"playing\"}",
    rust_variant: "Playing",
};

pub const LFO_RATE: NodeInfo = NodeInfo {
    kind: "lfo_rate",
    name: "LFO Rate",
    inputs: &[
        PortInfo { name: "rate", default: Some(0.5), unit: "Hz" },
        PortInfo { name: "time", default: Some(2000.0), unit: "ms" },
        PortInfo { name: "division", default: Some(4.0), unit: "divisor or sixteenth count" },
        PortInfo { name: "mode", default: Some(0.0), unit: "0 Rate, 1 Time, 2 Synced, 3 Triplet, 4 Dotted, 5 16th" },
        PortInfo { name: "tempo", default: Some(120.0), unit: "BPM" },
    ],
    outputs: &["out", "left", "right"],
    description: "Stateless Hz conversion. Clamps rate to 0.01..20, time to 1..20000 ms, rounded division to 1..64, rounded mode to 0..5 and tempo to 1..1000 BPM. Modes output rate, 1000/time, tempo*division/240, tempo*division/160, tempo*division/360 or tempo/(15*division). Broadcasts mono; no reset state.",
    configuration: "none",
    example: "{\"kind\":\"lfo_rate\"}",
    rust_variant: "LfoRate",
};
