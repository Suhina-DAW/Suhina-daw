//! `preview.play` and `preview.stop` (ADR 0011): the browser's preview of a sample or a MIDI
//! file, through the attached engine. They never change the project.

use std::path::Path;

use loopwerk_media::info::{FileKind, kind_of};
use loopwerk_model::TrackId;
use serde_json::{Value, json};

use crate::commands::track_at;
use crate::files::absolute;
use crate::{CommandError, Session};

/// The preview's level when none is given: −6 dB.
const DEFAULT_GAIN_DB: f64 = -6.0;

pub(crate) fn play(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let path = params.get("path").and_then(Value::as_str).ok_or("path must be the file to preview")?;
    let path = absolute(Path::new(path));
    let from = match params.get("from") {
        None | Some(Value::Null) => 0.0,
        Some(v) => v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).ok_or("from must be seconds, at least 0")?,
    };
    let gain_db = match params.get("gain") {
        None | Some(Value::Null) => DEFAULT_GAIN_DB,
        Some(v) => v.as_f64().filter(|g| g.is_finite() && *g <= 12.0).ok_or("gain must be decibels, at most 12")?,
    };
    let gain = 10f64.powf(gain_db / 20.0) as f32;
    let track: Option<TrackId> = match params.get("track") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "track")?),
    };
    match kind_of(&path) {
        Some(FileKind::Sample) => {
            let data = session.preview_audio(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let seconds = data.frames() as f64 / f64::from(data.sample_rate.max(1));
            session.player_mut()?.controller_mut().preview_sample(data, from, gain).map_err(CommandError)?;
            Ok(json!({ "path": path.display().to_string(), "kind": "sample", "seconds": seconds }))
        }
        Some(FileKind::Midi) => {
            let midi = loopwerk_media::midi::read_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let bpm = midi.tempo_bpm.unwrap_or(120.0);
            let notes: Vec<(f64, f64, u8, f32)> = midi
                .tracks
                .iter()
                .flat_map(|t| t.notes.iter())
                .map(|n| (n.time, n.duration, n.key, n.velocity))
                .filter(|n| n.0 * 60.0 / bpm >= from)
                .map(|(time, duration, key, velocity)| (time - from * bpm / 60.0, duration, key, velocity))
                .collect();
            let player = session.player_mut()?;
            let rate = player.sample_rate();
            let controller = player.controller_mut();
            let fallback = controller.preview_instrument();
            let prepared = loopwerk_engine::preview_notes(notes, bpm, rate, track, fallback);
            let seconds = prepared.length as f64 / f64::from(rate.max(1));
            controller.preview_notes(prepared, gain).map_err(CommandError)?;
            Ok(json!({ "path": path.display().to_string(), "kind": "midi", "seconds": seconds, "bpm": bpm }))
        }
        _ => Err(format!("{}: only samples and MIDI files can be previewed", path.display()).into()),
    }
}

pub(crate) fn stop(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.player_mut()?.controller_mut().preview_stop().map_err(CommandError)?;
    Ok(json!({ "stopped": true }))
}
