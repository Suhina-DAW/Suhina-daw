//! Editing the notes of a MIDI clip: the `note.*` commands the piano roll, the CLI and MCP run
//! (`docs/spec/ui-clip-editor.md` › Commands).
//!
//! A clip is named by `track` and `index` (an arrangement clip) or `scene` (a launcher slot); its
//! notes by their index in the content's note lane. Edits keep the notes' order, so the indices
//! of notes an edit did not delete stay valid for the next edit.

use serde_json::{Map, Value, json};
use suhina_engine::{MAX_START_BEAT, Quantization};
use suhina_model::{ContentId, ContentLane, LaneData, Note, Project, TrackId};

use crate::commands::{parse_quantization, track_id};
use crate::{CommandError, Session};

/// The most notes one command may name or add.
pub const MAX_NOTES_PER_COMMAND: usize = 10_000;
/// The most notes a clip may hold.
pub const MAX_NOTES_PER_CLIP: usize = 100_000;
/// The latest start and the longest length of a note, in beats: the engine's furthest start.
const MAX_BEAT: f64 = MAX_START_BEAT;
/// The velocity a note gets when `note.add` gives none, and the release velocity of every new
/// note (the MIDI default, 64).
const DEFAULT_VELOCITY: f64 = 100.0;
const RELEASE_VELOCITY: f32 = 64.0 / 127.0;
/// Beats this close below 0 are 0: a move by the exact distance to the start lands on it.
const EPSILON: f64 = 1e-9;

/// The clip a command names: its content and the track it plays on.
fn locate(p: &Project, params: &Value) -> Result<(ContentId, TrackId), CommandError> {
    let track = track_id(p, params)?;
    let whole = |key: &str| -> Result<Option<u64>, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{key} must be a whole number").into()),
        }
    };
    let clip = match (whole("index")?, whole("scene")?) {
        (Some(_), Some(_)) => return Err("give index or scene, not both".into()),
        (None, None) => return Err("give index (an arrangement clip) or scene (a launcher clip)".into()),
        (Some(index), None) => {
            let lane = p
                .arrangement
                .lanes
                .iter()
                .find(|l| l.track == Some(track))
                .ok_or("the track has no arrangement lane")?;
            usize::try_from(index).ok().and_then(|i| lane.clips.get(i)).ok_or_else(|| {
                format!("clip index {index} does not exist; the track has {} arrangement clips", lane.clips.len())
            })?
        }
        (None, Some(scene)) => {
            let column = p.launcher.columns.iter().find(|c| c.track == track).ok_or("the track has no clip slots")?;
            let slot = usize::try_from(scene)
                .ok()
                .and_then(|s| s.checked_sub(1))
                .and_then(|s| column.slots.get(s))
                .ok_or_else(|| format!("scene {scene} does not exist; the track has {} slots", column.slots.len()))?;
            slot.clip.as_ref().ok_or_else(|| format!("the slot of scene {scene} is empty"))?
        }
    };
    Ok((clip.content, track))
}

/// Whether a content lane plays on `track`: its own lane, or a shared pattern's lane for it.
fn plays_on(lane: &ContentLane, track: TrackId) -> bool {
    lane.target.is_none_or(|t| t == track)
}

fn check_midi(p: &Project, content: ContentId) -> Result<(), CommandError> {
    let content = p.content(content).ok_or("the clip's content is missing")?;
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return Err("an audio clip has no notes; give a MIDI clip".into());
    }
    Ok(())
}

/// The notes of the clip `params` names (none for an empty MIDI clip).
fn notes_of<'a>(p: &'a Project, params: &Value) -> Result<&'a [Note], CommandError> {
    let (content, track) = locate(p, params)?;
    check_midi(p, content)?;
    let content = p.content(content).ok_or("the clip's content is missing")?;
    Ok(content
        .lanes
        .iter()
        .filter(|l| plays_on(l, track))
        .find_map(|l| match &l.data {
            LaneData::Notes(notes) => Some(notes.as_slice()),
            LaneData::Audio(_) => None,
        })
        .unwrap_or(&[]))
}

/// The notes of the clip `params` names, to change; an empty MIDI clip gets a note lane.
fn notes_mut<'a>(p: &'a mut Project, params: &Value) -> Result<&'a mut Vec<Note>, CommandError> {
    let (id, track) = locate(p, params)?;
    check_midi(p, id)?;
    let content = p.contents.iter_mut().find(|c| c.id == id).ok_or("the clip's content is missing")?;
    let has_lane = content.lanes.iter().any(|l| plays_on(l, track) && matches!(l.data, LaneData::Notes(_)));
    if !has_lane {
        if !content.lanes.is_empty() {
            return Err("the clip is a pattern with no notes for this track".into());
        }
        content.lanes.push(ContentLane { target: None, data: LaneData::Notes(Vec::new()) });
    }
    content
        .lanes
        .iter_mut()
        .filter(|l| plays_on(l, track))
        .find_map(|l| match &mut l.data {
            LaneData::Notes(notes) => Some(notes),
            LaneData::Audio(_) => None,
        })
        .ok_or_else(|| "the clip has no notes".into())
}

/// The note indices in `params.notes`, each once, in rising order; every one must exist.
fn indices(params: &Value, count: usize) -> Result<Vec<usize>, CommandError> {
    let list = params.get("notes").and_then(Value::as_array).ok_or("notes must be a list of note indices")?;
    if list.is_empty() {
        return Err("notes is empty; give the indices of the notes, from note.list".into());
    }
    if list.len() > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} notes in one command, got {}", list.len()).into());
    }
    let mut out = Vec::with_capacity(list.len());
    for value in list {
        let index = value.as_u64().ok_or("each note must be a whole number, an index from note.list")?;
        let index = usize::try_from(index)
            .ok()
            .filter(|i| *i < count)
            .ok_or_else(|| format!("note {index} does not exist; the clip has {count} notes"))?;
        out.push(index);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// An optional finite number.
fn optional_number(params: &Value, key: &str) -> Result<Option<f64>, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{key} must be a number").into())
        }
    }
}

/// MIDI velocity 1 … 127 to the model's 0 … 1.
fn velocity_of(midi: f64) -> f32 {
    (midi.clamp(1.0, 127.0) / 127.0) as f32
}

/// The model's 0 … 1 velocity as MIDI 1 … 127.
pub fn midi_velocity(velocity: f32) -> u8 {
    if velocity.is_finite() { (f64::from(velocity) * 127.0).round().clamp(1.0, 127.0) as u8 } else { 1 }
}

fn note_json(index: usize, note: &Note) -> Value {
    json!({
        "index": index,
        "time": note.time,
        "length": note.duration,
        "key": note.key,
        "velocity": midi_velocity(note.velocity),
    })
}

pub(crate) fn list(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    Ok(Value::Array(notes.iter().enumerate().map(|(i, n)| note_json(i, n)).collect()))
}

/// One note `note.add` reads: `{time, length, key, velocity?}`.
fn new_note(value: &Value, at: usize) -> Result<Note, CommandError> {
    let object: &Map<String, Value> = value.as_object().ok_or_else(|| format!("note {at} must be an object"))?;
    if let Some(unknown) = object.keys().find(|k| !matches!(k.as_str(), "time" | "length" | "key" | "velocity")) {
        return Err(format!("note {at}: unknown field {unknown:?}; expected time, length, key, velocity").into());
    }
    let number = |key: &str| object.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
    let time = number("time").ok_or_else(|| format!("note {at}: time must be a number of beats"))?;
    if !(0.0..=MAX_BEAT).contains(&time) {
        return Err(format!("note {at}: time must be 0 to {MAX_BEAT} beats, got {time}").into());
    }
    let length = number("length").ok_or_else(|| format!("note {at}: length must be a number of beats"))?;
    if !(length > 0.0 && length <= MAX_BEAT) {
        return Err(format!("note {at}: length must be above 0 and at most {MAX_BEAT} beats, got {length}").into());
    }
    let key = object
        .get("key")
        .and_then(Value::as_u64)
        .and_then(|k| u8::try_from(k).ok())
        .filter(|k| *k <= 127)
        .ok_or_else(|| format!("note {at}: key must be a whole number from 0 to 127"))?;
    let velocity = match object.get("velocity") {
        None | Some(Value::Null) => DEFAULT_VELOCITY,
        Some(v) => v
            .as_f64()
            .filter(|v| (1.0..=127.0).contains(v))
            .ok_or_else(|| format!("note {at}: velocity must be 1 to 127"))?,
    };
    Ok(Note {
        time,
        duration: length,
        key,
        velocity: velocity_of(velocity),
        release_velocity: RELEASE_VELOCITY,
        channel: 0,
    })
}

pub(crate) fn add(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let list = params.get("notes").and_then(Value::as_array).ok_or("notes must be a list of {time, length, key}")?;
    if list.is_empty() {
        return Err("notes is empty; give at least one {time, length, key}".into());
    }
    if list.len() > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} notes in one command, got {}", list.len()).into());
    }
    let added = list.iter().enumerate().map(|(i, v)| new_note(v, i)).collect::<Result<Vec<_>, _>>()?;
    let notes = notes_mut(&mut session.project, params)?;
    let total = notes.len().saturating_add(added.len());
    if total > MAX_NOTES_PER_CLIP {
        return Err(format!("a clip holds at most {MAX_NOTES_PER_CLIP} notes; this would make {total}").into());
    }
    let first = notes.len();
    notes.extend(added);
    Ok(json!({ "added": (first..notes.len()).collect::<Vec<_>>(), "count": notes.len() }))
}

pub(crate) fn delete(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_mut(&mut session.project, params)?;
    let gone = indices(params, notes.len())?;
    // Removed from the back, so the indices still to remove stay where they are.
    for &index in gone.iter().rev() {
        notes.remove(index);
    }
    Ok(json!({ "deleted": gone, "count": notes.len() }))
}

pub(crate) fn move_notes(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = optional_number(params, "beats")?.unwrap_or(0.0);
    let semitones = match params.get("semitones") {
        None | Some(Value::Null) => 0,
        Some(v) => {
            v.as_i64().filter(|s| (-127..=127).contains(s)).ok_or("semitones must be a whole number, -127 to 127")?
        }
    };
    if params.get("beats").is_none() && params.get("semitones").is_none() {
        return Err("give beats (in time) or semitones (in pitch)".into());
    }
    if beats.abs() > MAX_BEAT {
        return Err(format!("beats must be within ±{MAX_BEAT}").into());
    }
    let notes = notes_mut(&mut session.project, params)?;
    let moved = indices(params, notes.len())?;
    let mut changed = Vec::with_capacity(moved.len());
    for &index in &moved {
        let note = notes.get(index).ok_or("a note is missing")?;
        let mut time = note.time + beats;
        if (-EPSILON..0.0).contains(&time) {
            time = 0.0;
        }
        if !(0.0..=MAX_BEAT).contains(&time) {
            return Err(format!("note {index} would start at beat {time}, outside 0 to {MAX_BEAT}").into());
        }
        let key = u8::try_from(i64::from(note.key) + semitones)
            .ok()
            .filter(|k| *k <= 127)
            .ok_or_else(|| format!("note {index} would leave the keys 0 to 127"))?;
        changed.push((index, Note { time, key, ..*note }));
    }
    for (index, note) in changed {
        if let Some(slot) = notes.get_mut(index) {
            *slot = note;
        }
    }
    Ok(json!({ "notes": moved }))
}

pub(crate) fn resize(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (length, by) = (optional_number(params, "length")?, optional_number(params, "by")?);
    if length.is_some() == by.is_some() {
        return Err("give length (beats) or by (beats to add), one of them".into());
    }
    let notes = notes_mut(&mut session.project, params)?;
    let resized = indices(params, notes.len())?;
    let mut lengths = Vec::with_capacity(resized.len());
    for &index in &resized {
        let note = notes.get(index).ok_or("a note is missing")?;
        let new = length.unwrap_or(note.duration + by.unwrap_or(0.0));
        if !(new > 0.0 && new <= MAX_BEAT) {
            return Err(
                format!("note {index} would be {new} beats long; a length is above 0 and at most {MAX_BEAT}").into()
            );
        }
        lengths.push((index, new));
    }
    for (index, new) in lengths {
        if let Some(note) = notes.get_mut(index) {
            note.duration = new;
        }
    }
    Ok(json!({ "notes": resized }))
}

pub(crate) fn set_velocity(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (velocity, by) = (optional_number(params, "velocity")?, optional_number(params, "by")?);
    match (velocity, by) {
        (Some(_), Some(_)) | (None, None) => return Err("give velocity (1 to 127) or by (to add), one of them".into()),
        (Some(v), None) if !(1.0..=127.0).contains(&v) => {
            return Err(format!("velocity must be 1 to 127, got {v}").into());
        }
        (None, Some(b)) if b.abs() > 127.0 => return Err(format!("by must be within ±127, got {b}").into()),
        _ => {}
    }
    let notes = notes_mut(&mut session.project, params)?;
    let changed = indices(params, notes.len())?;
    for &index in &changed {
        if let Some(note) = notes.get_mut(index) {
            let midi = velocity.unwrap_or_else(|| f64::from(midi_velocity(note.velocity)) + by.unwrap_or(0.0));
            note.velocity = velocity_of(midi.round());
        }
    }
    Ok(json!({ "notes": changed }))
}

/// The quantise grid `params.grid` names, in beats: a value as `launcher.setQuantization` takes it,
/// with bars as long as the project's time signature makes them.
fn quantize_step(p: &Project, params: &Value) -> Result<f64, CommandError> {
    let text = params.get("grid").and_then(Value::as_str).ok_or("grid must be a text such as \"1/16\" or \"1/8T\"")?;
    let step = match parse_quantization(text)? {
        Quantization::None => return Err("grid none has no lines to quantise to; give a value such as \"1/16\"".into()),
        Quantization::Bars(bars) => {
            let ts = p.transport.time_signature;
            let bar = if ts.denominator == 0 {
                4.0
            } else {
                f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator)
            };
            f64::from(bars) * bar
        }
        Quantization::Beats(beats) => beats,
    };
    if step.is_finite() && step > 0.0 { Ok(step) } else { Err(format!("grid {text:?} has no step").into()) }
}

/// The grid line nearest to `beat`; `beat` itself when it is already on one, so a note on the grid
/// keeps its exact value.
fn nearest_line(beat: f64, step: f64) -> f64 {
    let line = (beat / step).round() * step;
    if (line - beat).abs() <= EPSILON { beat } else { line }
}

/// `from` moved towards `to` by `amount` (0 … 1); `to` itself at 1.
fn towards(from: f64, to: f64, amount: f64) -> f64 {
    if amount >= 1.0 { to } else { from + (to - from) * amount }
}

pub(crate) fn quantize(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let step = quantize_step(&session.project, params)?;
    let amount = optional_number(params, "amount")?.unwrap_or(100.0);
    if !(0.0..=100.0).contains(&amount) {
        return Err(format!("amount must be 0 to 100 percent, got {amount}").into());
    }
    let amount = amount / 100.0;
    let ends = match params.get("ends") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("ends must be true or false")?,
    };
    let all = matches!(params.get("notes"), None | Some(Value::Null));
    // Every note of an empty clip is none: nothing to change, and no note lane to add.
    if all && notes_of(&session.project, params)?.is_empty() {
        return Ok(json!({ "notes": [], "moved": 0 }));
    }
    let notes = notes_mut(&mut session.project, params)?;
    let chosen = if all { (0..notes.len()).collect() } else { indices(params, notes.len())? };
    let mut changed = Vec::with_capacity(chosen.len());
    for &index in &chosen {
        let note = notes.get(index).ok_or("a note is missing")?;
        let start = nearest_line(note.time, step);
        let time = towards(note.time, start, amount).max(0.0);
        if time > MAX_BEAT {
            return Err(format!("note {index} would start at beat {time}, after {MAX_BEAT}").into());
        }
        let duration = if ends {
            let end = note.time + note.duration;
            let mut line = nearest_line(end, step);
            if line <= start + EPSILON {
                // On or before its quantised start: the end goes to the first line after it.
                line = ((start / step + EPSILON).floor() + 1.0) * step;
            }
            towards(end, line, amount) - time
        } else {
            note.duration
        };
        if !(duration > 0.0 && duration <= MAX_BEAT) {
            return Err(format!("note {index} would be {duration} beats long").into());
        }
        if time != note.time || duration != note.duration {
            changed.push((index, time, duration));
        }
    }
    let moved = changed.len();
    for (index, time, duration) in changed {
        if let Some(note) = notes.get_mut(index) {
            note.time = time;
            note.duration = duration;
        }
    }
    Ok(json!({ "notes": chosen, "moved": moved }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocities_round_trip_through_midi_values() {
        for midi in 1..=127u8 {
            assert_eq!(midi_velocity(velocity_of(f64::from(midi))), midi);
        }
        assert_eq!(midi_velocity(f32::NAN), 1);
        assert_eq!(midi_velocity(0.0), 1);
        assert_eq!(midi_velocity(2.0), 127);
    }
}
