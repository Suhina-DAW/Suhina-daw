//! The control messages between the engine and a plug-in host process (ADR 0006): one JSON
//! object per line over a local socket. Audio does not go here; it goes through `loopwerk-ipc`.
//!
//! Every message has an `op`. Engine to host: `hello`, `load`, `ping`, `get_state`,
//! `set_state`, `quit`. Host to engine: `hello`, `loaded`, `pong`, `state`, `error`.

use serde_json::{Value, json};

/// Bumped whenever a message changes; both sides refuse a different version.
pub const VERSION: u64 = 2;
/// Longest line either side accepts; state blobs are hex, so this bounds them at 32 MiB.
pub const MAX_LINE: usize = 64 * 1024 * 1024;

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
        ] {
            assert_eq!(Message::decode(line), None, "{line}");
        }
    }
}
