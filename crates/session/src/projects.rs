//! Several open projects, one of them active, like tabs. Commands go to the active project
//! unless a caller names another; only the active project plays (ADR 0007).

use serde_json::{Value, json};

use loopwerk_plugins::library::Library;

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, commands, param};
use crate::plugins::plugin_commands;
use crate::{CommandError, Session};

/// Projects that may be open at once; opening more is refused. Each holds its own undo history.
pub const MAX_OPEN_PROJECTS: usize = 32;

/// Identifies an open project for as long as it stays open. Ids are never reused within one
/// [`Projects`], so a stale id cannot reach a project opened later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub u32);

struct Open {
    id: ProjectId,
    name: String,
    session: Session,
}

/// The open projects in tab order, and which one is active; and the plug-in library, which
/// belongs to Loopwerk rather than to any one project.
#[derive(Default)]
pub struct Projects {
    open: Vec<Open>,
    active: Option<ProjectId>,
    last_id: u32,
    library: Library,
    /// What sessions start plug-ins with, made from the library when first needed and again
    /// after a scan or a change of how plug-ins are hosted.
    plugin_env: Option<std::sync::Arc<crate::PluginEnv>>,
    /// The window plug-in editors open over, for the active project's session.
    editor_owner: Option<u64>,
    /// The browser's library (ADR 0012); kept nowhere until one is given with
    /// [`Projects::set_file_library`].
    files: loopwerk_library::Library,
}

const PROJECT: Param = param("project", ParamType::Integer, "project id, from projects.list");

static PROJECT_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "projects.list",
        label: "List the open projects in tab order with id, name, source and whether each is active",
        params: &[],
        kind: CommandKind::Projects,
        run: list,
    },
    CommandSpec {
        id: "projects.switch",
        label: "Make an open project the active one; the previous one stops playing",
        params: &[PROJECT],
        kind: CommandKind::Projects,
        run: switch,
    },
    CommandSpec {
        id: "projects.next",
        label: "Make the next open project active, wrapping around after the last",
        params: &[],
        kind: CommandKind::Projects,
        run: next,
    },
    CommandSpec {
        id: "projects.previous",
        label: "Make the previous open project active, wrapping around before the first",
        params: &[],
        kind: CommandKind::Projects,
        run: previous,
    },
    CommandSpec {
        id: "projects.close",
        label: "Close an open project and its undo history; a neighbour becomes active if it was",
        params: &[PROJECT],
        kind: CommandKind::Projects,
        run: close,
    },
];

/// The commands that run on the open projects rather than on one project.
pub fn project_commands() -> &'static [CommandSpec<Projects>] {
    PROJECT_COMMANDS
}

impl Projects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Adds `session` after the last tab and makes it active. The previously active project
    /// stops playing. `name` defaults to the source file's name, or "Untitled N".
    pub fn add(&mut self, session: Session, name: Option<String>) -> Result<ProjectId, CommandError> {
        if self.open.len() >= MAX_OPEN_PROJECTS {
            return Err(format!("at most {MAX_OPEN_PROJECTS} projects can be open; close one first").into());
        }
        self.last_id = self.last_id.checked_add(1).ok_or("too many projects opened in this session")?;
        let id = ProjectId(self.last_id);
        let name = name
            .or_else(|| session.source().and_then(|s| s.file_stem()).map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| format!("Untitled {}", id.0));
        self.open.push(Open { id, name, session });
        self.activate(id);
        Ok(id)
    }

    pub fn active(&self) -> Option<ProjectId> {
        self.active
    }

    /// The ids of the open projects in tab order.
    pub fn ids(&self) -> Vec<ProjectId> {
        self.open.iter().map(|o| o.id).collect()
    }

    /// The name `id` is shown with, as in its tab.
    pub fn name(&self, id: ProjectId) -> Option<&str> {
        self.open.iter().find(|o| o.id == id).map(|o| o.name.as_str())
    }

    pub fn get(&self, id: ProjectId) -> Option<&Session> {
        self.open.iter().find(|o| o.id == id).map(|o| &o.session)
    }

    pub fn get_mut(&mut self, id: ProjectId) -> Option<&mut Session> {
        self.open.iter_mut().find(|o| o.id == id).map(|o| &mut o.session)
    }

    /// The session `id` names, or the active one when `id` is `None`.
    pub fn session_mut(&mut self, id: Option<ProjectId>) -> Result<&mut Session, CommandError> {
        let id = match id.or(self.active) {
            Some(id) => id,
            None => return Err(no_project()),
        };
        self.get_mut(id).ok_or_else(|| unknown_project(id))
    }

    /// Makes `id` active. The previously active project's player is closed, so only one project
    /// plays at a time.
    pub fn switch(&mut self, id: ProjectId) -> Result<(), CommandError> {
        if self.get(id).is_none() {
            return Err(unknown_project(id));
        }
        self.activate(id);
        Ok(())
    }

    /// Moves `step` tabs from the active project, wrapping around. Returns the new active id.
    pub fn cycle(&mut self, step: isize) -> Result<ProjectId, CommandError> {
        let len = self.open.len();
        let current = self.active.and_then(|a| self.open.iter().position(|o| o.id == a)).ok_or_else(no_project)?;
        let distance = step.unsigned_abs() % len;
        let target = if step >= 0 { (current + distance) % len } else { (current + len - distance) % len };
        let id = self.open.get(target).map(|o| o.id).ok_or_else(no_project)?;
        self.activate(id);
        Ok(id)
    }

    /// Closes `id` and returns its session. If it was active, the tab to its right becomes
    /// active (or the one to its left when it was the last).
    pub fn close(&mut self, id: ProjectId) -> Result<Session, CommandError> {
        let index = self.open.iter().position(|o| o.id == id).ok_or_else(|| unknown_project(id))?;
        let closed = self.open.remove(index);
        if self.active == Some(id) {
            self.active = None;
            let neighbour = self.open.get(index).or_else(|| self.open.last()).map(|o| o.id);
            if let Some(next) = neighbour {
                self.activate(next);
            }
        }
        Ok(closed.session)
    }

    /// Uses `library` for the `plugins.*` commands instead of the one in Loopwerk's settings
    /// folder (for tests, or a portable setup).
    pub fn with_library(library: Library) -> Self {
        Self { library, ..Self::default() }
    }

    /// The plug-in library. Taking it for changing counts as changing it: sessions get a new
    /// catalog the next time they start a plug-in.
    pub fn library_mut(&mut self) -> &mut Library {
        self.plugin_env = None;
        &mut self.library
    }

    /// What sessions start plug-ins with (ADR 0006): the catalog, the host executable and how
    /// plug-ins share host processes.
    pub fn plugin_env(&mut self) -> std::sync::Arc<crate::PluginEnv> {
        if let Some(env) = &self.plugin_env {
            return std::sync::Arc::clone(env);
        }
        let env = std::sync::Arc::new(crate::PluginEnv::from_library(&mut self.library));
        self.plugin_env = Some(std::sync::Arc::clone(&env));
        env
    }

    /// Uses `library` for the browser and the `library.*` commands: the desktop app and the
    /// MCP server give it Loopwerk's settings folder.
    pub fn set_file_library(&mut self, library: loopwerk_library::Library) {
        self.files = library;
    }

    /// The browser's library.
    pub fn file_library_mut(&mut self) -> &mut loopwerk_library::Library {
        &mut self.files
    }

    /// The window plug-in editors open over (a Win32 `HWND`, an X11 window id): the desktop
    /// app's main window, so editors float above it. `None` gives them windows of their own.
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.editor_owner = owner;
    }

    /// Keeps the active project's plug-ins in line with it (ADR 0006): starts new ones in the
    /// background, takes in the ones that have started, notices crashes. Call it often from the
    /// thread that owns the projects (the window calls it every frame); commands call it too.
    /// Returns what the user should hear about.
    pub fn poll_plugins(&mut self) -> Vec<String> {
        let Some(active) = self.active else { return Vec::new() };
        if self.get(active).is_none_or(|s| s.player().is_none()) {
            return Vec::new();
        }
        let env = self.plugin_env();
        let owner = self.editor_owner;
        let Some(session) = self.get_mut(active) else { return Vec::new() };
        session.set_plugin_env(env);
        session.set_editor_owner(owner);
        session.sync_plugins()
    }

    /// Runs command `id`: a `projects.*` or `plugins.*` command on the open projects (and the
    /// plug-in library), anything else on the active project. `commands.list` lists every kind.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        self.execute_in(None, id, params)
    }

    /// Like [`Projects::execute`], but a project command runs on `project` instead of the active
    /// one. Transport commands only work on the active project, the one that plays.
    pub fn execute_in(&mut self, project: Option<ProjectId>, id: &str, params: &Value) -> Result<Value, CommandError> {
        if id == "commands.list" {
            check_params(id, &[], params)?;
            return Ok(all_commands());
        }
        if let Some(spec) =
            PROJECT_COMMANDS.iter().chain(plugin_commands()).chain(crate::library_commands()).find(|c| c.id == id)
        {
            if project.is_some() {
                return Err(format!("{id} does not run on one project; it takes no target project").into());
            }
            check_params(id, spec.params, params)?;
            return (spec.run)(self, params);
        }
        // A command may need a plug-in that has started meanwhile, or add one.
        self.poll_plugins();
        let target = project.or(self.active);
        let result = self.session_mut(project)?.execute(id, params);
        self.poll_plugins();
        if let Ok(value) = &result {
            crate::library::record_use(&mut self.files, id, params, value);
        }
        // A project saved under a new name is shown by it.
        if id == "project.save"
            && let Ok(saved) = &result
            && saved.get("copy") == Some(&Value::Bool(false))
            && let Some(open) = self.open.iter_mut().find(|o| Some(o.id) == target)
            && let Some(stem) = open.session.source().and_then(|s| s.file_stem())
        {
            open.name = stem.to_string_lossy().into_owned();
        }
        result
    }

    fn activate(&mut self, id: ProjectId) {
        if self.active == Some(id) {
            return;
        }
        if let Some(previous) = self.active.and_then(|a| self.get_mut(a)) {
            previous.detach_player();
        }
        self.active = Some(id);
    }
}

/// Every command, project commands first, as `commands.list` shows them.
pub fn all_commands() -> Value {
    let projects =
        PROJECT_COMMANDS.iter().chain(plugin_commands()).chain(crate::library_commands()).map(CommandSpec::describe);
    Value::Array(projects.chain(commands().iter().map(CommandSpec::describe)).collect())
}

fn check_params(id: &str, expected: &[Param], params: &Value) -> Result<(), CommandError> {
    if !(params.is_object() || params.is_null()) {
        return Err(format!("{id}: parameters must be a JSON object").into());
    }
    if let Some(object) = params.as_object()
        && let Some(unknown) = object.keys().find(|key| !expected.iter().any(|p| p.name == key.as_str()))
    {
        let known: Vec<&str> = expected.iter().map(|p| p.name).collect();
        return Err(format!("{id}: unknown parameter {unknown:?}; expected {known:?}").into());
    }
    Ok(())
}

fn no_project() -> CommandError {
    CommandError("no project is open".into())
}

fn unknown_project(id: ProjectId) -> CommandError {
    CommandError(format!("no open project has id {}; projects.list shows them", id.0))
}

pub(crate) fn project_param(params: &Value) -> Result<ProjectId, CommandError> {
    params
        .get("project")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .map(ProjectId)
        .ok_or_else(|| "project must be a project id from projects.list".into())
}

fn list(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({
        "active": projects.active.map(|a| a.0),
        "projects": projects.open.iter().map(|o| json!({
            "project": o.id.0,
            "name": o.name,
            "source": o.session.source().map(|s| s.display().to_string()),
            "active": projects.active == Some(o.id),
            "playing": o.session.player().is_some(),
            "can_undo": o.session.can_undo(),
        })).collect::<Vec<_>>(),
    }))
}

fn switch(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = project_param(params)?;
    projects.switch(id)?;
    Ok(json!({ "active": id.0 }))
}

fn next(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "active": projects.cycle(1)?.0 }))
}

fn previous(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "active": projects.cycle(-1)?.0 }))
}

fn close(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = project_param(params)?;
    projects.close(id)?;
    Ok(json!({ "closed": id.0, "active": projects.active.map(|a| a.0) }))
}
