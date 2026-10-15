//! The Session view and the window, driven like a user would: clicks become the commands the
//! CLI and MCP run, and edits change the project through the session.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_session::Session;
use suhina_ui::arrangement_view::{self, ArrangementViewState};
use suhina_ui::demo::demo_project;
use suhina_ui::grid::Grid;
use suhina_ui::session_view::{self, SessionViewState};
use suhina_ui::themes::FileAction;
use suhina_ui::timeline::Timeline;
use suhina_ui::widgets::Action;
use suhina_ui::{App, OpenAs, Status, View, theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

/// A file dropped on the window in a test.
#[derive(Debug)]
struct TestFile(std::path::PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

struct SessionHarness {
    grid: Grid,
    state: SessionViewState,
    actions: Vec<Action>,
}

fn view() -> Harness<'static, SessionHarness> {
    let view = SessionHarness {
        grid: Grid::build(&demo_project(), None),
        state: SessionViewState::default(),
        actions: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn clicked(harness: &mut Harness<'static, SessionHarness>, label: &str) -> Vec<Action> {
    harness.state_mut().actions.clear();
    harness.get_by_label(label).click();
    harness.run();
    harness.state().actions.clone()
}

#[test]
fn clicking_a_clip_launches_it() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Launch Walk on Bass (scene 2)"),
        [Action::new("clip.launch", json!({ "track": 2, "scene": 2 }))]
    );
}

#[test]
fn clicking_an_empty_slot_with_a_stop_button_stops_the_track() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Stop Keys (scene 1)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 1 }))]
    );
}

#[test]
fn scenes_and_stop_buttons_send_their_commands() {
    let mut harness = view();
    assert_eq!(clicked(&mut harness, "Launch scene 3 Build"), [Action::new("scene.launch", json!({ "scene": 3 }))]);
    assert_eq!(clicked(&mut harness, "Stop Drums"), [Action::new("track.stop", json!({ "track": 1 }))]);
    assert_eq!(clicked(&mut harness, "Stop all clips"), [Action::new("launcher.stopAll", json!({}))]);
    assert_eq!(
        clicked(&mut harness, "Turn Vox off"),
        [Action::new("track.setMute", json!({ "track": 4, "mute": true }))]
    );
}

#[test]
fn return_tracks_have_a_mixer_but_no_slots() {
    let harness = view();
    assert!(harness.query_by_label("Turn A Reverb off").is_some());
    assert!(harness.query_by_label_contains("on A Reverb").is_none());
}

fn window(app: App) -> Harness<'static, App> {
    window_with_step(app, 0.25)
}

/// Like [`window`], with `step` seconds a frame (a double-click needs frames closer together),
/// and a second of frames for animations to settle.
fn window_with_step(app: App, step: f32) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(step)
        .with_max_steps((1.0 / step).ceil() as u64)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// The demo project in the window, with the browser hidden so the views have the whole width
/// (the browser has tests of its own, in `browser.rs`).
fn demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    app
}

#[test]
fn turning_a_track_off_is_an_edit_that_undoes() {
    let mut harness = window(demo_app());
    let muted = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[1].mute);
    assert_eq!(muted(&harness), Some(false));

    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert_eq!(muted(&harness), Some(true));
    assert!(harness.query_by_label("Turn Bass on").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(muted(&harness), Some(false));
}

#[test]
fn launching_without_an_audio_engine_says_why() {
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Beat A on Drums (scene 1)").click();
    harness.run();
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("no audio engine"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn an_empty_window_asks_for_a_project() {
    let harness = window(App::new());
    assert!(harness.state().session().is_none());
    assert!(matches!(harness.state().status(), Status::Hint(_)));
}

#[test]
fn tab_and_the_view_switch_change_the_view() {
    let mut harness = window(demo_app());
    assert_eq!(harness.state().view(), View::Session);
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
    assert!(harness.query_by_label("Clip Beat B on Drums at bar 17").is_some());
    harness.get_by_label("Session view").click();
    harness.run();
    assert_eq!(harness.state().view(), View::Session);
}

fn arrangement_app() -> Harness<'static, App> {
    let mut harness = window(demo_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

#[test]
fn dragging_a_clip_moves_it_on_the_bar_grid() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center();
    // 8 points a beat by default, so 64 points are two bars.
    let to = from + egui::vec2(64.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(30.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let starts: Vec<f64> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
        .unwrap_or_default();
    assert!(starts.contains(&72.0), "{starts:?}");
    assert!(!starts.contains(&64.0), "{starts:?}");
}

#[test]
fn moving_the_view_with_the_middle_button_over_a_clip_leaves_the_clip_alone() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect::<Vec<_>>())
    };
    let before = starts(&harness);
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center();
    let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.run();
    harness.input_mut().events.push(button(from, true));
    harness.run();
    for step in 1..=4 {
        harness.hover_at(from + egui::vec2(-20.0 * step as f32, 0.0));
        harness.run();
    }
    let to = from + egui::vec2(-80.0, 0.0);
    harness.input_mut().events.push(button(to, false));
    harness.run();
    assert_eq!(starts(&harness), before);
}

/// Drags the clip labelled `label` by `by` points in the Arrangement view.
fn drag_clip(harness: &mut Harness<'static, App>, label: &str, by: egui::Vec2) {
    let from = harness.get_by_label(label).rect().center();
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

#[test]
fn a_dropped_clip_replaces_what_it_covers_unless_midi_stacking_is_on() {
    let starts = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
    };
    // Beat B (bars 17-28) moves two bars later, over Fill (bars 29-32), which is shortened.
    let mut harness = arrangement_app();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(120.0, 8.0))), Some(true));

    let mut harness = arrangement_app();
    let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(112.0, 16.0))), Some(true));
}

#[test]
fn dragging_a_clip_down_moves_it_to_the_next_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center();
    // Lanes are 48 points high with a line between them.
    let to = from + egui::vec2(0.0, 49.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(0.0, 20.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let project = harness.state().session().map(|s| s.project().clone()).unwrap_or_default();
    let drums: Vec<f64> = project.arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    let bass: Vec<f64> = project.arrangement.lanes[1].clips.iter().map(|c| c.start).collect();
    assert!(!drums.contains(&64.0), "{drums:?}");
    assert!(bass.contains(&64.0), "{bass:?}");
}

/// The demo project in the Arrangement view with `renderer` (stopped, made from the demo
/// project) as its player, driven by the test instead of an audio device.
fn arrangement_app_with_engine(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// Renders a block, so the engine takes the commands sent so far.
fn render_a_block(renderer: &mut suhina_engine::Renderer) {
    renderer.process(&mut [0.0f32; 2 * 64]);
}

#[test]
fn a_click_on_the_ruler_moves_the_play_start_marker_on_the_grid() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let ruler = harness.get_by_label("Ruler").rect();
    // 8 points a beat by default, so the grid is a bar: 130 points in is beat 16.25, bar 5.
    let at = egui::pos2(ruler.left() + 130.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(monitor.start_beat(), 16.0);
    assert!(!monitor.running());

    // Play starts there; Stop goes back to it, as the setting says by default.
    harness.get_by_label("Play").click();
    harness.run();
    render_a_block(&mut renderer);
    assert!(monitor.running() && monitor.beat() >= 16.0);
    for _ in 0..400 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat(), monitor.beat()), (false, 16.0, 16.0));

    // With Stop set to stay, the marker moves to where it stopped.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    harness.get_by_label("Stop: Stay").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.stop),
        Some(suhina_ui::settings::StopReturn::Here)
    );
    harness.get_by_label("Close settings").click();
    harness.run();
    harness.get_by_label("Play").click();
    harness.run();
    for _ in 0..375 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    // 375 blocks of 64 frames at 120 bpm are one beat, and the blocks around Play and Stop.
    assert!((monitor.start_beat() - 17.0).abs() < 0.03, "{}", monitor.start_beat());
}

#[test]
fn arrangement_headers_turn_tracks_off() {
    let mut harness = arrangement_app();
    harness.get_by_label("Turn Keys off").click();
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().channels[2].mute), Some(true));
    assert!(harness.query_by_label("Keys volume").is_some());
    assert!(harness.query_by_label("Keys pan").is_some());
}

#[test]
fn ctrl_and_plus_or_minus_zoom_the_whole_window_and_the_zoom_is_saved() {
    let mut harness = window(demo_app());
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Plus);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(120));
    assert!((harness.ctx.zoom_factor() - 1.2).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    harness.run();
    assert_eq!(harness.state().settings().zoom, 110);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn a_saved_zoom_is_used_from_the_start_and_settings_can_change_it() {
    let mut app = demo_app();
    let ctx = egui::Context::default();
    let settings = suhina_ui::settings::Settings { zoom: 150, ..Default::default() };
    app.set_settings(&ctx, settings);
    let mut harness = window(app);
    assert!((harness.ctx.zoom_factor() - 1.5).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Interface").click();
    harness.run();
    harness.get_by_label("Zoom: -").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(140));
    harness.get_by_label("Zoom: 140 %").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn settings_change_right_away_and_are_handed_out_to_be_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    harness.get_by_label("Settings page: Interface").click();
    harness.run();
    harness.get_by_label("Animations: Off").click();
    harness.run();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.get_by_label("Track headers: Left").click();
    harness.run();
    let saved = harness.state_mut().take_changed_settings();
    assert_eq!(saved.as_ref().map(|s| s.animations), Some(false));
    assert_eq!(saved.map(|s| s.track_headers), Some(suhina_ui::settings::Side::Left));
    assert!(!suhina_ui::motion::enabled(&harness.ctx));
    assert_eq!(harness.state_mut().take_changed_settings(), None);
    harness.get_by_label("Close settings").click();
    harness.run();
    assert!(!harness.state().settings_open());
}

#[test]
fn a_theme_picked_in_settings_colours_the_window_and_is_saved() {
    let mut harness = window(demo_app());
    let mut colors = theme::Palette::DARK;
    colors.accent = egui::Color32::from_rgb(0x12, 0xAB, 0x9F);
    let night =
        suhina_ui::themes::ThemeEntry { id: "night".into(), theme: theme::Theme { name: "Night".into(), colors } };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, vec![night], Some("themes".into()));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    assert!(harness.query_by_label("Theme: Suhina Dark").is_some());
    harness.get_by_label("Theme: Night").click();
    harness.run();
    assert_eq!(theme::accent(), colors.accent);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.theme), Some("night".into()));

    // A copy to change becomes the theme in use, and is handed out to be written.
    harness.get_by_label("Duplicate theme").click();
    harness.run();
    let saved: Vec<_> = harness
        .state_mut()
        .take_theme_actions()
        .into_iter()
        .map(|a| match a {
            FileAction::Save { id, theme } => (id, theme.name),
            other => (format!("{other:?}"), String::new()),
        })
        .collect();
    assert_eq!(saved, [("night-copy".to_string(), "Night copy".to_string())]);
    assert_eq!(harness.state().settings().theme, "night-copy");
    for button in ["Reload themes", "Open the themes folder", "Back up themes", "Export the theme template"] {
        harness.get_by_label(button).click();
        harness.run();
    }
    assert_eq!(
        harness.state_mut().take_theme_actions(),
        [FileAction::Reload, FileAction::OpenFolder, FileAction::Backup, FileAction::ExportTemplate]
    );

    // A theme file dropped on the window is handed out to be brought in.
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile("dark blue.json".into())));
    harness.run();
    assert_eq!(harness.state_mut().take_theme_actions(), [FileAction::Import("dark blue.json".into())]);

    // A file saved elsewhere redraws the window.
    colors.accent = egui::Color32::from_rgb(1, 2, 3);
    let edited = suhina_ui::themes::ThemeEntry {
        id: "night-copy".into(),
        theme: theme::Theme { name: "Night copy".into(), colors },
    };
    harness.state_mut().put_theme(&ctx, edited);
    assert_eq!(theme::accent(), colors.accent);
}

#[test]
fn tab_only_switches_the_view() {
    let mut harness = window(demo_app());
    harness.run();
    for expected in [View::Arrangement, View::Session, View::Arrangement] {
        harness.key_press(Key::Tab);
        harness.run();
        assert_eq!(harness.state().view(), expected);
        // No widget took the keyboard, so the next Tab is a shortcut again.
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
    for key in [Key::ArrowDown, Key::ArrowRight] {
        harness.key_press(key);
        harness.run();
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
}

#[test]
fn ctrl_t_asks_for_a_new_project_and_ctrl_s_saves_one() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-save-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    let mut harness = window(app);

    // Ctrl+T: the dialog, which asks for its lists; a blank project is handed out to be made.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    harness.run();
    assert!(harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_lists_wanted(), Some(dir.join("Templates")));
    harness.get_by_label("New blank project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));
    assert!(!harness.state().project_dialog_open());

    // Ctrl+S on a project without a .suhina file asks for a name, in the projects folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Night drive");
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    let saved = dir.join("Projects").join("Night drive.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert_eq!(harness.state_mut().take_saved_path(), Some(saved.clone()));
    let session = harness.state().session();
    assert_eq!(session.and_then(|s| s.source()), Some(saved.as_path()));

    // Saved again without asking; a template is a copy in the templates folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    assert!(harness.query_by_label("Name").is_none(), "asked again");
    harness.state_mut().do_action("file.saveTemplate", &mut Vec::new());
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    assert!(dir.join("Templates").join("Night drive.suhina").is_file());
    assert_eq!(harness.state().session().and_then(|s| s.source()), Some(saved.as_path()));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn solo_is_exclusive_unless_ctrl_is_held() {
    let mut harness = window(demo_app());
    let solos = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels.iter().take(4).map(|c| c.solo).collect::<Vec<_>>())
    };
    harness.get_by_label("Solo Drums").click();
    harness.run();
    harness.get_by_label("Solo Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, false]));
    harness.get_by_label("Solo Vox").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, true]));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("End solo on Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, false, true]));
}

/// The demo project with a group track, "Group", that tracks can play into.
fn project_with_a_group() -> suhina_engine::model::Project {
    use suhina_engine::model::{Channel, ChannelId, ChannelRole, ContentKinds, Track, TrackId};
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(20),
        name: "Group".into(),
        content: ContentKinds::default(),
        channel: Some(ChannelId(120)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(120),
        name: "Group".into(),
        role: ChannelRole::Submix,
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    project
}

#[test]
fn a_tall_header_routes_its_track_into_a_group() {
    struct Arrangement {
        timeline: Timeline,
        state: ArrangementViewState,
        actions: Vec<Action>,
    }
    let mut state = ArrangementViewState::default();
    state.set_lane_height(suhina_engine::model::TrackId(2), 90.0);
    let view = Arrangement { timeline: Timeline::build(&project_with_a_group(), None), state, actions: Vec::new() };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut Arrangement| arrangement_view::show(ui, &v.timeline, &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    // Only the tall lane shows its output.
    assert_eq!(harness.query_all_by_label_contains("output:").count(), 1);
    harness.get_by_label("Bass output: Main").click();
    harness.run();
    harness.get_by_label("Group").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("track.setOutput", json!({ "track": 2, "to": 20 }))]);
}

fn names(h: &Harness<'static, App>) -> Vec<String> {
    h.state().session().map(|s| s.project().tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default()
}

#[test]
fn selected_tracks_group_with_ctrl_g() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Drums", "Bass"]);
    assert!(harness.query_by_label("Track Group").is_some());
}

#[test]
fn menus_delete_a_track_and_duplicate_a_clip() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label("Delete track").click();
    harness.run();
    assert!(!names(&harness).contains(&"Keys".to_string()));

    let clips =
        |h: &Harness<'static, App>| h.state().session().map_or(0, |s| s.project().arrangement.lanes[0].clips.len());
    let before = clips(&harness);
    harness.get_by_label("Clip Fill on Drums at bar 29").click_secondary();
    harness.run();
    harness.get_by_label_contains("Duplicate").click();
    harness.run();
    // The copy sits after Fill and shortens the clip it lands on, so there is one more clip.
    assert_eq!(clips(&harness), before + 1);
    let fill_copies = harness.state().session().map_or(0, |s| {
        s.project().arrangement.lanes[0].clips.iter().filter(|c| c.start == 128.0 && c.length == 16.0).count()
    });
    assert_eq!(fill_copies, 1, "the copy of Fill sits at bar 33");
}

#[test]
fn dragging_a_header_moves_the_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three lanes up (49 points each), into the upper half of Drums.
    let to = from - egui::vec2(0.0, 3.0 * 49.0 + 14.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(0.0, 40.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

/// The demo project with an audio track, "Loop", whose clip plays `loop.wav` (two seconds of a
/// tone that fades out), and the waveforms of that file.
fn project_with_audio() -> (suhina_engine::model::Project, suhina_ui::waveform::Waveforms) {
    use suhina_engine::model::*;
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(30),
        name: "Loop".into(),
        content: ContentKinds { notes: false, audio: true },
        channel: Some(ChannelId(130)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(130),
        name: "Loop".into(),
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    let region = AudioRegion {
        file: "loop.wav".into(),
        absolute_file: None,
        sample_rate: Some(8000),
        channels: Some(1),
        duration_seconds: Some(2.0),
        warped: true,
        stretch: StretchMode::default(),
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 4.0, audio_time: 2.0 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    project.contents.push(ClipContent {
        id: ContentId(300),
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
    });
    let clip = Clip {
        content: ContentId(300),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        loop_range: Some(LoopRange { start: 0.0, end: 4.0 }),
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        launch: Default::default(),
        extensions: Extensions::default(),
    };
    project.arrangement.lanes.push(ArrangementLane {
        name: "Loop".into(),
        track: Some(TrackId(30)),
        clips: vec![clip],
    });
    let samples: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.3).sin() * (1.0 - i as f32 / 16000.0)).collect();
    let mut pool = suhina_engine::AudioPool::new();
    pool.files.insert(
        "loop.wav".into(),
        std::sync::Arc::new(suhina_engine::AudioData { sample_rate: 8000, channels: 1, samples }),
    );
    (project, suhina_ui::waveform::from_pool(&pool))
}

fn audio_app() -> App {
    let (project, waveforms) = project_with_audio();
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    app
}

#[test]
fn dragging_an_audio_clips_gain_sets_it() {
    let mut harness = window(audio_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let field = harness.get_by_label("Gain of Loop on Loop").rect().center();
    harness.hover_at(field);
    harness.run();
    harness.drag_at(field);
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 15.0));
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    harness.drop_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    let gain = harness.state().session().and_then(|s| {
        s.project().content(suhina_engine::model::ContentId(300)).and_then(|c| match &c.lanes[0].data {
            suhina_engine::model::LaneData::Audio(region) => Some(region.gain),
            suhina_engine::model::LaneData::Notes(_) => None,
        })
    });
    // 30 points up at 0.1 dB a point: +3 dB.
    assert!(gain.is_some_and(|g| (g - 1.4125).abs() < 1e-3), "{gain:?}");
}

/// The Loop clip's fade-in and fade-out in the project.
fn loop_fades(harness: &Harness<'static, App>) -> Option<(f64, f64)> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| (clip.fade_in, clip.fade_out))
}

/// Drags the control labelled `label` sideways by `dx` points.
fn drag_sideways(harness: &mut Harness<'static, App>, label: &str, dx: f32) {
    let from = harness.get_by_label(label).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(dx / 2.0, 0.0));
    harness.run();
    harness.hover_at(from + egui::vec2(dx, 0.0));
    harness.run();
    harness.drop_at(from + egui::vec2(dx, 0.0));
    harness.run();
}

#[test]
fn dragging_a_fade_handle_sets_the_fade_on_the_grid_and_a_double_click_removes_it() {
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // The handles show on the selected clip.
    assert!(harness.query_by_label("Fade-in of Loop on Loop").is_none());
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();

    // 8 points a beat and a bar grid at this zoom: 3 beats right ends the fade on bar 2.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));
    // The fade-out, 3 beats left from the clip's end at beat 16: on bar 4, 4 beats long.
    drag_sideways(&mut harness, "Fade-out of Loop on Loop", -24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 4.0)));
    // Each drag is one undo step.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));

    // A fade cannot reach past the other one: the fade-in stops where the clip has room.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 200.0);
    assert_eq!(loop_fades(&harness), Some((12.0, 4.0)));

    // A double-click removes the fade.
    let handle = harness.get_by_label("Fade-in of Loop on Loop").rect().center();
    harness.hover_at(handle);
    harness.run();
    // Press and release in one frame each, a few frames apart: a double-click.
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fades(&harness), Some((0.0, 4.0)));
}

/// The Loop clip's fade-in shape and curve in the project.
fn loop_fade_in_curve(harness: &Harness<'static, App>) -> Option<suhina_engine::model::FadeCurve> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| clip.fade_in_curve)
}

/// A right-click on a fade handle picks the shape; the curve handle on the fade's line bends it
/// with a vertical drag and a double-click straightens it (#197).
#[test]
fn a_fades_shape_is_picked_from_its_menu_and_its_curve_dragged() {
    use suhina_engine::model::{FadeCurve, FadeShape};
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();
    // A 4-beat fade-in, 32 points wide: wide enough for a curve handle.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve::default()));

    harness.get_by_label("Fade-in of Loop on Loop").click_secondary();
    harness.run();
    harness.get_by_label("Equal power").click();
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));

    // Down by 12 points: the handle follows the pointer, and the curve bends towards −.
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect();
    let from = handle.center();
    let to = from + egui::vec2(0.0, 12.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from.lerp(to, 0.5));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let bent = loop_fade_in_curve(&harness).map(|c| c.curve).unwrap_or_default();
    assert!(bent < -0.05 && bent > -1.0, "{bent}");
    let now = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    assert!((now.y - to.y).abs() < 1.0, "the handle is at {now:?}, the pointer let go at {to:?}");
    assert_eq!(loop_fade_in_curve(&harness).map(|c| c.shape), Some(FadeShape::EqualPower));

    // A double-click sets the curve back to 0 (half a second after the drag, as a person would).
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    for _ in 0..30 {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));
}

fn two_projects() -> Harness<'static, App> {
    let mut app = App::new();
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    window(app)
}

#[test]
fn tabs_switch_keep_each_projects_edits_and_close() {
    let mut harness = two_projects();
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());

    // Turn a track off in the second project, then show the first: it is untouched.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(harness.state_mut().take_player_request(), ids.first().copied());
    assert!(harness.query_by_label("Turn Bass off").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    assert!(harness.query_by_label("Turn Bass on").is_some());

    // It has an edit, so closing it asks first.
    harness.get_by_label("Close Untitled 2").click();
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().len(), 1);
    assert!(harness.query_by_label("Tab Untitled 1").is_none(), "one project shows no tabs");
}

#[test]
fn opening_while_a_project_is_open_asks_and_can_remember() {
    let mut harness = window(demo_app());
    harness.state_mut().request_open("other.als".into());
    harness.run();
    assert!(harness.state_mut().take_open_request().is_none());
    harness.get_by_label("Remember").click();
    harness.run();
    harness.get_by_label("In a new tab").click();
    harness.run();
    let request = harness.state_mut().take_open_request();
    assert_eq!(request, Some(("other.als".into(), OpenAs::NewTab)));
    assert_eq!(harness.state().settings().on_open, suhina_ui::settings::OnOpen::NewTab);

    // Remembered: the next one opens in a new tab without asking.
    harness.state_mut().request_open("third.als".into());
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some(("third.als".into(), OpenAs::NewTab)));
}

#[test]
fn settings_add_and_remove_plugin_folders_and_scan_in_the_background() {
    let base = std::env::temp_dir().join(format!("suhina-ui-plugins-{}", std::process::id()));
    let folder = base.join("my plug-ins");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let library = suhina_plugins::library::Library::new(Some(base.join("settings")), base.join("no-such-host"))
        .without_standard_folders();
    let mut app = App::with_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Plug-ins").click();
    harness.run();

    let field = harness.get_by_label("Plug-in folder to add");
    field.focus();
    field.type_text(&folder.display().to_string());
    harness.run();
    harness.get_by_label("Add plug-in folder").click();
    harness.run();
    harness.run();
    let remove = harness.query_all_by_label_contains("Remove ").count();
    assert_eq!(remove, 1, "the added folder is listed with a Remove button");

    harness.get_by_label("Scan").click();
    // The window keeps redrawing while the scan runs, so it is stepped, not run until still.
    harness.step();
    for _ in 0..200 {
        if !harness.state().scanning() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("plug-in host"), "{message}"),
        other => panic!("expected the missing host to be reported, got {other:?}"),
    }

    harness.get_by_label_contains("Remove ").click();
    harness.run();
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Remove ").count(), 0);
    let _cleanup = std::fs::remove_dir_all(&base);
}

#[test]
fn a_key_set_in_settings_runs_its_action_and_is_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Shortcuts").click();
    harness.run();
    harness.get_by_label("Change keys of view.arrangement").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().keymap().label("view.arrangement").as_deref(), Some("A"));
    assert!(harness.state_mut().take_changed_keymap().is_some());

    harness.get_by_label("Close settings").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
}

#[test]
fn keys_copy_paste_and_delete_the_selected_clip() {
    let mut harness = arrangement_app();
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    // The copy lands right after Fill (bar 33) and shortens what it covers.
    assert!(spans(&harness).contains(&(128.0, 16.0)), "{:?}", spans(&harness));

    let before = spans(&harness).len();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(spans(&harness).len(), before - 1);
    assert!(!spans(&harness).contains(&(0.0, 64.0)));
}

#[test]
fn paste_and_duplicate_keys_follow_the_midi_stacking_setting() {
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    for key in [Key::V, Key::D] {
        let mut harness = arrangement_app();
        let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
        let ctx = harness.ctx.clone();
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        let before = spans(&harness);
        harness.get_by_label("Clip Beat B on Drums at bar 17").click();
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, key);
        harness.run();
        // The copy of Beat B (bars 17-28) lies over Fill (bars 29-32), which keeps its span.
        let after = spans(&harness);
        assert!(after.contains(&(112.0, 48.0)), "{key:?}: {after:?}");
        assert_eq!(after.len(), before.len() + 1, "{key:?}: {after:?}");
        assert!(before.iter().all(|s| after.contains(s)), "{key:?}: {before:?} -> {after:?}");
    }
}

/// The demo project with a utility device on Drums.
fn project_with_a_device() -> suhina_engine::model::Project {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5), DeviceParam::new("width", 1.2)],
        extensions: Extensions::default(),
    });
    project
}

#[test]
fn the_detail_area_shows_the_selected_tracks_devices_in_a_row_or_stacked() {
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    assert!(harness.query_by_label("Device Utility").is_none());
    harness.get_by_label("Track Drums").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some());

    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some());

    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_none(), "Shift+Tab hides the detail area");
}

#[test]
fn dragging_a_session_column_header_moves_the_track() {
    let mut harness = window(demo_app());
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three columns left (117 points each), into the left half of Drums.
    let to = from - egui::vec2(3.0 * 117.0 + 30.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(60.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

/// Renders the window with the demo project to `target/ui-snapshots/session.png` and
/// `arrangement.png`, to compare with the mockups in `docs/design/reference/`. Needs a GPU adapter, so it runs only
/// when asked: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_session_window() {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    image.save(folder.join("session.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.state_mut().set_project_lists(
        vec!["C:/Music/Suhina/Projects/Night drive.suhina".into(), "C:/Music/Sets/Old jam.als".into()],
        vec!["C:/Music/Suhina/Templates/Beat starter.suhina".into(), "C:/Music/Suhina/Templates/Live rig.als".into()],
    );
    harness.state_mut().open_project_dialog(false);
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("project-dialog.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.state_mut().do_action("file.saveAs", &mut Vec::new());
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("save-prompt.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-theme.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-arrangement.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Shortcuts").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("shortcuts.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: General").click();
    harness.run();

    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), audio_app());
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement-audio.png")).unwrap_or_else(|e| panic!("{e}"));

    // The detail area with a track's devices, in a row and stacked.
    let mut harness = Harness::builder().with_size(SIZE).with_pixels_per_point(2.0).wgpu().build_ui_state(
        |ui, app: &mut App| app.ui(ui),
        {
            let mut app = App::new();
            app.set_session(Session::new(project_with_a_device()), &[]);
            app
        },
    );
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-row.png")).unwrap_or_else(|e| panic!("{e}"));
    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-stack.png")).unwrap_or_else(|e| panic!("{e}"));

    // A Live Set dragged over the window darkens it.
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some("set.als".into()), ..Default::default() });
    harness.run_steps(12);
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("drop.png")).unwrap_or_else(|e| panic!("{e}"));
}

/// Renders the Session and Arrangement views in every theme that comes with Suhina, to
/// `target/ui-snapshots/theme-<id>-session.png` and `-arrangement.png`, to look them over. Needs
/// a GPU adapter: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_every_built_in_theme() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for (id, _) in theme::built_in_themes() {
        let mut harness =
            Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        harness.state_mut().use_theme(&ctx, id);
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-session.png"))).unwrap_or_else(|e| panic!("{e}"));
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-arrangement.png"))).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// An audio system with one device on one host, whose players run nowhere.
struct StandAudio;

impl suhina_session::AudioSystem for StandAudio {
    fn hosts(&self) -> Vec<suhina_session::audio::HostInfo> {
        use suhina_session::audio::{DeviceInfo, HostInfo};
        vec![HostInfo {
            name: "Stand".into(),
            default: true,
            devices: vec![DeviceInfo {
                name: "Stand Out".into(),
                default: true,
                channels: 2,
                sample_rates: vec![48_000],
                default_sample_rate: Some(48_000),
                buffer_sizes: vec![256, 512],
                buffer_range: Some((256, 512)),
                exclusive: false,
            }],
            error: None,
        }]
    }

    fn open(
        &self,
        project: &suhina_engine::model::Project,
        audio: &suhina_engine::AudioPool,
        _choice: &suhina_session::OutputChoice,
        _previous: Option<suhina_session::Player>,
    ) -> Result<suhina_session::audio::Opened, String> {
        let mut renderer = suhina_engine::Renderer::stopped(project, audio, 48_000).map_err(|e| e.to_string())?;
        let player = suhina_session::Player::new(renderer.controller(), None);
        Ok(suhina_session::audio::Opened { player, notice: None, plugins_kept: true })
    }
}

#[test]
fn settings_audio_lists_the_devices_and_a_buffer_picked_is_chosen_and_saved() {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio));
    app.projects_mut().open_output(None).unwrap_or_else(|e| panic!("{e}"));
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Audio").click();
    // The page redraws while it shows the latency, so it is stepped, not run until still.
    for _ in 0..3 {
        harness.step();
    }
    assert!(harness.query_by_label("Driver: Default (Stand)").is_some());
    assert!(harness.query_by_label("Device: Default (Stand Out)").is_some());
    harness.get_by_label("Buffer size: Default (1024 frames)").click();
    harness.step();
    harness.step();
    harness.get_by_label("512 frames (10.7 ms)").click();
    harness.step();
    harness.step();
    assert_eq!(harness.state().projects().output_choice().buffer_frames, Some(512));
    let saved = harness.state_mut().take_changed_settings().map(|s| s.audio_buffer);
    assert_eq!(saved, Some(512));
    assert!(harness.query_by_label("Buffer size: 512 frames (10.7 ms)").is_some());
}

#[test]
fn the_control_bar_shows_the_cpu_load_only_while_an_engine_plays() {
    // With an engine attached as its player, the control bar shows the load.
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load 0.0%").is_some());

    // Without one, there is nothing to measure and nothing shown.
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load").is_none());
}

/// The label of the active project's Close button in the tab row.
fn active_close_label(harness: &Harness<'static, App>) -> String {
    let projects = harness.state().projects();
    let name = projects.active().and_then(|id| projects.name(id)).unwrap_or_default();
    format!("Close {name}")
}

/// Whether the window asked to close itself in its last frame.
fn asked_to_close(harness: &Harness<'static, App>) -> bool {
    harness.output().viewport_output.values().any(|v| v.commands.contains(&egui::ViewportCommand::Close))
}

#[test]
fn closing_or_quitting_with_unsaved_changes_asks_first() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-unsaved-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let open = |h: &Harness<'static, App>| h.state().projects().ids().len();
    let unsaved = |h: &Harness<'static, App>| h.state().session().is_some_and(Session::has_unsaved_changes);
    assert_eq!(open(&harness), 2);

    // An edit in the second project; closing its tab asks first, and Cancel keeps it open.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert!(unsaved(&harness));
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    assert!(harness.query_by_label("Don't save the changes").is_some());
    harness.get_by_label("Cancel and keep the project open").click();
    harness.run();
    assert_eq!(open(&harness), 2);
    assert!(harness.query_by_label("Don't save the changes").is_none());

    // Don't save closes it.
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    assert_eq!(open(&harness), 1);

    // A project without changes closes without asking; quitting with one that has them asks,
    // and Save first asks for a name, saves, then lets the window close.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    harness.run();
    assert!(!asked_to_close(&harness));
    assert!(harness.state().unsaved_prompt_open());
    harness.get_by_label("Save the changes").click();
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Before quitting");
    harness.run();
    harness.get_by_label("Save").click();
    harness.step();
    harness.step();
    let saved = dir.join("Projects").join("Before quitting.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert!(!unsaved(&harness));
    assert!(!harness.state().unsaved_prompt_open());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_sample_map_opens_in_a_window_of_its_own_and_docks_back() {
    let mut harness = window(demo_app());
    // Hiding the browser for the test changed a setting already.
    let _ = harness.state_mut().take_changed_settings();
    assert!(harness.query_by_label("Dock").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::M);
    harness.run();
    // Without windows of their own (as here) the toolkit draws it as a window inside this one.
    assert!(harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(true));
    assert!(harness.query_by_label("Dock").is_some(), "the map's bar offers to dock it");
    assert!(harness.query_by_label("Large").is_none(), "no large view while it has a window");
    harness.get_by_label("Dock").click();
    harness.run();
    assert!(!harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
    assert!(harness.query_by_label("Dock").is_none());

    // The View menu does the same.
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label_contains("Sample map in a window").click();
    harness.run();
    assert!(harness.state().sample_map.view.window);
}

#[test]
fn a_saved_map_window_opens_at_the_start() {
    let mut app = demo_app();
    let settings = suhina_ui::settings::Settings {
        map_window: true,
        map_window_rect: Some([100, 80, 700, 500]),
        browser_open: false,
        ..Default::default()
    };
    app.set_settings(&egui::Context::default(), settings);
    let mut harness = window(app);
    assert!(harness.state().sample_map.view.window);
    assert!(harness.query_by_label("Dock").is_some());
    // The large view takes the map out of its window.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    harness.run();
    assert!(!harness.state().sample_map.view.window);
    assert!(harness.state().sample_map.view.large);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
}

/// Renders the Arrangement view with an audio clip, linear and in decibels, to
/// `target/ui-snapshots/arrangement-waveform*.png`, to look at the clip waveform in the window:
/// `cargo test -p suhina-ui --test window -- --ignored render_the_clip_waveform`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_clip_waveform() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for db in [false, true] {
        let mut app = audio_app();
        app.set_browser_open(false);
        let mut harness = Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        let mut settings = harness.state().settings().clone();
        settings.waveform_db = db;
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        let name = if db { "arrangement-waveform-db.png" } else { "arrangement-waveform.png" };
        image.save(folder.join(name)).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn settings_open_as_a_dialog_over_the_view_and_escape_closes_them() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    assert!(harness.query_by_label("Close settings").is_some());
    // The view stays behind the dialog.
    assert!(harness.query_by_label("Turn Bass off").is_some(), "the view is still drawn");
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
}

/// The wheel and pinch over an open dialog reach only the dialog: the view behind it keeps its
/// scroll and zoom (#232).
#[test]
fn the_wheel_over_open_settings_leaves_the_arrangement_alone() {
    let mut harness = arrangement_app();
    let clip = |h: &Harness<'static, App>| h.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let before = clip(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    // Over the dialog's backdrop, where the clip is, and over the dialog itself.
    for at in [before.center(), egui::pos2(SIZE.0 / 2.0, SIZE.1 / 2.0)] {
        harness.hover_at(at);
        harness.run();
        for modifiers in [Modifiers::NONE, Modifiers::COMMAND, Modifiers::ALT] {
            harness.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-120.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers,
            });
            harness.input_mut().events.push(egui::Event::Zoom(1.5));
            harness.run();
        }
    }
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
    assert_eq!(clip(&harness), before, "the arrangement neither scrolled nor zoomed");
}
