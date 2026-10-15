//! Build, validate, save, and run data-only Suhina device graphs (`.sd`).
//!
//! Version 2 supports composable stereo signal graphs, modulation and delayed feedback. A validated graph compiles off the audio thread into a processor whose `process`
//! method performs no allocation.

use std::fmt;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

mod graph;
mod midi;
mod nodes;
mod runtime;
pub mod ui;
pub use midi::PreparedMidiEffect;
pub use nodes::{FilterMode, NODE_CATALOG, NodeInfo, NodeKind, PortInfo, Waveform, node_reference_markdown};
pub use runtime::PreparedEffect;
pub use suhina_device_api::ProcessContext;

pub const EXTENSION: &str = "sd";
pub const FORMAT: &str = "suhina-device";
pub const VERSION: u32 = 2;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NODES: usize = 128;
pub const MAX_CONNECTIONS: usize = MAX_NODES * 5;
pub const MAX_DELAY_FRAMES: usize = 2_000_000;
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
    pub ui: Option<ui::DeviceUi>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Effect,
    Instrument,
    MidiEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub control: ParameterControl,
    pub unit: String,
    pub ui: ParameterUi,
    pub smoothing_ms: f64,
}

/// How the parameter is presented in the generated device panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterControl {
    #[default]
    Slider,
    LogSlider,
    Integer,
    Toggle,
    Knob,
    LogKnob,
    Number,
    VerticalSlider,
    Button,
    Choice,
    Radio,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelPosition {
    #[default]
    Left,
    Top,
    Hidden,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlShape {
    #[default]
    Rounded,
    Square,
    Pill,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParameterUi {
    pub tooltip: String,
    pub group: String,
    pub options: Vec<String>,
    pub step: f64,
    pub decimals: usize,
    pub size: Option<[f32; 2]>,
    pub position: Option<[f32; 2]>,
    pub accent: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub panel_color: Option<[u8; 3]>,
    pub text_color: Option<[u8; 3]>,
    pub font_size: f32,
    pub monospace: bool,
    pub corner_radius: f32,
    pub shape: ControlShape,
    pub label_position: LabelPosition,
    pub show_value: bool,
}
impl Default for ParameterUi {
    fn default() -> Self {
        Self {
            tooltip: String::new(),
            group: String::new(),
            options: Vec::new(),
            step: 0.0,
            decimals: 3,
            size: None,
            position: None,
            accent: None,
            background: None,
            panel_color: None,
            text_color: None,
            font_size: 12.0,
            monospace: false,
            corner_radius: 4.0,
            shape: ControlShape::Rounded,
            label_position: LabelPosition::Left,
            show_value: true,
        }
    }
}
fn default_smoothing_ms() -> f64 {
    20.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub to: String,
    #[serde(default = "default_output_port")]
    pub from_port: String,
    #[serde(default = "default_input_port")]
    pub to_port: String,
}
fn default_output_port() -> String {
    "out".into()
}
fn default_input_port() -> String {
    "in".into()
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
            ui: None,
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
            if parameter.unit.len() > MAX_TEXT_BYTES {
                return Err(DeviceSdkError(format!("parameter {:?}: unit exceeds 128 bytes", parameter.id)));
            }
            let ui = &parameter.ui;
            let minimum_height =
                ui.font_size + 16.0 + if ui.label_position == LabelPosition::Top { ui.font_size + 4.0 } else { 0.0 };
            if ui.size.is_some_and(|[w, h]| {
                !w.is_finite()
                    || !h.is_finite()
                    || !(24.0..=4096.0).contains(&w)
                    || !(minimum_height..=4096.0).contains(&h)
            }) || ui.position.is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0.0..=8192.0).contains(v)))
                || !ui.font_size.is_finite()
                || !(8.0..=48.0).contains(&ui.font_size)
                || !ui.corner_radius.is_finite()
                || !(0.0..=128.0).contains(&ui.corner_radius)
            {
                return Err(DeviceSdkError(format!("parameter {:?}: invalid UI geometry or font", parameter.id)));
            }
            if !parameter.smoothing_ms.is_finite()
                || !(0.0..=1000.0).contains(&parameter.smoothing_ms)
                || !parameter.ui.step.is_finite()
                || parameter.ui.step < 0.0
                || parameter.ui.decimals > 8
                || parameter.ui.tooltip.len() > 1024
                || parameter.ui.group.len() > 128
                || parameter.ui.options.len() > 128
                || parameter.ui.options.iter().any(|s| s.is_empty() || s.len() > 128)
            {
                return Err(DeviceSdkError(format!("parameter {:?}: invalid UI or smoothing settings", parameter.id)));
            }
            if matches!(parameter.control, ParameterControl::Choice | ParameterControl::Radio)
                && (parameter.ui.options.is_empty()
                    || parameter.min != 0.0
                    || parameter.max != (parameter.ui.options.len() - 1) as f64)
            {
                return Err(DeviceSdkError(format!(
                    "parameter {:?}: choices require range 0..options.len()-1",
                    parameter.id
                )));
            }
            let discrete = matches!(
                parameter.control,
                ParameterControl::Integer
                    | ParameterControl::Toggle
                    | ParameterControl::Button
                    | ParameterControl::Choice
                    | ParameterControl::Radio
            );
            if (discrete && [parameter.min, parameter.max, parameter.default].iter().any(|v| v.fract() != 0.0))
                || (matches!(parameter.control, ParameterControl::Toggle | ParameterControl::Button)
                    && (parameter.min != 0.0 || parameter.max != 1.0))
                || (matches!(parameter.control, ParameterControl::LogSlider | ParameterControl::LogKnob)
                    && parameter.min <= 0.0)
            {
                return Err(DeviceSdkError(format!(
                    "parameter {:?}: range/default incompatible with control",
                    parameter.id
                )));
            }
        }

        if let Some(ui) = &self.ui {
            ui.validate(&self.parameters)?;
        }
        self.validate_nodes()?;
        self.execution_order()?;
        self.delay_budget(384_000.0)?;
        Ok(())
    }

    /// Dependency order; delay input edges are committed after the current frame.
    pub fn ordered_nodes(&self) -> Vec<&Node> {
        self.execution_order().unwrap_or_default().into_iter().filter_map(|i| self.nodes.get(i)).collect()
    }

    pub fn to_sd(&self) -> Result<String, DeviceSdkError> {
        self.validate()?;
        let file = FileV1::from(self);
        let text = serde_json::to_string_pretty(&file)
            .map_err(|error| DeviceSdkError(format!("could not encode device graph: {error}")))?;
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError("encoded .sd exceeds 2 MiB".into()));
        }
        Ok(text)
    }

    pub fn from_sd(text: &str) -> Result<Self, DeviceSdkError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(DeviceSdkError(format!("a .sd file is limited to {} KiB", MAX_FILE_BYTES / 1024)));
        }
        let file: FileV1 =
            serde_json::from_str(text).map_err(|error| DeviceSdkError(format!("invalid .sd JSON: {error}")))?;
        if file.format != FORMAT || !(1..=VERSION).contains(&file.version) {
            return Err(DeviceSdkError(format!(
                "unsupported device format/version: {:?}/{}",
                file.format, file.version
            )));
        }
        if file.version == 1
            && (file
                .nodes
                .iter()
                .any(|node| !matches!(node.kind, NodeKind::AudioInput | NodeKind::AudioOutput | NodeKind::Gain { .. }))
                || file.connections.iter().any(|edge| edge.from_port != "out" || edge.to_port != "in"))
        {
            return Err(DeviceSdkError("version 1 only supports legacy input/gain/output nodes".into()));
        }
        if file.version == 1 {
            let mut sources = std::collections::HashSet::new();
            let mut destinations = std::collections::HashSet::new();
            if file.connections.iter().any(|edge| !sources.insert(&edge.from) || !destinations.insert(&edge.to)) {
                return Err(DeviceSdkError("version 1 connections must form a linear chain".into()));
            }
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

    /// Prepare at 48 kHz; use compile_at for the actual engine sample rate.
    pub fn compile(&self) -> Result<PreparedEffect, DeviceSdkError> {
        self.compile_at(48_000.0)
    }
    /// Compiles off the audio thread, including bounded delay storage.
    pub fn compile_at(&self, sample_rate: f64) -> Result<PreparedEffect, DeviceSdkError> {
        self.validate()?;
        if self.role != DeviceRole::Effect {
            return Err(DeviceSdkError("compile_at prepares audio-effect graphs only".into()));
        }
        PreparedEffect::new(self, sample_rate)
    }

    /// Compiles a MIDI-event graph off the audio thread.
    pub fn compile_midi(&self) -> Result<PreparedMidiEffect, DeviceSdkError> {
        if self.role != DeviceRole::MidiEffect {
            return Err(DeviceSdkError("compile_midi prepares midi_effect graphs only".into()));
        }
        self.validate()?;
        PreparedMidiEffect::new(self)
    }

    /// Compiles an instrument graph. MIDI events are supplied with their frame offsets at render time.
    pub fn compile_instrument_at(&self, sample_rate: f64) -> Result<PreparedEffect, DeviceSdkError> {
        if self.role != DeviceRole::Instrument {
            return Err(DeviceSdkError("compile_instrument_at prepares instrument graphs only".into()));
        }
        self.validate()?;
        PreparedEffect::new(self, sample_rate)
    }
}

impl Parameter {
    /// Clamp hostile values and preserve discrete parameter semantics.
    pub fn clamp(&self, value: f64) -> f64 {
        let value = if value.is_finite() { value.clamp(self.min, self.max) } else { self.default };
        match self.control {
            ParameterControl::Toggle
            | ParameterControl::Integer
            | ParameterControl::Button
            | ParameterControl::Choice
            | ParameterControl::Radio => value.round().clamp(self.min, self.max),
            _ => value,
        }
    }

    pub fn new(id: impl Into<String>, name: impl Into<String>, range: (f64, f64), default: f64) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            min: range.0,
            max: range.1,
            default,
            control: ParameterControl::Slider,
            unit: String::new(),
            ui: ParameterUi::default(),
            smoothing_ms: default_smoothing_ms(),
        }
    }
}

impl Node {
    pub fn new(id: impl Into<String>, kind: NodeKind) -> Self {
        Self { id: id.into(), kind, visible: true }
    }
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
        Self::ports(from, "out", to, "in")
    }
    pub fn ports(
        from: impl Into<String>,
        from_port: impl Into<String>,
        to: impl Into<String>,
        to_port: impl Into<String>,
    ) -> Self {
        Self { from: from.into(), to: to.into(), from_port: from_port.into(), to_port: to_port.into() }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<ui::DeviceUi>,
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
    #[serde(default)]
    unit: String,
    #[serde(default)]
    ui: ParameterUi,
    #[serde(default = "default_smoothing_ms")]
    smoothing_ms: f64,
}

#[derive(Serialize, Deserialize)]
struct NodeV1 {
    id: String,
    #[serde(default = "default_visible", skip_serializing_if = "is_visible")]
    visible: bool,
    #[serde(flatten)]
    kind: NodeKind,
}

fn default_visible() -> bool {
    true
}

fn is_visible(visible: &bool) -> bool {
    *visible
}

impl From<&DeviceGraph> for FileV1 {
    fn from(graph: &DeviceGraph) -> Self {
        let nodes = graph
            .nodes
            .iter()
            .map(|node| NodeV1 { id: node.id.clone(), visible: node.visible, kind: node.kind.clone() })
            .collect();
        Self {
            format: FORMAT.into(),
            version: VERSION,
            device: DeviceV1 {
                ui: graph.ui.clone(),
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
                        unit: parameter.unit.clone(),
                        ui: parameter.ui.clone(),
                        smoothing_ms: parameter.smoothing_ms,
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
            ui: file.device.ui,
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
                    unit: parameter.unit,
                    ui: parameter.ui,
                    smoothing_ms: parameter.smoothing_ms,
                })
                .collect(),
            nodes: file
                .nodes
                .into_iter()
                .map(|node| Node { id: node.id, visible: node.visible, kind: node.kind })
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
        let mut graph = gain_graph();
        graph.parameters[0].smoothing_ms = 0.0;
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
        let mut effect = restored.compile().unwrap();
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
    fn non_audio_roles_are_serializable_but_not_audio_effects() {
        let mut graph = DeviceGraph::new("org.test.instrument", "Instrument", DeviceRole::Instrument);
        graph.nodes = vec![Node::new("gate", NodeKind::MidiGate), Node::audio_output("out")];
        graph.connections = vec![Connection::new("gate", "out")];
        let encoded = graph.to_sd().expect("valid instrument graph serializes");
        assert_eq!(DeviceGraph::from_sd(&encoded).map(|g| g.role), Ok(DeviceRole::Instrument));
        assert!(graph.compile().is_err());
    }

    #[test]
    fn midi_effect_transposes_events_and_preserves_frames_and_controls() {
        use suhina_device_api::NoteEvent;
        let mut graph = DeviceGraph::new("org.test.transpose", "Transpose", DeviceRole::MidiEffect);
        graph.nodes = vec![
            Node::new("in", NodeKind::MidiInput),
            Node::new("shift", NodeKind::MidiTranspose { semitones: 12 }),
            Node::new("out", NodeKind::MidiOutput),
        ];
        graph.connections = vec![Connection::new("in", "shift"), Connection::new("shift", "out")];
        let encoded = graph.to_sd().expect("valid MIDI graph serializes");
        assert_eq!(DeviceGraph::from_sd(&encoded).map(|g| g.role), Ok(DeviceRole::MidiEffect));
        let prepared = graph.compile_midi().expect("valid MIDI graph");
        let input = [
            (5, NoteEvent::On { key: 60, velocity: 0.75 }),
            (9, NoteEvent::Off { key: 60, velocity: 0.25 }),
            (12, NoteEvent::Controller { number: 64, value: 1.0 }),
            (16, NoteEvent::On { key: 120, velocity: 1.0 }),
        ];
        let mut output = [(0, NoteEvent::AllNotesOff); 4];
        let count = prepared.process(&input, &mut output);
        assert_eq!(count, 3);
        assert_eq!(output[0], (5, NoteEvent::On { key: 72, velocity: 0.75 }));
        assert_eq!(output[1], (9, NoteEvent::Off { key: 72, velocity: 0.25 }));
        assert_eq!(output[2], (12, NoteEvent::Controller { number: 64, value: 1.0 }));
    }

    #[test]
    fn instrument_graph_reads_midi_gate_at_exact_event_frames() {
        use suhina_device_api::NoteEvent;
        let mut graph = DeviceGraph::new("org.test.instrument", "Gated Tone", DeviceRole::Instrument);
        graph.nodes = vec![
            Node::new("gate", NodeKind::MidiGate),
            Node::new("frequency", NodeKind::Constant { value: 440.0 }),
            Node::new("osc", NodeKind::Oscillator { shape: Waveform::Sine }),
            Node::new("amp", NodeKind::Multiply),
            Node::audio_output("out"),
        ];
        graph.connections = vec![
            Connection::ports("frequency", "out", "osc", "frequency"),
            Connection::ports("gate", "out", "amp", "a"),
            Connection::ports("osc", "out", "amp", "b"),
            Connection::new("amp", "out"),
        ];
        let mut instrument = graph.compile_instrument_at(48_000.0).expect("valid instrument graph");
        let events = [(0, NoteEvent::On { key: 60, velocity: 1.0 }), (16, NoteEvent::Off { key: 60, velocity: 0.5 })];
        let mut audio = [0.0; 64];
        instrument.process_instrument_context(&ProcessContext::default(), &events, &mut audio);
        assert!(audio[..32].iter().any(|sample| sample.abs() > 0.01));
        assert!(audio[32..].iter().all(|sample| *sample == 0.0));
    }
}
