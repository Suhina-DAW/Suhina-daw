//! Suhina's user interface (ADR 0009): the main window and its views, drawn with egui. A thin
//! client of the session: it reads the project and the engine's monitor, and every change it
//! makes is a command (`Session::execute`), the same ones the CLI and MCP run.

mod app;
pub mod arrangement_view;
pub mod browser;
pub mod browser_view;
pub mod demo;
pub mod detail_view;
pub mod drop;
pub mod grid;
pub mod keymap;
pub mod loading_dialog;
pub mod menus;
pub mod missing_files;
pub mod motion;
pub mod piano_roll;
pub mod project_dialog;
pub mod sample_map;
mod sample_map_crowd;
pub mod sample_map_panel;
pub mod session_view;
pub mod settings;
pub mod settings_view;
pub mod theme;
pub mod themes;
pub mod timeline;
pub mod waveform;
pub mod widgets;

pub use app::{App, NewProject, OpenAs, Status, View};
