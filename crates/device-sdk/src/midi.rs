// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prepared MIDI event graph processing. Compilation allocates; processing uses caller storage.
use crate::{DeviceGraph, DeviceRole, DeviceSdkError, NodeKind, Parameter};
use suhina_device_api::NoteEvent;

#[derive(Clone, Copy)]
enum Op {
    Transpose { semitones: i8, parameter: Option<usize> },
}

/// A compiled MIDI effect. Keep one output buffer allocated outside the audio callback and
/// pass it to [`process`](Self::process); event offsets are preserved exactly.
pub struct PreparedMidiEffect {
    ops: Vec<Op>,
    parameters: Vec<Parameter>,
    values: Vec<f64>,
    held: [Option<u8>; 128],
}

impl PreparedMidiEffect {
    pub(crate) fn new(graph: &DeviceGraph) -> Result<Self, DeviceSdkError> {
        if graph.role != DeviceRole::MidiEffect {
            return Err(DeviceSdkError("expected a midi_effect graph".into()));
        }
        let order = graph.execution_order()?;
        let mut ops = Vec::with_capacity(graph.nodes.len());
        let parameters = graph.parameters.clone();
        let values = parameters.iter().map(|parameter| parameter.default).collect();
        for index in order {
            let Some(node) = graph.nodes.get(index) else { continue };
            match &node.kind {
                NodeKind::MidiTranspose { semitones } => {
                    ops.push(Op::Transpose { semitones: *semitones, parameter: None });
                }
                NodeKind::MidiTransposeParameter { parameter } => {
                    let index = parameters.iter().position(|p| p.id == *parameter).ok_or_else(|| {
                        DeviceSdkError(format!("node {:?}: missing parameter {parameter:?}", node.id))
                    })?;
                    ops.push(Op::Transpose { semitones: 0, parameter: Some(index) });
                }
                _ => {}
            }
        }
        Ok(Self { ops, parameters, values, held: [None; 128] })
    }

    /// Update a prepared transpose control without allocating or rebuilding the graph.
    pub fn set_parameter(&mut self, index: usize, value: f64) {
        if let (Some(parameter), Some(current)) = (self.parameters.get(index), self.values.get_mut(index)) {
            *current = parameter.clamp(value);
        }
    }

    /// Transforms events in order. Returns the number written; if the output is too small,
    /// processing stops without writing beyond it. Caller should size for the input event cap.
    pub fn process(&mut self, input: &[(usize, NoteEvent)], output: &mut [(usize, NoteEvent)]) -> usize {
        let mut written = 0;
        'events: for &(frame, mut event) in input {
            if written >= output.len() {
                break;
            }
            if let NoteEvent::Off { key, velocity } = event {
                let Some(mapped) = self.held.get_mut(usize::from(key)).and_then(Option::take) else { continue };
                if let Some(slot) = output.get_mut(written) {
                    *slot = (frame, NoteEvent::Off { key: mapped, velocity });
                    written += 1;
                }
                continue;
            }
            let original = match event {
                NoteEvent::On { key, .. } => {
                    if let Some(held) = self.held.get_mut(usize::from(key)) {
                        *held = None;
                    }
                    Some(key)
                }
                NoteEvent::AllNotesOff => {
                    self.held.fill(None);
                    None
                }
                _ => None,
            };
            for op in &self.ops {
                let Op::Transpose { semitones, parameter } = *op;
                let semitones = parameter
                    .and_then(|index| self.values.get(index))
                    .map_or(f64::from(semitones), |value| *value)
                    .round()
                    .clamp(f64::from(i8::MIN), f64::from(i8::MAX)) as i8;
                if let NoteEvent::On { key, velocity } = event {
                    let Some(key) = i16::from(key).checked_add(i16::from(semitones)).filter(|k| (0..=127).contains(k))
                    else {
                        continue 'events;
                    };
                    event = NoteEvent::On { key: key as u8, velocity };
                }
            }
            let Some(slot) = output.get_mut(written) else { break };
            if let (Some(original), NoteEvent::On { key, .. }) = (original, event)
                && let Some(held) = self.held.get_mut(usize::from(original))
            {
                *held = Some(key);
            }
            *slot = (frame, event);
            written += 1;
        }
        written
    }
}
