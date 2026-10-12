//! Adding tracks, and placing samples and MIDI files as clips (`docs/spec/browser.md` › Files):
//! what the browser's drops and agents do. Each command is one undo step. A sample's decoded
//! audio joins the session's pool and reaches a playing engine with the next graph (ADR 0011).

use std::path::{Path, PathBuf};

use loopwerk_device_api::Role;
use loopwerk_engine::AudioData;
use loopwerk_media::info::{FileKind, Tempo, TempoSource, kind_of};
use loopwerk_model::{
    ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds,
    ContentLane, LaneData, LauncherColumn, LoopRange, Note, Project, Scene, Slot, StretchMode, Track, TrackId,
    WarpMarker,
};
use serde_json::{Value, json};

use crate::commands::{make_room, number, track_at};
use crate::{CommandError, Session};

/// Files one command places at most.
pub const MAX_FILES: usize = 256;
/// Scenes a drop may add up to.
const MAX_SCENES: usize = 1024;
/// Tracks a project may have before adding refuses.
const MAX_TRACKS: usize = 1024;

/// What a new track plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Midi,
}

impl TrackKind {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "audio" => Some(Self::Audio),
            "midi" | "notes" => Some(Self::Midi),
            _ => None,
        }
    }

    fn contents(self) -> ContentKinds {
        match self {
            Self::Audio => ContentKinds { notes: false, audio: true },
            Self::Midi => ContentKinds { notes: true, audio: false },
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Midi => "MIDI",
        }
    }

    fn of(track: &Track) -> Option<Self> {
        if track.content.audio {
            Some(Self::Audio)
        } else if track.content.notes {
            Some(Self::Midi)
        } else {
            None
        }
    }
}

/// Where new tracks go when no track is named: after the last track that is neither a return
/// nor in a group (before the returns).
fn end_of_regular_tracks(p: &Project) -> usize {
    let is_return = |t: &Track| {
        t.channel.and_then(|c| p.channels.iter().find(|ch| ch.id == c)).is_some_and(|c| c.role == ChannelRole::Effect)
    };
    p.tracks.iter().rposition(|t| !is_return(t)).map_or(0, |i| i + 1)
}

/// Adds a track of `kind` named `name` with its channel, clip slots for every scene and an
/// arrangement lane, right after `after` (in its group) or at the end of the regular tracks.
pub(crate) fn insert_track(
    p: &mut Project,
    kind: TrackKind,
    name: &str,
    after: Option<TrackId>,
) -> Result<TrackId, CommandError> {
    if p.tracks.len() >= MAX_TRACKS {
        return Err(format!("a project holds at most {MAX_TRACKS} tracks").into());
    }
    let id = TrackId(p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many tracks")?);
    let channel =
        ChannelId(p.channels.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many channels")?);
    let (at, parent) = match after {
        Some(after) => {
            let index = p.tracks.iter().position(|t| t.id == after).ok_or("the track to add after is missing")?;
            let parent = p.tracks.get(index).and_then(|t| t.parent);
            // After the track and anything in it (a group's tracks).
            let mut end = index + 1;
            while let Some(next) = p.tracks.get(end) {
                if within(p, next.id, after) {
                    end += 1;
                } else {
                    break;
                }
            }
            (end, parent)
        }
        None => (end_of_regular_tracks(p), None),
    };
    // Into its group, or into the main channel like the other tracks.
    let main = p.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| c.id);
    let output = match parent {
        Some(group) => p.tracks.iter().find(|t| t.id == group).and_then(|t| t.channel),
        None => main,
    };
    p.channels.push(Channel {
        id: channel,
        name: name.to_string(),
        role: ChannelRole::Regular,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Default::default(),
    });
    let track = Track {
        id,
        name: name.to_string(),
        color: None,
        content: kind.contents(),
        channel: Some(channel),
        parent,
        extensions: Default::default(),
    };
    let at = at.min(p.tracks.len());
    p.tracks.insert(at, track);
    let slots = p.launcher.scenes.len();
    p.launcher.columns.push(LauncherColumn { track: id, slots: vec![Slot { clip: None, has_stop: true }; slots] });
    p.arrangement.lanes.push(ArrangementLane { name: name.to_string(), track: Some(id), clips: Vec::new() });
    Ok(id)
}

/// Whether `track` is inside the group `group` (at any depth).
fn within(p: &Project, track: TrackId, group: TrackId) -> bool {
    let mut current = p.tracks.iter().find(|t| t.id == track).and_then(|t| t.parent);
    let mut steps = 0;
    while let Some(parent) = current {
        if parent == group {
            return true;
        }
        steps += 1;
        if steps > p.tracks.len() {
            return false;
        }
        current = p.tracks.iter().find(|t| t.id == parent).and_then(|t| t.parent);
    }
    false
}

/// "Audio 3": the next number among tracks of that kind.
fn numbered_name(p: &Project, kind: TrackKind) -> String {
    let count = p.tracks.iter().filter(|t| TrackKind::of(t) == Some(kind)).count();
    format!("{} {}", kind.default_name(), count + 1)
}

fn text<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| format!("{name} must be a text"))?,
        )),
    }
}

fn flag(params: &Value, name: &str) -> Result<bool, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| format!("{name} must be true or false").into()),
    }
}

/// `track.add`: a new audio or MIDI track, with a device when one is named.
pub(crate) fn add_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let device = text(params, "device")?;
    let preset = text(params, "preset")?;
    // What the device is decides the kind when none is given.
    let device_role = match (device, preset) {
        (_, Some(_)) => Some(Role::Instrument),
        (Some(id), None) => match loopwerk_device_registry::info(id) {
            Some(info) => Some(info.role),
            None => {
                let env = session.plugins.env().ok_or_else(|| format!("no built-in device or plug-in {id:?}"))?;
                let plugin =
                    env.catalog.find(id).or_else(|| env.catalog.plugins().find(|p| p.spec() == id)).ok_or_else(
                        || format!("no built-in device or plug-in {id:?}; devices.list and plugins.list list them"),
                    )?;
                Some(if crate::is_instrument(plugin) { Role::Instrument } else { Role::Effect })
            }
        },
        (None, None) => None,
    };
    let kind = match text(params, "kind")? {
        Some(k) => TrackKind::parse(k).ok_or("kind must be audio or midi")?,
        None => match device_role {
            Some(Role::Instrument) => TrackKind::Midi,
            Some(Role::Effect) => TrackKind::Audio,
            None => return Err("kind must be audio or midi (or name a device)".into()),
        },
    };
    if kind == TrackKind::Audio && device_role == Some(Role::Instrument) {
        return Err("an instrument goes on a MIDI track".into());
    }
    let after = match params.get("after") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "after")?),
    };
    let name = match text(params, "name")? {
        Some(n) => n.to_string(),
        None => match (device, preset) {
            (_, Some(path)) => file_stem(Path::new(path)),
            (Some(id), None) => loopwerk_device_registry::info(id).map_or_else(
                || session.plugins.env().and_then(|e| e.catalog.find(id).map(|p| p.name.clone())).unwrap_or_default(),
                |i| i.name.to_string(),
            ),
            (None, None) => numbered_name(&session.project, kind),
        },
    };
    let name = if name.trim().is_empty() { numbered_name(&session.project, kind) } else { name };
    let track = insert_track(&mut session.project, kind, &name, after)?;
    let mut added = Value::Null;
    if device.is_some() || preset.is_some() {
        let mut device_params = json!({ "track": track.0 });
        if let Some(path) = preset {
            device_params["preset"] = json!(path);
            if let Some(voice) = params.get("voice") {
                device_params["voice"] = voice.clone();
            }
        }
        added = match device {
            Some(id) if loopwerk_device_registry::info(id).is_some() => {
                device_params["id"] = json!(id);
                crate::devices::add(session, &device_params)?
            }
            Some(id) if preset.is_none() => {
                device_params["plugin"] = json!(id);
                crate::devices::add_plugin(session, &device_params)?
            }
            _ => crate::devices::add(session, &device_params)?,
        };
    }
    Ok(
        json!({ "track": track.0, "name": name, "kind": match kind { TrackKind::Audio => "audio", TrackKind::Midi => "midi" }, "device": added }),
    )
}

/// A file's name without its extension.
fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One clip to place, made from a file.
struct Source {
    kind: TrackKind,
    name: String,
    /// Content length in beats (the clip's length too).
    length: f64,
    lane: LaneData,
    /// Whether the clip loops in a slot.
    loops: bool,
}

/// `clip.addFiles`: samples and MIDI files as clips on a track, or on new tracks.
pub(crate) fn add_files(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let paths: Vec<String> = params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths must be a list of file paths")?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or("paths must be a list of file paths"))
        .collect::<Result<_, _>>()?;
    if paths.is_empty() {
        return Err("paths is empty".into());
    }
    let start = match params.get("start") {
        None | Some(Value::Null) => None,
        Some(_) => Some(number(params, "start")?),
    };
    let scene = match params.get("scene") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or("scene must be a whole number from 1")?,
        ),
    };
    match (start, scene) {
        (Some(_), Some(_)) | (None, None) => {
            return Err("give either start (a beat in the arrangement) or scene (a row of clip slots)".into());
        }
        (Some(s), None) if !(0.0..=loopwerk_engine::MAX_START_BEAT).contains(&s) => {
            return Err(format!("start must be from 0 to {}", loopwerk_engine::MAX_START_BEAT).into());
        }
        (None, Some(s)) if s >= MAX_SCENES => return Err(format!("scene must be at most {MAX_SCENES}").into()),
        _ => {}
    }
    let separate = flag(params, "separate")?;
    let stack = flag(params, "stack")?;
    let stretch = match text(params, "warp")? {
        None => StretchMode::Rhythmic,
        Some(mode) => {
            stretch_mode(mode).ok_or("warp must be repitch, rhythmic, tonal, texture, smooth or high-quality")?
        }
    };
    let target = match params.get("track") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "track")?),
    };
    let target_kind = match target {
        Some(t) => {
            let track = session.project.tracks.iter().find(|x| x.id == t).ok_or("the track is missing")?;
            Some(
                TrackKind::of(track)
                    .ok_or_else(|| format!("track {:?} holds no clips (a group or a return)", track.name))?,
            )
        }
        None => None,
    };

    // Read every file; the ones that cannot be read are skipped with a reason.
    let tempo = session.project.transport.tempo_bpm;
    let bar = bar_beats(&session.project);
    let mut sources: Vec<Source> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for path in paths.iter().take(MAX_FILES) {
        match read_sources(session, Path::new(path), tempo, bar, stretch) {
            Ok(mut list) => sources.append(&mut list),
            Err(reason) => skipped.push(json!({ "path": path, "reason": reason })),
        }
    }
    for path in paths.iter().skip(MAX_FILES) {
        skipped.push(json!({ "path": path, "reason": format!("more than {MAX_FILES} files in one go") }));
    }
    if sources.is_empty() {
        let reasons: Vec<String> = skipped
            .iter()
            .map(|s| format!("{}: {}", s["path"].as_str().unwrap_or(""), s["reason"].as_str().unwrap_or("")))
            .collect();
        return Err(format!("nothing could be placed ({})", reasons.join("; ")).into());
    }

    // Which track each clip goes on.
    let p = &mut session.project;
    let mut new_tracks: Vec<TrackId> = Vec::new();
    let mut groups: Vec<(TrackId, Vec<Source>)> = Vec::new();
    let mut last = target;
    let (fits, rest): (Vec<Source>, Vec<Source>) =
        sources.into_iter().partition(|s| target_kind.is_some_and(|k| k == s.kind));
    if let Some(target) = target
        && !fits.is_empty()
    {
        if separate {
            let mut iter = fits.into_iter();
            if let Some(first) = iter.next() {
                groups.push((target, vec![first]));
            }
            for source in iter {
                let id = insert_track(p, source.kind, &source.name, last)?;
                new_tracks.push(id);
                last = Some(id);
                groups.push((id, vec![source]));
            }
        } else {
            groups.push((target, fits));
        }
    }
    // The rest go on new tracks: one per kind, or one each.
    if separate {
        for source in rest {
            let id = insert_track(p, source.kind, &source.name, last)?;
            new_tracks.push(id);
            if target.is_some() {
                last = Some(id);
            }
            groups.push((id, vec![source]));
        }
    } else {
        let (audio, midi): (Vec<Source>, Vec<Source>) = rest.into_iter().partition(|s| s.kind == TrackKind::Audio);
        for (kind, of_kind) in [(TrackKind::Audio, audio), (TrackKind::Midi, midi)] {
            if of_kind.is_empty() {
                continue;
            }
            let name = match of_kind.as_slice() {
                [one] => one.name.clone(),
                _ => numbered_name(p, kind),
            };
            let id = insert_track(p, kind, &name, last)?;
            new_tracks.push(id);
            if target.is_some() {
                last = Some(id);
            }
            groups.push((id, of_kind));
        }
    }

    // Place them.
    let mut placed: Vec<Value> = Vec::new();
    for (track, list) in groups {
        let mut at = start.unwrap_or(0.0);
        for (i, source) in list.into_iter().enumerate() {
            let id =
                ContentId(p.contents.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many clips")?);
            let length = source.length;
            p.contents.push(ClipContent {
                id,
                name: source.name.clone(),
                length,
                lanes: vec![ContentLane { target: None, data: source.lane }],
                extensions: Default::default(),
            });
            let mut clip = Clip {
                content: id,
                start: 0.0,
                length,
                offset: 0.0,
                loop_range: None,
                enabled: true,
                fade_in: 0.0,
                fade_out: 0.0,
                extensions: Default::default(),
            };
            match scene {
                Some(first) => {
                    let row = first + i;
                    if row >= MAX_SCENES {
                        skipped.push(json!({ "path": source.name, "reason": format!("past scene {MAX_SCENES}") }));
                        continue;
                    }
                    while p.launcher.scenes.len() <= row {
                        p.launcher.scenes.push(Scene::default());
                        for column in &mut p.launcher.columns {
                            column.slots.push(Slot { clip: None, has_stop: true });
                        }
                    }
                    if source.loops {
                        clip.loop_range = Some(LoopRange { start: 0.0, end: length });
                    }
                    let column = p
                        .launcher
                        .columns
                        .iter_mut()
                        .find(|c| c.track == track)
                        .ok_or("the track has no clip slots")?;
                    let slot = column.slots.get_mut(row).ok_or("the slot is missing")?;
                    slot.clip = Some(clip);
                    placed.push(json!({ "track": track.0, "scene": row + 1, "name": source.name, "length": length }));
                }
                None => {
                    clip.start = at;
                    let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
                        Some(lane) => lane,
                        None => {
                            let name =
                                p.tracks.iter().find(|t| t.id == track).map(|t| t.name.clone()).unwrap_or_default();
                            p.arrangement.lanes.push(ArrangementLane { name, track: Some(track), clips: Vec::new() });
                            p.arrangement.lanes.len() - 1
                        }
                    };
                    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
                    if !stack {
                        make_room(&mut lane.clips, at, at + length);
                    }
                    lane.clips.push(clip);
                    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
                    placed.push(json!({ "track": track.0, "start": at, "name": source.name, "length": length }));
                    at += length;
                }
            }
        }
    }
    let tracks: Vec<u32> = new_tracks.iter().map(|t| t.0).collect();
    Ok(json!({ "clips": placed, "tracks": tracks, "skipped": skipped }))
}

fn stretch_mode(text: &str) -> Option<StretchMode> {
    Some(match text {
        "repitch" => StretchMode::Repitch,
        "rhythmic" | "beats" => StretchMode::Rhythmic,
        "tonal" => StretchMode::Tonal,
        "texture" => StretchMode::Texture,
        "smooth" => StretchMode::Smooth,
        "high-quality" | "pro" => StretchMode::HighQuality,
        _ => return None,
    })
}

fn bar_beats(p: &Project) -> f64 {
    let ts = p.transport.time_signature;
    if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
}

/// The clips one file gives: one for a sample, one per MIDI track for a MIDI file.
fn read_sources(
    session: &mut Session,
    path: &Path,
    tempo: f64,
    bar: f64,
    stretch: StretchMode,
) -> Result<Vec<Source>, String> {
    let absolute = absolute(path);
    if !absolute.is_file() {
        return Err("not found".into());
    }
    let name = file_stem(&absolute);
    match kind_of(&absolute) {
        Some(FileKind::Sample) => {
            let data = session.decoded_audio(&absolute)?;
            let duration = data.frames() as f64 / f64::from(data.sample_rate.max(1));
            if !(duration.is_finite() && duration > 0.0) {
                return Err("no audio".into());
            }
            let tempo_of_file = file_tempo(&absolute, &data);
            let key = absolute.display().to_string();
            let (warped, beats, markers) = match tempo_of_file {
                Some(t) => {
                    let beats = duration * t.bpm / 60.0;
                    (
                        true,
                        beats,
                        vec![
                            WarpMarker { time: 0.0, audio_time: 0.0 },
                            WarpMarker { time: beats, audio_time: duration },
                        ],
                    )
                }
                None => (false, duration * tempo / 60.0, Vec::new()),
            };
            if !(beats.is_finite() && beats > 0.0) {
                return Err("its length in beats is not usable".into());
            }
            let region = AudioRegion {
                file: key.clone(),
                absolute_file: Some(key),
                sample_rate: Some(data.sample_rate),
                channels: Some(data.channels),
                duration_seconds: Some(duration),
                warped,
                stretch: if warped { stretch } else { StretchMode::default() },
                warp_markers: markers,
                gain: 1.0,
                transpose: 0.0,
            };
            Ok(vec![Source {
                kind: TrackKind::Audio,
                name,
                length: beats,
                lane: LaneData::Audio(region),
                loops: warped,
            }])
        }
        Some(FileKind::Midi) => {
            let midi = loopwerk_media::midi::read_file(&absolute)?;
            if midi.tracks.is_empty() {
                return Err("no notes".into());
            }
            Ok(midi
                .tracks
                .into_iter()
                .map(|track| {
                    let end = track.end();
                    let length = ((end / bar).ceil().max(1.0)) * bar;
                    let clip_name = match &track.name {
                        Some(n) => format!("{name} – {n}"),
                        None => name.clone(),
                    };
                    let notes: Vec<Note> = track.notes;
                    Source { kind: TrackKind::Midi, name: clip_name, length, lane: LaneData::Notes(notes), loops: true }
                })
                .collect())
        }
        Some(FileKind::Preset) => Err("a preset goes on a track as a device".into()),
        Some(FileKind::Project) => Err("a project is opened, not placed".into()),
        None => Err("not a sample or a MIDI file".into()),
    }
}

/// A sample's tempo: its metadata or name, else detected (`docs/spec/browser.md` › Tempo).
pub fn file_tempo(path: &Path, data: &AudioData) -> Option<Tempo> {
    let from_file = loopwerk_media::info::sample_info(path).ok().and_then(|i| i.tempo);
    from_file.or_else(|| loopwerk_media::tempo::detect(data).map(|bpm| Tempo { bpm, source: TempoSource::Detected }))
}

/// `path` made absolute against the working folder, without touching the disk.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
