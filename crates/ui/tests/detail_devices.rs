// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area's Clip / Devices switch and the double-click on a track that shows its devices
//! (`docs/spec/ui-clip-editor.md` › Where it opens): from an Arrangement header, a Session column's
//! name and its channel strip, opening a hidden detail area, and never changing the project.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Device, DeviceKind, Extensions, Project, TrackId};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 900.0);
/// A MIDI clip on Drums in the Arrangement view, and one in a Session slot.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const SLOT: &str = "Select Beat B on Drums (scene 3)";

fn device(name: &str) -> Device {
    Device {
        name: name.into(),
        kind: DeviceKind::Builtin(name.to_lowercase()),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    }
}

/// The demo project with a Utility on Drums and a Limiter on Bass.
fn project() -> Project {
    let mut project = demo_project();
    project.channels[0].devices.push(device("Utility"));
    project.channels[1].devices.push(device("Limiter"));
    project
}

/// The window with the project in the Session view, frames close enough for a double-click.
fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn arrangement() -> Harness<'static, App> {
    let mut harness = window();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// A double-click at `at`, after a second without clicks.
fn double_click_at(harness: &mut Harness<'static, App>, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// A spot of a header's own background, clear of its controls and of the
/// splitter at its left edge: 10 px in, halfway down.
fn header_spot(harness: &Harness<'static, App>, track: &str) -> Pos2 {
    harness.get_by_label(&format!("Track {track}")).rect().left_center() + vec2(10.0, 0.0)
}

/// What the detail area shows: the piano roll (its toolbar's ×), a named device, the Clip / Devices
/// switch.
fn piano(harness: &Harness<'static, App>) -> bool {
    harness.query_by_label_contains("Piano roll,").is_some()
}

fn shows_device(harness: &Harness<'static, App>, name: &str) -> bool {
    harness.query_by_label(&format!("Device {name}")).is_some()
}

fn switch(harness: &Harness<'static, App>) -> bool {
    harness.query_by_label("Show Clip").is_some() && harness.query_by_label("Show Devices").is_some()
}

fn project_now(harness: &Harness<'static, App>) -> Project {
    harness.state().session().unwrap().project().clone()
}

fn hide_detail_area(harness: &mut Harness<'static, App>) {
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Show Clip").is_none() && !piano(harness), "the detail area is hidden");
}

#[test]
fn the_switch_goes_from_the_clip_to_its_tracks_devices_and_back() {
    let mut harness = arrangement();
    let before = project_now(&harness);
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness) && switch(&harness), "a MIDI clip shows its piano roll and the switch");
    assert!(!shows_device(&harness, "Utility"));

    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(!piano(&harness), "the devices take the piano roll's place");
    assert!(shows_device(&harness, "Utility"), "Drums' chain shows");
    assert!(switch(&harness), "the clip column stays, with the switch to go back");
    assert!(harness.query_by_label("Clip Beat B, start 17.1").is_some());

    harness.get_by_label("Show Clip").click();
    harness.run();
    assert!(piano(&harness) && !shows_device(&harness, "Utility"), "the piano roll is back");

    // The key does the same, both ways.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(!piano(&harness) && shows_device(&harness, "Utility"));
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(piano(&harness) && !shows_device(&harness, "Utility"));

    // On the devices, a click on the selected clip shows its editor again.
    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(!piano(&harness));
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness), "a click on a clip edits it");

    // Window state only: the project never changed, and there is nothing to undo.
    assert_eq!(project_now(&harness), before);
    assert!(!harness.state().session().unwrap().can_undo());
}

#[test]
fn the_key_opens_a_hidden_detail_area_on_the_devices_and_needs_a_clip_with_an_editor() {
    let mut harness = arrangement();
    // No clip selected: the key does nothing.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(!switch(&harness) && !piano(&harness));

    harness.get_by_label(CLIP).click();
    harness.run();
    hide_detail_area(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(shows_device(&harness, "Utility") && !piano(&harness), "opened on the devices");
}

#[test]
fn a_double_click_on_an_arrangement_header_shows_that_tracks_devices() {
    let mut harness = arrangement();
    let before = project_now(&harness);
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness));

    // Drums' own header: the clip stays selected, the switch goes to Devices.
    let spot = header_spot(&harness, "Drums");
    double_click_at(&mut harness, spot);
    assert!(!piano(&harness) && shows_device(&harness, "Utility"), "Drums' devices in place of the notes");
    assert!(switch(&harness));
    assert!(harness.state().selected_clip().is_some(), "the clip on that track stays selected");
    assert_eq!(harness.state().arrangement().selected_tracks.first(), Some(&TrackId(1)));

    // Another track's header: its devices, and the clip on Drums is no longer selected.
    let spot = header_spot(&harness, "Bass");
    double_click_at(&mut harness, spot);
    assert!(shows_device(&harness, "Limiter") && !shows_device(&harness, "Utility"));
    assert_eq!(harness.state().selected_clip(), None);
    assert!(!switch(&harness), "no clip, no switch");

    // A single click keeps its meaning: selecting a clip edits it.
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness));
    assert_eq!(project_now(&harness), before);
}

#[test]
fn a_double_click_on_a_header_opens_the_hidden_detail_area() {
    let mut harness = arrangement();
    hide_detail_area(&mut harness);
    assert!(!shows_device(&harness, "Limiter"));
    let spot = header_spot(&harness, "Bass");
    double_click_at(&mut harness, spot);
    assert!(shows_device(&harness, "Limiter"), "the detail area opened on Bass' devices");
}

#[test]
fn a_double_click_on_a_session_column_name_shows_its_devices() {
    let mut harness = window();
    harness.get_by_label(SLOT).click();
    harness.run();
    assert!(piano(&harness), "the slot's MIDI clip shows its piano roll");
    hide_detail_area(&mut harness);

    let name = harness.get_by_label("Track Bass").rect().center();
    double_click_at(&mut harness, name);
    assert!(shows_device(&harness, "Limiter"), "the detail area opened on Bass' devices");
    assert!(!piano(&harness) && !switch(&harness), "the click on the name cleared the slot selection");
}

#[test]
fn a_double_click_on_the_channel_strip_shows_its_devices_and_keeps_the_slot_on_that_track() {
    let mut harness = window();
    let before = project_now(&harness);
    harness.get_by_label(SLOT).click();
    harness.run();
    assert!(piano(&harness));

    // The strip's name row is its own background for clicks.
    let strip = harness.get_by_label("Drums strip").rect().center();
    double_click_at(&mut harness, strip);
    assert!(!piano(&harness) && shows_device(&harness, "Utility"), "Drums' devices");
    assert!(switch(&harness), "the slot on Drums stays selected, with the switch");
    harness.get_by_label("Show Clip").click();
    harness.run();
    assert!(piano(&harness), "and the switch brings its notes back");

    // Bass' strip, with the detail area hidden: it opens, and the slot on Drums is let go.
    hide_detail_area(&mut harness);
    let strip = harness.get_by_label("Bass strip").rect().center();
    double_click_at(&mut harness, strip);
    assert!(shows_device(&harness, "Limiter") && !switch(&harness));
    assert_eq!(project_now(&harness), before);
}

#[test]
fn a_double_click_on_a_strip_control_is_the_controls_own() {
    let mut harness = window();
    let fader = harness.get_by_label("Drums volume").rect().center();
    hide_detail_area(&mut harness);
    double_click_at(&mut harness, fader);
    assert!(!shows_device(&harness, "Utility"), "the fader's double-click resets it, nothing more");
}
