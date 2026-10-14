//! Build, validate, save, and run data-only Suhina device graphs (`.sd`).
//!
//! Version 1 supports linear effect graphs made from audio input, gain, and audio output
//! nodes. A validated graph compiles off the audio thread into a processor whose `process`
//! method performs no allocation.

use std::fmt;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const EXTENSION: &str = "sd";
pub const FORMAT: &str = "suhina-device";
pub const VERSION: u32 = 1;
pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_NODES: usize = 128;
pub const MAX_CONNECTIONS: usize = MAX_NODES - 1;
pub const MAX_PARAMETERS: usize = 128;
const MAX_TEXT_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceGraph {
    pub id: String,
    pub name: String,
    pub role: DeviceRole,
    pub parameters: Vec<Parameter>,
    pub nodes: Vec<Node>,
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Effect,
    Instrument,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub control: ParameterControl,
}

/// How the parameter is presented in the generated device panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterControl {
    #[default]
    Slider,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    AudioInput,
    Gain { parameter: String },
    AudioOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSdkError(pub String);

impl fmt::Display for DeviceSdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceSdkError {}

impl DeviceGraph {
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: DeviceRole) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            role,
            parameters: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), DeviceSdkError> {
        if self.id.is_empty()
            || self.id.len() > MAX_TEXT_BYTES
            || self.name.is_empty()
            || self.name.len() > MAX_TEXT_BYTES
        {
            return Err(DeviceSdkError("device id and name must be 1 to 128 bytes".into()));
        }
        if self.nodes.len() < 2 || self.nodes.len() > MAX_NODES {
            return Err(DeviceSdkError(format!("a graph must have 2 to {MAX_NODES} nodes")));
        }
        if self.connections.len() > MAX_CONNECTIONS {
            return Err(DeviceSdkError(format!("a graph may have at most {MAX_CONNECTIONS} connections")));
        }
        if self.parameters.len() > MAX_PARAMETERS {
            return Err(DeviceSdkError(format!("a graph may have at most {MAX_PARAMETERS} parameters")));
        }

        let mut param_ids = std::collections::HashSet::new();
        for parameter in &self.parameters {
            if parameter.id.is_empty()
                || parameter.id.len() > MAX_TEXT_BYTES
                || parameter.name.is_empty()
                || parameter.name.len() > MAX_TEXT_BYTES
                || !param_ids.insert(parameter.id.as_str())
            {
                return Err(DeviceSdkError(
                    "parameter ids and names must be unique, non-empty, and at most 128 bytes".into(),
                ));
            }
            if !parameter.min.is_finite()
                || !parameter.max.is_finite()
                || !parameter.default.is_finite()
                || parameter.min < -(f32::MAX as f64)
                || parameter.max > f32::MAX as f64
                || parameter.min > parameter.max
                || !(parameter.min..=parameter.max).contains(&parameter.default)
            {
                return Err(DeviceSdkError(format!("parameter {:?} has an invalid range or default", parameter.id)));
            }
        }

        let mut node_ids = std::collections::HashSet::new();
        let mut inputs = 0;
        let mut outputs = 0;
        for node in &self.nodes {
            if node.id.is_empty() || node.id.len() > MAX_TEXT_BYTES || !node_ids.insert(node.id.as_str()) {
                return Err(DeviceSdkError("node ids must be unique, non-empty, and at most 128 bytes".into()));
            }
            match &node.kind {
                NodeKind::AudioInput => inputs += 1,
                NodeKind::AudioOutput => outputs += 1,
                NodeKind::Gain { parameter } if param_ids.contains(parameter.as_str()) => {}
                NodeKind::Gain { parameter } => {
                    return Err(DeviceSdkError(format!(
                        "gain node {:?} refers to missing parameter {parameter:?}",
                        node.id
                    )));
                }
            }
        }
        if inputs != 1 || outputs != 1 {
            return Err(DeviceSdkError("a graph must have exactly one audio input and one audio output".into()));
        }

        let mut incoming = std::collections::HashMap::new();
        let mut outgoing = std::collections::HashMap::new();
        for connection in &self.connections {
            if !node_ids.contains(connection.from.as_str()) || !node_ids.contains(connection.to.as_str()) {
                return Err(DeviceSdkError("a connection refers to a missing node".into()));
            }
            if outgoing.insert(connection.from.as_str(), connection.to.as_str()).is_some()
                || incoming.insert(connection.to.as_str(), connection.from.as_str()).is_some()
            {
                return Err(DeviceSdkError(
                    "version 1 graphs are linear: each node has at most one input and output".into(),
                ));
            }
        }
        let input = self.nodes.iter().find(|node| node.kind == NodeKind::AudioInput).map(|node| node.id.as_str());
        let output = self.nodes.iter().find(|node| node.kind == NodeKind::AudioOutput).map(|node| node.id.as_str());
        let (Some(mut current), Some(output)) = (input, output) else {
            return Err(DeviceSdkError("graph input or output is missing".into()));
        };
        if incoming.contains_key(current) || outgoing.contains_key(output) {
            return Err(DeviceSdkError("audio input and output must be the endpoints of the graph".into()));
        }
        let mut visited = 1;
        while let Some(next) = outgoing.get(current) {
            current = next;
            visited += 1;
            if visited > self.nodes.len() {
                return Err(DeviceSdkError("graph contains a cycle".into()));
            }
        }
        if current != output || visited != self.nodes.len() {
            return Err(DeviceSdkError("every node must lie on the path from audio input to audio output".into()));
        }
        Ok(())
    }

    /// The nodes in signal-flow order. The graph must be valid before this is called.
    pub fn ordered_nodes(&self) -> Vec<&Node> {
        let Some(mut current) =
            self.nodes.iter().find(|node| node.kind == NodeKind::AudioInput).map(|node| node.id.as_str())
        else {
            return Vec::new();
        };
        let mut ordered = Vec::with_capacity(self.nodes.len());
        while ordered.len() < self.nodes.len() {
            let Some(node) = self.nodes.iter().find(|node| node.id == current) else { break };
            ordered.push(node);
            if node.kind == NodeKind::AudioOutput {
                break;
            }
            let Some(next) = self.connections.iter().find(|edge| edge.from == current).map(|edge| edge.to.as_str())
            else {
                break;
            };
            current = next;
        }
        ordered
    }

    pub fn to_sd(&self) -> Result<String, DeviceSdkError> {
        self.validate()?;
        let file = FileV1::from(self);
        serde_json::to_string_pretty(&file)
            .map_err(|error| DeviceSdkError(format!("could not encode device graph: {error}")))
    }

    pub fn from_sd(text: &str) -> Result<Self, DeviceSdkError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError(format!("a .sd file is limited to {} KiB", MAX_FILE_BYTES / 1024)));
        }
        let file: FileV1 =
            serde_json::from_str(text).map_err(|error| DeviceSdkError(format!("invalid .sd JSON: {error}")))?;
        if file.format != FORMAT || file.version != VERSION {
            return Err(DeviceSdkError(format!(
                "unsupported device format/version: {:?}/{}",
                file.format, file.version
            )));
        }
        let graph: Self = file.into();
        graph.validate()?;
        Ok(graph)
    }

    pub fn read_file(path: &Path) -> Result<Self, DeviceSdkError> {
        let file = std::fs::File::open(path)
            .map_err(|error| DeviceSdkError(format!("could not open {}: {error}", path.display())))?;
        let mut text = String::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|error| DeviceSdkError(format!("could not read {}: {error}", path.display())))?;
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError(format!("a .sd file is limited to {} KiB", MAX_FILE_BYTES / 1024)));
        }
        Self::from_sd(&text)
    }

    pub fn write_file(&self, path: &Path) -> Result<(), DeviceSdkError> {
        let text = self.to_sd()?;
        let failed = |error: std::io::Error| DeviceSdkError(format!("could not save {}: {error}", path.display()));
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".saving");
        let temporary = std::path::PathBuf::from(temporary);
        std::fs::write(&temporary, text).map_err(failed)?;
        std::fs::rename(&temporary, path).map_err(|error| {
            let _gone = std::fs::remove_file(&temporary);
            failed(error)
        })
    }

    /// Compiles a validated linear graph off the audio thread.
    pub fn compile(&self) -> Result<PreparedEffect, DeviceSdkError> {
        self.validate()?;
        if self.role != DeviceRole::Effect {
            return Err(DeviceSdkError("version 1 executes effect graphs only".into()));
        }
        let mut gains = Vec::new();
        for node in self.ordered_nodes() {
            if let NodeKind::Gain { parameter } = &node.kind {
                let index =
                    self.parameters.iter().position(|item| item.id == *parameter).ok_or_else(|| {
                        DeviceSdkError(format!("gain node refers to missing parameter {parameter:?}"))
                    })?;
                gains.push((index, self.parameters[index].default as f32));
            }
        }
        Ok(PreparedEffect {
            gains,
            parameters: self
                .parameters
                .iter()
                .map(|parameter| (parameter.min, parameter.max, parameter.default))
                .collect(),
        })
    }
}

/// A precompiled effect graph. Construct it before audio processing; processing and indexed
/// parameter updates do not allocate.
pub struct PreparedEffect {
    gains: Vec<(usize, f32)>,
    parameters: Vec<(f64, f64, f64)>,
}

impl PreparedEffect {
    pub fn set_parameter(&mut self, index: usize, value: f64) {
        let Some((min, max, default)) = self.parameters.get(index) else { return };
        let value = if value.is_finite() { value.clamp(*min, *max) } else { *default };
        for (param_index, gain) in &mut self.gains {
            if *param_index == index {
                *gain = value as f32;
            }
        }
    }

    pub fn process(&self, interleaved_stereo: &mut [f32]) {
        for (_, gain) in &self.gains {
            for sample in interleaved_stereo.iter_mut() {
                *sample *= *gain;
            }
        }
    }
}

impl Parameter {
    pub fn new(id: impl Into<String>, name: impl Into<String>, range: (f64, f64), default: f64) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            min: range.0,
            max: range.1,
            default,
            control: ParameterControl::Slider,
        }
    }
}

impl Node {
    pub fn audio_input(id: impl Into<String>) -> Self {
        Self { id: id.into(), kind: NodeKind::AudioInput, visible: true }
    }
    pub fn gain(id: impl Into<String>, parameter: impl Into<String>) -> Self {
        Self { id: id.into(), kind: NodeKind::Gain { parameter: parameter.into() }, visible: true }
    }
    pub fn audio_output(id: impl Into<String>) -> Self {
        Self { id: id.into(), kind: NodeKind::AudioOutput, visible: true }
    }
}

impl Connection {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self { from: from.into(), to: to.into() }
    }
}

#[derive(Serialize, Deserialize)]
struct FileV1 {
    format: String,
    version: u32,
    device: DeviceV1,
    nodes: Vec<NodeV1>,
    connections: Vec<Connection>,
}

#[derive(Serialize, Deserialize)]
struct DeviceV1 {
    id: String,
    name: String,
    role: DeviceRole,
    parameters: Vec<ParameterV1>,
}

#[derive(Serialize, Deserialize)]
struct ParameterV1 {
    id: String,
    name: String,
    min: f64,
    max: f64,
    default: f64,
    #[serde(default)]
    control: ParameterControl,
}

#[derive(Serialize, Deserialize)]
struct NodeV1 {
    id: String,
    #[serde(default = "default_visible", skip_serializing_if = "is_visible")]
    visible: bool,
    #[serde(flatten)]
    kind: NodeKindV1,
}

fn default_visible() -> bool {
    true
}

fn is_visible(visible: &bool) -> bool {
    *visible
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NodeKindV1 {
    AudioInput,
    Gain { parameter: String },
    AudioOutput,
}

impl From<&DeviceGraph> for FileV1 {
    fn from(graph: &DeviceGraph) -> Self {
        let nodes = graph
            .nodes
            .iter()
            .map(|node| NodeV1 {
                id: node.id.clone(),
                visible: node.visible,
                kind: match &node.kind {
                    NodeKind::AudioInput => NodeKindV1::AudioInput,
                    NodeKind::Gain { parameter } => NodeKindV1::Gain { parameter: parameter.clone() },
                    NodeKind::AudioOutput => NodeKindV1::AudioOutput,
                },
            })
            .collect();
        Self {
            format: FORMAT.into(),
            version: VERSION,
            device: DeviceV1 {
                id: graph.id.clone(),
                name: graph.name.clone(),
                role: graph.role,
                parameters: graph
                    .parameters
                    .iter()
                    .map(|parameter| ParameterV1 {
                        id: parameter.id.clone(),
                        name: parameter.name.clone(),
                        min: parameter.min,
                        max: parameter.max,
                        default: parameter.default,
                        control: parameter.control,
                    })
                    .collect(),
            },
            nodes,
            connections: graph.connections.clone(),
        }
    }
}

impl From<FileV1> for DeviceGraph {
    fn from(file: FileV1) -> Self {
        Self {
            id: file.device.id,
            name: file.device.name,
            role: file.device.role,
            parameters: file
                .device
                .parameters
                .into_iter()
                .map(|parameter| Parameter {
                    id: parameter.id,
                    name: parameter.name,
                    min: parameter.min,
                    max: parameter.max,
                    default: parameter.default,
                    control: parameter.control,
                })
                .collect(),
            nodes: file
                .nodes
                .into_iter()
                .map(|node| Node {
                    id: node.id,
                    visible: node.visible,
                    kind: match node.kind {
                        NodeKindV1::AudioInput => NodeKind::AudioInput,
                        NodeKindV1::Gain { parameter } => NodeKind::Gain { parameter },
                        NodeKindV1::AudioOutput => NodeKind::AudioOutput,
                    },
                })
                .collect(),
            connections: file.connections,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_graph() -> DeviceGraph {
        let mut graph = DeviceGraph::new("org.example.gain", "Example Gain", DeviceRole::Effect);
        graph.parameters.push(Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
        graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
        graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
        graph
    }

    #[test]
    fn gain_graph_round_trips_and_processes_audio() {
        let graph = gain_graph();
        let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(restored, graph);
        let mut effect = restored.compile().unwrap_or_else(|e| panic!("{e}"));
        effect.set_parameter(0, 0.5);
        let mut buffer = [1.0, -1.0, 0.5, -0.5];
        effect.process(&mut buffer);
        assert_eq!(buffer, [0.5, -0.5, 0.25, -0.25]);
        effect.set_parameter(0, f64::NAN);
        let mut buffer = [1.0, -1.0];
        effect.process(&mut buffer);
        assert_eq!(buffer, [1.0, -1.0]);
    }

    #[test]
    fn compiled_gains_follow_connections_regardless_of_storage_order() {
        let mut graph = DeviceGraph::new("org.example.chain", "Gain Chain", DeviceRole::Effect);
        graph.parameters =
            vec![Parameter::new("up", "Up", (0.0, 1e30), 1e30), Parameter::new("down", "Down", (0.0, 1.0), 1e-30)];
        graph.nodes = vec![
            Node::audio_input("in"),
            Node::gain("up1", "up"),
            Node::gain("up2", "up"),
            Node::gain("down1", "down"),
            Node::gain("down2", "down"),
            Node::audio_output("out"),
        ];
        graph.connections = vec![
            Connection::new("in", "up1"),
            Connection::new("up1", "down1"),
            Connection::new("down1", "up2"),
            Connection::new("up2", "down2"),
            Connection::new("down2", "out"),
        ];
        let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
        let effect = restored.compile().unwrap();
        let mut buffer = [1.0, -1.0];
        effect.process(&mut buffer);
        for (sample, expected) in buffer.into_iter().zip([1.0, -1.0]) {
            assert!(sample.is_finite());
            assert!((sample - expected).abs() < 1e-5, "{sample}");
        }
    }

    #[test]
    fn hidden_nodes_round_trip_without_changing_the_graph() {
        let mut graph = gain_graph();
        graph.nodes[0].visible = false;
        graph.nodes[1].visible = false;
        graph.nodes[2].visible = false;
        let text = graph.to_sd().unwrap_or_default();
        assert_eq!(text.matches("\"visible\": false").count(), 3);
        assert_eq!(DeviceGraph::from_sd(&text).map(|restored| restored.nodes), Ok(graph.nodes));
    }

    #[test]
    fn invalid_graphs_and_oversized_files_are_refused() {
        let mut graph = gain_graph();
        graph.connections.pop();
        assert!(graph.validate().is_err());
        let too_large = " ".repeat(MAX_FILE_BYTES + 1);
        assert!(DeviceGraph::from_sd(&too_large).is_err());
        assert!(DeviceGraph::from_sd("not json").is_err());
        assert!(DeviceGraph::from_sd(r#"{"format":"suhina-device","version":99}"#).is_err());
    }

    #[test]
    fn malformed_parameters_and_missing_gain_parameters_are_refused() {
        let mut graph = gain_graph();
        graph.parameters[0].default = 3.0;
        assert!(graph.validate().is_err());
        graph.parameters[0].default = 1.0;
        graph.nodes[1] = Node::gain("gain", "missing");
        assert!(graph.validate().is_err());
    }

    #[test]
    fn instrument_role_is_serializable_but_not_executable_yet() {
        let mut graph = gain_graph();
        graph.role = DeviceRole::Instrument;
        assert!(DeviceGraph::from_sd(&graph.to_sd().unwrap_or_default()).is_ok());
        assert!(graph.compile().is_err());
    }
}
