//! Loopwerk's user interface (ADR 0009): the main window and its views, drawn with egui. A thin
//! client of the session: it reads the project and the engine's monitor, and every change it
//! makes is a command (`Session::execute`), the same ones the CLI and MCP run.

mod app;
pub mod arrangement_view;
pub mod demo;
pub mod grid;
pub mod menus;
pub mod motion;
pub mod session_view;
pub mod settings;
pub mod settings_view;
pub mod theme;
pub mod timeline;
pub mod widgets;

pub use app::{App, Status, View};
