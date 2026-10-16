// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation lanes in the Arrangement view (#420), driven like a user would: points added,
//! moved and deleted with the pointer and the keyboard become `automation.*` commands that
//! change the project through the session, with undo.

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{AutomationPoint, Project};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

fn arrangement(project: Project) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// The demo project in the Arrangement view with the Drums track's lanes shown.
fn with_lanes() -> Harness<'static, App> {
    let mut harness = arrangement(demo_project());
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    harness
}

/// The points of the Drums track's volume lane, and whether it plays.
fn volume(harness: &Harness<'static, App>) -> (Vec<AutomationPoint>, bool) {
    let Some(session) = harness.state().session() else { return (Vec::new(), true) };
    let project = session.project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let lane = project.channels.iter().find(|c| Some(c.id) == channel).and_then(|c| c.lane("volume"));
    lane.map_or((Vec::new(), true), |l| (l.points.clone(), l.enabled))
}

/// The points of the Drums track's pan lane.
fn pan(harness: &Harness<'static, App>) -> (Vec<AutomationPoint>, bool) {
    let Some(session) = harness.state().session() else { return (Vec::new(), true) };
    let project = session.project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let lane = project.channels.iter().find(|c| Some(c.id) == channel).and_then(|c| c.lane("pan"));
    lane.map_or((Vec::new(), true), |l| (l.points.clone(), l.enabled))
}

/// A press and release of the left button at `pos`.
fn click_at(harness: &mut Harness<'static, App>, pos: Pos2) {
    harness.hover_at(pos);
    harness.run();
    harness.drag_at(pos);
    harness.run();
    harness.drop_at(pos);
    harness.run();
}

fn drag(harness: &mut Harness<'static, App>, from: Pos2, by: egui::Vec2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

/// Where the volume lane is on screen.
fn lane(harness: &Harness<'static, App>) -> egui::Rect {
    harness.get_by_label("Volume automation on Drums").rect()
}

fn add_point(harness: &mut Harness<'static, App>, x: f32) {
    let rect = lane(harness);
    click_at(harness, Pos2::new(rect.left() + x, rect.center().y));
}

#[test]
fn the_envelope_button_shows_and_hides_a_tracks_lanes() {
    let mut harness = arrangement(demo_project());
    assert!(harness.query_by_label("Volume automation on Drums").is_none());
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
    // The next track moved down to make room.
    harness.get_by_label("Hide automation of Drums").click();
    harness.run();
    assert!(harness.query_by_label("Volume automation on Drums").is_none());
}

#[test]
fn a_click_on_a_lane_adds_a_point_on_the_grid_and_undo_removes_it() {
    let mut harness = with_lanes();
    // 8 points a beat by default, where the grid is a bar: 100 points in is beat 12.5, bar 4.
    add_point(&mut harness, 100.0);
    let (points, enabled) = volume(&harness);
    assert_eq!(points.len(), 1, "{points:?}");
    assert_eq!(points[0].beat, 12.0);
    assert!(enabled);
    // Half-way up the lane is below 0 dB (0 dB sits at 85 %).
    assert!(points[0].value > 0.0 && points[0].value < 1.0, "{}", points[0].value);
    assert!(harness.query_by_label("Volume point 1 on Drums").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(volume(&harness).0.is_empty());
}

#[test]
fn alt_adds_a_point_off_the_grid() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let pos = Pos2::new(rect.left() + 100.0, rect.center().y);
    harness.hover_at(pos);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.drag_at(pos);
    harness.run();
    harness.drop_at(pos);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 1, "{points:?}");
    assert!((points[0].beat - 12.5).abs() < 0.2, "{}", points[0].beat);
}

#[test]
fn dragging_a_point_moves_it_on_the_grid_in_one_undo_step() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    let before = volume(&harness).0;
    let point = harness.get_by_label("Volume point 1 on Drums").rect().center();
    // Two bars later (64 points) and up.
    drag(&mut harness, point, vec2(64.0, -10.0));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].beat, 20.0);
    assert!(points[0].value > before[0].value, "{points:?} {before:?}");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(volume(&harness).0, before);
}

#[test]
fn a_double_click_or_delete_removes_points() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    add_point(&mut harness, 200.0);
    assert_eq!(volume(&harness).0.len(), 2);

    // A click selects a point and Delete removes it.
    let point = harness.get_by_label("Volume point 2 on Drums").rect().center();
    click_at(&mut harness, point);
    harness.key_press(Key::Delete);
    harness.run();
    let (points, _) = volume(&harness);
    assert_eq!(points.iter().map(|p| p.beat).collect::<Vec<_>>(), [12.0]);

    // A double-click removes the last, and the lane goes with it.
    let at = harness.get_by_label("Volume point 1 on Drums").rect().center();
    harness.hover_at(at);
    harness.run();
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
    assert!(volume(&harness).0.is_empty());
    // The lane stays shown, ready for a new point.
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
}

#[test]
fn selected_points_move_and_delete_together() {
    let mut harness = with_lanes();
    for x in [100.0, 200.0, 300.0] {
        add_point(&mut harness, x);
    }
    // A box around the first two selects them.
    let rect = lane(&harness);
    drag(&mut harness, Pos2::new(rect.left() + 60.0, rect.top() + 2.0), vec2(180.0, rect.height() - 4.0));
    let first = harness.get_by_label("Volume point 1 on Drums").rect().center();
    drag(&mut harness, first, vec2(32.0, 0.0));
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [16.0, 28.0, 36.0]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [12.0, 24.0, 36.0]);

    // Shift-click adds a point to the selection; Delete removes both in one step.
    let one = harness.get_by_label("Volume point 1 on Drums").rect().center();
    let three = harness.get_by_label("Volume point 3 on Drums").rect().center();
    click_at(&mut harness, one);
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    click_at(&mut harness, three);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.key_press(Key::Delete);
    harness.run();
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [24.0]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(volume(&harness).0.len(), 3);
}

#[test]
fn a_lane_turned_off_by_a_manual_change_turns_on_again_from_its_header() {
    let mut project = demo_project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
        let mut lane = suhina_engine::model::AutomationLane::new("volume");
        lane.insert(AutomationPoint::new(0.0, 0.5));
        lane.insert(AutomationPoint::new(8.0, 1.0));
        lane.enabled = false;
        channel.automation.push(lane);
    }
    let mut harness = arrangement(project);
    // The lane with points is the one shown first.
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    assert!(!volume(&harness).1);
    harness.get_by_label("Turn on again Volume automation on Drums").click();
    harness.run();
    assert!(volume(&harness).1);
    assert!(harness.query_by_label("Turn off Volume automation on Drums").is_some());
}

#[test]
fn a_lane_shows_another_parameter_and_more_lanes_are_added() {
    let mut harness = with_lanes();
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    // The next parameter not shown: the pan.
    assert!(harness.query_by_label("Pan automation on Drums").is_some());
    harness.get_by_label("Hide Pan lane on Drums").click();
    harness.run();
    assert!(harness.query_by_label("Pan automation on Drums").is_none());
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
}

#[test]
fn dragging_a_segment_up_bends_its_curve() {
    let mut harness = with_lanes();
    // The pan's lane: its values are spread evenly up the lane.
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    let rect = harness.get_by_label("Pan automation on Drums").rect();
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.bottom() - 8.0));
    click_at(&mut harness, Pos2::new(rect.left() + 228.0, rect.top() + 8.0));
    let (points, _) = pan(&harness);
    assert_eq!(points.len(), 2);
    assert!(points[1].value > points[0].value);
    // The middle of the straight line between them, dragged up: a rising segment moves fast first.
    let a = harness.get_by_label("Pan point 1 on Drums").rect().center();
    let b = harness.get_by_label("Pan point 2 on Drums").rect().center();
    let middle = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    drag(&mut harness, middle, vec2(0.0, -20.0));
    let (points, _) = pan(&harness);
    assert!(points[0].curve > 0.1, "{points:?}");
    assert_eq!(points[1].curve, 0.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(pan(&harness).0[0].curve, 0.0);
}

/// Renders the Arrangement view with lanes shown to `target/ui-snapshots/automation-lanes.png`,
/// for looking at: `cargo test -p suhina-ui --test automation_lanes -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_lanes() {
    let mut project = demo_project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
        let mut volume = suhina_engine::model::AutomationLane::new("volume");
        for (beat, value, curve) in [(0.0, 0.3, 0.0), (16.0, 1.0, 0.6), (48.0, 0.5, -0.5), (64.0, 0.9, 0.0)] {
            volume.insert(AutomationPoint { beat, value, curve });
        }
        let mut pan = suhina_engine::model::AutomationLane::new("pan");
        pan.insert(AutomationPoint::new(8.0, -0.6));
        pan.insert(AutomationPoint::new(40.0, 0.6));
        pan.enabled = false;
        channel.automation.extend([volume, pan]);
    }
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    let one = harness.get_by_label("Volume point 2 on Drums").rect().center();
    click_at(&mut harness, one);
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
    image.save(out.join("automation-lanes.png")).unwrap_or_else(|e| panic!("{e}"));
}
