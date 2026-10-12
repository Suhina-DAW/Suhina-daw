//! The control messages between the engine and a plug-in host process (ADR 0006): one JSON
//! object per line over a local socket. Audio does not go here; it goes through `loopwerk-ipc`.
//!
//! Every message has an `op`. Engine to host: `hello`, `load`, `ping`, `get_state`,
//! `set_state`, `get_params`, `set_param`, `quit`. Host to engine: `hello`, `loaded`, `pong`,
//! `state`, `params`, `param`, `error`.

use serde_json::{Value, json};

/// Bumped whenever a message changes; both sides refuse a different version.
pub const VERSION: u64 = 3;
/// Longest line either side accepts; state blobs are hex, so this bounds them at 32 MiB.
pub const MAX_LINE: usize = 64 * 1024 * 1024;

/// Most parameters a plug-in's list carries.
pub const MAX_PARAMS: usize = 1 << 16;

/// One of a plug-in's parameters. Values are in the plug-in's own range, `min` to `max`: a
/// VST3 plug-in's are normalized (0 to 1), a CLAP plug-in's are its plain values.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    /// Where the plug-in groups it (a CLAP module path, a VST3 unit), or empty.
    pub group: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// The value as the plug-in shows it ("-3.2 dB"), or empty.
    pub text: String,
    /// Number of steps for a stepped parameter (a switch has 1), 0 for a continuous one.
    pub steps: u32,
    pub automatable: bool,
    pub read_only: bool,
    pub hidden: bool,
}

impl ParamInfo {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "group": self.group, "min": self.min, "max": self.max,
            "default": self.default, "value": self.value, "text": self.text, "steps": self.steps,
            "automatable": self.automatable, "read_only": self.read_only, "hidden": self.hidden,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
        let flag = |key: &str| value.get(key).and_then(Value::as_bool);
        Some(Self {
            id: u32::try_from(value.get("id")?.as_u64()?).ok()?,
            name: text("name")?,
            group: text("group")?,
            min: number("min")?,
            max: number("max")?,
            default: number("default")?,
            value: number("value")?,
            text: text("text")?,
            steps: u32::try_from(value.get("steps")?.as_u64()?).ok()?,
            automatable: flag("automatable")?,
            read_only: flag("read_only")?,
            hidden: flag("hidden")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello {
        version: u64,
    },
    /// Load `plugin`, activate it at `sample_rate` and serve audio on the link at `link`.
    Load {
        plugin: String,
        link: String,
        sample_rate: f64,
    },
    Loaded {
        latency: u32,
    },
    Ping {
        id: u64,
    },
    Pong {
        id: u64,
    },
    GetState,
    /// `controller` is a VST3 plug-in's controller state, when it has one apart from the
    /// component's (`data`).
    SetState {
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    State {
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    GetParams,
    Params {
        params: Vec<ParamInfo>,
    },
    /// Set parameter `id` to `value` (in its own range); answered with `Param`.
    SetParam {
        id: u32,
        value: f64,
    },
    Param {
        param: ParamInfo,
    },
    Error {
        message: String,
    },
    Quit,
}

impl Message {
    /// One line of JSON, without the newline.
    pub fn encode(&self) -> String {
        let value = match self {
            Self::Hello { version } => json!({ "op": "hello", "version": version }),
            Self::Load { plugin, link, sample_rate } => {
                json!({ "op": "load", "plugin": plugin, "link": link, "sample_rate": sample_rate })
            }
            Self::Loaded { latency } => json!({ "op": "loaded", "latency": latency }),
            Self::Ping { id } => json!({ "op": "ping", "id": id }),
            Self::Pong { id } => json!({ "op": "pong", "id": id }),
            Self::GetState => json!({ "op": "get_state" }),
            Self::SetState { data, controller } => {
                json!({ "op": "set_state", "data": hex(data), "controller": controller.as_deref().map(hex) })
            }
            Self::State { data, controller } => {
                json!({ "op": "state", "data": hex(data), "controller": controller.as_deref().map(hex) })
            }
            Self::Error { message } => json!({ "op": "error", "message": message }),
            Self::GetParams => json!({ "op": "get_params" }),
            Self::Params { params } => {
                json!({ "op": "params", "params": params.iter().map(ParamInfo::to_json).collect::<Vec<_>>() })
            }
            Self::SetParam { id, value } => json!({ "op": "set_param", "id": id, "value": value }),
            Self::Param { param } => json!({ "op": "param", "param": param.to_json() }),
            Self::Quit => json!({ "op": "quit" }),
        };
        value.to_string()
    }

    /// Parses one line; `None` for anything that is not a known, well-formed message.
    pub fn decode(line: &str) -> Option<Self> {
        if line.len() > MAX_LINE {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_u64);
        let bytes = |key: &str| value.get(key).and_then(Value::as_str).and_then(unhex);
        // Absent or null is `None`; anything else must be hex.
        let optional_bytes = |key: &str| match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(v) => v.as_str().and_then(unhex).map(Some),
        };
        Some(match value.get("op")?.as_str()? {
            "hello" => Self::Hello { version: number("version")? },
            "load" => Self::Load {
                plugin: text("plugin")?,
                link: text("link")?,
                sample_rate: value.get("sample_rate").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0)?,
            },
            "loaded" => Self::Loaded { latency: u32::try_from(number("latency")?).ok()? },
            "ping" => Self::Ping { id: number("id")? },
            "pong" => Self::Pong { id: number("id")? },
            "get_state" => Self::GetState,
            "set_state" => Self::SetState { data: bytes("data")?, controller: optional_bytes("controller")? },
            "state" => Self::State { data: bytes("data")?, controller: optional_bytes("controller")? },
            "error" => Self::Error { message: text("message")? },
            "get_params" => Self::GetParams,
            "params" => {
                let list = value.get("params")?.as_array()?;
                if list.len() > MAX_PARAMS {
                    return None;
                }
                Self::Params { params: list.iter().map(ParamInfo::from_json).collect::<Option<Vec<_>>>()? }
            }
            "set_param" => Self::SetParam {
                id: u32::try_from(number("id")?).ok()?,
                value: value.get("value").and_then(Value::as_f64).filter(|v| v.is_finite())?,
            },
            "param" => Self::Param { param: ParamInfo::from_json(value.get("param")?)? },
            "quit" => Self::Quit,
            _ => return None,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16);
    pairs.iter().map(|[high, low]| Some((digit(*high)? * 16 + digit(*low)?) as u8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain() -> ParamInfo {
        ParamInfo {
            id: 0,
            name: "Gain".into(),
            group: "Main".into(),
            min: 0.0,
            max: 1.0,
            default: 0.5,
            value: 0.25,
            text: "-12.0 dB".into(),
            steps: 0,
            automatable: true,
            read_only: false,
            hidden: false,
        }
    }

    #[test]
    fn every_message_round_trips() {
        let messages = [
            Message::Hello { version: VERSION },
            Message::Load { plugin: "test:gain".into(), link: "/dev/shm/x".into(), sample_rate: 44_100.0 },
            Message::Loaded { latency: 64 },
            Message::Ping { id: u64::MAX },
            Message::Pong { id: 7 },
            Message::GetState,
            Message::SetState { data: vec![0, 1, 0xab, 0xff], controller: None },
            Message::SetState { data: vec![1], controller: Some(vec![2, 3]) },
            Message::State { data: Vec::new(), controller: None },
            Message::State { data: vec![4], controller: Some(Vec::new()) },
            Message::GetParams,
            Message::Params { params: vec![gain(), ParamInfo { id: u32::MAX, steps: 1, hidden: true, ..gain() }] },
            Message::Params { params: Vec::new() },
            Message::SetParam { id: 7, value: -0.5 },
            Message::Param { param: gain() },
            Message::Error { message: "no such plug-in: \"x\"\n".into() },
            Message::Quit,
        ];
        for message in messages {
            let line = message.encode();
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(Message::decode(&line), Some(message), "{line}");
        }
    }

    #[test]
    fn malformed_lines_are_refused() {
        for line in [
            "",
            "{",
            "[]",
            r#"{"op":"nope"}"#,
            r#"{"op":"ping"}"#,
            r#"{"op":"ping","id":-1}"#,
            r#"{"op":"loaded","latency":5000000000}"#,
            r#"{"op":"state","data":"abc"}"#,
            r#"{"op":"state","data":"zz"}"#,
            r#"{"op":"state","data":"","controller":5}"#,
            r#"{"op":"set_state","data":"","controller":"z"}"#,
            r#"{"op":"load","plugin":"x","link":"y","sample_rate":0}"#,
            r#"{"op":"load","plugin":"x","link":"y"}"#,
            r#"{"op":42}"#,
            r#"{"op":"set_param","id":1}"#,
            r#"{"op":"set_param","id":-1,"value":0.5}"#,
            r#"{"op":"params","params":[{"id":1}]}"#,
            r#"{"op":"param"}"#,
        ] {
            assert_eq!(Message::decode(line), None, "{line}");
        }
    }
}
