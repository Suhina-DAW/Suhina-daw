// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const ADD: NodeInfo = NodeInfo {
    kind: "add",
    name: "Add",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "a + b. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"add\"}",
    rust_variant: "Add",
};

pub const SUBTRACT: NodeInfo = NodeInfo {
    kind: "subtract",
    name: "Subtract",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "a - b. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"subtract\"}",
    rust_variant: "Subtract",
};

pub const MULTIPLY: NodeInfo = NodeInfo {
    kind: "multiply",
    name: "Multiply",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "a * b. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"multiply\"}",
    rust_variant: "Multiply",
};

pub const DIVIDE: NodeInfo = NodeInfo {
    kind: "divide",
    name: "Divide",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "a / b; zero divisor gives zero. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"divide\"}",
    rust_variant: "Divide",
};

pub const MIN: NodeInfo = NodeInfo {
    kind: "min",
    name: "Min",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "minimum of a and b. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"min\"}",
    rust_variant: "Min",
};

pub const MAX: NodeInfo = NodeInfo {
    kind: "max",
    name: "Max",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "maximum of a and b. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"max\"}",
    rust_variant: "Max",
};

pub const POWER: NodeInfo = NodeInfo {
    kind: "power",
    name: "Power",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "a raised to b; invalid results give zero. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"power\"}",
    rust_variant: "Power",
};

pub const LESS: NodeInfo = NodeInfo {
    kind: "less",
    name: "Less",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "1 when a < b, otherwise 0. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"less\"}",
    rust_variant: "Less",
};

pub const GREATER: NodeInfo = NodeInfo {
    kind: "greater",
    name: "Greater",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "1 when a > b, otherwise 0. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"greater\"}",
    rust_variant: "Greater",
};

pub const ABS: NodeInfo = NodeInfo {
    kind: "abs",
    name: "Abs",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "absolute value. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"abs\"}",
    rust_variant: "Abs",
};

pub const NEGATE: NodeInfo = NodeInfo {
    kind: "negate",
    name: "Negate",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "negative value. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"negate\"}",
    rust_variant: "Negate",
};

pub const SQRT: NodeInfo = NodeInfo {
    kind: "sqrt",
    name: "Sqrt",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "square root of max(in, 0). Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"sqrt\"}",
    rust_variant: "Sqrt",
};

pub const EXP: NodeInfo = NodeInfo {
    kind: "exp",
    name: "Exp",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "exponential. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"exp\"}",
    rust_variant: "Exp",
};

pub const LOG: NodeInfo = NodeInfo {
    kind: "log",
    name: "Log",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "natural logarithm; nonpositive input gives zero. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"log\"}",
    rust_variant: "Log",
};

pub const FLOOR: NodeInfo = NodeInfo {
    kind: "floor",
    name: "Floor",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "round down. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"floor\"}",
    rust_variant: "Floor",
};

pub const FRACT: NodeInfo = NodeInfo {
    kind: "fract",
    name: "Fract",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "in - floor(in). Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"fract\"}",
    rust_variant: "Fract",
};

pub const SIN: NodeInfo = NodeInfo {
    kind: "sin",
    name: "Sin",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "sine in radians. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"sin\"}",
    rust_variant: "Sin",
};

pub const COS: NodeInfo = NodeInfo {
    kind: "cos",
    name: "Cos",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "cosine in radians. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"cos\"}",
    rust_variant: "Cos",
};

pub const TANH: NodeInfo = NodeInfo {
    kind: "tanh",
    name: "Tanh",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "hyperbolic tangent. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"tanh\"}",
    rust_variant: "Tanh",
};

pub const ATAN: NodeInfo = NodeInfo {
    kind: "atan",
    name: "Atan",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "arc tangent in radians. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"atan\"}",
    rust_variant: "Atan",
};

pub const DB_TO_GAIN: NodeInfo = NodeInfo {
    kind: "db_to_gain",
    name: "DbToGain",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "10^(in / 20), input in dB limited to -120..120. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"db_to_gain\"}",
    rust_variant: "DbToGain",
};

pub const GAIN_TO_DB: NodeInfo = NodeInfo {
    kind: "gain_to_db",
    name: "GainToDb",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "20*log10(abs(in)), floor -120 dB. Per channel, no state.",
    configuration: "none",
    example: "{\"kind\":\"gain_to_db\"}",
    rust_variant: "GainToDb",
};

pub const CLAMP: NodeInfo = NodeInfo {
    kind: "clamp",
    name: "Clamp",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "min", default: Some(-1.0), unit: "signal" },
        PortInfo { name: "max", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Limits input between min and max; reversed bounds are swapped.",
    configuration: "none",
    example: "{\"kind\":\"clamp\"}",
    rust_variant: "Clamp",
};

pub const MIX: NodeInfo = NodeInfo {
    kind: "mix",
    name: "Mix",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
        PortInfo { name: "amount", default: Some(0.5), unit: "fraction" },
    ],
    outputs: &["out", "left", "right"],
    description: "Linear a*(1-amount)+b*amount; amount limited to 0..1.",
    configuration: "none",
    example: "{\"kind\":\"mix\"}",
    rust_variant: "Mix",
};

pub const SELECT: NodeInfo = NodeInfo {
    kind: "select",
    name: "Select",
    inputs: &[
        PortInfo { name: "a", default: None, unit: "signal" },
        PortInfo { name: "b", default: None, unit: "signal" },
        PortInfo { name: "condition", default: Some(0.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Selects b when condition > 0, otherwise a; per channel.",
    configuration: "none",
    example: "{\"kind\":\"select\"}",
    rust_variant: "Select",
};
