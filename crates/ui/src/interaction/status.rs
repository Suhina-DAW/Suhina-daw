// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The line in the status bar: what the app or a view tells the user after an action.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Hint(String),
    Done(String),
    Error(String),
}
