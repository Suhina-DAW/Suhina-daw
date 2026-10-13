//! The MCP server. Following PhotoCraft's conventions, it exposes a few generic tools rather than
//! one tool per command: agents discover commands with `command_list` and run them with
//! `command_run` or `command_batch`, so the tool list stays small as commands grow.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use suhina_engine::model::{LaneData, Project};
use suhina_session::{AudioSystem, CommandKind, Player, ProjectId, Projects, Session, all_commands, project_commands};

use crate::render::{LaunchStep, render_launch_script, render_wav, scene_as_arrangement};
use crate::workspace::{Workspace, display_path};

const INSTRUCTIONS: &str = "Suhina, an open-source clip-launching DAW. Open a Live Set with project_open \
(paths are relative to the read root granted at launch), look at it with project_inspect, discover command ids \
and parameters with command_list, and change it with command_run or command_batch (each editing command is one \
undo step; edit.undo reverts it). Several projects can be open, like tabs: each project_open adds one and makes it \
active; projects.list, projects.switch, projects.next and projects.close manage them, and the `project` argument \
of a tool targets another open project without switching. project_render writes a WAV file below the write root; with `launches` it \
renders clip launches offline. Transport commands (clip.launch, scene.launch, track.stop, transport.play, \
transport.stop, transport.setPosition, launcher.state) play on a real audio device only when the server was started with --audio.";

/// Most steps a single `command_batch` may run.
const MAX_BATCH_STEPS: usize = 256;

#[derive(Clone)]
pub struct SuhinaMcp {
    projects: Arc<Mutex<Projects>>,
    workspace: Arc<Workspace>,
    /// Opened projects play out loud (`--audio`): the projects have an audio system.
    audio_output: bool,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    /// Path of a `.als` Live Set, relative to the read root.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    /// Open project id from projects.list; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only commands whose id contains this text, e.g. `track`.
    #[serde(default)]
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// Command id from command_list, e.g. `track.setPan`.
    pub id: String,
    /// Command parameters as a JSON object, e.g. `{"track": "Bass", "pan": -1}`.
    #[serde(default)]
    pub params: Option<Value>,
    /// Open project id from projects.list to run on; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepParams {
    /// Command id from command_list, e.g. `track.setPan`.
    pub id: String,
    /// Command parameters as a JSON object.
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Commands to run in order, e.g. `[{"id": "transport.setTempo", "params": {"bpm": 128}}]`. Each editing
    /// command is its own undo step; the batch is not one transaction.
    pub steps: Vec<StepParams>,
    /// Stop at the first failing step (default true).
    #[serde(default)]
    pub stop_on_error: Option<bool>,
    /// Open project id from projects.list to run on; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderParams {
    /// Output `.wav` path, relative to the write root.
    pub output: String,
    /// Render this launcher scene (1-based) instead of the arrangement.
    #[serde(default)]
    pub scene: Option<usize>,
    /// Render clip launches instead: commands run at times in seconds, starting with the transport
    /// stopped, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}]`. Needs `seconds`.
    #[serde(default)]
    pub launches: Option<Vec<LaunchParams>>,
    /// Length of a `launches` render in seconds (at most 600).
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Sample rate in Hz (default 48000).
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Replace the file if it exists (default false).
    #[serde(default)]
    pub overwrite: Option<bool>,
    /// Open project id from projects.list to render; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchParams {
    /// When to run the command, in seconds from the start of the render.
    pub at: f64,
    /// A transport or read command id, e.g. `clip.launch`.
    pub id: String,
    #[serde(default)]
    pub params: Option<Value>,
}

fn ok_json(value: &Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn fail(message: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.to_string())])
}

impl SuhinaMcp {
    /// A server with no project open.
    pub fn new(workspace: Workspace) -> Self {
        Self {
            projects: Arc::new(Mutex::new(Projects::new())),
            workspace: Arc::new(workspace),
            audio_output: false,
            tool_router: Self::tool_router(),
        }
    }

    /// Gives agents the user's browser library (`library.*`, ADR 0012), kept up to date on a
    /// thread of its own. Agents may search it and read what it lists inside the read root.
    pub fn with_file_library(self, mut library: suhina_library::Library) -> Self {
        let _ = library.start();
        if let Ok(mut projects) = self.projects.lock() {
            projects.set_file_library(library);
        }
        self
    }

    /// Plays each opened project on `system`'s devices, so transport and launcher commands are
    /// heard and the `audio.*` commands choose the output.
    pub fn with_audio_output(mut self, system: Arc<dyn AudioSystem>) -> Self {
        if let Ok(mut projects) = self.projects.lock() {
            projects.set_audio_system(system);
            self.audio_output = true;
        }
        self
    }

    /// Serves MCP over stdin and stdout until the client disconnects.
    pub async fn serve_stdio(self) -> Result<(), String> {
        let running = self.serve((tokio::io::stdin(), tokio::io::stdout())).await.map_err(|e| e.to_string())?;
        running.waiting().await.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Runs `f` with the open projects on a blocking thread. A panic in a command would poison
    /// the lock, but commands restore the project on failure, so the projects stay usable.
    async fn with_projects<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Projects) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let projects = Arc::clone(&self.projects);
        tokio::task::spawn_blocking(move || {
            let mut guard = projects.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard)
        })
        .await
        .unwrap_or_else(|e| Err(format!("task failed: {e}")))
    }

    /// Runs `f` with the session `project` names, or the active one.
    async fn with_session<T: Send + 'static>(
        &self,
        project: Option<u32>,
        f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.with_projects(move |projects| f(projects.session_mut(project.map(ProjectId)).map_err(|e| e.0)?)).await
    }
}

#[tool_router]
impl SuhinaMcp {
    #[tool(
        description = "Open a Live Set (.als) below the read root and make it the session's project. Returns a summary and the importer's warnings (unsupported content is skipped, not fatal)."
    )]
    async fn project_open(&self, Parameters(p): Parameters<OpenParams>) -> Result<CallToolResult, McpError> {
        let path = match self.workspace.readable(&p.path) {
            Ok(path) => path,
            Err(e) => return Ok(fail(e)),
        };
        let output = self.audio_output.clone();
        let result = self
            .with_projects(move |projects| {
                let (opened, mut warnings) = Session::open(&path).map_err(|e| e.0)?;
                let id = projects.add(opened, None).map_err(|e| e.0)?;
                let session = projects.session_mut(Some(id)).map_err(|e| e.0)?;
                let mut info = session.execute("project.info", &json!({})).map_err(|e| e.0)?;
                info["source"] = json!(display_path(&path));
                let audio_output = if output { start_audio(projects, id, &mut warnings) } else { Value::Null };
                Ok(json!({ "project": id.0, "info": info, "warnings": warnings, "audio_output": audio_output }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "The open project as JSON: summary (tempo, counts, undo state), tracks with mixer settings and devices, and arrangement clips."
    )]
    async fn project_inspect(&self, Parameters(p): Parameters<InspectParams>) -> Result<CallToolResult, McpError> {
        let result = self
            .with_session(p.project, |session| {
                let mut read = |id: &str| session.execute(id, &json!({})).map_err(|e| e.0);
                Ok(json!({ "info": read("project.info")?, "tracks": read("project.tracks")?, "clips": read("project.clips")? }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "List command ids with a description, kind (read, edit, history) and parameter schema. Run them with command_run."
    )]
    async fn command_list(&self, Parameters(p): Parameters<ListParams>) -> Result<CallToolResult, McpError> {
        let filter = p.filter.unwrap_or_default();
        let all = match all_commands() {
            Value::Array(all) => all,
            _ => Vec::new(),
        };
        let list: Vec<Value> = all
            .into_iter()
            .filter(|c| c["id"].as_str().is_some_and(|id| id.contains(filter.as_str())))
            .map(|mut c| {
                if let Some(object) = c.as_object_mut()
                    && let Some(label) = object.remove("label")
                {
                    object.insert("description".into(), label);
                }
                c
            })
            .collect();
        Ok(ok_json(&Value::Array(list)))
    }

    #[tool(
        description = "Run one command by id with its parameters. Editing commands record one undo step; on error the project is unchanged."
    )]
    async fn command_run(&self, Parameters(p): Parameters<RunParams>) -> Result<CallToolResult, McpError> {
        if let Some(why) = refused(&p.id) {
            return Ok(fail(why));
        }
        let params = match confine(&self.workspace, &p.id, p.params.unwrap_or_else(|| json!({}))) {
            Ok(params) => params,
            Err(e) => return Ok(fail(e)),
        };
        let output = self.audio_output.clone();
        let target = p.project.map(ProjectId);
        let result = self
            .with_projects(move |projects| {
                let mut value = projects.execute_in(target, &p.id, &params).map_err(|e| e.0)?;
                if let Some(audio_output) = play_active(projects, &p.id, output)
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert("audio_output".into(), audio_output);
                }
                Ok(value)
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "Run several commands in order. Each editing command is its own undo step. Returns each step's result or error; stops at the first error unless stop_on_error is false."
    )]
    async fn command_batch(&self, Parameters(p): Parameters<BatchParams>) -> Result<CallToolResult, McpError> {
        if p.steps.len() > MAX_BATCH_STEPS {
            return Ok(fail(format!("a batch may have at most {MAX_BATCH_STEPS} steps, got {}", p.steps.len())));
        }
        let stop_on_error = p.stop_on_error.unwrap_or(true);
        let output = self.audio_output.clone();
        let workspace = Arc::clone(&self.workspace);
        let target = p.project.map(ProjectId);
        let result = self
            .with_projects(move |projects| {
                let mut results = Vec::new();
                let mut failed = false;
                for step in p.steps {
                    let params = confine(&workspace, &step.id, step.params.unwrap_or_else(|| json!({})));
                    let outcome = match (refused(&step.id), params) {
                        (Some(why), _) => Err(why.into()),
                        (None, Err(e)) => Err(suhina_session::CommandError(e)),
                        (None, Ok(params)) => projects.execute_in(target, &step.id, &params),
                    };
                    if outcome.is_ok() {
                        play_active(projects, &step.id, output);
                    }
                    match outcome {
                        Ok(value) => results.push(json!({ "id": step.id, "ok": true, "result": value })),
                        Err(e) => {
                            failed = true;
                            results.push(json!({ "id": step.id, "ok": false, "error": e.0 }));
                            if stop_on_error {
                                break;
                            }
                        }
                    }
                }
                Ok((failed, Value::Array(results)))
            })
            .await;
        Ok(match result {
            Ok((false, steps)) => ok_json(&json!({ "steps": steps })),
            Ok((true, steps)) => {
                let mut r = ok_json(&json!({ "steps": steps }));
                r.is_error = Some(true);
                r
            }
            Err(e) => fail(e),
        })
    }

    #[tool(
        description = "Render the arrangement (or one launcher scene, or a script of clip launches) offline to a 32-bit float stereo WAV below the write root. Built-in devices and installed plug-ins (found in the plug-in catalog, with their saved state) run as in playback; notes on a track without an instrument play as sine tones; audio clips play. `plugins` in the result lists plug-ins that are missing or did not start."
    )]
    async fn project_render(&self, Parameters(p): Parameters<RenderParams>) -> Result<CallToolResult, McpError> {
        let path = match self.workspace.writable(&p.output, p.overwrite.unwrap_or(false)) {
            Ok(path) => path,
            Err(e) => return Ok(fail(e)),
        };
        let sample_rate = p.sample_rate.unwrap_or(48_000);
        let scene = p.scene;
        let launches: Option<Vec<LaunchStep>> = p.launches.map(|steps| {
            steps
                .into_iter()
                .map(|s| LaunchStep { at: s.at, id: s.id, params: s.params.unwrap_or_else(|| json!({})) })
                .collect()
        });
        let seconds = p.seconds;
        let project_id = p.project;
        let result = self
            .with_projects(move |projects| {
                let env = projects.plugin_env();
                let session = projects.session_mut(project_id.map(ProjectId)).map_err(|e| e.0)?;
                if let Some(steps) = launches {
                    if scene.is_some() {
                        return Err("give either scene or launches, not both".into());
                    }
                    let seconds = seconds.ok_or("launches needs seconds, the length of the render")?;
                    let project = session.project().clone();
                    let (audio, warnings) = project_audio(session, &project);
                    let (summary, steps) =
                        render_launch_script(&project, &audio, &path, sample_rate, &steps, seconds, Some(&env))?;
                    return Ok(json!({
                        "output": display_path(&path),
                        "warnings": warnings,
                        "plugins": summary.plugins,
                        "seconds": summary.seconds,
                        "sample_rate": summary.sample_rate,
                        "peak": summary.peak,
                        "steps": steps,
                    }));
                }
                if seconds.is_some() {
                    return Err("seconds applies only to launches".into());
                }
                let project = match scene {
                    Some(n) => {
                        scene_as_arrangement(session.project(), n.checked_sub(1).ok_or("scenes are numbered from 1")?)?
                    }
                    None => session.project().clone(),
                };
                let (audio, warnings) = project_audio(session, &project);
                let summary = render_wav(&project, &audio, &path, sample_rate, Some(&env))?;
                Ok(json!({
                    "output": display_path(&path),
                    "warnings": warnings,
                    "plugins": summary.plugins,
                    "seconds": summary.seconds,
                    "sample_rate": summary.sample_rate,
                    "peak": summary.peak,
                }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }
}

/// Starts project `id` playing on the chosen output. Returns the device description, or why it
/// is not available; missing audio files and a fallback to the default device are added to
/// `warnings`.
fn start_audio(projects: &mut Projects, id: ProjectId, warnings: &mut Vec<String>) -> Value {
    let Ok(session) = projects.session_mut(Some(id)) else { return Value::Null };
    let project = session.project().clone();
    let (audio, audio_warnings) = project_audio(session, &project);
    warnings.extend(audio_warnings);
    session.set_audio(audio);
    match projects.open_output(Some(id)) {
        Ok(notice) => {
            warnings.extend(notice);
            let described = projects.get(id).and_then(Session::player).and_then(Player::output_info);
            json!(described.map(|i| i.describe()))
        }
        Err(e) => json!(format!("not available: {e}")),
    }
}

/// The audio `project` (the session's, or a scene of it) plays: its files decoded from where
/// the session's project file is, with what the session has decoded already (files added since
/// it opened, ADR 0011) for the rest.
fn project_audio(session: &Session, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
    let folder = session
        .audio_folder()
        .map(Path::to_path_buf)
        .or_else(|| session.source().and_then(|s| s.parent()).map(Path::to_path_buf));
    let mut have = session.audio().clone();
    let missing = project
        .contents
        .iter()
        .flat_map(|c| &c.lanes)
        .any(|lane| matches!(&lane.data, LaneData::Audio(region) if !have.files.contains_key(&region.file)));
    let mut warnings = Vec::new();
    if missing {
        let (loaded, found) = suhina_media::load_project_audio(project, folder.as_deref().unwrap_or(Path::new(".")));
        for (key, data) in loaded.files {
            have.files.entry(key).or_insert(data);
        }
        warnings = found;
    }
    (have, warnings)
}

/// `params` with every path a command reads checked against the read root and made real
/// (`docs/spec/browser.md` › Commands): files for `clip.addFiles`, `preview.play` and presets,
/// folders for `library.list` and a folder place, and file items for `library.*`.
fn confine(workspace: &Workspace, id: &str, mut params: Value) -> Result<Value, String> {
    match id {
        "clip.addFiles" => {
            if let Some(paths) = params.get_mut("paths").and_then(Value::as_array_mut) {
                for path in paths.iter_mut() {
                    let given = path.as_str().ok_or("paths must be a list of file paths")?;
                    *path = json!(display_path(&workspace.readable(given)?));
                }
            }
        }
        "preview.play" => confine_file(workspace, &mut params, "path")?,
        "device.add" | "track.add" => confine_file(workspace, &mut params, "preset")?,
        "library.list" => {
            let folder = params.get("folder").and_then(Value::as_str).ok_or("folder must be a text")?;
            params["folder"] = json!(display_path(&workspace.readable_folder(folder)?));
        }
        "library.search" => {
            if let Some(place) = params.get("place").and_then(Value::as_str)
                && place != "pinned"
                && !place.starts_with("recent:")
            {
                params["place"] = json!(display_path(&workspace.readable_folder(place)?));
            }
        }
        "library.info" | "library.pin" | "library.unpin" | "library.tag" => {
            if let Some(item) = params.get("item").and_then(Value::as_str)
                && !item.starts_with("device:")
                && !item.starts_with("plugin:")
            {
                let (file, voice) = match item.rsplit_once('#') {
                    Some((file, voice)) if voice.parse::<usize>().is_ok() => (file, Some(voice.to_string())),
                    _ => (item, None),
                };
                let file = display_path(&workspace.readable(file)?);
                params["item"] = json!(voice.map_or(file.clone(), |v| format!("{file}#{v}")));
            }
        }
        _ => {}
    }
    Ok(params)
}

fn confine_file(workspace: &Workspace, params: &mut Value, name: &str) -> Result<(), String> {
    if let Some(given) = params.get(name).and_then(Value::as_str) {
        let path = workspace.readable(given)?;
        params[name] = json!(display_path(&path));
    }
    Ok(())
}

/// Commands agents may not run. They choose the folders whose plug-ins a scan loads and runs,
/// so an agent could otherwise add its write root and run code it wrote there. The user chooses
/// plug-in folders (`suhina-cli plugins add-folder`, later the UI); agents may still scan them.
const NOT_FOR_AGENTS: [&str; 4] =
    ["plugins.addFolder", "plugins.removeFolder", "library.addFolder", "library.removeFolder"];

/// Why an agent may not run command `id`, if it may not.
fn refused(id: &str) -> Option<String> {
    NOT_FOR_AGENTS.contains(&id).then(|| {
        if id.starts_with("library.") {
            format!("{id} is not available to agents: the user chooses the browser's folders")
        } else {
            format!("{id} is not available to agents: the user chooses plug-in folders (suhina-cli plugins add-folder)")
        }
    })
}

/// After a project command, starts the newly active project playing when the server has audio
/// output, since switching closes the previous project's player. Returns what was started.
fn play_active(projects: &mut Projects, id: &str, output: bool) -> Option<Value> {
    if !output {
        return None;
    }
    if !project_commands().iter().any(|c| c.id == id && c.kind == CommandKind::Projects) {
        return None;
    }
    let active = projects.active()?;
    if projects.get(active)?.player().is_some() {
        return None;
    }
    let mut warnings = Vec::new();
    let started = start_audio(projects, active, &mut warnings);
    Some(json!({ "device": started, "warnings": warnings }))
}

/// Adds a title and behaviour hints to each tool, and closes its schema to unknown arguments.
fn annotated(mut tool: Tool) -> Tool {
    let words = tool.name.replace('_', " ");
    let mut chars = words.chars();
    let title = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
    let read_only = matches!(tool.name.as_ref(), "project_inspect" | "command_list");
    // Opening replaces the session's project; commands and renders change the project or files.
    let destructive = matches!(tool.name.as_ref(), "project_open" | "command_run" | "command_batch" | "project_render");
    tool.title = Some(title.clone());
    tool.annotations =
        Some(ToolAnnotations::from_raw(Some(title), Some(read_only), Some(destructive), Some(read_only), Some(false)));
    Arc::make_mut(&mut tool.input_schema).insert("additionalProperties".into(), json!(false));
    tool
}

impl ServerHandler for SuhinaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("suhina", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools: Vec<Tool> = self.tool_router.list_all().into_iter().map(annotated).collect();
        serde_json::from_value(json!({ "tools": tools })).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if let Some(tool) = self.tool_router.get(&request.name) {
            let properties = tool.input_schema.get("properties").and_then(Value::as_object);
            if let Some(args) = &request.arguments
                && let Some(key) = args.keys().find(|k| properties.is_none_or(|p| !p.contains_key(*k)))
            {
                return Err(McpError::invalid_params(format!("unknown argument `{key}` for {}", request.name), None));
            }
        }
        self.tool_router.call(ToolCallContext::new(self, request, context)).await
    }
}

#[cfg(test)]
mod refused_tests {
    use super::refused;

    #[test]
    fn agents_may_scan_but_not_choose_plugin_folders() {
        assert!(refused("plugins.addFolder").is_some());
        assert!(refused("plugins.removeFolder").is_some());
        for id in ["plugins.scan", "plugins.list", "plugins.folders", "plugins.check", "track.setPan"] {
            assert!(refused(id).is_none(), "{id}");
        }
    }
}

#[cfg(test)]
mod confine_tests {
    use super::*;

    fn roots(name: &str) -> (std::path::PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!("suhina-confine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside/pack")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        for file in ["inside/pack/kick.wav", "inside/bank.syx", "outside/secret.wav"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let workspace = Workspace::new(Some(&dir.join("inside")), None).unwrap();
        (dir, workspace)
    }

    #[test]
    fn paths_agents_give_must_be_inside_the_read_root() {
        let (dir, workspace) = roots("paths");
        let ok = confine(&workspace, "clip.addFiles", json!({ "paths": ["pack/kick.wav"], "start": 0.0 })).unwrap();
        assert!(ok["paths"][0].as_str().unwrap().ends_with("kick.wav"));
        assert!(Path::new(ok["paths"][0].as_str().unwrap()).is_absolute());
        let outside = dir.join("outside").join("secret.wav").display().to_string();
        for (id, params) in [
            ("clip.addFiles", json!({ "paths": [outside.clone()] })),
            ("clip.addFiles", json!({ "paths": ["../outside/secret.wav"] })),
            ("preview.play", json!({ "path": outside.clone() })),
            ("device.add", json!({ "track": 1, "preset": outside.clone() })),
            ("library.list", json!({ "folder": dir.join("outside").display().to_string() })),
            ("library.search", json!({ "place": dir.join("outside").display().to_string() })),
            ("library.info", json!({ "item": outside.clone() })),
            ("library.tag", json!({ "item": format!("{outside}#3"), "tag": "Red", "on": true })),
        ] {
            assert!(confine(&workspace, id, params.clone()).is_err(), "{id} {params}");
        }
        let kept = json!({ "item": "device:overdrive" });
        assert_eq!(confine(&workspace, "library.pin", kept.clone()).unwrap(), kept);
        let place = json!({ "place": "recent:samples" });
        assert_eq!(confine(&workspace, "library.search", place.clone()).unwrap(), place);
        let voice = confine(&workspace, "library.info", json!({ "item": "bank.syx#4" })).unwrap();
        assert!(voice["item"].as_str().unwrap().ends_with("bank.syx#4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_may_search_the_library_but_not_choose_its_folders() {
        assert!(refused("library.addFolder").is_some());
        assert!(refused("library.removeFolder").is_some());
        for id in ["library.search", "library.refresh", "library.folders", "clip.addFiles", "preview.play"] {
            assert!(refused(id).is_none(), "{id}");
        }
    }
}
