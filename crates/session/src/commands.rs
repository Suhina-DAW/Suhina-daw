//! The command registry. Each command is a [`CommandSpec`]; new commands are added to
//! [`COMMANDS`] with tests below.

use serde_json::{Value, json};
use suhina_engine::{ColumnStatus, LaunchCommand, MAX_START_BEAT, Quantization, Queued};
use suhina_model::{
    ArrangementLane, Channel, ChannelId, ChannelRole, Clip, FadeCurve, FadeShape, LaneData, LoopRange, Project, Scene,
    Slot, TimeSignature, Track, TrackId,
};

use crate::{CommandError, Session, automation, devices, files, follow, meters, notes, preview};

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
    /// Scans for plug-ins or changes where scans look (ADR 0006); belongs to Suhina, not to
    /// one project. Runs on [`crate::Projects`].
    Plugins,
    /// Acts on one of the project's running plug-ins (opens or closes its editor, restarts
    /// it); never changes the project or its undo history.
    Plugin,
    /// The browser's library: folders, search, pins, recent items, tags (ADR 0012); belongs
    /// to Suhina, not to one project. Runs on [`crate::Projects`].
    Library,
    /// Lists or chooses the audio output (host, device, sample rate, buffer size) or reports
    /// on it; belongs to Suhina, not to one project. Runs on [`crate::Projects`].
    Audio,
}

impl ParamType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Bool => "bool",
            Self::Track => "track",
            Self::Decibels => "decibels",
            Self::Text => "text",
            Self::Tracks => "tracks",
            Self::Texts => "texts",
            Self::Integers => "integers",
            Self::Notes => "notes",
            Self::Numbers => "numbers",
        }
    }
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
            Self::Audio => "audio",
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
    /// A list of whole numbers (note indices).
    Integers,
    /// A list of new notes, each `{time, length, key, velocity?}`.
    Notes,
    /// A list of numbers.
    Numbers,
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
/// Lets the calls of one drag make one undo step.
const CONTINUE: Param = optional(
    "continue",
    ParamType::Bool,
    "true adds the change to the undo step of the previous call of this command on this track, \
     when no other edit came between (one drag, one step)",
);
const DEVICE: Param = param("device", ParamType::Integer, "device index on the track, from 0, as in track.devices");
const RETURN: Param = param("return", ParamType::Track, "return track id or name, from project.tracks");
const SCENE: Param = param("scene", ParamType::Integer, "scene number, from 1, as in project.launcher");
/// A note command's clip: an arrangement clip by index, or a launcher clip by scene.
const CLIP_INDEX: Param =
    optional("index", ParamType::Integer, "an arrangement clip's index on the track, from project.clips");
const CLIP_SCENE: Param =
    optional("scene", ParamType::Integer, "instead of index: the clip in the track's slot of this scene, from 1");
const NOTES: Param = param("notes", ParamType::Integers, "note indices, from note.list; at most 10000");
const AUTOMATED: Param = param(
    "param",
    ParamType::Text,
    "the parameter: volume, pan or mute of the track's mixer (no device), or a device's parameter id, from devices.list (a plug-in's from device.pluginParams)",
);
const AUTOMATED_DEVICE: Param = optional(
    "device",
    ParamType::Integer,
    "device index on the track, from 0, as in track.devices; left out for the track's mixer",
);
const POINT: Param =
    param("point", ParamType::Integer, "point index on the lane, from 0 in beat order, as in automation.list");
const CURVE: Param = optional(
    "curve",
    ParamType::Number,
    "shape of the segment to the next point, -1 to 1: 0 a straight line (the default), above 0 fast first, below 0 slow first",
);

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
            if matches!(
                p.ty,
                ParamType::Tracks | ParamType::Texts | ParamType::Integers | ParamType::Notes | ParamType::Numbers
            ) {
                let items = match p.ty {
                    ParamType::Tracks => json!({ "type": ["integer", "string"] }),
                    ParamType::Integers => json!({ "type": "integer", "minimum": 0 }),
                    ParamType::Notes => json!({
                        "type": "object",
                        "properties": {
                            "time": { "type": "number", "description": "start in beats from the clip's start" },
                            "length": { "type": "number", "description": "length in beats, above 0" },
                            "key": { "type": "integer", "description": "MIDI key 0 to 127; 60 is C3" },
                            "velocity": { "type": "number", "description": "1 to 127 (default 100)" },
                        },
                        "required": ["time", "length", "key"],
                        "additionalProperties": false,
                    }),
                    ParamType::Numbers => json!({ "type": "number" }),
                    _ => json!({ "type": "string" }),
                };
                properties.insert(p.name.to_string(), json!({ "type": "array", "items": items, "description": p.doc }));
                continue;
            }
            let ty = match p.ty {
                ParamType::Tracks | ParamType::Texts | ParamType::Integers | ParamType::Notes | ParamType::Numbers => {
                    json!("array")
                }
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

    /// The command as one row of the table in `docs/commands.md`.
    pub fn markdown_row(&self) -> String {
        let cell = |text: &str| text.replace('|', r"\|");
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let optional = if p.required { "" } else { ", optional" };
                format!("`{}` ({}{optional}): {}", p.name, p.ty.name(), cell(p.doc))
            })
            .collect();
        let params = if params.is_empty() { "none".to_string() } else { params.join("; ") };
        format!("| `{}` | {} | {} | {params} |", self.id, self.kind.name(), cell(self.label))
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
        label: "Save the project as a Suhina project (.suhina) with its plug-ins' state as it is now; without a path, to the project file it was opened from or last saved to",
        params: &[
            optional("path", ParamType::Text, "file to save to; .suhina is added when missing"),
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
            CONTINUE,
        ],
        kind: CommandKind::Edit,
        run: set_volume,
    },
    CommandSpec {
        id: "track.setPan",
        label: "Set a track's pan",
        params: &[TRACK, param("pan", ParamType::Number, "-1 is full left, 0 centre, 1 full right"), CONTINUE],
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
        label: "Set an audio clip's fade-in and fade-out: lengths in beats (together at most the clip's length), shapes and curves",
        params: &[
            TRACK,
            param("index", ParamType::Integer, "clip index on the track, from project.clips"),
            optional("in", ParamType::Number, "fade-in length in beats from the clip's start, at least 0; 0 is none"),
            optional("out", ParamType::Number, "fade-out length in beats before the clip's end, at least 0; 0 is none"),
            optional("inShape", ParamType::Text, "fade-in shape: linear, equal_power, fast, slow or s_curve"),
            optional("outShape", ParamType::Text, "fade-out shape: linear, equal_power, fast, slow or s_curve"),
            optional(
                "inCurve",
                ParamType::Number,
                "fade-in curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
            optional(
                "outCurve",
                ParamType::Number,
                "fade-out curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
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
        label: "Place a copy of an arrangement clip right after it, replacing what the copy covers unless stacked",
        params: &[
            TRACK,
            param("index", ParamType::Integer, "clip index on the track, from project.clips"),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copy as they are; false (default) shortens, splits or removes them",
            ),
        ],
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
        id: "track.setCue",
        label: "Turn a track's cue on or off: a cued track is also heard on the cue output (headphones, output.setCuePair), after its own volume and pan, while the main mix stays as it is",
        params: &[TRACK, param("cue", ParamType::Bool, "true cues the track")],
        kind: CommandKind::Edit,
        run: set_cue,
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
        label: "List the clip launcher: scenes (name, tempo) and each track's slots (clip name, follow action, stop button)",
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
        label: "Start the transport from the play start marker (beat 0 unless moved), or from an arrangement clip's start: the arrangement plays, launches follow the beat",
        params: &[
            optional(
                "from",
                ParamType::Text,
                "marker (default): from the play start marker; clip: the marker moves to the start of the clip track and index name, and playback starts there",
            ),
            optional("track", ParamType::Track, "with from: clip, the clip's track (id or name)"),
            optional("index", ParamType::Integer, "with from: clip, the clip's index on the track, from project.clips"),
        ],
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
        id: "mixer.levels",
        label: "Read the level meters of the playing engine: the main output's and each track's peak and RMS level per side in dB (null for silence), and whether it clipped",
        params: &[],
        kind: CommandKind::Transport,
        run: meters::levels,
    },
    CommandSpec {
        id: "mixer.resetClip",
        label: "Turn off clip indicators: a track's, the main output's, or every one when neither is given",
        params: &[
            optional("track", ParamType::Track, "track id or name, from project.tracks"),
            optional("main", ParamType::Bool, "true turns off the main output's"),
        ],
        kind: CommandKind::Transport,
        run: meters::reset_clip,
    },
    CommandSpec {
        id: "clip.launch",
        label: "Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track)",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch_clip,
    },
    CommandSpec {
        id: "clip.setLoop",
        label: "Switch the loop of the clip in a track's slot: on, it repeats its whole length until stopped; off, it plays once",
        params: &[TRACK, SCENE, param("loop", ParamType::Bool, "true loops the clip, false plays it once")],
        kind: CommandKind::Edit,
        run: set_slot_loop,
    },
    CommandSpec {
        id: "scene.launch",
        label: "Launch a scene: every track launches its slot in that row at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: launch_scene,
    },
    CommandSpec {
        id: "scene.insert",
        label: "Insert an empty scene (a row of empty slots with stop buttons) at a position, or after the last scene",
        params: &[
            optional(
                "scene",
                ParamType::Integer,
                "number the new scene gets, from 1 to one past the last scene; after the last scene if left out",
            ),
            optional("name", ParamType::Text, "the new scene's name; empty if left out"),
        ],
        kind: CommandKind::Edit,
        run: insert_scene,
    },
    CommandSpec {
        id: "scene.duplicate",
        label: "Copy a scene, with its name, tempo, time signature and the clips in its row, to a new scene right below it",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: duplicate_scene,
    },
    CommandSpec {
        id: "scene.delete",
        label: "Delete a scene and the clips in its row; the last scene left cannot be deleted",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: delete_scene,
    },
    CommandSpec {
        id: "scene.rename",
        label: "Rename a scene",
        params: &[SCENE, param("name", ParamType::Text, "the new name; empty clears it")],
        kind: CommandKind::Edit,
        run: rename_scene,
    },
    CommandSpec {
        id: "scene.setTempo",
        label: "Set the tempo a scene switches the project to when it launches, or clear it",
        params: &[
            SCENE,
            optional("bpm", ParamType::Number, "20 … 999; left out or null, the scene keeps the tempo playing"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_tempo,
    },
    CommandSpec {
        id: "scene.setTimeSignature",
        label: "Set the time signature a scene switches to when it launches, or clear it",
        params: &[
            SCENE,
            optional(
                "numerator",
                ParamType::Integer,
                "beats in a bar, 1 … 99; left out with the denominator, the scene keeps the time signature playing",
            ),
            optional("denominator", ParamType::Integer, "the note value of a beat: 1, 2, 4, 8 or 16"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_time_signature,
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
        id: "clip.setFollowAction",
        label: "Set what a slot clip does after it has played for a while: action A or B, by chance",
        params: &[
            TRACK,
            SCENE,
            optional("enabled", ParamType::Bool, "whether the clip runs its follow action"),
            optional(
                "time",
                ParamType::Number,
                "beats from the launch to the action, more than 0; used when not linked",
            ),
            optional("linked", ParamType::Bool, "true: the time is the clip's length"),
            optional(
                "actionA",
                ParamType::Text,
                "nothing, stop, again, previous, next, first, last, any, other, or \"jump N\" for the slot of scene N",
            ),
            optional("actionB", ParamType::Text, "like actionA"),
            optional("chanceA", ParamType::Integer, "weight of action A, 0 to 100"),
            optional("chanceB", ParamType::Integer, "weight of action B, 0 to 100"),
        ],
        kind: CommandKind::Edit,
        run: follow::set_follow_action,
    },
    CommandSpec {
        id: "devices.list",
        label: "List the built-in effects and instruments (id, name, effect or instrument) with their parameters (id, range, default, unit, options)",
        params: &[],
        kind: CommandKind::Read,
        run: devices::list,
    },
    CommandSpec {
        id: "track.devices",
        label: "List the devices on a track in order, with the parameter values of every built-in effect and instrument",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: devices::track_devices,
    },
    CommandSpec {
        id: "device.add",
        label: "Add a built-in effect or instrument to a track; an instrument goes first and plays the track's notes, an effect goes last unless a position is given",
        params: &[
            TRACK,
            optional(
                "id",
                ParamType::Text,
                "id of a built-in effect or instrument, from devices.list (not needed with a preset)",
            ),
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
                "the id of a built-in effect or instrument, from devices.list, or a plug-in from plugins.list",
            ),
            optional("preset", ParamType::Text, "a preset file for the device (DX7 voice data for the FM synth)"),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1"),
        ],
        kind: CommandKind::Edit,
        run: files::add_track,
    },
    CommandSpec {
        id: "project.missingFiles",
        label: "List the audio files the project's clips name that cannot be found, with where they were and how many clips play each",
        params: &[],
        kind: CommandKind::Read,
        run: crate::relink::missing_files,
    },
    CommandSpec {
        id: "project.relinkFiles",
        label: "Point every clip that plays a missing (or any) audio file at another file, as one undo step; library.findFiles finds candidates",
        params: &[
            param("files", ParamType::Texts, "the files as project.missingFiles names them"),
            param("to", ParamType::Texts, "for each, the file to play instead (the same order); at most 4096"),
        ],
        kind: CommandKind::Edit,
        run: crate::relink::relink_files,
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
        id: "preview.noteOn",
        label: "Play a note through a track's instrument and hold it until preview.noteOff, as the piano roll's key column does",
        params: &[
            TRACK,
            param("key", ParamType::Integer, "the key to play, 0 to 127 (60 is C3)"),
            optional("velocity", ParamType::Number, "how hard, 1 to 127 (default 100)"),
        ],
        kind: CommandKind::Transport,
        run: preview::note_on,
    },
    CommandSpec {
        id: "preview.noteOff",
        label: "Let go of the note preview.noteOn started on a track, or of every held preview note",
        params: &[TRACK, optional("key", ParamType::Integer, "the key to let go of; every held key when left out")],
        kind: CommandKind::Transport,
        run: preview::note_off,
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
        label: "Set a parameter of a device on a track, heard from the next block; the value is limited to the parameter's range",
        params: &[
            TRACK,
            DEVICE,
            param(
                "param",
                ParamType::Text,
                "parameter id: a built-in effect or instrument's from devices.list, a plug-in's (a number) from device.pluginParams",
            ),
            param(
                "value",
                ParamType::Number,
                "value in the parameter's unit (dB, Hz, 0..1 for percentages, option index for choices); a plug-in's in its own range, from device.pluginParams",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::set_param,
    },
    CommandSpec {
        id: "device.setAutomation",
        label: "Automate a parameter of a device over the arrangement: points of beat and value, a straight line between them; no points removes the automation (automation.addPoint edits one point)",
        params: &[
            TRACK,
            DEVICE,
            param(
                "param",
                ParamType::Text,
                "parameter id: a built-in effect or instrument's from devices.list, a plug-in's (a number) from device.pluginParams",
            ),
            param(
                "points",
                ParamType::Numbers,
                "a beat then a value for each point, in the parameter's range: [0, 0.2, 4, 0.8] moves from 0.2 at beat 0 to 0.8 at beat 4; two points at one beat jump; [] removes the lane",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::set_automation,
    },
    CommandSpec {
        id: "automation.addPoint",
        label: "Add a point to a parameter's automation (envelope) over the arrangement, making the lane if it has none; the value moves from point to point along each segment's curve",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("beat", ParamType::Number, "arrangement beat, from 0; a second point at one beat makes a jump"),
            param(
                "value",
                ParamType::Number,
                "value in the unit the parameter's set command takes: volume in dB (at most +6; below -150 is silence), pan -1 to 1, mute 0 or 1, a device's as device.setParam",
            ),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: automation::add_point,
    },
    CommandSpec {
        id: "automation.movePoint",
        label: "Move a point of a parameter's automation to another beat or value, or change its curve",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            POINT,
            optional("beat", ParamType::Number, "new arrangement beat"),
            optional("value", ParamType::Number, "new value, as for automation.addPoint"),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: automation::move_point,
    },
    CommandSpec {
        id: "automation.deletePoint",
        label: "Delete a point of a parameter's automation; the lane goes with its last point",
        params: &[TRACK, AUTOMATED_DEVICE, AUTOMATED, POINT],
        kind: CommandKind::Edit,
        run: automation::delete_point,
    },
    CommandSpec {
        id: "automation.movePoints",
        label: "Move several points of one parameter's automation together, by a number of beats and by a value, in one undo step; values stay in the parameter's range",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
            optional(
                "beats",
                ParamType::Number,
                "beats to move the points by: later above 0, earlier below; they must stay from beat 0",
            ),
            optional(
                "value",
                ParamType::Number,
                "how much to change each value by, in the unit of automation.addPoint (dB for the volume, where silence stays silent)",
            ),
            optional("curve", ParamType::Number, "a curve to give every moved point's segment, -1 to 1"),
        ],
        kind: CommandKind::Edit,
        run: automation::move_points,
    },
    CommandSpec {
        id: "automation.deletePoints",
        label: "Delete several points of one parameter's automation in one undo step; the lane goes with its last point",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
        ],
        kind: CommandKind::Edit,
        run: automation::delete_points,
    },
    CommandSpec {
        id: "automation.clear",
        label: "Remove a parameter's automation, or without param every lane of the track's mixer or of the device",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            optional("param", ParamType::Text, "the parameter, as for automation.addPoint; left out for every lane"),
        ],
        kind: CommandKind::Edit,
        run: automation::clear,
    },
    CommandSpec {
        id: "automation.setEnabled",
        label: "Turn a parameter's automation off or on again; a manual change to an automated parameter turns it off, and the value set by hand holds until it is on again",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("enabled", ParamType::Bool, "false turns the lane off, true on"),
        ],
        kind: CommandKind::Edit,
        run: automation::set_enabled,
    },
    CommandSpec {
        id: "automation.list",
        label: "List a track's automation: its mixer's lanes and each device's, with their points (beat, value, curve) and whether they play",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: automation::list,
    },
    CommandSpec {
        id: "device.pluginParams",
        label: "List the parameters of a running plug-in device: id, name, range, default, current value and the plug-in's own text for it",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::plugin_params,
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
        label: "Open a plug-in device's editor in a window, or inside the device view when Suhina embeds editors (Options › Settings › Plug-ins); changes there are heard at once",
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
    CommandSpec {
        id: "track.setSend",
        label: "Set how much of a track goes to a return track (its send); the return runs its devices over what it is sent",
        params: &[
            TRACK,
            RETURN,
            param("level", ParamType::Decibels, "send level in dB, at most +6; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: crate::sends::set_send,
    },
    CommandSpec {
        id: "track.setSendMode",
        label: "Take a track's send to a return before its volume and pan (pre) or after them (post, the default)",
        params: &[TRACK, RETURN, param("mode", ParamType::Text, "\"pre\" or \"post\"")],
        kind: CommandKind::Edit,
        run: crate::sends::set_send_mode,
    },
    CommandSpec {
        id: "return.add",
        label: "Add a return track, after every other track, playing into the main channel; tracks reach it through sends",
        params: &[optional("name", ParamType::Text, "its name (default: A Return, B Return and so on)")],
        kind: CommandKind::Edit,
        run: crate::sends::add_return,
    },
    CommandSpec {
        id: "return.remove",
        label: "Remove a return track and every send to it",
        params: &[RETURN],
        kind: CommandKind::Edit,
        run: crate::sends::remove_return,
    },
    CommandSpec {
        id: "return.rename",
        label: "Rename a return track",
        params: &[RETURN, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: crate::sends::rename_return,
    },
    CommandSpec {
        id: "note.list",
        label: "List a MIDI clip's notes: index, time and length in beats, key (60 is C3), velocity 1 to 127",
        params: &[TRACK, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Read,
        run: notes::list,
    },
    CommandSpec {
        id: "note.add",
        label: "Add notes to a MIDI clip; they go after its other notes, and the result gives their indices",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            param("notes", ParamType::Notes, "the notes, each {time, length, key, velocity?}; at most 10000"),
        ],
        kind: CommandKind::Edit,
        run: notes::add,
    },
    CommandSpec {
        id: "note.delete",
        label: "Delete notes of a MIDI clip; later notes move up in the list",
        params: &[TRACK, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Edit,
        run: notes::delete,
    },
    CommandSpec {
        id: "note.move",
        label: "Move notes of a MIDI clip in time and in pitch; fails if any note would start before 0 or leave keys 0 to 127",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("beats", ParamType::Number, "beats to move by, later when positive"),
            optional("semitones", ParamType::Integer, "semitones to move by, higher when positive"),
        ],
        kind: CommandKind::Edit,
        run: notes::move_notes,
    },
    CommandSpec {
        id: "note.resize",
        label: "Set the length of notes of a MIDI clip, or change it by a number of beats",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("length", ParamType::Number, "the new length in beats, above 0"),
            optional("by", ParamType::Number, "instead: beats to add to each length (negative shortens)"),
        ],
        kind: CommandKind::Edit,
        run: notes::resize,
    },
    CommandSpec {
        id: "note.setVelocity",
        label: "Set the velocity of notes of a MIDI clip, or change it by an amount, each kept within 1 to 127",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("velocity", ParamType::Number, "the new velocity, 1 to 127"),
            optional("by", ParamType::Number, "instead: an amount to add to each velocity (negative lowers)"),
        ],
        kind: CommandKind::Edit,
        run: notes::set_velocity,
    },
    CommandSpec {
        id: "note.quantize",
        label: "Quantise notes of a MIDI clip: move their starts, and with ends their ends, towards the nearest grid line",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            optional("notes", ParamType::Integers, "note indices, from note.list; every note when left out"),
            param("grid", ParamType::Text, "the grid, as launcher.setQuantization takes it: 1/16, 1/8T, 1 bar, ..."),
            optional("amount", ParamType::Number, "how far towards the line, 0 to 100 percent (default 100: onto it)"),
            optional("ends", ParamType::Bool, "also move each note's end to the nearest line (default false)"),
        ],
        kind: CommandKind::Edit,
        run: notes::quantize,
    },
    CommandSpec {
        id: "note.copy",
        label: "Copy notes of a MIDI clip for note.paste; changes nothing",
        params: &[TRACK, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Read,
        run: notes::copy,
    },
    CommandSpec {
        id: "note.cut",
        label: "Copy notes of a MIDI clip for note.paste and delete them",
        params: &[TRACK, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Edit,
        run: notes::cut,
    },
    CommandSpec {
        id: "note.paste",
        label: "Add the copied notes to a MIDI clip, keeping their spacing; the result gives their indices",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            optional(
                "time",
                ParamType::Number,
                "where the first copied note starts, in beats; default: right after the copied notes",
            ),
        ],
        kind: CommandKind::Edit,
        run: notes::paste,
    },
    CommandSpec {
        id: "note.duplicate",
        label: "Add a copy of notes of a MIDI clip right after their end; the result gives the copies' indices",
        params: &[TRACK, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Edit,
        run: notes::duplicate,
    },
    CommandSpec {
        id: "note.split",
        label: "Split notes of a MIDI clip at a beat, each note that spans it into two; the result gives the new notes' indices",
        params: &[
            TRACK,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            param("at", ParamType::Number, "the beat to split at, in beats from the content's start"),
        ],
        kind: CommandKind::Edit,
        run: notes::split,
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
            Some(source) if suhina_project_file::is_project_file(source) => source.to_path_buf(),
            _ => return Err("the project has no .suhina file yet; give a path to save it to".into()),
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
                    "sends": channel.map(|c| crate::sends::sends_json(p, c)),
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
        "fade_in_shape": clip.fade_in_curve.shape.name(),
        "fade_in_curve": clip.fade_in_curve.curve,
        "fade_out_shape": clip.fade_out_curve.shape.name(),
        "fade_out_curve": clip.fade_out_curve.curve,
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
    let off = automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::VOLUME);
    Ok(with_override(json!({ "volume_db": to_db(gain) }), off))
}

fn set_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    let channel = channel_param(session, params)?;
    channel.pan = pan;
    let off = automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::PAN);
    Ok(with_override(json!({ "pan": pan }), off))
}

fn set_mute(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let mute = params.get("mute").and_then(Value::as_bool).ok_or("mute must be true or false")?;
    let channel = channel_param(session, params)?;
    channel.mute = mute;
    let off = automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::MUTE);
    Ok(with_override(json!({ "mute": mute }), off))
}

/// A set command's answer, saying `"automation": "off"` when the change turned the parameter's
/// lane off (#395).
pub(crate) fn with_override(mut answer: Value, off: bool) -> Value {
    if off && let Some(map) = answer.as_object_mut() {
        map.insert("automation".into(), json!("off"));
    }
    answer
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
        cue: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Default::default(),
        automation: Vec::new(),
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
pub(crate) fn remove_track(p: &mut Project, track: TrackId, channel: ChannelId) {
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
            (Some(r), Some(g), Some(b)) => Some(suhina_model::Rgb(r, g, b)),
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
    let shape_of = |name: &str| -> Result<Option<FadeShape>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(Value::String(s)) => FadeShape::from_name(s).map(Some).ok_or_else(|| {
                let names: Vec<&str> = FadeShape::ALL.iter().map(|s| s.name()).collect();
                format!("{name} must be one of {}, got {s:?}", names.join(", ")).into()
            }),
            Some(_) => Err(format!("{name} must be a shape name").into()),
        }
    };
    let curve_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let curve = number(params, name)?;
                if !(-1.0..=1.0).contains(&curve) {
                    return Err(format!("{name} must be from -1 to +1, got {curve}").into());
                }
                Ok(Some(curve))
            }
        }
    };
    let (fade_in, fade_out) = (length_of("in")?, length_of("out")?);
    let (in_shape, out_shape) = (shape_of("inShape")?, shape_of("outShape")?);
    let (in_curve, out_curve) = (curve_of("inCurve")?, curve_of("outCurve")?);
    if fade_in.is_none()
        && fade_out.is_none()
        && in_shape.is_none()
        && out_shape.is_none()
        && in_curve.is_none()
        && out_curve.is_none()
    {
        return Err("give in, out (beats), inShape, outShape, inCurve or outCurve".into());
    }
    let (lane, index) = lane_and_index(&session.project, params)?;
    let p = &mut session.project;
    let clip = p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?;
    let audio = p.content(clip.content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
    if !audio {
        return Err("only an audio clip has fades".into());
    }
    let changed = Clip {
        fade_in: fade_in.unwrap_or(clip.fade_in),
        fade_out: fade_out.unwrap_or(clip.fade_out),
        fade_in_curve: FadeCurve {
            shape: in_shape.unwrap_or(clip.fade_in_curve.shape),
            curve: in_curve.unwrap_or(clip.fade_in_curve.curve),
        },
        fade_out_curve: FadeCurve {
            shape: out_shape.unwrap_or(clip.fade_out_curve.shape),
            curve: out_curve.unwrap_or(clip.fade_out_curve.curve),
        },
        ..clip.clone()
    };
    if !changed.fades_valid() {
        if !(changed.fade_in_curve.is_valid() && changed.fade_out_curve.is_valid()) {
            return Err("the clip's fade curves are outside -1 to +1; set inCurve and outCurve".into());
        }
        return Err(format!(
            "the fades ({} + {} beats) are longer than the clip ({} beats)",
            changed.fade_in, changed.fade_out, changed.length
        )
        .into());
    }
    let result = json!({
        "fade_in": changed.fade_in,
        "fade_out": changed.fade_out,
        "fade_in_shape": changed.fade_in_curve.shape.name(),
        "fade_in_curve": changed.fade_in_curve.curve,
        "fade_out_shape": changed.fade_out_curve.shape.name(),
        "fade_out_curve": changed.fade_out_curve.curve,
    });
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
    let id = suhina_model::ContentId(p.contents.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    p.contents.push(suhina_model::ClipContent { id, ..content });
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
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let (lane, index) = lane_and_index(&session.project, params)?;
    let lane = session.project.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    let mut copy = lane.clips.get(index).cloned().ok_or("the clip is missing")?;
    copy.start += copy.length;
    if !stack {
        make_room(&mut lane.clips, copy.start, copy.start + copy.length);
    }
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

fn set_cue(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let cue = params.get("cue").and_then(Value::as_bool).ok_or("cue must be true or false")?;
    channel_param(session, params)?.cue = cue;
    Ok(json!({ "cue": cue }))
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
        .map(|(scene, number): (_, u32)| {
            json!({
                "scene": number,
                "name": scene.name,
                "tempo_bpm": scene.tempo_bpm,
                "time_signature": scene.time_signature.map(signature_text),
            })
        })
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
                        "follow_action": follow::describe(&clip.follow_action),
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
    let meter = monitor.meter();
    let statuses: Vec<Option<ColumnStatus>> = (0..monitor.columns()).map(|i| monitor.column(i)).collect();
    let p = session.project();
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
    let (bar, into_bar) = meter.locate(beat);
    let position = format!("{bar}.{}", into_bar.floor() as u64 + 1);
    Ok(json!({
        "running": running,
        "metronome": monitor_metronome,
        "tempo_bpm": tempo,
        "beat": beat,
        "time_signature": signature_text(meter.time_signature),
        "position": position,
        "start_beat": if start_beat.is_finite() { (start_beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 },
        "tracks": tracks,
        "preview": preview_state,
        "cpu": cpu,
    }))
}

fn send(session: &mut Session, command: LaunchCommand) -> Result<(), CommandError> {
    session.player_mut()?.send(command)
}

fn play(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let start = match params.get("from") {
        None => None,
        Some(Value::String(s)) if s == "marker" => None,
        Some(Value::String(s)) if s == "clip" => Some(clip_start(&session.project, params)?),
        Some(other) => return Err(format!("from must be \"marker\" or \"clip\", got {other}").into()),
    };
    if start.is_none() && (params.get("track").is_some() || params.get("index").is_some()) {
        return Err("track and index go with from: \"clip\"".into());
    }
    // The marker moves first, so playback starts there; while playing, that is a jump.
    if let Some(beat) = start {
        send(session, LaunchCommand::SetPosition { beat })?;
    }
    send(session, LaunchCommand::Play)?;
    Ok(match start {
        Some(beat) => json!({ "transport": "playing", "start_beat": beat }),
        None => json!({ "transport": "playing" }),
    })
}

/// Where the arrangement clip `track` and `index` name starts, in beats, as a play start marker.
fn clip_start(p: &Project, params: &Value) -> Result<f64, CommandError> {
    let (lane, index) = lane_and_index(p, params)?;
    let start = p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?.start;
    if !(0.0..=MAX_START_BEAT).contains(&start) {
        return Err(format!("the clip starts at beat {start}, outside 0 … {MAX_START_BEAT}").into());
    }
    Ok(start)
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

/// `clip.setLoop`: a slot clip's loop on (over its whole length, or the loop it already has) or
/// off (`docs/spec/clip-launching.md` › Playing a clip).
fn set_slot_loop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = params.get("loop").and_then(Value::as_bool).ok_or("loop must be true or false")?;
    let column = column_index(&session.project, params)?;
    let scene = scene_index(&session.project, params)?;
    let clip = session
        .project
        .launcher
        .columns
        .get_mut(column)
        .and_then(|c| c.slots.get_mut(scene))
        .and_then(|s| s.clip.as_mut())
        .ok_or("that slot has no clip")?;
    clip.loop_range = match (on, clip.loop_range) {
        (false, _) => None,
        (true, Some(range)) => Some(range),
        (true, None) => Some(LoopRange { start: 0.0, end: clip.length }),
    };
    Ok(json!({ "scene": scene + 1, "loop": on }))
}

fn launch_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::LaunchScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "launch" }))
}

/// Scenes a project can have; inserting stops there.
const MAX_SCENES: usize = 1000;

/// Gives every launcher column one slot per scene, so a row can be inserted or removed in all of
/// them at the same index.
fn even_slots(p: &mut Project) {
    let scenes = p.launcher.scenes.len();
    for column in &mut p.launcher.columns {
        column.slots.resize_with(scenes, || Slot { clip: None, has_stop: true });
    }
}

/// An optional scene name: trimmed text, empty when left out.
fn scene_name(params: &Value) -> Result<String, CommandError> {
    match params.get("name") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(name)) => Ok(name.trim().to_string()),
        Some(_) => Err("name must be a text".into()),
    }
}

fn insert_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let count = p.launcher.scenes.len();
    if count >= MAX_SCENES {
        return Err(format!("the project has {count} scenes, as many as it can have").into());
    }
    let at = match params.get("scene") {
        None | Some(Value::Null) => count,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|i| *i <= count)
            .ok_or_else(|| format!("scene must be a whole number from 1 to {}", count + 1))?,
    };
    let name = scene_name(params)?;
    even_slots(p);
    p.launcher.scenes.insert(at, Scene { name, ..Scene::default() });
    for column in &mut p.launcher.columns {
        column.slots.insert(at, Slot { clip: None, has_stop: true });
    }
    Ok(json!({ "scene": at + 1 }))
}

fn duplicate_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() >= MAX_SCENES {
        return Err(format!("the project has {} scenes, as many as it can have", p.launcher.scenes.len()).into());
    }
    even_slots(p);
    let copy = p.launcher.scenes.get(scene).cloned().unwrap_or_default();
    p.launcher.scenes.insert(scene + 1, copy);
    let mut next_content = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0);
    for column in &mut p.launcher.columns {
        let mut slot = column.slots.get(scene).cloned().unwrap_or(Slot { clip: None, has_stop: true });
        // Each copied clip plays its own content, so editing one does not change the other.
        if let Some(clip) = slot.clip.as_mut()
            && let Some(content) = p.contents.iter().find(|c| c.id == clip.content).cloned()
        {
            next_content = next_content.checked_add(1).ok_or("the project has too many clip contents")?;
            let id = suhina_model::ContentId(next_content);
            p.contents.push(suhina_model::ClipContent { id, ..content });
            clip.content = id;
        }
        column.slots.insert(scene + 1, slot);
    }
    Ok(json!({ "scene": scene + 2 }))
}

fn delete_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() <= 1 {
        return Err("the last scene cannot be deleted".into());
    }
    even_slots(p);
    p.launcher.scenes.remove(scene);
    for column in &mut p.launcher.columns {
        column.slots.remove(scene);
    }
    Ok(json!({ "deleted": scene + 1 }))
}

fn rename_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.name.clone_from(&name);
    }
    Ok(json!({ "scene": scene + 1, "name": name }))
}

fn set_scene_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let bpm = match params.get("bpm") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let bpm = number(params, "bpm")?;
            if !(20.0..=999.0).contains(&bpm) {
                return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
            }
            Some(bpm)
        }
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.tempo_bpm = bpm;
    }
    Ok(json!({ "scene": scene + 1, "tempo_bpm": bpm }))
}

fn set_scene_time_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let part = |key: &str| match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| CommandError::from(format!("{key} must be a whole number"))),
    };
    let signature = match (part("numerator")?, part("denominator")?) {
        (None, None) => None,
        (Some(numerator), Some(denominator)) => {
            if !(1..=99).contains(&numerator) {
                return Err(format!("numerator must be between 1 and 99, got {numerator}").into());
            }
            if ![1, 2, 4, 8, 16].contains(&denominator) {
                return Err(format!("denominator must be 1, 2, 4, 8 or 16, got {denominator}").into());
            }
            Some(TimeSignature { numerator, denominator })
        }
        _ => return Err("give both numerator and denominator, or neither to clear the time signature".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.time_signature = signature;
    }
    Ok(json!({ "scene": scene + 1, "time_signature": signature.map(signature_text) }))
}

fn signature_text(ts: TimeSignature) -> String {
    format!("{}/{}", ts.numerator, ts.denominator)
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
pub(crate) fn column_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let track = track_id(p, params)?;
    p.launcher
        .columns
        .iter()
        .position(|c| c.track == track)
        .ok_or_else(|| format!("track {} has no clip slots", track_name(p, track)).into())
}

/// The 0-based scene index of the 1-based `params.scene`.
pub(crate) fn scene_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
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
pub(crate) fn to_db(gain: f64) -> Option<f64> {
    (gain > 0.0).then(|| (20.0 * gain.log10() * 100.0).round() / 100.0)
}
