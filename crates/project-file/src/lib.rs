//! Suhina's own project file, `.suhina`: the project model (ADR 0001) as versioned JSON, so a
//! project can be saved and opened again with nothing lost, and read by people and agents
//! (ADR 0010, `docs/spec/suhina-format.md`).
//!
//! ```json
//! { "format": "suhina", "version": 1, "project": { "transport": { … }, "tracks": [ … ], … } }
//! ```
//!
//! Files saved before October 2026 end in `.locr` and name the format `"locr"`; they are the
//! same format and still open ([`OLD_EXTENSION`], [`is_project_file`]).
//!
//! Binary data (plug-in state, extensions) is base64. Audio files are referred to by path,
//! relative to the project file; [`rebase_audio`] moves those paths when a project is saved
//! somewhere else.

use std::fmt;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value, json};
use suhina_model::{
    Arrangement, ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId,
    ContentKinds, ContentLane, Device, DeviceKind, DeviceParam, Extension, Extensions, FadeCurve, FadeShape, LaneData,
    LaunchMode, LaunchQuantization, LaunchSettings, Launcher, LauncherColumn, LoopRange, Note, Project, Rgb, Scene,
    Send, Slot, StretchMode, TimeSignature, Track, TrackId, Transport, WarpMarker,
};

/// The file extension, without the dot.
pub const EXTENSION: &str = "suhina";
/// The extension of project files saved before October 2026, without the dot: the same format,
/// still read, never chosen for a new file.
pub const OLD_EXTENSION: &str = "locr";
/// The `format` every file names.
const FORMAT: &str = "suhina";
/// The `format` files saved before October 2026 name; read as [`FORMAT`].
const OLD_FORMAT: &str = "locr";
/// The version this crate writes, and the newest it reads.
pub const VERSION: u64 = 1;
/// A larger file is refused before it is parsed.
const MAX_FILE: u64 = 512 * 1024 * 1024;

/// Why a file could not be read as a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileError(pub String);

impl fmt::Display for ProjectFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProjectFileError {}

impl From<String> for ProjectFileError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for ProjectFileError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

/// Whether `path` names a project file by its extension: `.suhina`, or `.locr` from before
/// October 2026 (either case).
pub fn is_project_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case(OLD_EXTENSION))
}

/// Reads a project file.
pub fn read_file(path: &Path) -> Result<Project, ProjectFileError> {
    let file = std::fs::File::open(path).map_err(|e| format!("could not open the project: {e}"))?;
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).map_err(|e| format!("could not read the project: {e}"))?;
    if text.len() as u64 > MAX_FILE {
        return Err(format!("the project is larger than {} MiB", MAX_FILE / 1024 / 1024).into());
    }
    from_json(&text)
}

/// Writes `project` to `path`: to a file beside it first, then moved over it, so a failed save
/// never leaves half a project.
pub fn write_file(path: &Path, project: &Project) -> Result<(), ProjectFileError> {
    let failed = |e: std::io::Error| ProjectFileError(format!("could not save {}: {e}", path.display()));
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".saving");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, to_json(project)).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _gone = std::fs::remove_file(&temporary);
        failed(e)
    })
}

/// The project as the text of a `.suhina` file.
pub fn to_json(project: &Project) -> String {
    let file = json!({ "format": FORMAT, "version": VERSION, "project": project_value(project) });
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Reads the text of a `.suhina` file, or of a `.locr` one saved before October 2026. The
/// project must validate (ids unique, references resolve), as an imported one must.
pub fn from_json(text: &str) -> Result<Project, ProjectFileError> {
    let root = serde_json::from_str::<Value>(text).map_err(|e| format!("the project is not JSON: {e}"))?;
    let root = root.as_object().ok_or("a project file is a JSON object")?;
    if !matches!(root.get("format").and_then(Value::as_str), Some(FORMAT | OLD_FORMAT)) {
        return Err("this is not a Suhina project (its format is not \"suhina\")".to_string().into());
    }
    let version = root.get("version").and_then(Value::as_u64).ok_or("the project's version is missing")?;
    if version == 0 || version > VERSION {
        return Err(format!("the project is version {version}; this Suhina reads up to version {VERSION}").into());
    }
    let project = read_project(root.get("project").ok_or("the project is missing")?)?;
    project.validate().map_err(|errors| {
        let list: Vec<String> = errors.iter().take(5).map(ToString::to_string).collect();
        format!("the project does not hold together: {}", list.join("; "))
    })?;
    Ok(project)
}

/// Moves the audio file paths of `project` from `from` (the folder they are relative to now)
/// to `to` (the folder of the file it is saved in): each relative path is written relative to
/// `to` where both are on one drive, and the absolute path is kept for when it does not resolve.
pub fn rebase_audio(project: &mut Project, from: &Path, to: &Path) {
    for content in &mut project.contents {
        for lane in &mut content.lanes {
            let LaneData::Audio(region) = &mut lane.data else { continue };
            let here = from.join(&region.file);
            let file = if here.is_file() {
                here
            } else {
                match region.absolute_file.as_deref().map(PathBuf::from) {
                    Some(absolute) if absolute.is_file() => absolute,
                    _ => here,
                }
            };
            let absolute = normalise(&std::path::absolute(&file).unwrap_or(file));
            if let Some(relative) = relative_to(&absolute, &normalise(&std::path::absolute(to).unwrap_or(to.into()))) {
                region.file = slashes(&relative);
            }
            region.absolute_file = Some(absolute.display().to_string());
        }
    }
}

/// `path` with `.` and `..` resolved without touching the disk.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to the folder `base`, both absolute; `None` across drives.
fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    let (mut a, mut b) = (path.components().peekable(), base.components().peekable());
    match (a.peek(), b.peek()) {
        (Some(Component::Prefix(x)), Some(Component::Prefix(y))) if x != y => return None,
        _ => {}
    }
    while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
        if x != y {
            break;
        }
        a.next();
        b.next();
    }
    let mut relative = PathBuf::new();
    for _ in b {
        relative.push("..");
    }
    relative.extend(a);
    Some(relative)
}

/// A path written with `/`, the same on every system.
fn slashes(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

// Writing.

fn project_value(p: &Project) -> Value {
    let mut out = Map::new();
    out.insert(
        "transport".into(),
        json!({
            "tempo_bpm": number(p.transport.tempo_bpm),
            "time_signature": [p.transport.time_signature.numerator, p.transport.time_signature.denominator],
        }),
    );
    out.insert("tracks".into(), p.tracks.iter().map(track_value).collect());
    out.insert("channels".into(), p.channels.iter().map(channel_value).collect());
    out.insert("contents".into(), p.contents.iter().map(content_value).collect());
    out.insert(
        "arrangement".into(),
        json!({ "lanes": p.arrangement.lanes.iter().map(|lane| json!({
            "name": lane.name,
            "track": lane.track.map(|t| t.0),
            "clips": lane.clips.iter().map(clip_value).collect::<Vec<_>>(),
        })).collect::<Vec<_>>() }),
    );
    out.insert(
        "launcher".into(),
        json!({
            "scenes": p.launcher.scenes.iter().map(|s| json!({ "name": s.name, "tempo_bpm": s.tempo_bpm.map(number) })).collect::<Vec<_>>(),
            "columns": p.launcher.columns.iter().map(|c| json!({
                "track": c.track.0,
                "slots": c.slots.iter().map(|s| json!({ "clip": s.clip.as_ref().map(clip_value), "has_stop": s.has_stop })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    );
    with_extensions(&mut out, &p.extensions);
    Value::Object(out)
}

fn track_value(t: &Track) -> Value {
    let mut out = json!({
        "id": t.id.0,
        "name": t.name,
        "color": t.color.map(|Rgb(r, g, b)| format!("#{r:02X}{g:02X}{b:02X}")),
        "notes": t.content.notes,
        "audio": t.content.audio,
        "channel": t.channel.map(|c| c.0),
        "parent": t.parent.map(|p| p.0),
    });
    if let Some(map) = out.as_object_mut() {
        with_extensions(map, &t.extensions);
    }
    out
}

fn channel_value(c: &Channel) -> Value {
    let role = match c.role {
        ChannelRole::Regular => "regular",
        ChannelRole::Master => "master",
        ChannelRole::Effect => "effect",
        ChannelRole::Submix => "submix",
    };
    let mut out = json!({
        "id": c.id.0,
        "name": c.name,
        "role": role,
        "volume": number(c.volume),
        "pan": number(c.pan),
        "mute": c.mute,
        "solo": c.solo,
        "output": c.output.map(|o| o.0),
        "sends": c.sends.iter().map(|s| json!({ "target": s.target.0, "level": number(s.level), "pre_fader": s.pre_fader })).collect::<Vec<_>>(),
        "devices": c.devices.iter().map(device_value).collect::<Vec<_>>(),
    });
    if let Some(map) = out.as_object_mut() {
        with_extensions(map, &c.extensions);
    }
    out
}

fn device_value(d: &Device) -> Value {
    let kind = match &d.kind {
        DeviceKind::Builtin(id) => json!({ "type": "builtin", "id": id }),
        DeviceKind::Clap { id } => json!({ "type": "clap", "id": id }),
        DeviceKind::Vst3 { id } => json!({ "type": "vst3", "id": id }),
        DeviceKind::Vst2 { id } => json!({ "type": "vst2", "id": id }),
        DeviceKind::Foreign { format, id } => json!({ "type": "foreign", "format": format, "id": id }),
    };
    let mut out = json!({
        "name": d.name,
        "kind": kind,
        "enabled": d.enabled,
        "state": d.state.as_ref().map(|s| BASE64.encode(s)),
        "params": d.params.iter().map(|p| json!({ "id": p.id, "value": number(p.value) })).collect::<Vec<_>>(),
    });
    if let Some(map) = out.as_object_mut() {
        with_extensions(map, &d.extensions);
    }
    out
}

fn content_value(c: &ClipContent) -> Value {
    let lanes: Vec<Value> = c
        .lanes
        .iter()
        .map(|lane| {
            let target = lane.target.map(|t| t.0);
            match &lane.data {
                LaneData::Notes(notes) => json!({ "target": target, "notes": notes.iter().map(|n| json!({
                    "time": number(n.time),
                    "duration": number(n.duration),
                    "key": n.key,
                    "velocity": number(f64::from(n.velocity)),
                    "release_velocity": number(f64::from(n.release_velocity)),
                    "channel": n.channel,
                })).collect::<Vec<_>>() }),
                LaneData::Audio(a) => json!({ "target": target, "audio": audio_value(a) }),
            }
        })
        .collect();
    let mut out = json!({ "id": c.id.0, "name": c.name, "length": number(c.length), "lanes": lanes });
    if let Some(map) = out.as_object_mut() {
        with_extensions(map, &c.extensions);
    }
    out
}

fn audio_value(a: &AudioRegion) -> Value {
    json!({
        "file": a.file,
        "absolute_file": a.absolute_file,
        "sample_rate": a.sample_rate,
        "channels": a.channels,
        "duration_seconds": a.duration_seconds.map(number),
        "warped": a.warped,
        "stretch": stretch_name(a.stretch),
        "warp_markers": a.warp_markers.iter().map(|m| json!({ "time": number(m.time), "audio_time": number(m.audio_time) })).collect::<Vec<_>>(),
        "gain": number(a.gain),
        "transpose": number(a.transpose),
        "stream": a.stream,
    })
}

fn clip_value(c: &Clip) -> Value {
    let mut out = json!({
        "content": c.content.0,
        "start": number(c.start),
        "length": number(c.length),
        "offset": number(c.offset),
        "loop": c.loop_range.map(|l| json!([number(l.start), number(l.end)])),
        "enabled": c.enabled,
        "fade_in": number(c.fade_in),
        "fade_out": number(c.fade_out),
        "fade_in_shape": c.fade_in_curve.shape.name(),
        "fade_in_curve": number(c.fade_in_curve.curve),
        "fade_out_shape": c.fade_out_curve.shape.name(),
        "fade_out_curve": number(c.fade_out_curve.curve),
    });
    if let Some(map) = out.as_object_mut() {
        // Only a clip whose launch settings were changed carries them, so most clips (and every
        // arrangement clip that never was in a slot) stay as they were written before.
        if !c.launch.is_default() {
            map.insert("launch".into(), launch_value(&c.launch));
        }
        with_extensions(map, &c.extensions);
    }
    out
}

fn launch_value(l: &LaunchSettings) -> Value {
    json!({ "mode": l.mode.name(), "legato": l.legato, "quantization": l.quantization.name() })
}

fn with_extensions(map: &mut Map<String, Value>, extensions: &Extensions) {
    if !extensions.0.is_empty() {
        let list = extensions
            .0
            .iter()
            .map(|e| json!({ "format": e.format, "key": e.key, "data": BASE64.encode(&e.data) }))
            .collect();
        map.insert("extensions".into(), Value::Array(list));
    }
}

/// JSON has no NaN or infinity; a number that is not finite is written as 0.
fn number(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn stretch_name(mode: StretchMode) -> &'static str {
    match mode {
        StretchMode::Repitch => "repitch",
        StretchMode::Rhythmic => "rhythmic",
        StretchMode::Tonal => "tonal",
        StretchMode::Texture => "texture",
        StretchMode::Smooth => "smooth",
        StretchMode::HighQuality => "high_quality",
    }
}

// Reading. Every value is checked; a wrong one is an error naming where it is.

type Obj = Map<String, Value>;

fn object<'a>(value: &'a Value, at: &str) -> Result<&'a Obj, ProjectFileError> {
    value.as_object().ok_or_else(|| ProjectFileError(format!("{at}: expected an object")))
}

fn list<'a>(o: &'a Obj, key: &str, at: &str) -> Result<&'a [Value], ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(format!("{at}.{key}: expected a list").into()),
    }
}

fn float(o: &Obj, key: &str, at: &str) -> Result<f64, ProjectFileError> {
    o.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| ProjectFileError(format!("{at}.{key}: expected a number")))
}

fn float_or(o: &Obj, key: &str, default: f64, at: &str) -> Result<f64, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(_) => float(o, key, at),
    }
}

fn optional_float(o: &Obj, key: &str, at: &str) -> Result<Option<f64>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => float(o, key, at).map(Some),
    }
}

fn whole<T: TryFrom<u64>>(o: &Obj, key: &str, at: &str) -> Result<T, ProjectFileError> {
    o.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| T::try_from(n).ok())
        .ok_or_else(|| ProjectFileError(format!("{at}.{key}: expected a whole number in range")))
}

fn optional_whole<T: TryFrom<u64>>(o: &Obj, key: &str, at: &str) -> Result<Option<T>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => whole(o, key, at).map(Some),
    }
}

fn flag(o: &Obj, key: &str, default: bool, at: &str) -> Result<bool, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{at}.{key}: expected true or false").into()),
    }
}

fn text(o: &Obj, key: &str, at: &str) -> Result<String, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{at}.{key}: expected text").into()),
    }
}

fn required_text(o: &Obj, key: &str, at: &str) -> Result<String, ProjectFileError> {
    match o.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(format!("{at}.{key}: expected text").into()),
    }
}

fn bytes(o: &Obj, key: &str, at: &str) -> Result<Option<Vec<u8>>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            BASE64.decode(s).map(Some).map_err(|e| ProjectFileError(format!("{at}.{key}: not base64: {e}")))
        }
        Some(_) => Err(format!("{at}.{key}: expected base64 text").into()),
    }
}

fn extensions(o: &Obj, at: &str) -> Result<Extensions, ProjectFileError> {
    let mut out = Vec::new();
    for (i, item) in list(o, "extensions", at)?.iter().enumerate() {
        let at = format!("{at}.extensions[{i}]");
        let e = object(item, &at)?;
        out.push(Extension {
            format: required_text(e, "format", &at)?,
            key: required_text(e, "key", &at)?,
            data: bytes(e, "data", &at)?.unwrap_or_default(),
        });
    }
    Ok(Extensions(out))
}

fn read_project(value: &Value) -> Result<Project, ProjectFileError> {
    let p = object(value, "project")?;
    let transport = match p.get("transport") {
        None | Some(Value::Null) => Transport::default(),
        Some(t) => {
            let t = object(t, "project.transport")?;
            let signature = match t.get("time_signature") {
                Some(Value::Array(pair)) if pair.len() == 2 => {
                    let part = |v: &Value| v.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| *n > 0);
                    match (pair.first().and_then(part), pair.get(1).and_then(part)) {
                        (Some(numerator), Some(denominator)) => TimeSignature { numerator, denominator },
                        _ => {
                            return Err("project.transport.time_signature: expected two whole numbers"
                                .to_string()
                                .into());
                        }
                    }
                }
                None | Some(Value::Null) => TimeSignature { numerator: 4, denominator: 4 },
                Some(_) => {
                    return Err("project.transport.time_signature: expected [numerator, denominator]"
                        .to_string()
                        .into());
                }
            };
            let tempo = float_or(t, "tempo_bpm", 120.0, "project.transport")?;
            if !(1.0..=999.0).contains(&tempo) {
                return Err(format!("project.transport.tempo_bpm: {tempo} is not a tempo").into());
            }
            Transport { tempo_bpm: tempo, time_signature: signature }
        }
    };
    let tracks = list(p, "tracks", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_track(v, &format!("project.tracks[{i}]")))
        .collect::<Result<_, _>>()?;
    let channels = list(p, "channels", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_channel(v, &format!("project.channels[{i}]")))
        .collect::<Result<_, _>>()?;
    let contents = list(p, "contents", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_content(v, &format!("project.contents[{i}]")))
        .collect::<Result<_, _>>()?;
    let arrangement = match p.get("arrangement") {
        None | Some(Value::Null) => Arrangement::default(),
        Some(a) => {
            let a = object(a, "project.arrangement")?;
            let mut lanes = Vec::new();
            for (i, v) in list(a, "lanes", "project.arrangement")?.iter().enumerate() {
                let at = format!("project.arrangement.lanes[{i}]");
                let lane = object(v, &at)?;
                let clips = list(lane, "clips", &at)?
                    .iter()
                    .enumerate()
                    .map(|(j, c)| read_clip(c, &format!("{at}.clips[{j}]")))
                    .collect::<Result<_, _>>()?;
                lanes.push(ArrangementLane {
                    name: text(lane, "name", &at)?,
                    track: optional_whole(lane, "track", &at)?.map(TrackId),
                    clips,
                });
            }
            Arrangement { lanes }
        }
    };
    let launcher = match p.get("launcher") {
        None | Some(Value::Null) => Launcher::default(),
        Some(l) => {
            let l = object(l, "project.launcher")?;
            let mut scenes = Vec::new();
            for (i, v) in list(l, "scenes", "project.launcher")?.iter().enumerate() {
                let at = format!("project.launcher.scenes[{i}]");
                let s = object(v, &at)?;
                scenes.push(Scene { name: text(s, "name", &at)?, tempo_bpm: optional_float(s, "tempo_bpm", &at)? });
            }
            let mut columns = Vec::new();
            for (i, v) in list(l, "columns", "project.launcher")?.iter().enumerate() {
                let at = format!("project.launcher.columns[{i}]");
                let c = object(v, &at)?;
                let mut slots = Vec::new();
                for (j, s) in list(c, "slots", &at)?.iter().enumerate() {
                    let at = format!("{at}.slots[{j}]");
                    let s = object(s, &at)?;
                    let clip = match s.get("clip") {
                        None | Some(Value::Null) => None,
                        Some(clip) => Some(read_clip(clip, &format!("{at}.clip"))?),
                    };
                    slots.push(Slot { clip, has_stop: flag(s, "has_stop", false, &at)? });
                }
                columns.push(LauncherColumn { track: TrackId(whole(c, "track", &at)?), slots });
            }
            Launcher { scenes, columns }
        }
    };
    Ok(Project { transport, tracks, channels, contents, arrangement, launcher, extensions: extensions(p, "project")? })
}

fn read_track(value: &Value, at: &str) -> Result<Track, ProjectFileError> {
    let t = object(value, at)?;
    let color = match t.get("color") {
        None | Some(Value::Null) => None,
        Some(Value::String(hex)) => {
            let digits = hex.strip_prefix('#').unwrap_or(hex);
            let parsed = (digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(digits, 16).ok())
                .flatten();
            let rgb = parsed.ok_or_else(|| ProjectFileError(format!("{at}.color: expected #RRGGBB")))?;
            Some(Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
        }
        Some(_) => return Err(format!("{at}.color: expected #RRGGBB").into()),
    };
    Ok(Track {
        id: TrackId(whole(t, "id", at)?),
        name: text(t, "name", at)?,
        color,
        content: ContentKinds { notes: flag(t, "notes", false, at)?, audio: flag(t, "audio", false, at)? },
        channel: optional_whole(t, "channel", at)?.map(ChannelId),
        parent: optional_whole(t, "parent", at)?.map(TrackId),
        extensions: extensions(t, at)?,
    })
}

fn read_channel(value: &Value, at: &str) -> Result<Channel, ProjectFileError> {
    let c = object(value, at)?;
    let role = match c.get("role").and_then(Value::as_str) {
        None | Some("regular") => ChannelRole::Regular,
        Some("master") => ChannelRole::Master,
        Some("effect") => ChannelRole::Effect,
        Some("submix") => ChannelRole::Submix,
        Some(other) => return Err(format!("{at}.role: {other} is not a channel role").into()),
    };
    let mut sends = Vec::new();
    for (i, v) in list(c, "sends", at)?.iter().enumerate() {
        let at = format!("{at}.sends[{i}]");
        let s = object(v, &at)?;
        sends.push(Send {
            target: ChannelId(whole(s, "target", &at)?),
            level: float_or(s, "level", 1.0, &at)?,
            pre_fader: flag(s, "pre_fader", false, &at)?,
        });
    }
    let devices = list(c, "devices", at)?
        .iter()
        .enumerate()
        .map(|(i, v)| read_device(v, &format!("{at}.devices[{i}]")))
        .collect::<Result<_, _>>()?;
    let volume = float_or(c, "volume", 1.0, at)?;
    let pan = float_or(c, "pan", 0.0, at)?;
    if volume < 0.0 || !(-1.0..=1.0).contains(&pan) {
        return Err(format!("{at}: volume must not be negative and pan must be from -1 to 1").into());
    }
    Ok(Channel {
        id: ChannelId(whole(c, "id", at)?),
        name: text(c, "name", at)?,
        role,
        volume,
        pan,
        mute: flag(c, "mute", false, at)?,
        solo: flag(c, "solo", false, at)?,
        output: optional_whole(c, "output", at)?.map(ChannelId),
        sends,
        devices,
        extensions: extensions(c, at)?,
    })
}

fn read_device(value: &Value, at: &str) -> Result<Device, ProjectFileError> {
    let d = object(value, at)?;
    let kind_at = format!("{at}.kind");
    let k = object(d.get("kind").unwrap_or(&Value::Null), &kind_at)?;
    let kind = match k.get("type").and_then(Value::as_str) {
        Some("builtin") => DeviceKind::Builtin(required_text(k, "id", &kind_at)?),
        Some("clap") => DeviceKind::Clap { id: required_text(k, "id", &kind_at)? },
        Some("vst3") => DeviceKind::Vst3 { id: required_text(k, "id", &kind_at)? },
        Some("vst2") => DeviceKind::Vst2 { id: whole(k, "id", &kind_at)? },
        Some("foreign") => {
            DeviceKind::Foreign { format: required_text(k, "format", &kind_at)?, id: required_text(k, "id", &kind_at)? }
        }
        _ => return Err(format!("{kind_at}.type: expected builtin, clap, vst3, vst2 or foreign").into()),
    };
    let mut params = Vec::new();
    for (i, v) in list(d, "params", at)?.iter().enumerate() {
        let at = format!("{at}.params[{i}]");
        let p = object(v, &at)?;
        params.push(DeviceParam { id: required_text(p, "id", &at)?, value: float(p, "value", &at)? });
    }
    Ok(Device {
        name: text(d, "name", at)?,
        kind,
        enabled: flag(d, "enabled", true, at)?,
        state: bytes(d, "state", at)?,
        params,
        extensions: extensions(d, at)?,
    })
}

fn read_content(value: &Value, at: &str) -> Result<ClipContent, ProjectFileError> {
    let c = object(value, at)?;
    let mut lanes = Vec::new();
    for (i, v) in list(c, "lanes", at)?.iter().enumerate() {
        let at = format!("{at}.lanes[{i}]");
        let lane = object(v, &at)?;
        let target = optional_whole(lane, "target", &at)?.map(TrackId);
        let data = match (lane.get("notes"), lane.get("audio")) {
            (Some(_), None) => {
                let mut notes = Vec::new();
                for (j, n) in list(lane, "notes", &at)?.iter().enumerate() {
                    let at = format!("{at}.notes[{j}]");
                    let n = object(n, &at)?;
                    let key: u8 = whole(n, "key", &at)?;
                    let channel: u8 = optional_whole(n, "channel", &at)?.unwrap_or(0);
                    if key > 127 || channel > 15 {
                        return Err(format!("{at}: key must be 0 to 127 and channel 0 to 15").into());
                    }
                    notes.push(Note {
                        time: float(n, "time", &at)?,
                        duration: float(n, "duration", &at)?,
                        key,
                        velocity: float_or(n, "velocity", 1.0, &at)?.clamp(0.0, 1.0) as f32,
                        release_velocity: float_or(n, "release_velocity", 0.0, &at)?.clamp(0.0, 1.0) as f32,
                        channel,
                    });
                }
                LaneData::Notes(notes)
            }
            (None, Some(audio)) => LaneData::Audio(read_audio(audio, &format!("{at}.audio"))?),
            _ => return Err(format!("{at}: a lane has either notes or audio").into()),
        };
        lanes.push(ContentLane { target, data });
    }
    Ok(ClipContent {
        id: ContentId(whole(c, "id", at)?),
        name: text(c, "name", at)?,
        length: float(c, "length", at)?,
        lanes,
        extensions: extensions(c, at)?,
    })
}

fn read_audio(value: &Value, at: &str) -> Result<AudioRegion, ProjectFileError> {
    let a = object(value, at)?;
    let stretch = match a.get("stretch").and_then(Value::as_str) {
        Some("repitch") => StretchMode::Repitch,
        Some("rhythmic") => StretchMode::Rhythmic,
        Some("tonal") => StretchMode::Tonal,
        Some("texture") => StretchMode::Texture,
        None | Some("smooth") => StretchMode::Smooth,
        Some("high_quality") => StretchMode::HighQuality,
        Some(other) => return Err(format!("{at}.stretch: {other} is not a stretch mode").into()),
    };
    let mut warp_markers = Vec::new();
    for (i, v) in list(a, "warp_markers", at)?.iter().enumerate() {
        let at = format!("{at}.warp_markers[{i}]");
        let m = object(v, &at)?;
        warp_markers.push(WarpMarker { time: float(m, "time", &at)?, audio_time: float(m, "audio_time", &at)? });
    }
    let absolute_file = match a.get("absolute_file") {
        None | Some(Value::Null) => None,
        Some(_) => Some(required_text(a, "absolute_file", at)?),
    };
    Ok(AudioRegion {
        file: required_text(a, "file", at)?,
        absolute_file,
        sample_rate: optional_whole(a, "sample_rate", at)?,
        channels: optional_whole(a, "channels", at)?,
        duration_seconds: optional_float(a, "duration_seconds", at)?,
        warped: flag(a, "warped", false, at)?,
        stretch,
        warp_markers,
        gain: float_or(a, "gain", 1.0, at)?,
        transpose: float_or(a, "transpose", 0.0, at)?,
        stream: optional_whole(a, "stream", at)?,
    })
}

fn read_clip(value: &Value, at: &str) -> Result<Clip, ProjectFileError> {
    let c = object(value, at)?;
    let loop_range = match c.get("loop") {
        None | Some(Value::Null) => None,
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let at_point = |i: usize| pair.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
            match (at_point(0), at_point(1)) {
                (Some(start), Some(end)) if start <= end => Some(LoopRange { start, end }),
                _ => return Err(format!("{at}.loop: expected [start, end] with start before end").into()),
            }
        }
        Some(_) => return Err(format!("{at}.loop: expected [start, end]").into()),
    };
    Ok(Clip {
        content: ContentId(whole(c, "content", at)?),
        start: float(c, "start", at)?,
        length: float(c, "length", at)?,
        offset: float_or(c, "offset", 0.0, at)?,
        loop_range,
        enabled: flag(c, "enabled", true, at)?,
        fade_in: float_or(c, "fade_in", 0.0, at)?,
        fade_out: float_or(c, "fade_out", 0.0, at)?,
        fade_in_curve: fade_curve(c, "fade_in", at)?,
        fade_out_curve: fade_curve(c, "fade_out", at)?,
        launch: read_launch(c, at)?,
        extensions: extensions(c, at)?,
    })
}

/// A clip's launch settings, `launch`: the defaults when missing (files written before launch
/// settings), and for each of its keys that is missing.
fn read_launch(c: &Obj, at: &str) -> Result<LaunchSettings, ProjectFileError> {
    let at = format!("{at}.launch");
    let l = match c.get("launch") {
        None | Some(Value::Null) => return Ok(LaunchSettings::default()),
        Some(value) => object(value, &at)?,
    };
    let name = |key: &str| match l.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => Ok(Some(name.as_str())),
        Some(_) => Err(ProjectFileError(format!("{at}.{key}: expected a name"))),
    };
    let mode = match name("mode")? {
        None => LaunchMode::default(),
        Some(text) => LaunchMode::from_name(text)
            .ok_or_else(|| ProjectFileError(format!("{at}.mode: unknown launch mode {text:?}")))?,
    };
    let quantization = match name("quantization")? {
        None => LaunchQuantization::default(),
        Some(text) => LaunchQuantization::from_name(text)
            .ok_or_else(|| ProjectFileError(format!("{at}.quantization: unknown launch quantisation {text:?}")))?,
    };
    Ok(LaunchSettings { mode, legato: flag(l, "legato", false, &at)?, quantization })
}

/// A fade's shape and curve, `<fade>_shape` and `<fade>_curve`: linear and 0 when missing (files
/// written before fade shapes).
fn fade_curve(c: &Obj, fade: &str, at: &str) -> Result<FadeCurve, ProjectFileError> {
    let shape_key = format!("{fade}_shape");
    let shape = match c.get(&shape_key) {
        None | Some(Value::Null) => FadeShape::default(),
        Some(Value::String(name)) => FadeShape::from_name(name)
            .ok_or_else(|| ProjectFileError(format!("{at}.{shape_key}: unknown fade shape {name:?}")))?,
        Some(_) => return Err(format!("{at}.{shape_key}: expected a fade shape name").into()),
    };
    let curve_key = format!("{fade}_curve");
    let curve = float_or(c, &curve_key, 0.0, at)?;
    if !(-1.0..=1.0).contains(&curve) {
        return Err(format!("{at}.{curve_key}: expected -1 to 1, got {curve}").into());
    }
    Ok(FadeCurve { shape, curve })
}

#[cfg(test)]
mod tests;
