//! The piano roll's second step in the window (`docs/spec/ui-clip-editor.md`): pulled to the top
//! by its splitter, Alt and the wheel zooming key rows, the toolbar's tools (Select, Draw, Paint,
//! Erase) and its own grid value.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::piano_roll::{Tool, key_name};
use suhina_ui::settings::{PianoRollTop, Settings};
use suhina_ui::widgets::Action;
use suhina_ui::{App, theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project in the Arrangement view with the clip selected, so the piano roll shows it;
/// `settings` as Settings has them.
fn piano_app(renderer: &mut suhina_engine::Renderer, settings: Settings) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness
}

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap()
}

/// The edited clip's notes in the project.
fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.clone(),
        LaneData::Audio(_) => Vec::new(),
    }
}

fn note_rect(h: &Harness<'static, App>, note: &Note) -> Rect {
    h.get_by_label(&format!("Note {} at beat {}", key_name(note.key), note.time)).rect()
}

/// Where beat `time` of key `key` is on screen, measured from two notes.
fn spot(h: &Harness<'static, App>, time: f64, key: u8) -> Pos2 {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    let row = (a.top() - b.top()) / (f32::from(n[1].key) - f32::from(n[0].key));
    pos2(a.left() + (time - n[0].time) as f32 * beat + 2.0, a.center().y - (f32::from(key) - f32::from(n[0].key)) * row)
}

/// An empty key row in view: one between the clip's notes, or just under the lowest.
fn empty_key(h: &Harness<'static, App>) -> u8 {
    let n = notes(h);
    let (low, high) = (n.iter().map(|n| n.key).min().unwrap(), n.iter().map(|n| n.key).max().unwrap());
    (low..=high).find(|k| n.iter().all(|n| n.key != *k)).unwrap_or(low - 1)
}

fn button(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    button(h, pos, true);
    button(h, pos, false);
    h.run();
}

/// Presses at `from`, moves to `to` in `steps` frames and lets go.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2, steps: usize) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for step in 1..=steps {
        h.hover_at(from + (to - from) * (step as f32 / steps as f32));
        h.step();
    }
    button(h, to, false);
    h.run();
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

fn tool(h: &mut Harness<'static, App>, name: &str) {
    h.get_by_label(&format!("{name} tool")).click();
    h.run();
}

fn splitter(h: &Harness<'static, App>) -> Rect {
    h.get_by_label("Detail area splitter").rect()
}

/// Drags the detail area's splitter to `y`.
fn pull_to(h: &mut Harness<'static, App>, y: f32) {
    let from = splitter(h).center();
    drag(h, from, pos2(from.x, y), 6);
}

#[test]
fn pulled_to_the_top_the_view_shows_only_the_edited_track() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let start = splitter(&h).top();
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some(), "the other tracks show first");

    pull_to(&mut h, 40.0);
    // Above the piano roll: the ruler and the edited track's lane, nothing else.
    assert!(h.query_by_label(CLIP).is_some(), "the edited clip stays in view");
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_none(), "no other track shows");
    let top = splitter(&h).top();
    assert!(top < start - 100.0, "the splitter went up from {start} to {top}");
    let lane = h.get_by_label(CLIP).rect();
    assert!(lane.bottom() <= top + 1.0, "the edited track sits right above the piano roll");

    // Dragged down again, the view comes back with every track.
    pull_to(&mut h, 500.0);
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some());
    assert!((splitter(&h).center().y - 500.0).abs() < 20.0, "at {:?}", splitter(&h));
}

#[test]
fn pulled_to_the_top_over_the_whole_window_nothing_stays_above() {
    let mut renderer = engine();
    let settings = Settings { piano_roll_top: PianoRollTop::Window, ..Settings::default() };
    let mut h = piano_app(&mut renderer, settings);
    pull_to(&mut h, 40.0);
    assert!(h.query_by_label(CLIP).is_none(), "no arrangement shows");
    assert!(h.query_by_label_contains("Piano roll,").is_some(), "the piano roll does");
    // A double-click on the splitter brings the view back, and another takes it to the top.
    let at = splitter(&h).center();
    h.get_by_label("Detail area splitter").hover();
    h.run();
    for _ in 0..2 {
        button(&mut h, at, true);
        button(&mut h, at, false);
    }
    h.run();
    assert!(h.query_by_label(CLIP).is_some(), "the view is back");
}

#[test]
fn the_view_shows_every_track_again_once_the_piano_roll_is_gone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    pull_to(&mut h, 40.0);
    // Shift+Tab hides the detail area: the view is whole again.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some());
}

#[test]
fn alt_and_the_wheel_zoom_key_rows() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let height = h.state().piano_roll().key_height;
    h.hover_at(grid.center());
    h.step();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 60.0),
        modifiers: Modifiers::ALT,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    assert!(h.state().piano_roll().key_height > height, "rows grew from {height}");
}

#[test]
fn the_draw_tool_adds_a_note_with_a_click_and_its_length_with_a_drag() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Draw");
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    let before = notes(&h).len();
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first, key);
    click(&mut h, p0);
    let after = notes(&h);
    assert_eq!(after.len(), before + 1);
    // The piano roll's own grid is a sixteenth: the note is a quarter beat long.
    let added = after.last().unwrap();
    assert_eq!((added.key, added.duration), (key, 0.25));

    // A drag from an empty spot sets the length, on the grid.
    let at = spot(&h, first + 2.0, key);
    let to = spot(&h, first + 3.0, key);
    drag(&mut h, at, to, 4);
    let drawn = *notes(&h).last().unwrap();
    assert_eq!(notes(&h).len(), before + 2);
    assert!((drawn.duration - 1.0).abs() < 1e-9, "{drawn:?}");
    undo(&mut h);
    assert_eq!(notes(&h).len(), before + 1, "one drawn note is one undo step");
}

#[test]
fn b_switches_between_select_and_draw() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // B is the piano roll's after a click in it.
    let at = note_rect(&h, &notes(&h)[0]).center();
    click(&mut h, at);
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Select);
}

#[test]
fn the_paint_tool_paints_a_row_of_notes_in_one_undo_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Paint");
    let before = notes(&h).len();
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    // Across a beat at a sixteenth: four notes, a quarter beat apart.
    let p0 = spot(&h, first, key);
    let p1 = spot(&h, first + 0.9, key);
    drag(&mut h, p0, p1, 12);
    let painted: Vec<Note> = notes(&h).into_iter().skip(before).collect();
    assert_eq!(painted.len(), 4, "{painted:?}");
    for (i, note) in painted.iter().enumerate() {
        assert_eq!(note.key, key);
        assert!((note.time - (painted[0].time + i as f64 * 0.25)).abs() < 1e-9, "{painted:?}");
    }
    undo(&mut h);
    assert_eq!(notes(&h).len(), before, "the whole stroke is one undo step");
}

#[test]
fn the_erase_tool_deletes_what_it_passes_over_in_one_undo_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // Two notes side by side in an empty row to erase with one stroke.
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([
        { "time": first, "length": 0.25, "key": key, "velocity": 100 },
        { "time": first + 0.5, "length": 0.25, "key": key, "velocity": 100 },
    ]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let before = notes(&h).len();
    tool(&mut h, "Erase");
    let p0 = spot(&h, first + 0.05, key);
    let p1 = spot(&h, first + 0.6, key);
    drag(&mut h, p0, p1, 10);
    assert_eq!(notes(&h).len(), before - 2);
    undo(&mut h);
    assert_eq!(notes(&h).len(), before);
    // A click erases one.
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    click(&mut h, at);
    assert_eq!(notes(&h).len(), before - 1);
}

#[test]
fn the_piano_roll_keeps_its_own_grid_unless_settings_link_it_to_the_control_bar() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // The control bar's grid value sets the Arrangement view's only.
    h.state_mut().run(&Action::new("launcher.setQuantization", json!({ "value": "1/4" })));
    h.run();
    assert!(h.query_by_label("Grid value 1/16").is_some(), "the piano roll's own grid shows");
    tool(&mut h, "Draw");
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first, key);
    click(&mut h, p0);
    assert_eq!(notes(&h).last().unwrap().duration, 0.25);

    // Following the control bar, its value is the piano roll's.
    let settings = Settings { piano_grid_follows: true, ..Settings::default() };
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert!(h.query_by_label("Grid value 1/4").is_some());
}

#[test]
fn the_toolbar_grid_menu_changes_the_piano_rolls_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    h.get_by_label("Grid value 1/16").click();
    h.run();
    // The menu's item is picked as a screen reader picks it: a simulated press does not reach
    // an open menu in the test harness (neither the control bar's).
    h.get_by_label("1/8").click_accesskit();
    h.run();
    assert!(h.query_by_label("Grid value 1/8").is_some());
    tool(&mut h, "Draw");
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first + 1.0, key);
    click(&mut h, p0);
    assert_eq!(notes(&h).last().unwrap().duration, 0.5);
}

#[test]
fn a_new_note_takes_the_length_of_the_note_last_clicked() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([{ "time": first, "length": 1.5, "key": key, "velocity": 60 }]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    tool(&mut h, "Draw");
    let long = *notes(&h).last().unwrap();
    let p0 = note_rect(&h, &long).center();
    click(&mut h, p0);
    let p0 = spot(&h, first + 2.0, key);
    click(&mut h, p0);
    let drawn = notes(&h).last().copied().unwrap();
    assert_eq!(drawn.duration, 1.5);
    assert_eq!(suhina_session::midi_velocity(drawn.velocity), 60);
}

#[test]
fn settings_general_has_both_piano_roll_settings_and_they_change_the_settings() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().settings_open());
    // Both rows fit on the page, inside the window.
    for name in ["Pulled to the top: Whole window", "Piano roll grid: Control bar"] {
        let rect = h.get_by_label(name).rect();
        assert!(rect.bottom() < SIZE.1, "{name} at {rect:?}");
    }
    h.get_by_label("Pulled to the top: Whole window").click();
    h.run();
    h.get_by_label("Piano roll grid: Control bar").click();
    h.run();
    assert_eq!(h.state().settings().piano_roll_top, PianoRollTop::Window);
    assert!(h.state().settings().piano_grid_follows);
}

#[test]
fn a_paint_stroke_that_starts_on_a_note_paints_and_leaves_that_note_alone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Paint");
    let before = notes(&h);
    let start = before[0];
    // From the middle of a note to a beat later, in its row.
    let from = note_rect(&h, &start).center();
    let to = spot(&h, start.time + 1.0, start.key);
    drag(&mut h, from, to, 12);
    let after = notes(&h);
    assert_eq!(after[0], start, "the note the stroke started on did not move");
    let painted: Vec<&Note> = after.iter().skip(before.len()).collect();
    assert!(!painted.is_empty(), "the stroke painted");
    assert!(painted.iter().all(|n| n.key == start.key && n.time > start.time), "{painted:?}");
}
