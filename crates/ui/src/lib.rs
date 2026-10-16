// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's user interface (ADR 0009): the main window and its views, drawn with egui. A thin
//! client of the session: it reads the project and the engine's monitor, and every change it
//! makes is a command (`Session::execute`), the same ones the CLI and MCP run.

pub mod app;
pub mod arrangement;
pub mod browser;
pub mod demo;
pub mod detail;
pub mod interaction;
pub mod kit;
pub mod session;
pub mod settings;

pub use app::{App, NewProject, OpenAs, View};
pub use interaction::status::Status;
