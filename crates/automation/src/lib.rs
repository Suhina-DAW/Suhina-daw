//! Agent-facing automation: an MCP server (the official `rmcp` SDK) over a Suhina session,
//! file access limited to granted roots, and the offline WAV render shared with the CLI.

pub mod render;
pub mod server;
pub mod workspace;

pub use render::{LaunchStep, RenderSummary, render_launch_script, render_wav, scene_as_arrangement};
pub use server::{AudioOutput, SuhinaMcp};
pub use workspace::{Workspace, display_path};
