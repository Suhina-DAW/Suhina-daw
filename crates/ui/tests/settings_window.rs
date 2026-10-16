// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings window: each page scrolls to its last setting in a short window
//! (`docs/spec/ui-settings.md`).

use egui::{Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, theme};

/// A window too short for most pages.
const SHORT: (f32, f32) = (1000.0, 420.0);

fn short_window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SHORT)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness
}

/// Runs frames until the window rests; the Audio page repaints all the time, so it never does.
fn settle(harness: &mut Harness<'static, App>) {
    let _ = harness.run_ok();
}

/// Whether the control named `label` is inside the window, under the Settings title.
fn shown(harness: &Harness<'static, App>, label: &str) -> bool {
    let title = harness.get_by_label("Close settings").rect();
    let rect = harness.get_by_label(label).rect();
    rect.top() > title.bottom() && rect.bottom() < SHORT.1
}

/// Turns the wheel over the page until `label` shows, at most `turns` times; returns how often.
fn wheel_to(harness: &mut Harness<'static, App>, label: &str, turns: usize) -> usize {
    let title = harness.get_by_label("Close settings").rect();
    // The middle of the page, right of the list of pages.
    let over = Pos2::new(title.right() - 200.0, (title.bottom() + SHORT.1) / 2.0);
    for turn in 0..turns {
        if shown(harness, label) {
            return turn;
        }
        harness.hover_at(over);
        harness.step();
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -80.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        settle(harness);
    }
    turns
}

#[test]
fn every_page_scrolls_to_its_last_setting_in_a_short_window() {
    let mut harness = short_window();
    // Each page, and the control that ends it.
    let pages = [
        ("General", "Alt+right plays from the point: On"),
        ("Interface", "Zoom: +"),
        ("Theme", "Export the theme template"),
        ("Arrangement", "Mixer controls: Knob"),
        ("Detail area", "Select on launch: Off"),
        ("Browser", "Tag limit: 1024"),
        ("Graphics", "Renderer: OpenGL"),
        ("Audio", "List the audio devices again"),
        ("Shortcuts", "Reset all keys"),
        ("Plug-ins", "Rescan all"),
    ];
    for (page, last) in pages {
        harness.get_by_label(&format!("Settings page: {page}")).click();
        settle(&mut harness);
        let turns = wheel_to(&mut harness, last, 40);
        assert!(shown(&harness, last), "{page}: {last} stays out of view");
        assert!(turns < 40, "{page}: the wheel did not reach {last}");
    }
}

#[test]
fn the_general_page_needs_the_wheel_and_another_page_opens_at_its_top() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Alt+right plays from the point: On";
    assert!(!shown(&harness, last), "the short window shows the whole General page");
    assert!(wheel_to(&mut harness, last, 40) > 0);
    // Scrolled down, the page's first setting is out of view; another page starts at its top.
    assert!(!shown(&harness, "When opening a project: Ask"));
    harness.get_by_label("Settings page: Browser").click();
    harness.run();
    assert!(shown(&harness, "Browser side: Left"));
    harness.get_by_label("Settings page: General").click();
    harness.run();
    assert!(shown(&harness, "When opening a project: Ask"));
}

#[test]
fn a_control_given_the_focus_is_scrolled_into_view() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Alt+right plays from the point: On";
    assert!(!shown(&harness, last));
    harness.get_by_label(last).focus();
    harness.run();
    assert!(shown(&harness, last), "the focused control stays out of view");
}
