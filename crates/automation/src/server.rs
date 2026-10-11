//! The MCP server. Following PhotoCraft's conventions, it exposes a few generic tools rather than
//! one tool per command: agents discover commands with `command_list` and run them with
//! `command_run` or `command_batch`, so the tool list stays small as commands grow.

use std::sync::{Arc, Mutex, PoisonError};

use loopwerk_engine::AudioPool;
use loopwerk_engine::model::Project;
use loopwerk_session::{Player, Session, commands};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::render::{LaunchStep, render_launch_script, render_wav, scene_as_arrangement};
use crate::workspace::{Workspace, display_path};

const INSTRUCTIONS: &str = "Loopwerk, an open-source clip-launching DAW. Open a Live Set with project_open \
(paths are relative to the read root granted at launch), look at it with project_inspect, discover command ids \
and parameters with command_list, and change it with command_run or command_batch (each editing command is one \
undo step; edit.undo reverts it). project_render writes a WAV file below the write root; with `launches` it \
renders clip launches offline. Transport commands (clip.launch, scene.launch, track.stop, transport.play, \
launcher.state) play on a real audio device only when the server was started with --audio.";

/// Most steps a single `command_batch` may run.
const MAX_BATCH_STEPS: usize = 256;

/// Opens real-time audio output for a project, so transport commands are heard. The CLI
/// provides one when the server is started with `--audio`.
pub trait AudioOutput: Send + Sync {
    /// Starts an engine for `project` with its transport stopped. Returns the player and a
    /// description of the output device.
    fn open(&self, project: &Project, audio: &AudioPool) -> Result<(Player, String), String>;
}

#[derive(Clone)]
pub struct LoopwerkMcp {
    session: Arc<Mutex<Session>>,
    workspace: Arc<Workspace>,
    audio_output: Option<Arc<dyn AudioOutput>>,
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
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Commands to run in order, e.g. `[{"id": "transport.setTempo", "params": {"bpm": 128}}]`. Each editing
    /// command is its own undo step; the batch is not one transaction.
    pub steps: Vec<RunParams>,
    /// Stop at the first failing step (default true).
    #[serde(default)]
    pub stop_on_error: Option<bool>,
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

impl LoopwerkMcp {
    pub fn new(session: Session, workspace: Workspace) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            workspace: Arc::new(workspace),
            audio_output: None,
            tool_router: Self::tool_router(),
        }
    }

    /// Plays each opened project on `output`, so transport and launcher commands are heard.
    pub fn with_audio_output(mut self, output: Arc<dyn AudioOutput>) -> Self {
        self.audio_output = Some(output);
        self
    }

    /// Serves MCP over stdin and stdout until the client disconnects.
    pub async fn serve_stdio(self) -> Result<(), String> {
        let running = self.serve((tokio::io::stdin(), tokio::io::stdout())).await.map_err(|e| e.to_string())?;
        running.waiting().await.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Runs `f` with the session on a blocking thread. A panic in a command would poison the
    /// lock, but commands restore the project on failure, so the session stays usable.
    async fn with_session<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let session = Arc::clone(&self.session);
        tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard)
        })
        .await
        .unwrap_or_else(|e| Err(format!("task failed: {e}")))
    }
}

#[tool_router]
impl LoopwerkMcp {
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
            .with_session(move |session| {
                let (opened, mut warnings) = Session::open(&path).map_err(|e| e.0)?;
                *session = opened;
                let mut info = session.execute("project.info", &json!({})).map_err(|e| e.0)?;
                info["source"] = json!(display_path(&path));
                let mut audio_output = Value::Null;
                if let Some(output) = output {
                    let folder = path.parent().unwrap_or(&path);
                    let (audio, audio_warnings) = loopwerk_media::load_project_audio(session.project(), folder);
                    warnings.extend(audio_warnings);
                    audio_output = match output.open(session.project(), &audio) {
                        Ok((player, description)) => {
                            session.attach_player(player);
                            json!(description)
                        }
                        Err(e) => json!(format!("not available: {e}")),
                    };
                }
                Ok(json!({ "project": info, "warnings": warnings, "audio_output": audio_output }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "The open project as JSON: summary (tempo, counts, undo state), tracks with mixer settings and devices, and arrangement clips."
    )]
    async fn project_inspect(&self) -> Result<CallToolResult, McpError> {
        let result = self
            .with_session(|session| {
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
        let list: Vec<Value> = commands()
            .iter()
            .filter(|c| c.id.contains(filter.as_str()))
            .map(|c| json!({ "id": c.id, "description": c.label, "kind": c.kind.name(), "params": c.input_schema() }))
            .collect();
        Ok(ok_json(&Value::Array(list)))
    }

    #[tool(
        description = "Run one command by id with its parameters. Editing commands record one undo step; on error the project is unchanged."
    )]
    async fn command_run(&self, Parameters(p): Parameters<RunParams>) -> Result<CallToolResult, McpError> {
        let params = p.params.unwrap_or_else(|| json!({}));
        let result = self.with_session(move |session| session.execute(&p.id, &params).map_err(|e| e.0)).await;
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
        let result = self
            .with_session(move |session| {
                let mut results = Vec::new();
                let mut failed = false;
                for step in p.steps {
                    let params = step.params.unwrap_or_else(|| json!({}));
                    match session.execute(&step.id, &params) {
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
        description = "Render the arrangement (or one launcher scene, or a script of clip launches) offline to a 32-bit float stereo WAV below the write root. Notes play as sine tones for now; audio clips play (warped clips change pitch with speed); plug-ins are not rendered yet."
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
        let result = self
            .with_session(move |session| {
                if let Some(steps) = launches {
                    if scene.is_some() {
                        return Err("give either scene or launches, not both".into());
                    }
                    let seconds = seconds.ok_or("launches needs seconds, the length of the render")?;
                    let project = session.project().clone();
                    let folder = session.source().and_then(|s| s.parent()).map(|p| p.to_path_buf());
                    let (audio, warnings) = match folder {
                        Some(folder) => loopwerk_media::load_project_audio(&project, &folder),
                        None => Default::default(),
                    };
                    let (summary, steps) = render_launch_script(&project, &audio, &path, sample_rate, &steps, seconds)?;
                    return Ok(json!({
                        "output": display_path(&path),
                        "warnings": warnings,
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
                let folder = session.source().and_then(|s| s.parent()).map(|p| p.to_path_buf());
                let (audio, warnings) = match folder {
                    Some(folder) => loopwerk_media::load_project_audio(&project, &folder),
                    None => Default::default(),
                };
                let summary = render_wav(&project, &audio, &path, sample_rate)?;
                Ok(json!({
                    "output": display_path(&path),
                    "warnings": warnings,
                    "seconds": summary.seconds,
                    "sample_rate": summary.sample_rate,
                    "peak": summary.peak,
                }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }
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

impl ServerHandler for LoopwerkMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("loopwerk", env!("CARGO_PKG_VERSION")))
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
