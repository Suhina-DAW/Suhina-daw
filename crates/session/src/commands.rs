//! The command registry. Each command is a [`CommandSpec`]; new commands are added to
//! [`COMMANDS`] with tests below.

use loopwerk_engine::{ColumnStatus, LaunchCommand, MAX_START_BEAT, Quantization, Queued};
use loopwerk_model::{ArrangementLane, Channel, ChannelId, ChannelRole, Clip, LaneData, Project, Track, TrackId};
use serde_json::{Value, json};

use crate::{CommandError, Session, devices, files, preview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Reads the project; never changes it.
    Read,
    /// Changes the project and records one undo step.
    Edit,
    /// Moves through the undo history.
    History,
    /// Controls playback in the attached audio engine (transport, clip launching); never
    /// changes the project or its undo history.
    Transport,
    /// Opens, switches or closes whole projects (the open projects, like tabs); never changes a
    /// project or its undo history. Runs on [`crate::Projects`], not on one session.
    Projects,
    /// Scans for plug-ins or changes where scans look (ADR 0006); belongs to Loopwerk, not to
    /// one project. Runs on [`crate::Projects`].
    Plugins,
    /// Acts on one of the project's running plug-ins (opens or closes its editor, restarts
    /// it); never changes the project or its undo history.
    Plugin,
    /// The browser's library: folders, search, pins, recent items, tags (ADR 0012); belongs
    /// to Loopwerk, not to one project. Runs on [`crate::Projects`].
    Library,
}

impl CommandKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::History => "history",
            Self::Transport => "transport",
            Self::Projects => "projects",
            Self::Plugins => "plugins",
            Self::Plugin => "plugin",
            Self::Library => "library",
        }
    }
}

/// How a command runs: on one session, or (for [`CommandKind::Projects`]) on the open projects.
pub type Run<T> = fn(&mut T, &Value) -> Result<Value, CommandError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Number,
    Integer,
    Bool,
    /// A track id (integer) or a track name (string).
    Track,
    /// A number, or the string "-inf".
    Decibels,
    Text,
    /// A list of tracks, each an id or a name.
    Tracks,
    /// A list of texts (file paths).
    Texts,
}

#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub ty: ParamType,
    pub required: bool,
    pub doc: &'static str,
}

pub(crate) const fn param(name: &'static str, ty: ParamType, doc: &'static str) -> Param {
    Param { name, ty, required: true, doc }
}

pub(crate) const fn optional(name: &'static str, ty: ParamType, doc: &'static str) -> Param {
    Param { name, ty, required: false, doc }
}

const TRACK: Param = param("track", ParamType::Track, "track id or name, from project.tracks");
const DEVICE: Param = param("device", ParamType::Integer, "device index on the track, from 0, as in track.devices");
const SCENE: Param = param("scene", ParamType::Integer, "scene number, from 1, as in project.launcher");

pub struct CommandSpec<T: 'static = Session> {
    /// Stable id in `area.verb` form; part of the public interface for agents.
    pub id: &'static str,
    /// One line saying what the command does.
    pub label: &'static str,
    pub params: &'static [Param],
    pub kind: CommandKind,
    pub run: Run<T>,
}

impl<T> CommandSpec<T> {
    /// The parameters as a JSON Schema object, for MCP tool listings.
    pub fn input_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        for p in self.params {
            if matches!(p.ty, ParamType::Tracks | ParamType::Texts) {
                let items = if p.ty == ParamType::Tracks {
                    json!({ "type": ["integer", "string"] })
                } else {
                    json!({ "type": "string" })
                };
                properties.insert(p.name.to_string(), json!({ "type": "array", "items": items, "description": p.doc }));
                continue;
            }
            let ty = match p.ty {
                ParamType::Tracks | ParamType::Texts => json!("array"),
                ParamType::Number => json!("number"),
                ParamType::Integer => json!("integer"),
                ParamType::Bool => json!("boolean"),
                ParamType::Track => json!(["integer", "string"]),
                ParamType::Decibels => json!(["number", "string"]),
                ParamType::Text => json!("string"),
            };
            properties.insert(p.name.to_string(), json!({ "type": ty, "description": p.doc }));
        }
        let required: Vec<&str> = self.params.iter().filter(|p| p.required).map(|p| p.name).collect();
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
    }

    /// The command as `commands.list` and the MCP server's `command_list` show it.
    pub fn describe(&self) -> Value {
        json!({ "id": self.id, "label": self.label, "kind": self.kind.name(), "params": self.input_schema() })
    }
}

/// Every command, in the order `commands.list` shows them.
pub fn commands() -> &'static [CommandSpec] {
    COMMANDS
}

static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "commands.list",
        label: "List every command with its parameters",
        params: &[],
        kind: CommandKind::Read,
        run: list,
    },
    CommandSpec {
        id: "project.save",
        label: "Save the project as a Loopwerk project (.locr) with its plug-ins' state as it is now; without a path, to the .locr file it was opened from or last saved to",
        params: &[
            optional("path", ParamType::Text, "file to save to; .locr is added when missing"),
            optional(
                "copy",
                ParamType::Bool,
                "true writes a copy (a template, a backup) and the project stays with its own file",
            ),
        ],
        kind: CommandKind::Read,
        run: save,
    },
    CommandSpec {
        id: "project.info",
        label: "Summarise the open project: tempo, time signature, counts of tracks, scenes and clips, undo state",
        params: &[],
        kind: CommandKind::Read,
        run: info,
    },
    CommandSpec {
        id: "project.tracks",
        label: "List tracks with id, name, volume (dB), pan, mute and devices",
        params: &[],
        kind: CommandKind::Read,
        run: tracks,
    },
    CommandSpec {
        id: "project.clips",
        label: "List arrangement clips with track, index, start and length in beats, and note count",
        params: &[optional("track", ParamType::Track, "only clips on this track (id or name)")],
        kind: CommandKind::Read,
        run: clips,
    },
    CommandSpec {
        id: "transport.setTempo",
        label: "Set the project tempo",
        params: &[param("bpm", ParamType::Number, "beats per minute, 20 to 999")],
        kind: CommandKind::Edit,
        run: set_tempo,
    },
    CommandSpec {
        id: "track.setVolume",
        label: "Set a track's volume",
        params: &[
            TRACK,
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: set_volume,
    },
    CommandSpec {
        id: "track.setPan",
        label: "Set a track's pan",
        params: &[TRACK, param("pan", ParamType::Number, "-1 is full left, 0 centre, 1 full right")],
        kind: CommandKind::Edit,
        run: set_pan,
    },
    CommandSpec {
        id: "track.setMute",
        label: "Mute or unmute a track",
        params: &[TRACK, param("mute", ParamType::Bool, "true mutes the track")],
        kind: CommandKind::Edit,
        run: set_mute,
    },
    CommandSpec {
        id: "track.group",
        label: "Put tracks into a new group track, which plays into where they played",
        params: &[
            param("tracks", ParamType::Tracks, "the tracks to group (ids or names), all in the same group or none"),
            optional("name", ParamType::Text, "the group's name (\"Group\" if left out)"),
        ],
        kind: CommandKind::Edit,
        run: group_tracks,
    },
    CommandSpec {
        id: "track.ungroup",
        label: "Dissolve a group track: its tracks stay, playing into where the group played",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: ungroup_track,
    },
    CommandSpec {
        id: "track.move",
        label: "Move a track (a group with its tracks) to another place in the track order",
        params: &[TRACK, param("to", ParamType::Integer, "its new position, from 0, in project.tracks order")],
        kind: CommandKind::Edit,
        run: move_track,
    },
    CommandSpec {
        id: "track.rename",
        label: "Rename a track",
        params: &[TRACK, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: rename_track,
    },
    CommandSpec {
        id: "track.setColor",
        label: "Set a track's colour",
        params: &[TRACK, param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the default")],
        kind: CommandKind::Edit,
        run: set_color,
    },
    CommandSpec {
        id: "track.delete",
        label: "Delete a track with its clips; a deleted group's tracks stay",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: delete_track,
    },
    CommandSpec {
        id: "clip.delete",
        label: "Delete an arrangement clip",
        params: &[TRACK, param("index", ParamType::Integer, "clip index on the track, from project.clips")],
        kind: CommandKind::Edit,
        run: delete_clip,
    },
    CommandSpec {
        id: "clip.setGain",
        label: "Set an audio clip's gain (every clip that plays the same audio content follows)",
        params: &[
            TRACK,
            param("index", ParamType::Integer, "clip index on the track, from project.clips"),
            param("db", ParamType::Decibels, "gain in dB, at most +24; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: set_clip_gain,
    },
    CommandSpec {
        id: "clip.setFades",
        label: "Set an audio clip's fade-in and fade-out lengths, in beats; together at most the clip's length",
        params: &[
            TRACK,
            param("index", ParamType::Integer, "clip index on the track, from project.clips"),
            optional("in", ParamType::Number, "fade-in length in beats from the clip's start, at least 0; 0 is none"),
            optional("out", ParamType::Number, "fade-out length in beats before the clip's end, at least 0; 0 is none"),
        ],
        kind: CommandKind::Edit,
        run: set_clip_fades,
    },
    CommandSpec {
        id: "clip.copy",
        label: "Copy an arrangement clip, with what it plays, for clip.paste",
        params: &[TRACK, param("index", ParamType::Integer, "clip index on the track, from project.clips")],
        kind: CommandKind::Read,
        run: copy_clip,
    },
    CommandSpec {
        id: "clip.cut",
        label: "Copy an arrangement clip for clip.paste and delete it",
        params: &[TRACK, param("index", ParamType::Integer, "clip index on the track, from project.clips")],
        kind: CommandKind::Edit,
        run: cut_clip,
    },
    CommandSpec {
        id: "clip.paste",
        label: "Place the copied clip on a track, replacing what it covers unless stacked",
        params: &[
            TRACK,
            param("start", ParamType::Number, "where it starts, in beats from the beginning, at least 0"),
            optional("stack", ParamType::Bool, "true leaves clips under it as they are"),
        ],
        kind: CommandKind::Edit,
        run: paste_clip,
    },
    CommandSpec {
        id: "clip.duplicate",
        label: "Place a copy of an arrangement clip right after it, replacing what the copy covers",
        params: &[TRACK, param("index", ParamType::Integer, "clip index on the track, from project.clips")],
        kind: CommandKind::Edit,
        run: duplicate_clip,
    },
    CommandSpec {
        id: "track.setOutput",
        label: "Route a track's audio to the main output or into a group track",
        params: &[TRACK, param("to", ParamType::Track, "\"main\", or a group track (id or name) to play into")],
        kind: CommandKind::Edit,
        run: set_output,
    },
    CommandSpec {
        id: "track.setSolo",
        label: "Solo a track or end its solo: while any track is soloed, only soloed tracks, the groups they play into and return tracks are heard",
        params: &[
            TRACK,
            param("solo", ParamType::Bool, "true solos the track"),
            optional("exclusive", ParamType::Bool, "true also ends every other solo (false by default)"),
        ],
        kind: CommandKind::Edit,
        run: set_solo,
    },
    CommandSpec {
        id: "clip.move",
        label: "Move an arrangement clip to a new start position, and to another track if asked; it replaces what it covers unless stacked",
        params: &[
            TRACK,
            param("index", ParamType::Integer, "clip index on the track, from project.clips"),
            param("start", ParamType::Number, "new start in beats from the beginning, at least 0"),
            optional(
                "to_track",
                ParamType::Track,
                "track to move the clip to (id or name); it must take the clip's kind: notes or audio",
            ),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under it as they are (they all play); false (default) shortens, splits or removes them",
            ),
        ],
        kind: CommandKind::Edit,
        run: move_clip,
    },
    CommandSpec {
        id: "project.launcher",
        label: "List the clip launcher: scenes (name, tempo) and each track's slots (clip name, stop button)",
        params: &[],
        kind: CommandKind::Read,
        run: launcher_grid,
    },
    CommandSpec {
        id: "launcher.state",
        label: "What the audio engine is playing: transport, tempo, position, each track's playing and queued clip, and the engine's CPU load",
        params: &[],
        kind: CommandKind::Transport,
        run: launcher_state,
    },
    CommandSpec {
        id: "transport.play",
        label: "Start the transport from the play start marker (beat 0 unless moved): the arrangement plays, launches follow the beat",
        params: &[],
        kind: CommandKind::Transport,
        run: play,
    },
    CommandSpec {
        id: "transport.stop",
        label: "Stop the transport, the arrangement and every launched clip at once; stopping while stopped moves the play start marker to beat 0",
        params: &[optional(
            "return",
            ParamType::Text,
            "marker (default): the playhead goes back to the play start marker; here: the marker moves to where the playhead stopped",
        )],
        kind: CommandKind::Transport,
        run: stop,
    },
    CommandSpec {
        id: "transport.setPosition",
        label: "Move the play start marker, where the transport starts; while playing, playback jumps there at once",
        params: &[param("beats", ParamType::Number, "beats from the beginning, 0 … 120000")],
        kind: CommandKind::Transport,
        run: set_position,
    },
    CommandSpec {
        id: "transport.setMetronome",
        label: "Turn the metronome on or off: a click on every beat while playing, higher on the bar",
        params: &[
            param("on", ParamType::Bool, "true turns the click on"),
            optional(
                "db",
                ParamType::Decibels,
                "click level in dB, at most +6; keeps the current level (-6 at first) if left out",
            ),
        ],
        kind: CommandKind::Transport,
        run: set_metronome,
    },
    CommandSpec {
        id: "clip.launch",
        label: "Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track)",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch_clip,
    },
    CommandSpec {
        id: "scene.launch",
        label: "Launch a scene: every track launches its slot in that row at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: launch_scene,
    },
    CommandSpec {
        id: "track.stop",
        label: "Stop the clip a track is playing, at the next quantisation boundary",
        params: &[TRACK],
        kind: CommandKind::Transport,
        run: stop_track,
    },
    CommandSpec {
        id: "launcher.stopAll",
        label: "Stop every launched clip at the next quantisation boundary",
        params: &[],
        kind: CommandKind::Transport,
        run: stop_all,
    },
    CommandSpec {
        id: "launcher.setQuantization",
        label: "Set the grid launches wait for (default 1 bar)",
        params: &[param(
            "value",
            ParamType::Text,
            "none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32",
        )],
        kind: CommandKind::Transport,
        run: set_quantization,
    },
    CommandSpec {
        id: "launcher.backToArrangement",
        label: "Return every track that launched a clip to playing its arrangement clips",
        params: &[],
        kind: CommandKind::Transport,
        run: back_to_arrangement,
    },
    CommandSpec {
        id: "devices.list",
        label: "List the built-in devices (id, name, effect or instrument) with their parameters (id, range, default, unit, options)",
        params: &[],
        kind: CommandKind::Read,
        run: devices::list,
    },
    CommandSpec {
        id: "track.devices",
        label: "List the devices on a track in order, with every built-in device's parameter values",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: devices::track_devices,
    },
    CommandSpec {
        id: "device.add",
        label: "Add a built-in device to a track; an instrument goes first and plays the track's notes, an effect goes last unless a position is given",
        params: &[
            TRACK,
            optional("id", ParamType::Text, "built-in device id, from devices.list (not needed with a preset)"),
            optional(
                "position",
                ParamType::Integer,
                "where in the chain, from 0 (default: last; an instrument is always first)",
            ),
            optional("preset", ParamType::Text, "a preset file to load: DX7 voice data (.syx) for the FM synth"),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1 (default 1)"),
        ],
        kind: CommandKind::Edit,
        run: devices::add,
    },
    CommandSpec {
        id: "track.add",
        label: "Add an audio or MIDI track with its mixer channel, clip slots and arrangement lane, optionally with a device (an instrument makes a MIDI track, an effect an audio track)",
        params: &[
            optional("kind", ParamType::Text, "audio or midi (default: from the device)"),
            optional("name", ParamType::Text, "the track's name (default: the device's, or Audio n / MIDI n)"),
            optional(
                "after",
                ParamType::Track,
                "put it after this track, in its group (default: after the last regular track)",
            ),
            optional(
                "device",
                ParamType::Text,
                "a built-in device id from devices.list, or a plug-in from plugins.list",
            ),
            optional("preset", ParamType::Text, "a preset file for the device (DX7 voice data for the FM synth)"),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1"),
        ],
        kind: CommandKind::Edit,
        run: files::add_track,
    },
    CommandSpec {
        id: "clip.addFiles",
        label: "Place samples and MIDI files as clips: on a track (or new tracks when none is given), one after another from a beat in the arrangement or down the clip slots from a scene; separate gives each clip a track of its own",
        params: &[
            param(
                "paths",
                ParamType::Texts,
                "the files: samples (wav, aiff, flac, mp3, ogg) and MIDI files (.mid); at most 256",
            ),
            optional(
                "track",
                ParamType::Track,
                "the track to place them on; files it cannot hold go on a new track after it (default: new tracks)",
            ),
            optional("start", ParamType::Number, "arrangement beat the first clip starts at"),
            optional("scene", ParamType::Integer, "scene number, from 1, of the first clip slot"),
            optional(
                "separate",
                ParamType::Bool,
                "one track per clip, side by side, instead of one after another (default false)",
            ),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new ones cover (default false)"),
            optional(
                "warp",
                ParamType::Text,
                "stretch mode for samples with a known tempo: repitch, rhythmic (default), tonal, texture, smooth, high-quality",
            ),
        ],
        kind: CommandKind::Edit,
        run: files::add_files,
    },
    CommandSpec {
        id: "preview.play",
        label: "Preview a sample or a MIDI file once, at its own speed, after the main channel; a MIDI file plays through a track's instrument or the FM synth",
        params: &[
            param("path", ParamType::Text, "the sample or MIDI file"),
            optional("from", ParamType::Number, "seconds into the file to start at (default 0)"),
            optional("track", ParamType::Track, "a MIDI file plays through this track's instrument when it has one"),
            optional("gain", ParamType::Number, "level in dB (default -6)"),
        ],
        kind: CommandKind::Transport,
        run: preview::play,
    },
    CommandSpec {
        id: "preview.stop",
        label: "Stop the preview",
        params: &[],
        kind: CommandKind::Transport,
        run: preview::stop,
    },
    CommandSpec {
        id: "device.remove",
        label: "Remove a device from a track",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Edit,
        run: devices::remove,
    },
    CommandSpec {
        id: "device.setEnabled",
        label: "Switch a device on or off; a device that is off passes the sound through",
        params: &[TRACK, DEVICE, param("enabled", ParamType::Bool, "false switches the device off")],
        kind: CommandKind::Edit,
        run: devices::set_enabled,
    },
    CommandSpec {
        id: "device.setParam",
        label: "Set a parameter of a built-in device on a track; the value is limited to the parameter's range",
        params: &[
            TRACK,
            DEVICE,
            param("param", ParamType::Text, "parameter id, from devices.list"),
            param(
                "value",
                ParamType::Number,
                "value in the parameter's unit (dB, Hz, 0..1 for percentages, option index for choices)",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::set_param,
    },
    CommandSpec {
        id: "device.addPlugin",
        label: "Add an installed plug-in to a track; an instrument goes first and plays the track's notes, an effect goes last unless a position is given. It starts while the project plays",
        params: &[
            TRACK,
            param("plugin", ParamType::Text, "plug-in name, id or file, from plugins.list"),
            optional(
                "position",
                ParamType::Integer,
                "where in the chain, from 0 (default: last; an instrument is always first)",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::add_plugin,
    },
    CommandSpec {
        id: "device.plugin",
        label: "Where a plug-in device stands: loading, running, missing, crashed or failed, the installed plug-in found for it, its latency, whether its editor is open",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Read,
        run: devices::plugin_status,
    },
    CommandSpec {
        id: "device.openEditor",
        label: "Open a plug-in device's editor in a window of its own; changes there are heard at once",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::open_editor,
    },
    CommandSpec {
        id: "device.closeEditor",
        label: "Close a plug-in device's editor",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::close_editor,
    },
    CommandSpec {
        id: "device.restart",
        label: "Restart a plug-in device that crashed or stopped answering, in a fresh process with its last state",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::restart,
    },
    CommandSpec { id: "edit.undo", label: "Undo the last edit", params: &[], kind: CommandKind::History, run: undo },
    CommandSpec {
        id: "edit.redo",
        label: "Redo the last undone edit",
        params: &[],
        kind: CommandKind::History,
        run: redo,
    },
];

fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(
        COMMANDS
            .iter()
            .map(|c| {
                json!({ "id": c.id, "label": c.label, "params": c.input_schema()["properties"], "kind": c.kind.name() })
            })
            .collect(),
    ))
}

fn save(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copy = match params.get("copy") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("copy must be true or false".into()),
    };
    let path = match params.get("path") {
        Some(Value::String(path)) if !path.trim().is_empty() => std::path::PathBuf::from(path.trim()),
        Some(Value::String(_)) | None | Some(Value::Null) => match session.source() {
            Some(source) if source.extension().is_some_and(|e| e.eq_ignore_ascii_case(loopwerk_locr::EXTENSION)) => {
                source.to_path_buf()
            }
            _ => return Err("the project has no .locr file yet; give a path to save it to".into()),
        },
        Some(_) => return Err("path must be text".into()),
    };
    let written = session.save(&path, copy)?;
    Ok(json!({ "path": written.display().to_string(), "copy": copy }))
}

fn info(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let arrangement_clips: usize = p.arrangement.lanes.iter().map(|l| l.clips.len()).sum();
    let launcher_clips = p.launcher.columns.iter().flat_map(|c| &c.slots).filter(|s| s.clip.is_some()).count();
    Ok(json!({
        "source": session.source().map(|s| s.display().to_string()),
        "unsaved_changes": session.has_unsaved_changes(),
        "tempo_bpm": p.transport.tempo_bpm,
        "time_signature": format!("{}/{}", p.transport.time_signature.numerator, p.transport.time_signature.denominator),
        "tracks": p.tracks.len(),
        "scenes": p.launcher.scenes.len(),
        "arrangement_clips": arrangement_clips,
        "launcher_clips": launcher_clips,
        "can_undo": session.can_undo(),
        "can_redo": session.can_redo(),
    }))
}

fn tracks(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    Ok(Value::Array(
        p.tracks
            .iter()
            .map(|t| {
                let channel = channel_of(p, t);
                let clips =
                    p.arrangement.lanes.iter().filter(|l| l.track == Some(t.id)).map(|l| l.clips.len()).sum::<usize>();
                json!({
                    "id": t.id.0,
                    "name": t.name,
                    "plays": if t.content.notes { "notes" } else if t.content.audio { "audio" } else { "none" },
                    "volume_db": channel.map(|c| to_db(c.volume)),
                    "pan": channel.map(|c| c.pan),
                    "mute": channel.map(|c| c.mute),
                    "devices": channel.map(|c| c.devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()),
                    "arrangement_clips": clips,
                })
            })
            .collect(),
    ))
}

fn clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let only = match params.get("track") {
        Some(_) => Some(track_id(p, params)?),
        None => None,
    };
    let mut out = Vec::new();
    for lane in &p.arrangement.lanes {
        let Some(track) = lane.track else { continue };
        if only.is_some_and(|t| t != track) {
            continue;
        }
        let name = p.tracks.iter().find(|t| t.id == track).map(|t| t.name.as_str()).unwrap_or_default();
        for (index, clip) in lane.clips.iter().enumerate() {
            out.push(clip_json(p, name, track, index, clip));
        }
    }
    Ok(Value::Array(out))
}

fn clip_json(p: &Project, track_name: &str, track: TrackId, index: usize, clip: &Clip) -> Value {
    let content = p.content(clip.content);
    let (notes, audio) = content.map_or((0, None), |c| {
        c.lanes.iter().fold((0, None), |(n, a), lane| match &lane.data {
            LaneData::Notes(notes) => (n + notes.len(), a),
            LaneData::Audio(region) => (n, Some(region)),
        })
    });
    json!({
        "track": track.0,
        "track_name": track_name,
        "index": index,
        "name": content.map(|c| c.name.as_str()),
        "start": clip.start,
        "length": clip.length,
        "enabled": clip.enabled,
        "fade_in": clip.fade_in,
        "fade_out": clip.fade_out,
        "notes": notes,
        "audio_file": audio.map(|r| r.file.as_str()),
        "audio_stream": audio.and_then(|r| r.stream),
    })
}

fn set_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bpm = number(params, "bpm")?;
    if !(20.0..=999.0).contains(&bpm) {
        return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
    }
    session.project.transport.tempo_bpm = bpm;
    Ok(json!({ "tempo_bpm": bpm }))
}

fn set_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > 6.0 {
        return Err(format!("db must be at most +6, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let channel = channel_param(session, params)?;
    channel.volume = gain;
    Ok(json!({ "volume_db": to_db(gain) }))
}

fn set_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    channel_param(session, params)?.pan = pan;
    Ok(json!({ "pan": pan }))
}

fn set_mute(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let mute = params.get("mute").and_then(Value::as_bool).ok_or("mute must be true or false")?;
    channel_param(session, params)?.mute = mute;
    Ok(json!({ "mute": mute }))
}

/// The ids in `params.tracks`, each resolved like a track parameter.
fn track_list(p: &Project, params: &Value) -> Result<Vec<TrackId>, CommandError> {
    let list = params.get("tracks").and_then(Value::as_array).ok_or("tracks must be a list of track ids or names")?;
    let mut ids = Vec::new();
    for value in list {
        let id = track_at(p, &json!({ "track": value }), "track")?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("tracks must name at least one track".into());
    }
    Ok(ids)
}

fn channel_id_of(p: &Project, track: TrackId) -> Option<ChannelId> {
    p.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel)
}

/// A track and every track inside it (for a group), in project order.
fn with_descendants(p: &Project, track: TrackId) -> Vec<TrackId> {
    let mut family = vec![track];
    // Bounded by the track count, so a parent cycle in a malformed project cannot loop.
    for _ in 0..p.tracks.len() {
        let before = family.len();
        for t in &p.tracks {
            if t.parent.is_some_and(|parent| family.contains(&parent)) && !family.contains(&t.id) {
                family.push(t.id);
            }
        }
        if family.len() == before {
            break;
        }
    }
    p.tracks.iter().map(|t| t.id).filter(|id| family.contains(id)).collect()
}

pub(crate) fn group_tracks(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let members = track_list(p, params)?;
    let parents: Vec<Option<TrackId>> =
        members.iter().map(|id| p.tracks.iter().find(|t| t.id == *id).and_then(|t| t.parent)).collect();
    let parent = parents.first().copied().flatten();
    if parents.iter().any(|x| *x != parent) {
        return Err("the tracks to group must all be in the same group, or in none".into());
    }
    let name = match params.get("name") {
        None => "Group".to_string(),
        Some(v) => v.as_str().filter(|n| !n.trim().is_empty()).ok_or("name must be a text, not empty")?.to_string(),
    };
    let id = TrackId(p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0) + 1);
    let channel_id = ChannelId(p.channels.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    let output = parent.and_then(|t| channel_id_of(p, t));
    p.channels.push(Channel {
        id: channel_id,
        name: name.clone(),
        role: ChannelRole::Submix,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Default::default(),
    });
    // The group takes the place of its first track; its tracks (with theirs) follow it.
    let family: Vec<TrackId> = members.iter().flat_map(|m| with_descendants(p, *m)).collect();
    let first = p.tracks.iter().position(|t| family.contains(&t.id)).unwrap_or(p.tracks.len());
    let moved: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    let at = first.min(p.tracks.len());
    let group = Track {
        id,
        name,
        color: None,
        content: Default::default(),
        channel: Some(channel_id),
        parent,
        extensions: Default::default(),
    };
    p.tracks.splice(at..at, std::iter::once(group).chain(moved));
    for track in p.tracks.iter_mut().filter(|t| members.contains(&t.id)) {
        track.parent = Some(id);
    }
    let member_channels: Vec<ChannelId> = members.iter().filter_map(|m| channel_id_of(p, *m)).collect();
    for channel in p.channels.iter_mut().filter(|c| member_channels.contains(&c.id)) {
        channel.output = Some(channel_id);
    }
    Ok(json!({ "track": id.0 }))
}

fn ungroup_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let group = track_id(p, params)?;
    let channel = channel_id_of(p, group).and_then(|id| p.channels.iter().find(|c| c.id == id));
    let Some(channel) = channel.filter(|c| c.role == ChannelRole::Submix) else {
        return Err("only a group track can be ungrouped".into());
    };
    let (group_channel, output) = (channel.id, channel.output);
    let parent = p.tracks.iter().find(|t| t.id == group).and_then(|t| t.parent);
    for track in p.tracks.iter_mut().filter(|t| t.parent == Some(group)) {
        track.parent = parent;
    }
    for c in p.channels.iter_mut().filter(|c| c.output == Some(group_channel)) {
        c.output = output;
    }
    remove_track(p, group, group_channel);
    Ok(json!({ "ungrouped": group.0 }))
}

/// Removes a track, its channel, its lanes and slots, and every reference to them.
fn remove_track(p: &mut Project, track: TrackId, channel: ChannelId) {
    p.tracks.retain(|t| t.id != track);
    p.channels.retain(|c| c.id != channel);
    for c in &mut p.channels {
        if c.output == Some(channel) {
            c.output = None;
        }
        c.sends.retain(|s| s.target != channel);
    }
    p.arrangement.lanes.retain(|l| l.track != Some(track));
    p.launcher.columns.retain(|c| c.track != track);
    for content in &mut p.contents {
        content.lanes.retain(|l| l.target != Some(track));
    }
}

fn move_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let to = params.get("to").and_then(Value::as_u64).ok_or("to must be a whole number from 0")?;
    let family = with_descendants(p, track);
    let block: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    let at = usize::try_from(to).unwrap_or(usize::MAX).min(p.tracks.len());
    p.tracks.splice(at..at, block);
    Ok(json!({ "track": track.0, "position": at }))
}

fn rename_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let name = name.ok_or("name must be a text, not empty")?.to_string();
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.tracks.iter_mut().find(|t| t.id == track) {
        t.name.clone_from(&name);
    }
    if let Some(channel) = channel_id_of(&session.project, track)
        && let Some(c) = session.project.channels.iter_mut().find(|c| c.id == channel)
    {
        c.name.clone_from(&name);
    }
    Ok(json!({ "name": name }))
}

fn set_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("color").and_then(Value::as_str).ok_or("color must be \"#rrggbb\" or \"none\"")?;
    let color = if text.eq_ignore_ascii_case("none") {
        None
    } else {
        let hex = text.strip_prefix('#').filter(|h| h.len() == 6).ok_or("color must be \"#rrggbb\" or \"none\"")?;
        let part = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (part(0), part(2), part(4)) {
            (Some(r), Some(g), Some(b)) => Some(loopwerk_model::Rgb(r, g, b)),
            _ => return Err("color must be \"#rrggbb\" or \"none\"".into()),
        }
    };
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.tracks.iter_mut().find(|t| t.id == track) {
        t.color = color;
    }
    Ok(json!({ "color": text }))
}

fn delete_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let p = &session.project;
    let is_group = channel_id_of(p, track)
        .and_then(|id| p.channels.iter().find(|c| c.id == id))
        .is_some_and(|c| c.role == ChannelRole::Submix);
    if is_group {
        ungroup_track(session, params)?;
        return Ok(json!({ "deleted": track.0 }));
    }
    let channel = channel_id_of(&session.project, track);
    let p = &mut session.project;
    match channel {
        Some(channel) => remove_track(p, track, channel),
        None => {
            p.tracks.retain(|t| t.id != track);
            p.arrangement.lanes.retain(|l| l.track != Some(track));
            p.launcher.columns.retain(|c| c.track != track);
        }
    }
    Ok(json!({ "deleted": track.0 }))
}

/// The lane of `params.track` and the clip index in it, checked.
fn lane_and_index(p: &Project, params: &Value) -> Result<(usize, usize), CommandError> {
    let track = track_id(p, params)?;
    let index = params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.clips")?;
    let lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let count = p.arrangement.lanes.get(lane).map_or(0, |l| l.clips.len());
    let index = usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
    Ok((lane, index))
}

fn set_clip_gain(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > 24.0 {
        return Err(format!("db must be at most +24, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let (lane, index) = lane_and_index(&session.project, params)?;
    let content = session
        .project
        .arrangement
        .lanes
        .get(lane)
        .and_then(|l| l.clips.get(index))
        .map(|c| c.content)
        .ok_or("the clip is missing")?;
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    let mut audio = false;
    for lane in &mut content.lanes {
        if let LaneData::Audio(region) = &mut lane.data {
            region.gain = gain;
            audio = true;
        }
    }
    if !audio {
        return Err("only an audio clip has a gain".into());
    }
    Ok(json!({ "gain_db": to_db(gain) }))
}

fn set_clip_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let length_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let beats = number(params, name)?;
                if beats < 0.0 {
                    return Err(format!("{name} must be at least 0 beats, got {beats}").into());
                }
                Ok(Some(beats))
            }
        }
    };
    let (fade_in, fade_out) = (length_of("in")?, length_of("out")?);
    if fade_in.is_none() && fade_out.is_none() {
        return Err("give in, out or both, in beats".into());
    }
    let (lane, index) = lane_and_index(&session.project, params)?;
    let p = &mut session.project;
    let clip = p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?;
    let audio = p.content(clip.content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
    if !audio {
        return Err("only an audio clip has fades".into());
    }
    let changed =
        Clip { fade_in: fade_in.unwrap_or(clip.fade_in), fade_out: fade_out.unwrap_or(clip.fade_out), ..clip.clone() };
    if !changed.fades_valid() {
        return Err(format!(
            "the fades ({} + {} beats) are longer than the clip ({} beats)",
            changed.fade_in, changed.fade_out, changed.length
        )
        .into());
    }
    let result = json!({ "fade_in": changed.fade_in, "fade_out": changed.fade_out });
    if let Some(slot) = p.arrangement.lanes.get_mut(lane).and_then(|l| l.clips.get_mut(index)) {
        *slot = changed;
    }
    Ok(result)
}

fn copy_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (lane, index) = lane_and_index(&session.project, params)?;
    let clip = session.project.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).cloned();
    let clip = clip.ok_or("the clip is missing")?;
    let content = session.project.content(clip.content).cloned().ok_or("the clip's content is missing")?;
    let name = content.name.clone();
    session.clipboard = Some((clip, content));
    Ok(json!({ "copied": name }))
}

fn cut_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    copy_clip(session, params)?;
    delete_clip(session, params)
}

fn paste_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (clip, content) = session.clipboard.clone().ok_or("nothing has been copied")?;
    let start = number(params, "start")?;
    if start < 0.0 {
        return Err(format!("start must be at least 0, got {start}").into());
    }
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let track = track_id(&session.project, params)?;
    let p = &mut session.project;
    let target = p.tracks.iter().find(|t| t.id == track).ok_or("the track is missing")?;
    let notes = content.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_)));
    let audio = content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)));
    if (notes && !target.content.notes) || (audio && !target.content.audio) {
        let what = if audio { "an audio clip" } else { "a MIDI clip" };
        return Err(format!("track {:?} cannot hold {what}", target.name).into());
    }
    // The copy plays its own content, so editing one does not change the other.
    let id = loopwerk_model::ContentId(p.contents.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    p.contents.push(loopwerk_model::ClipContent { id, ..content });
    let placed = Clip { content: id, start, ..clip };
    let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
        Some(lane) => lane,
        None => {
            let name = p.tracks.iter().find(|t| t.id == track).map(|t| t.name.clone()).unwrap_or_default();
            p.arrangement.lanes.push(ArrangementLane { name, track: Some(track), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, start, start + placed.length);
    }
    lane.clips.push(placed.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let index = lane.clips.iter().position(|c| *c == placed).unwrap_or_default();
    Ok(json!({ "track": track.0, "index": index, "start": start }))
}

fn delete_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (lane, index) = lane_and_index(&session.project, params)?;
    if let Some(lane) = session.project.arrangement.lanes.get_mut(lane) {
        lane.clips.remove(index);
    }
    Ok(json!({ "deleted": index }))
}

fn duplicate_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (lane, index) = lane_and_index(&session.project, params)?;
    let lane = session.project.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    let mut copy = lane.clips.get(index).cloned().ok_or("the clip is missing")?;
    copy.start += copy.length;
    make_room(&mut lane.clips, copy.start, copy.start + copy.length);
    lane.clips.push(copy.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let new_index = lane.clips.iter().position(|c| *c == copy).unwrap_or_default();
    Ok(json!({ "index": new_index, "start": copy.start }))
}

fn set_output(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from = channel_param(session, params)?.id;
    let p = &session.project;
    let to = match params.get("to") {
        Some(Value::String(main)) if main.eq_ignore_ascii_case("main") => None,
        Some(_) => {
            let track = track_at(p, params, "to")?;
            let target = p
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|id| p.channels.iter().find(|c| c.id == id))
                .ok_or("the target track has no mixer channel")?;
            if target.role != ChannelRole::Submix {
                return Err(format!("{:?} is not a group track; route to \"main\" or a group", target.name).into());
            }
            // The group must not play, directly or through other groups, into the track itself.
            let mut next = Some(target.id);
            for _ in 0..=p.channels.len() {
                match next {
                    Some(id) if id == from => {
                        return Err(format!("routing into {:?} would make a loop", target.name).into());
                    }
                    Some(id) => next = p.channels.iter().find(|c| c.id == id).and_then(|c| c.output),
                    None => break,
                }
            }
            Some(target.id)
        }
        None => return Err("to must be \"main\" or a group track".into()),
    };
    let channel = channel_param(session, params)?;
    channel.output = to;
    let name = to
        .and_then(|id| session.project.tracks.iter().find(|t| t.channel == Some(id)))
        .map_or_else(|| "main".to_string(), |t| t.name.clone());
    Ok(json!({ "to": name }))
}

fn set_solo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let solo = params.get("solo").and_then(Value::as_bool).ok_or("solo must be true or false")?;
    let exclusive = match params.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
    };
    let id = channel_param(session, params)?.id;
    for channel in &mut session.project.channels {
        if channel.id == id {
            channel.solo = solo;
        } else if exclusive {
            channel.solo = false;
        }
    }
    Ok(json!({ "solo": solo }))
}

fn set_metronome(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    // Without a level the metronome keeps the one it has (-6 dB to begin with).
    let gain = match params.get("db") {
        None => None,
        Some(_) => {
            let db = number_or_neg_infinity(params, "db")?;
            if db > 6.0 {
                return Err(format!("db must be at most +6, got {db}").into());
            }
            Some(if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) })
        }
    };
    send(session, LaunchCommand::SetMetronome { on, gain: gain.map(|g| g as f32) })?;
    let mut result = json!({ "metronome": on });
    if let Some(gain) = gain {
        result["db"] = json!(to_db(gain));
    }
    Ok(result)
}

fn move_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let index = params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.clips")?;
    let start = number(params, "start")?;
    if start < 0.0 {
        return Err(format!("start must be at least 0, got {start}").into());
    }
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let to = match params.get("to_track") {
        Some(_) => track_at(&session.project, params, "to_track")?,
        None => track,
    };
    let p = &mut session.project;
    let from_lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let count = p.arrangement.lanes.get(from_lane).map_or(0, |l| l.clips.len());
    let index = usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
    if to != track {
        let clip = p.arrangement.lanes.get(from_lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?;
        let kinds = p.content(clip.content).map_or((false, false), |c| {
            (
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
            )
        });
        let target = p.tracks.iter().find(|t| t.id == to).ok_or("the target track is missing")?;
        let fits = (!kinds.0 || target.content.notes) && (!kinds.1 || target.content.audio) && (kinds.0 || kinds.1);
        if !fits {
            let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("track {:?} cannot hold {what}", target.name).into());
        }
    }
    let to_lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(to)) {
        Some(lane) => lane,
        None => {
            let name = p.tracks.iter().find(|t| t.id == to).map(|t| t.name.clone()).unwrap_or_default();
            p.arrangement.lanes.push(ArrangementLane { name, track: Some(to), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let mut clip = p.arrangement.lanes.get_mut(from_lane).ok_or("the lane is missing")?.clips.remove(index);
    clip.start = start;
    let lane = p.arrangement.lanes.get_mut(to_lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, start, start + clip.length);
    }
    lane.clips.push(clip.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let new_index = lane.clips.iter().position(|c| *c == clip).unwrap_or_default();
    Ok(json!({ "start": start, "track": to.0, "index": new_index }))
}

/// Clears `[from, to)` on a lane for a clip placed there (docs/spec/arrangement-editing.md):
/// clips entirely inside are removed, clips reaching in are shortened, and a clip around it is
/// split in two, the second part continuing its content from where it was. A split clip's
/// first part keeps its fade-in and the second its fade-out; fades are fitted to what is left.
pub(crate) fn make_room(clips: &mut Vec<Clip>, from: f64, to: f64) {
    let mut kept = Vec::with_capacity(clips.len() + 1);
    for clip in clips.drain(..) {
        let (start, end) = (clip.start, clip.start + clip.length);
        if end <= from || start >= to {
            kept.push(clip);
            continue;
        }
        let split = start < from && end > to;
        if start < from {
            let mut first = Clip { length: from - start, ..clip.clone() };
            if split {
                first.fade_out = 0.0;
            }
            first.fit_fades();
            kept.push(first);
        }
        if end > to {
            let offset = content_at(&clip, to - start);
            let mut second = Clip { start: to, length: end - to, offset, ..clip };
            if split {
                second.fade_in = 0.0;
            }
            second.fit_fades();
            kept.push(second);
        }
    }
    *clips = kept;
}

/// Where in its content a clip is `beats` after it starts, following its loop.
fn content_at(clip: &Clip, beats: f64) -> f64 {
    let at = clip.offset + beats;
    match clip.loop_range {
        Some(r) if r.end > r.start && at >= r.end => r.start + (at - r.start).rem_euclid(r.end - r.start),
        _ => at,
    }
}

fn launcher_grid(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let scenes: Vec<Value> = p
        .launcher
        .scenes
        .iter()
        .zip(1..)
        .map(|(scene, number): (_, u32)| json!({ "scene": number, "name": scene.name, "tempo_bpm": scene.tempo_bpm }))
        .collect();
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .map(|column| {
            let slots: Vec<Value> = column
                .slots
                .iter()
                .zip(1..)
                .map(|(slot, number): (_, u32)| match &slot.clip {
                    Some(clip) => json!({
                        "scene": number,
                        "clip": p.content(clip.content).map(|c| c.name.as_str()),
                        "length": clip.length,
                        "loop": clip.loop_range.is_some(),
                        "enabled": clip.enabled,
                    }),
                    None => json!({ "scene": number, "clip": null, "stop_button": slot.has_stop }),
                })
                .collect();
            json!({ "track": column.track.0, "track_name": track_name(p, column.track), "slots": slots })
        })
        .collect();
    Ok(json!({ "scenes": scenes, "tracks": tracks }))
}

fn launcher_state(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let preview_state = session.player().map(|player| {
        let preview = player.preview_monitor();
        json!({ "playing": preview.playing(), "seconds": (preview.seconds() * 1000.0).round() / 1000.0 })
    });
    let cpu = session.player().map(|player| player.cpu_monitor().load_percent());
    let monitor = session.player_mut()?.monitor();
    let (running, beat, tempo) = (monitor.running(), monitor.beat(), monitor.tempo_bpm());
    let start_beat = monitor.start_beat();
    let monitor_metronome = monitor.metronome();
    let statuses: Vec<Option<ColumnStatus>> = (0..monitor.columns()).map(|i| monitor.column(i)).collect();
    let p = session.project();
    let ts = p.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .zip(statuses)
        .map(|(column, status)| {
            let status = status.unwrap_or(ColumnStatus { playing: None, queued: None, overrides_arrangement: false });
            let clip_name = |slot: usize| {
                column
                    .slots
                    .get(slot)
                    .and_then(|s| s.clip.as_ref())
                    .and_then(|c| p.content(c.content))
                    .map(|c| c.name.clone())
            };
            json!({
                "track": column.track.0,
                "track_name": track_name(p, column.track),
                "playing": status.playing.map(|slot| json!({ "scene": slot + 1, "clip": clip_name(slot) })),
                "queued": match status.queued {
                    None => Value::Null,
                    Some(Queued::Stop) => json!("stop"),
                    Some(Queued::Launch(slot)) => json!({ "scene": slot + 1, "clip": clip_name(slot) }),
                },
                "plays_arrangement": !status.overrides_arrangement,
            })
        })
        .collect();
    // Rounded once, so the beat and the position agree.
    let beat = if beat.is_finite() { (beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 };
    Ok(json!({
        "running": running,
        "metronome": monitor_metronome,
        "tempo_bpm": tempo,
        "beat": beat,
        "position": format!("{}.{}", (beat / bar_beats).floor() as u64 + 1, (beat % bar_beats).floor() as u64 + 1),
        "start_beat": if start_beat.is_finite() { (start_beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 },
        "tracks": tracks,
        "preview": preview_state,
        "cpu": cpu,
    }))
}

fn send(session: &mut Session, command: LaunchCommand) -> Result<(), CommandError> {
    session.player_mut()?.send(command)
}

fn play(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::Play)?;
    Ok(json!({ "transport": "playing" }))
}

fn stop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let command = match params.get("return") {
        None => LaunchCommand::Stop,
        Some(Value::String(s)) if s == "marker" => LaunchCommand::Stop,
        Some(Value::String(s)) if s == "here" => LaunchCommand::StopHere,
        Some(other) => return Err(format!("return must be \"marker\" or \"here\", got {other}").into()),
    };
    send(session, command)?;
    Ok(json!({ "transport": "stopped" }))
}

fn set_position(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = number(params, "beats")?;
    if !(0.0..=MAX_START_BEAT).contains(&beats) {
        return Err(format!("beats must be between 0 and {MAX_START_BEAT}, got {beats}").into());
    }
    send(session, LaunchCommand::SetPosition { beat: beats })?;
    Ok(json!({ "start_beat": beats }))
}

fn launch_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    let scene = scene_index(session.project(), params)?;
    let slot = session.project().launcher.columns.get(column).and_then(|c| c.slots.get(scene));
    let queued = match slot {
        Some(slot) if slot.clip.as_ref().is_some_and(|c| c.enabled) => "launch",
        Some(slot) if slot.clip.is_some() => return Err("the clip in that slot is deactivated".into()),
        Some(slot) if slot.has_stop => "stop",
        _ => return Err("that slot is empty and has no stop button".into()),
    };
    send(session, LaunchCommand::LaunchClip { column, slot: scene })?;
    Ok(json!({ "scene": scene + 1, "queued": queued }))
}

fn launch_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::LaunchScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "launch" }))
}

fn stop_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    send(session, LaunchCommand::StopColumn { column })?;
    Ok(json!({ "queued": "stop" }))
}

fn stop_all(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::StopAll)?;
    Ok(json!({ "queued": "stop" }))
}

fn set_quantization(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("value").and_then(Value::as_str).ok_or("value must be a string such as \"1 bar\"")?;
    let quantization = parse_quantization(text)?;
    send(session, LaunchCommand::SetQuantization(quantization))?;
    Ok(json!({ "quantization": text.trim() }))
}

fn back_to_arrangement(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::BackToArrangement)?;
    Ok(json!({ "arrangement": true }))
}

/// Parses a launch quantisation: "none", "1 bar", "2 bars", "1/4", "1/8T" and so on.
pub fn parse_quantization(text: &str) -> Result<Quantization, CommandError> {
    let normal = text.trim().to_ascii_lowercase();
    match normal.as_str() {
        "none" | "off" => return Ok(Quantization::None),
        "8 bars" => return Ok(Quantization::Bars(8)),
        "4 bars" => return Ok(Quantization::Bars(4)),
        "2 bars" => return Ok(Quantization::Bars(2)),
        "1 bar" => return Ok(Quantization::Bars(1)),
        _ => {}
    }
    let (value, triplet) = match normal.strip_suffix('t') {
        Some(value) => (value, true),
        None => (normal.as_str(), false),
    };
    let beats = match (value, triplet) {
        ("1/2", _) => 2.0,
        ("1/4", _) => 1.0,
        ("1/8", _) => 0.5,
        ("1/16", _) => 0.25,
        ("1/32", false) => 0.125,
        _ => {
            return Err(format!(
                "unknown quantisation {text:?}; use none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, \
                 1/8T, 1/16, 1/16T or 1/32"
            )
            .into());
        }
    };
    Ok(Quantization::Beats(if triplet { beats * 2.0 / 3.0 } else { beats }))
}

/// The launcher column of `params.track`.
fn column_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let track = track_id(p, params)?;
    p.launcher
        .columns
        .iter()
        .position(|c| c.track == track)
        .ok_or_else(|| format!("track {} has no clip slots", track_name(p, track)).into())
}

/// The 0-based scene index of the 1-based `params.scene`.
fn scene_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let count = p.launcher.scenes.len();
    let number = params.get("scene").and_then(Value::as_u64).ok_or("scene must be a whole number from 1")?;
    usize::try_from(number)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|i| *i < count)
        .ok_or_else(|| format!("scene {number} does not exist; the project has {count} scenes").into())
}

fn track_name(p: &Project, track: TrackId) -> String {
    p.tracks.iter().find(|t| t.id == track).map_or_else(|| track.0.to_string(), |t| t.name.clone())
}

fn undo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.undo() { Ok(json!({ "undone": true })) } else { Err("nothing to undo".into()) }
}

fn redo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.redo() { Ok(json!({ "redone": true })) } else { Err("nothing to redo".into()) }
}

/// Resolves `params.track`: a track id (number) or a name. Names match exactly first, then
/// ignoring case, and must be unambiguous.
pub(crate) fn track_id(p: &Project, params: &Value) -> Result<TrackId, CommandError> {
    track_at(p, params, "track")
}

/// Resolves the track id or name in `params[key]`.
pub(crate) fn track_at(p: &Project, params: &Value, key: &str) -> Result<TrackId, CommandError> {
    match params.get(key) {
        Some(Value::Number(n)) => {
            let id = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("track id must be a whole number")?;
            p.tracks
                .iter()
                .find(|t| t.id.0 == id)
                .map(|t| t.id)
                .ok_or_else(|| format!("track {id} does not exist").into())
        }
        Some(Value::String(name)) => {
            if let Some(t) = p.tracks.iter().find(|t| t.name == *name) {
                return Ok(t.id);
            }
            let matches: Vec<&Track> = p.tracks.iter().filter(|t| t.name.eq_ignore_ascii_case(name)).collect();
            match matches.as_slice() {
                [t] => Ok(t.id),
                [] => Err(format!("no track named {name:?}; project.tracks lists them").into()),
                _ => Err(format!("several tracks are named {name:?}; use the track id").into()),
            }
        }
        _ => Err(format!("{key} must be a track id or name").into()),
    }
}

pub(crate) fn channel_param<'a>(session: &'a mut Session, params: &Value) -> Result<&'a mut Channel, CommandError> {
    let track = track_id(&session.project, params)?;
    let channel = session
        .project
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .ok_or("the track has no mixer channel")?;
    session.project.channels.iter_mut().find(|c| c.id == channel).ok_or_else(|| "the track's channel is missing".into())
}

fn channel_of<'a>(p: &'a Project, track: &Track) -> Option<&'a Channel> {
    let id = track.channel?;
    p.channels.iter().find(|c| c.id == id)
}

pub(crate) fn number(params: &Value, name: &str) -> Result<f64, CommandError> {
    params
        .get(name)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{name} must be a number").into())
}

/// Like [`number`], but also accepts the string "-inf".
fn number_or_neg_infinity(params: &Value, name: &str) -> Result<f64, CommandError> {
    match params.get(name) {
        Some(Value::String(s)) if s == "-inf" => Ok(f64::NEG_INFINITY),
        _ => number(params, name),
    }
}

/// Linear gain to decibels, rounded to 0.01 dB; `None` (JSON null) for silence.
fn to_db(gain: f64) -> Option<f64> {
    (gain > 0.0).then(|| (20.0 * gain.log10() * 100.0).round() / 100.0)
}
