//! The Session view and the window, driven like a user would: clicks become the commands the
//! CLI and MCP run, and edits change the project through the session.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use loopwerk_session::Session;
use loopwerk_ui::arrangement_view::{self, ArrangementViewState};
use loopwerk_ui::demo::demo_project;
use loopwerk_ui::grid::Grid;
use loopwerk_ui::session_view::{self, SessionViewState};
use loopwerk_ui::timeline::Timeline;
use loopwerk_ui::widgets::Action;
use loopwerk_ui::{App, OpenAs, Status, View, theme};
use serde_json::json;

const SIZE: (f32, f32) = (1280.0, 800.0);

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
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn demo_app() -> App {
    let mut app = App::new();
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
    let settings = loopwerk_ui::settings::Settings { midi_stacking: true, ..Default::default() };
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
fn settings_change_right_away_and_are_handed_out_to_be_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    harness.get_by_label("Animations: Off").click();
    harness.run();
    harness.get_by_label("Track headers: Left").click();
    harness.run();
    let saved = harness.state_mut().take_changed_settings();
    assert_eq!(saved.as_ref().map(|s| s.animations), Some(false));
    assert_eq!(saved.map(|s| s.track_headers), Some(loopwerk_ui::settings::Side::Left));
    assert!(!loopwerk_ui::motion::enabled(&harness.ctx));
    assert_eq!(harness.state_mut().take_changed_settings(), None);
    harness.get_by_label("Close settings").click();
    harness.run();
    assert!(!harness.state().settings_open());
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
fn project_with_a_group() -> loopwerk_engine::model::Project {
    use loopwerk_engine::model::{Channel, ChannelId, ChannelRole, ContentKinds, Track, TrackId};
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
    state.set_lane_height(loopwerk_engine::model::TrackId(2), 90.0);
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
fn project_with_audio() -> (loopwerk_engine::model::Project, loopwerk_ui::waveform::Waveforms) {
    use loopwerk_engine::model::*;
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
        extensions: Extensions::default(),
    };
    project.arrangement.lanes.push(ArrangementLane {
        name: "Loop".into(),
        track: Some(TrackId(30)),
        clips: vec![clip],
    });
    let samples: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.3).sin() * (1.0 - i as f32 / 16000.0)).collect();
    let mut pool = loopwerk_engine::AudioPool::new();
    pool.files.insert(
        "loop.wav".into(),
        std::sync::Arc::new(loopwerk_engine::AudioData { sample_rate: 8000, channels: 1, samples }),
    );
    (project, loopwerk_ui::waveform::from_pool(&pool))
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
        s.project().content(loopwerk_engine::model::ContentId(300)).and_then(|c| match &c.lanes[0].data {
            loopwerk_engine::model::LaneData::Audio(region) => Some(region.gain),
            loopwerk_engine::model::LaneData::Notes(_) => None,
        })
    });
    // 30 points up at 0.1 dB a point: +3 dB.
    assert!(gain.is_some_and(|g| (g - 1.4125).abs() < 1e-3), "{gain:?}");
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

    harness.get_by_label("Close Untitled 2").click();
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
    assert_eq!(harness.state().settings().on_open, loopwerk_ui::settings::OnOpen::NewTab);

    // Remembered: the next one opens in a new tab without asking.
    harness.state_mut().request_open("third.als".into());
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some(("third.als".into(), OpenAs::NewTab)));
}

#[test]
fn settings_add_and_remove_plugin_folders_and_scan_in_the_background() {
    let base = std::env::temp_dir().join(format!("loopwerk-ui-plugins-{}", std::process::id()));
    let folder = base.join("my plug-ins");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let library = loopwerk_plugins::library::Library::new(Some(base.join("settings")), base.join("no-such-host"))
        .without_standard_folders();
    let mut app = App::with_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Plug-ins").click();
    harness.run();

    let field = harness.get_by_role(egui::accesskit::Role::TextInput);
    field.focus();
    field.type_text(&folder.display().to_string());
    harness.run();
    harness.get_by_label("Add folder").click();
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

/// Renders the window with the demo project to `target/ui-snapshots/session.png` and
/// `arrangement.png`, to compare with the mockups in `docs/design/reference/`. Needs a GPU adapter, so it runs only
/// when asked: `cargo test -p loopwerk-ui -- --ignored`.
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

    // A Live Set dragged over the window darkens it.
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some("set.als".into()), ..Default::default() });
    harness.run_steps(12);
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("drop.png")).unwrap_or_else(|e| panic!("{e}"));
}
