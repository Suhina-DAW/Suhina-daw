//! Prepared MIDI event graph processing. Compilation allocates; processing uses caller storage.
use crate::{DeviceGraph, DeviceRole, DeviceSdkError, NodeKind};
use suhina_device_api::NoteEvent;

#[derive(Clone, Copy)]
enum Op {
    Transpose(i8),
}

/// A compiled MIDI effect. Keep one output buffer allocated outside the audio callback and
/// pass it to [`process`](Self::process); event offsets are preserved exactly.
pub struct PreparedMidiEffect {
    ops: Vec<Op>,
}

impl PreparedMidiEffect {
    pub(crate) fn new(graph: &DeviceGraph) -> Result<Self, DeviceSdkError> {
        if graph.role != DeviceRole::MidiEffect {
            return Err(DeviceSdkError("expected a midi_effect graph".into()));
        }
        let order = graph.execution_order()?;
        let mut ops = Vec::with_capacity(graph.nodes.len());
        for index in order {
            let Some(node) = graph.nodes.get(index) else { continue };
            if let NodeKind::MidiTranspose { semitones } = node.kind {
                ops.push(Op::Transpose(semitones));
            }
        }
        Ok(Self { ops })
    }

    /// Transforms events in order. Returns the number written; if the output is too small,
    /// processing stops without writing beyond it. Caller should size for the input event cap.
    pub fn process(&self, input: &[(usize, NoteEvent)], output: &mut [(usize, NoteEvent)]) -> usize {
        let mut written = 0;
        'events: for &(frame, mut event) in input {
            for op in &self.ops {
                match (*op, event) {
                    (Op::Transpose(semitones), NoteEvent::On { key, velocity }) => {
                        let Some(key) =
                            i16::from(key).checked_add(i16::from(semitones)).filter(|k| (0..=127).contains(k))
                        else {
                            continue 'events;
                        };
                        event = NoteEvent::On { key: key as u8, velocity };
                    }
                    (Op::Transpose(semitones), NoteEvent::Off { key, velocity }) => {
                        let Some(key) =
                            i16::from(key).checked_add(i16::from(semitones)).filter(|k| (0..=127).contains(k))
                        else {
                            continue 'events;
                        };
                        event = NoteEvent::Off { key: key as u8, velocity };
                    }
                    _ => {}
                }
            }
            let Some(slot) = output.get_mut(written) else { break };
            *slot = (frame, event);
            written += 1;
        }
        written
    }
}
