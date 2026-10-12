//! A Loopwerk session: an open project, where it came from, and its undo history. Every change
//! goes through a command (see `docs/decisions/0002-commands.md`), so the CLI, the MCP server and
//! later the UI all drive the project the same way.

mod blank;
mod commands;
mod devices;
mod files;
mod library;
mod plugins;
mod preview;
mod projects;
mod rack;

use std::fmt;
use std::path::{Path, PathBuf};

use loopwerk_engine::{Controller, LauncherMonitor};
use loopwerk_model::Project;
use serde_json::Value;

pub use blank::blank_project;
pub use commands::{CommandKind, CommandSpec, Param, ParamType, Run, commands, parse_quantization};
pub use files::{MAX_FILES, file_tempo};
pub use library::{Category, Found, file_info, library_commands, search_items};
pub use plugins::{device_plugin, plugin_commands};
pub use projects::{MAX_OPEN_PROJECTS, ProjectId, Projects, all_commands, project_commands};
pub use rack::{
    Hosted, OfflinePlugins, PluginEnv, PluginProgress, PluginReport, PluginStatus, is_instrument, start_offline,
};

/// Undo steps kept per session; the oldest are dropped first.
const MAX_UNDO: usize = 256;

/// Why a command did not run. The session is unchanged when this is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError(pub String);

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandError {}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

/// A running audio engine that plays the session's project: transport and launcher commands
/// go to it. Whatever keeps the audio output open (a stream, a thread) is held in `keep_alive`
/// and closed when the player is dropped.
pub struct Player {
    controller: Controller,
    _keep_alive: Option<Box<dyn std::any::Any + Send>>,
}

impl Player {
    pub fn new(controller: Controller, keep_alive: Option<Box<dyn std::any::Any + Send>>) -> Self {
        Self { controller, _keep_alive: keep_alive }
    }

    pub fn send(&mut self, command: loopwerk_engine::LaunchCommand) -> Result<(), CommandError> {
        self.controller.send(command).map_err(CommandError)
    }

    pub fn monitor(&self) -> &LauncherMonitor {
        self.controller.monitor()
    }

    /// Makes the engine play `project` from now on, keeping what is playing (ADR 0005).
    pub fn update(&mut self, project: &Project) -> Result<(), CommandError> {
        self.controller.update(project).map_err(CommandError)
    }

    /// Sets one parameter of a device on a track from the next audio block, without building a
    /// new graph (ADR 0005, amendment).
    pub fn set_device_param(&mut self, change: ParamEdit) -> Result<(), CommandError> {
        self.controller.set_device_param(change.track, change.device, change.param, change.value).map_err(CommandError)
    }

    /// The engine's sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.controller.sample_rate()
    }

    /// What graphs built from now on know about the project's plug-in devices (ADR 0006).
    pub fn set_plugin_info(&mut self, info: std::collections::HashMap<u64, loopwerk_engine::PluginSlotInfo>) {
        self.controller.set_plugin_info(info);
    }

    /// Builds the engine's graphs from now on with `audio` (ADR 0011).
    pub fn set_audio(&mut self, audio: std::sync::Arc<loopwerk_engine::AudioPool>) {
        self.controller.set_audio(audio);
    }

    /// Whether a preview plays, and where (ADR 0011).
    pub fn preview_monitor(&self) -> &loopwerk_engine::PreviewMonitor {
        self.controller.preview_monitor()
    }

    pub(crate) fn controller_mut(&mut self) -> &mut Controller {
        &mut self.controller
    }

    /// Hands a running plug-in's audio half to the engine for the plug-in device `key`.
    pub fn send_plugin(
        &mut self,
        key: u64,
        processor: Box<dyn loopwerk_engine::PluginProcessor>,
    ) -> Result<(), CommandError> {
        self.controller.send_plugin(key, processor).map_err(CommandError)
    }
}

/// An edit that is only a device parameter's new value, which a playing engine can take
/// without a new graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdit {
    pub track: loopwerk_model::TrackId,
    /// The device's index among the track channel's devices.
    pub device: usize,
    /// The parameter's index in the device's info.
    pub param: usize,
    pub value: f64,
}

pub struct Session {
    project: Project,
    source: Option<PathBuf>,
    /// The folder the project's relative audio paths start from: the folder of the file it was
    /// opened from (a template's, for a project made from one). It stays when the project is
    /// saved elsewhere, because the decoded audio is kept under those paths; saving writes them
    /// relative to the new file.
    audio_folder: Option<PathBuf>,
    /// Counts changes to the project (edits, undo, redo), and the count when it was last saved.
    changes: u64,
    saved_at: u64,
    undo: Vec<Project>,
    redo: Vec<Project>,
    player: Option<Player>,
    /// A copied arrangement clip and the content it plays, for `clip.paste`.
    clipboard: Option<(loopwerk_model::Clip, loopwerk_model::ClipContent)>,
    /// Set by an edit command whose only change is one device parameter.
    pub(crate) param_edit: Option<ParamEdit>,
    /// The project's plug-ins, running while it plays (ADR 0006).
    pub(crate) plugins: rack::Rack,
    /// The project's decoded audio, by the key its regions name (ADR 0011).
    audio: loopwerk_engine::AudioPool,
    /// Files added to `audio` since the player last got it.
    audio_changed: bool,
    /// Files decoded ahead of a command that uses them (`provide_audio`), newest last.
    decoded: Vec<(String, std::sync::Arc<loopwerk_engine::AudioData>)>,
}

/// Files kept decoded ahead of the commands that use them.
const MAX_DECODED: usize = 16;

impl Session {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            source: None,
            audio_folder: None,
            changes: 0,
            saved_at: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            player: None,
            clipboard: None,
            param_edit: None,
            plugins: rack::Rack::default(),
            audio: loopwerk_engine::AudioPool::new(),
            audio_changed: false,
            decoded: Vec::new(),
        }
    }

    /// The project's decoded audio, which players are started with (ADR 0011).
    pub fn audio(&self) -> &loopwerk_engine::AudioPool {
        &self.audio
    }

    /// Gives the session its project's decoded audio (`media::load_project_audio`), before a
    /// player is started with [`Session::audio`].
    pub fn set_audio(&mut self, audio: loopwerk_engine::AudioPool) {
        self.audio = audio;
        self.audio_changed = true;
    }

    /// Hands over a file decoded ahead (on another thread), so a command that uses `path` next
    /// (`clip.addFiles`, `preview.play`) need not decode it. Only the latest few are kept.
    pub fn provide_audio(&mut self, path: &Path, data: std::sync::Arc<loopwerk_engine::AudioData>) {
        let key = files::absolute(path).display().to_string();
        self.decoded.retain(|(k, _)| *k != key);
        if self.decoded.len() >= MAX_DECODED {
            self.decoded.remove(0);
        }
        self.decoded.push((key, data));
    }

    /// The decoded audio of the file at `path` (absolute): from the pool, from what was
    /// provided, or decoded now.
    pub(crate) fn decoded_audio(&mut self, path: &Path) -> Result<std::sync::Arc<loopwerk_engine::AudioData>, String> {
        let key = path.display().to_string();
        let data = if let Some(data) = self.audio.files.get(&key) {
            return Ok(std::sync::Arc::clone(data));
        } else if let Some((_, data)) = self.decoded.iter().find(|(k, _)| *k == key) {
            std::sync::Arc::clone(data)
        } else {
            std::sync::Arc::new(loopwerk_media::decode_file(path)?)
        };
        self.audio.files.insert(key, std::sync::Arc::clone(&data));
        self.audio_changed = true;
        Ok(data)
    }

    /// Decoded audio for a preview, which does not join the project's pool.
    pub(crate) fn preview_audio(&mut self, path: &Path) -> Result<std::sync::Arc<loopwerk_engine::AudioData>, String> {
        let key = path.display().to_string();
        if let Some(data) = self.audio.files.get(&key) {
            return Ok(std::sync::Arc::clone(data));
        }
        if let Some((_, data)) = self.decoded.iter().find(|(k, _)| *k == key) {
            return Ok(std::sync::Arc::clone(data));
        }
        let data = std::sync::Arc::new(loopwerk_media::decode_file(path)?);
        self.provide_audio(path, std::sync::Arc::clone(&data));
        Ok(data)
    }

    /// Connects an audio engine playing this session's project; transport and launcher commands
    /// go to it from now on. Replaces (and so closes) any previous player. It should have been
    /// started with [`Session::audio`]; audio added later reaches it with the next edit.
    pub fn attach_player(&mut self, player: Player) {
        self.player = Some(player);
        self.audio_changed = false;
    }

    /// Disconnects the audio engine; the project's plug-ins stop (each keeps its last state for
    /// when the project plays again).
    pub fn detach_player(&mut self) -> Option<Player> {
        self.plugins.stop_all();
        self.player.take()
    }

    /// Lets the session start the project's plug-ins: the catalog to find them in and how to
    /// host them (ADR 0006). Without it plug-in devices pass the sound through.
    pub fn set_plugin_env(&mut self, env: std::sync::Arc<PluginEnv>) {
        self.plugins.set_env(env);
    }

    /// Brings the project's running plug-ins in line with it (starts new ones in the
    /// background, stops removed ones, takes in started ones, notices crashes). Call it often
    /// from the session's thread, as the window does every frame. Returns what the user should
    /// hear about.
    pub fn sync_plugins(&mut self) -> Vec<String> {
        self.plugins.sync(&self.project, self.player.as_mut())
    }

    /// The window plug-in editors open over, owned by it (a Win32 `HWND`, an X11 window id):
    /// they float above it, have no taskbar entry of their own and go with it.
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.plugins.set_owner(owner);
    }

    /// How far the project's plug-ins have got with starting (all zero while it does not play).
    pub fn plugin_progress(&self) -> PluginProgress {
        self.plugins.progress()
    }

    /// Waits up to `timeout` until none of the project's plug-ins is still starting.
    pub fn wait_for_plugins(&mut self, timeout: std::time::Duration) -> Vec<String> {
        match self.player.as_mut() {
            Some(player) => self.plugins.wait_started(&self.project, player, timeout),
            None => Vec::new(),
        }
    }

    /// What is known about the plug-in device at `device` among `track`'s channel's devices;
    /// `None` for devices that are not plug-ins or while the project does not play.
    pub fn plugin_report(&mut self, track: loopwerk_model::TrackId, device: usize) -> Option<PluginReport> {
        let key = plugin_key_at(&self.project, track, device)?;
        self.plugins.report(key)
    }

    pub fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    pub(crate) fn player_mut(&mut self) -> Result<&mut Player, CommandError> {
        self.player.as_mut().ok_or_else(|| {
            CommandError(
                "no audio engine is playing this project: start one with `loopwerk-cli play --launcher`, or the MCP                  server with --audio"
                    .into(),
            )
        })
    }

    /// Opens a project: a Loopwerk project (`.locr`) or a Live Set to import (`.als`). Returns
    /// the session and the importer's warnings.
    pub fn open(path: &Path) -> Result<(Self, Vec<String>), CommandError> {
        let is_locr = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(loopwerk_locr::EXTENSION));
        let (project, warnings) = if is_locr {
            let project = loopwerk_locr::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            (project, Vec::new())
        } else {
            let import = loopwerk_als::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            (import.project, import.warnings)
        };
        let mut session = Self::new(project);
        session.source = Some(path.to_path_buf());
        session.audio_folder = path.parent().map(Path::to_path_buf);
        Ok((session, warnings))
    }

    /// A new, untitled project made from the template file `path` (any project Loopwerk opens).
    /// Its audio is still found where the template's is.
    pub fn from_template(path: &Path) -> Result<(Self, Vec<String>), CommandError> {
        let (mut session, warnings) = Self::open(path)?;
        session.source = None;
        Ok((session, warnings))
    }

    /// The folder the project's relative audio paths start from, as the audio is loaded.
    pub fn audio_folder(&self) -> Option<&Path> {
        self.audio_folder.as_deref()
    }

    /// Whether the project changed since it was opened or last saved.
    pub fn has_unsaved_changes(&self) -> bool {
        self.changes != self.saved_at
    }

    /// Saves the project as a Loopwerk project at `path` (`.locr` is added when it is missing),
    /// with the running plug-ins' state as it is now and audio paths written relative to the new
    /// file. The session then belongs to that file, unless `copy` (a template, a backup), which
    /// leaves the session as it was. Returns the path written.
    pub fn save(&mut self, path: &Path, copy: bool) -> Result<PathBuf, CommandError> {
        let mut path = path.to_path_buf();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case(loopwerk_locr::EXTENSION)) {
            let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
            if name.is_empty() {
                return Err("give the project a file name".into());
            }
            name.push(".");
            name.push(loopwerk_locr::EXTENSION);
            path.set_file_name(name);
        }
        let mut project = self.project.clone();
        let states = self.plugins.current_states();
        for device in loopwerk_engine::plugin_devices(&project) {
            let Some(state) = states.get(&device.key) else { continue };
            let slot = project
                .channels
                .iter_mut()
                .find(|c| c.id == device.channel)
                .and_then(|c| c.devices.get_mut(device.position));
            if let Some(slot) = slot {
                slot.state = Some(state.data.clone());
                slot.extensions.0.retain(|e| e.key != "Vst3ControllerState");
                if let Some(controller) = &state.controller {
                    slot.extensions.0.push(loopwerk_model::Extension {
                        format: "vst3".into(),
                        key: "Vst3ControllerState".into(),
                        data: controller.clone(),
                    });
                }
            }
        }
        let from = match &self.audio_folder {
            Some(folder) => folder.clone(),
            None => std::env::current_dir().unwrap_or_default(),
        };
        let to = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        loopwerk_locr::rebase_audio(&mut project, &from, &to);
        loopwerk_locr::write_file(&path, &project).map_err(|e| CommandError(e.0))?;
        if !copy {
            self.source = Some(path.clone());
            self.saved_at = self.changes;
        }
        Ok(path)
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Runs the command `id` with JSON `params` and returns its JSON result. An editing command
    /// records one undo step when it changes the project; if it fails, or leaves a project that
    /// does not validate, the project is restored and an error is returned.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        let spec = commands().iter().find(|c| c.id == id).ok_or_else(|| unknown_command(id))?;
        if !(params.is_object() || params.is_null()) {
            return Err(format!("{id}: parameters must be a JSON object").into());
        }
        if let Some(object) = params.as_object()
            && let Some(unknown) = object.keys().find(|key| !spec.params.iter().any(|p| p.name == key.as_str()))
        {
            let known: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
            return Err(format!("{id}: unknown parameter {unknown:?}; expected {known:?}").into());
        }
        match spec.kind {
            CommandKind::Read
            | CommandKind::Transport
            | CommandKind::Projects
            | CommandKind::Plugins
            | CommandKind::Library
            | CommandKind::Plugin => (spec.run)(self, params),
            CommandKind::History => {
                let value = (spec.run)(self, params)?;
                self.update_player()?;
                Ok(value)
            }
            CommandKind::Edit => {
                self.param_edit = None;
                let before = self.project.clone();
                let result = (spec.run)(self, params).and_then(|value| match self.project.validate() {
                    Ok(()) => Ok(value),
                    Err(errors) => Err(CommandError(format!("{id} would leave an invalid project: {errors:?}"))),
                });
                match result {
                    Ok(value) => {
                        if self.project != before {
                            self.push_undo(before);
                            // A parameter change goes to the engine as a message; anything else,
                            // or a message that cannot be sent, as a new graph.
                            let sent = match (self.param_edit.take(), self.player.as_mut()) {
                                (Some(change), Some(player)) => player.set_device_param(change).is_ok(),
                                _ => false,
                            };
                            if !sent {
                                self.update_player()?;
                            }
                        }
                        Ok(value)
                    }
                    Err(e) => {
                        self.project = before;
                        Err(e)
                    }
                }
            }
        }
    }

    /// Lets a playing engine hear the project as it is now, with any audio added to the pool.
    /// The edit stays made if this fails.
    fn update_player(&mut self) -> Result<(), CommandError> {
        let Some(player) = self.player.as_mut() else { return Ok(()) };
        if self.audio_changed {
            player.set_audio(std::sync::Arc::new(self.audio.clone()));
            self.audio_changed = false;
        }
        player.update(&self.project).map_err(|e| {
            CommandError(format!("the edit was made, but the playing engine could not follow it: {}", e.0))
        })
    }

    fn push_undo(&mut self, before: Project) {
        self.changes = self.changes.wrapping_add(1);
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.project, previous));
        self.changes = self.changes.wrapping_add(1);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.project, next));
        self.changes = self.changes.wrapping_add(1);
        true
    }
}

/// The key of the plug-in device at `device` among `track`'s channel's devices.
pub(crate) fn plugin_key_at(project: &Project, track: loopwerk_model::TrackId, device: usize) -> Option<u64> {
    let channel = project.tracks.iter().find(|t| t.id == track)?.channel?;
    loopwerk_engine::plugin_devices(project)
        .into_iter()
        .find(|d| d.channel == channel && d.position == device)
        .map(|d| d.key)
}

fn unknown_command(id: &str) -> CommandError {
    let area = id.split('.').next().unwrap_or_default();
    let similar: Vec<&str> = commands().iter().map(|c| c.id).filter(|c| c.starts_with(area)).collect();
    if similar.is_empty() {
        CommandError(format!("unknown command {id}; commands.list shows them all"))
    } else {
        CommandError(format!("unknown command {id}; did you mean one of: {}", similar.join(", ")))
    }
}
