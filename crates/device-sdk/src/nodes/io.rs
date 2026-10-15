use super::{NodeInfo, PortInfo};

pub const AUDIO_INPUT: NodeInfo = NodeInfo {
    kind: "audio_input",
    name: "Audio Input",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Stereo input of the track; no state.",
    configuration: "none",
    example: "{\"kind\":\"audio_input\"}",
    rust_variant: "AudioInput",
};

pub const AUDIO_OUTPUT: NodeInfo = NodeInfo {
    kind: "audio_output",
    name: "Audio Output",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Final stereo output; no state.",
    configuration: "none",
    example: "{\"kind\":\"audio_output\"}",
    rust_variant: "AudioOutput",
};

pub const GAIN: NodeInfo = NodeInfo {
    kind: "gain",
    name: "Gain",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Multiplies by the declared parameter in linear amplitude; legacy gain node.",
    configuration: " parameter: String",
    example: "{\"kind\":\"gain\",\"parameter\":\"amount\"}",
    rust_variant: "Gain",
};

pub const CONSTANT: NodeInfo = NodeInfo {
    kind: "constant",
    name: "Constant",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts finite configuration value to both channels.",
    configuration: " value: f64",
    example: "{\"kind\":\"constant\",\"value\":0.5}",
    rust_variant: "Constant",
};

pub const PARAMETER: NodeInfo = NodeInfo {
    kind: "parameter",
    name: "Parameter",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Broadcasts the current declared parameter value; values use their declared unit.",
    configuration: " parameter: String",
    example: "{\"kind\":\"parameter\",\"parameter\":\"amount\"}",
    rust_variant: "Parameter",
};
