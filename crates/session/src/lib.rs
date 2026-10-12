//! A Loopwerk session: an open project, where it came from, and its undo history. Every change
//! goes through a command (see `docs/decisions/0002-commands.md`), so the CLI, the MCP server and
//! later the UI all drive the project the same way.

mod commands;
mod devices;
mod plugins;
mod projects;

use std::fmt;
use std::path::{Path, PathBuf};

use loopwerk_engine::{Controller, LauncherMonitor};
use loopwerk_model::Project;
use serde_json::Value;

pub use commands::{CommandKind, CommandSpec, Param, ParamType, Run, commands, parse_quantization};
pub use plugins::{device_plugin, plugin_commands};
pub use projects::{MAX_OPEN_PROJECTS, ProjectId, Projects, all_commands, commands_markdown, project_commands};

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
    undo: Vec<Project>,
    redo: Vec<Project>,
    player: Option<Player>,
    /// A copied arrangement clip and the content it plays, for `clip.paste`.
    clipboard: Option<(loopwerk_model::Clip, loopwerk_model::ClipContent)>,
    /// Set by an edit command whose only change is one device parameter.
    pub(crate) param_edit: Option<ParamEdit>,
}

impl Session {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            source: None,
            undo: Vec::new(),
            redo: Vec::new(),
            player: None,
            clipboard: None,
            param_edit: None,
        }
    }

    /// Connects an audio engine playing this session's project; transport and launcher commands
    /// go to it from now on. Replaces (and so closes) any previous player.
    pub fn attach_player(&mut self, player: Player) {
        self.player = Some(player);
    }

    pub fn detach_player(&mut self) -> Option<Player> {
        self.player.take()
    }

    pub fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    fn player_mut(&mut self) -> Result<&mut Player, CommandError> {
        self.player.as_mut().ok_or_else(|| {
            CommandError(
                "no audio engine is playing this project: start one with `loopwerk-cli play --launcher`, or the MCP                  server with --audio"
                    .into(),
            )
        })
    }

    /// Opens a Live Set. Returns the session and the importer's warnings.
    pub fn open(path: &Path) -> Result<(Self, Vec<String>), CommandError> {
        let import = loopwerk_als::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut session = Self::new(import.project);
        session.source = Some(path.to_path_buf());
        Ok((session, import.warnings))
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
            CommandKind::Read | CommandKind::Transport | CommandKind::Projects | CommandKind::Plugins => {
                (spec.run)(self, params)
            }
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

    /// Lets a playing engine hear the project as it is now. The edit stays made if this fails.
    fn update_player(&mut self) -> Result<(), CommandError> {
        let Some(player) = self.player.as_mut() else { return Ok(()) };
        player.update(&self.project).map_err(|e| {
            CommandError(format!("the edit was made, but the playing engine could not follow it: {}", e.0))
        })
    }

    fn push_undo(&mut self, before: Project) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.project, previous));
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.project, next));
        true
    }
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
