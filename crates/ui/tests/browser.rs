//! The browser panel in the window (`docs/spec/browser.md`), driven like a user would: search,
//! filters, places, the keyboard, and dragging samples, MIDI files and devices into the
//! project, which runs the same commands agents use.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{ChannelId, LaneData};
use suhina_library::Library;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

/// A file dropped on the window from the system in a test.
#[derive(Debug)]
struct TestFile(PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-browser-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path, seconds: f32) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let spec =
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * 8000.0) as usize {
        writer.write_sample(8000i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// A MIDI file with one note a beat long.
fn midi(path: &Path) {
    let mut file = b"MThd\0\0\0\x06\0\0\0\x01\0\x60".to_vec();
    let track = [0x00, 0x90, 60, 100, 0x60, 0x80, 60, 0, 0x00, 0xFF, 0x2F, 0x00];
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    std::fs::write(path, file).unwrap();
}

/// A sample pack: drums, a loop, a MIDI file and a project.
fn pack(root: &Path) {
    wav(&root.join("Drums").join("Kick Hard.wav"), 0.25);
    wav(&root.join("Drums").join("Snare.wav"), 0.25);
    wav(&root.join("Loop 120bpm.wav"), 2.0);
    midi(&root.join("Groove.mid"));
    std::fs::write(root.join("Song.suhina"), b"{}").unwrap();
}

/// The window with the demo project and a browser whose library is kept in `settings` and
/// holds `folder`.
fn window(settings: &Path, folder: &Path) -> Harness<'static, App> {
    let mut app = App::new();
    let mut library = Library::new(Some(settings.to_path_buf()));
    library.add_folder(folder).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// A plug-in catalog as a scan leaves one, in the settings' plug-in library: two effects of
/// one vendor and an instrument of another.
fn catalog(settings: &Path) {
    let text = serde_json::json!({
        "version": 2,
        "entries": [
            { "path": "/VST3/Alpha Verb.vst3", "modified": 1_760_000_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6477", "name": "Alpha Verb", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Reverb"], "replaces": [] }] },
            { "path": "/VST3/Alpha Delay.vst3", "modified": 1_760_100_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6476", "name": "Alpha Delay", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Delay"], "replaces": [] }] },
            { "path": "/CLAP/Zeta Synth.clap", "modified": 1_760_200_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6478", "name": "Zeta Synth", "vendor": "Zeta", "version": "2.0", "features": ["instrument", "synthesizer"], "replaces": [] }] },
        ],
    });
    std::fs::create_dir_all(settings.join("plugins")).unwrap();
    std::fs::write(settings.join("plugins").join("plugin-catalog.json"), text.to_string()).unwrap();
}

/// The window with the demo project, a browser whose library is in `settings`, and the plug-in
/// catalog [`catalog`] put in the same settings.
fn window_with_plugins(settings: &Path, folder: &Path) -> Harness<'static, App> {
    catalog(settings);
    let mut app = App::with_library(suhina_plugins::library::Library::new(
        Some(settings.join("plugins")),
        PathBuf::from("no-host"),
    ));
    let mut library = Library::new(Some(settings.to_path_buf()));
    library.add_folder(folder).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn row(name: &str) -> String {
    format!("Browser {name}")
}

/// The screen height of a row's centre, to compare two rows' order.
fn row_y(harness: &Harness<'static, App>, name: &str) -> f32 {
    harness.get_by_label(&row(name)).rect().center().y
}

/// Drags from `from` to `to` and lets go, a frame at a time.
fn drag(harness: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    // A drag repaints every frame, so the window is stepped rather than run to rest.
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.1, 0.3, 0.6, 1.0] {
        harness.hover_at(from + (to - from) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.step();
    harness.step();
}

/// Presses and lets go twice, a frame apart: a double-click.
fn double_click(harness: &mut Harness<'static, App>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    harness.hover_at(at);
    harness.run();
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// Presses and lets go once, at `pos`, a frame apart, without waiting: two in a row are a
/// double-click.
fn quick_click_at(harness: &mut Harness<'static, App>, pos: Pos2) {
    harness.hover_at(pos);
    harness.step();
    harness.drag_at(pos);
    harness.step();
    harness.drop_at(pos);
    harness.step();
}

/// Presses at `at` and lets go a frame later, like a mouse click.
fn click_at(harness: &mut Harness<'static, App>, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.step();
    harness.drop_at(at);
    harness.step();
    harness.run();
}

/// Where the chevron of a top-level row is drawn.
fn chevron_of(harness: &Harness<'static, App>, label: &str) -> Pos2 {
    let rect = harness.get_by_label(label).rect();
    Pos2::new(rect.left() + 9.0, rect.center().y)
}

/// Where the name of a top-level row is drawn.
fn title_of(harness: &Harness<'static, App>, label: &str) -> Pos2 {
    let rect = harness.get_by_label(label).rect();
    Pos2::new(rect.left() + 40.0, rect.center().y)
}

/// Drags `path` from the system over the window to `at` and drops it there, as the windowing
/// layer reports it: hovered files for a frame, then the dropped file.
fn system_drop(harness: &mut Harness<'static, App>, path: &Path, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some(path.into()), ..Default::default() });
    harness.step();
    // The windowing layer stops reporting them as hovered when they are dropped.
    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(path.into())));
    harness.step();
}

/// Steps the window until `done` holds (a drop decodes its samples on another thread).
fn until(harness: &mut Harness<'static, App>, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        harness.step();
        if done(harness.state()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_tree_lists_devices_and_folders_and_opens_them() {
    let (settings, folder) = (temp("tree-settings"), temp("tree"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    for heading in ["Instruments", "Effects", "Plug-ins"] {
        assert!(harness.query_by_label(&row(heading)).is_some(), "{heading}");
    }
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a heading opens with a click");
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a folder opens with a double-click");
    double_click(&mut harness, &row("Drums"));
    harness.run();
    assert!(harness.query_by_label(&row("Kick Hard.wav")).is_some());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_chevron_opens_it_once_and_a_second_closes_it() {
    let (settings, folder) = (temp("heading-chevron-settings"), temp("heading-chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = chevron_of(&harness, &row("Effects"));
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "the section stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_none(), "a second click closes it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_title_opens_it_once_and_a_second_closes_it() {
    let (settings, folder) = (temp("heading-title-settings"), temp("heading-title"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = title_of(&harness, &row("Effects"));
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "the section stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_none(), "a second click closes it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_folders_chevron_opens_it_and_on_its_title_only_selects_it() {
    let (settings, folder) = (temp("folder-chevron-settings"), temp("folder-chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = row(&folder.file_name().unwrap().to_string_lossy());
    let title = title_of(&harness, &name);
    click_at(&mut harness, title);
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a click on the title does not open it");
    assert_eq!(harness.state().browser.selected().len(), 1, "it selects it");
    let at = chevron_of(&harness, &name);
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "the folder stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a second click closes it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_double_click_on_a_folders_chevron_opens_it_and_stays_open() {
    let (settings, folder) = (temp("chevron-settings"), temp("chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // Clicking the chevron of the library folder twice in quick succession — the second
    // click inside egui's double-click window (0.3 s, 6 pt) — opens it once and leaves it
    // open, instead of opening and closing it again (#293).
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    let at = harness.get_by_label(&row(&name)).rect();
    let chevron = Pos2::new(at.left() + 8.0, at.center().y);
    quick_click_at(&mut harness, chevron);
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a double-click on the chevron opens the folder");
    // Past the double-click window, a click on the chevron closes it again: every distinct
    // click toggles.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a click after the window closes it");
    // A double-click on the row itself still opens it.
    double_click(&mut harness, &row(&name));
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a double-click on the folder opens it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_chevron_opens_it_and_stays_open() {
    let (settings, folder) = (temp("heading-chevron-stays-settings"), temp("heading-chevron-stays"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = harness.get_by_label(&row("Instruments")).rect();
    let chevron = Pos2::new(at.left() + 8.0, at.center().y);
    // A single click on a heading's chevron opens it and it stays open: the press and the
    // release of the click must not each toggle it (#293).
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a click on the heading's chevron opens it");
    // Past the double-click window (eight frames is 0.4 s, the window is 0.3 s), the next
    // click closes it again.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_none(), "a click after the window closes it");
    // A rapid double-click on the chevron opens it once: the second click of the pair is a
    // double and does not toggle again.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a double-click on the chevron opens it once");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn search_and_the_category_filters_narrow_the_results() {
    let (settings, folder) = (temp("search-settings"), temp("search"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "kick".into();
    harness.run();
    assert!(harness.query_by_label(&row("Kick Hard.wav")).is_some());
    assert!(harness.query_by_label(&row("Snare.wav")).is_none());

    harness.state_mut().browser.search.clear();
    harness.get_by_label("Show MIDI").click();
    harness.run();
    harness.state_mut().browser.search = "o".into();
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some());
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_none(), "MIDI only");
    harness.get_by_label("Show Instruments").click();
    harness.run();
    harness.state_mut().browser.search = "synth".into();
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some());
    assert!(harness.query_by_label(&row("Overdrive")).is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn ctrl_f_searches_ctrl_b_hides_and_rows_never_take_the_keyboard() {
    let (settings, folder) = (temp("keys-settings"), temp("keys"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let focused = harness.ctx.memory(|m| m.focused());
    assert_eq!(focused, None, "a click on a row leaves the keyboard to the shortcuts");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    harness.run();
    assert!(
        harness.ctx.memory(|m| m.has_focus(egui::Id::new("browser-search"))),
        "Ctrl+F puts the cursor in the search"
    );
    // Typing goes to the field, and Down selects the first result.
    harness.input_mut().events.push(egui::Event::Text("snare".into()));
    harness.run();
    assert_eq!(harness.state().browser.search, "snare");
    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(harness.state().browser.selected().first().map(|s| s.name.as_str()), Some("Snare.wav"));
    // Escape clears the search, a second Escape gives the keyboard back.
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.state().browser.search.is_empty());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.ctx.memory(|m| m.has_focus(egui::Id::new("browser-search"))));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    harness.run();
    assert!(!harness.state().settings().browser_open);
    assert!(harness.query_by_label(&row("Instruments")).is_none());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    harness.run();
    assert!(harness.state().settings().browser_open);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dragged_onto_a_slot_becomes_a_clip_there() {
    let (settings, folder) = (temp("slot-settings"), temp("slot"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "groove".into();
    harness.run();
    let from = harness.get_by_label(&row("Groove.mid")).rect().center();
    let to = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    drag(&mut harness, from, to);
    let keys_clip = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.launcher.columns.iter().find(|c| Some(c.track) == keys).and_then(|c| c.slots[0].clip.as_ref()).is_some()
        })
    };
    assert!(until(&mut harness, keys_clip), "the clip is in Keys' first slot");
    assert!(harness.state().browser.drag.is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dropped_from_the_system_onto_a_slot_becomes_a_clip_there() {
    let (settings, folder) = (temp("system-slot-settings"), temp("system-slot"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let to = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    system_drop(&mut harness, &folder.join("Groove.mid"), to);
    let keys_clip = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.launcher.columns.iter().find(|c| Some(c.track) == keys).and_then(|c| c.slots[0].clip.as_ref()).is_some()
        })
    };
    assert!(until(&mut harness, keys_clip), "the clip is in Keys' first slot");
    assert!(!matches!(harness.state().status(), suhina_ui::Status::Error(_)), "no error");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_sample_dropped_from_the_system_onto_the_empty_space_makes_an_audio_track() {
    let (settings, folder) = (temp("system-empty-settings"), temp("system-empty"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    let to = harness.get_by_label("+ Audio track").rect().center() + egui::vec2(0.0, 40.0);
    system_drop(&mut harness, &folder.join("Loop 120bpm.wav"), to);
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "a new track");
    let session = harness.state().session().unwrap();
    assert!(session.project().tracks.iter().any(|t| t.name == "Loop 120bpm" && t.content.audio));
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dropped_from_the_system_onto_an_arrangement_lane_lands_on_that_track() {
    let (settings, folder) = (temp("system-lane-settings"), temp("system-lane"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().do_action("view.toggle", &mut Vec::new());
    harness.run();
    let header = harness.get_by_label("Track Keys").rect();
    // The lane is beside the header, on the side away from the window's edge.
    let x = if header.center().x < SIZE.0 / 2.0 { header.right() + 60.0 } else { header.left() - 60.0 };
    system_drop(&mut harness, &folder.join("Groove.mid"), Pos2::new(x, header.center().y));
    let landed = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            let groove = |c: &suhina_engine::model::Clip| {
                p.contents.iter().any(|x| x.id == c.content && x.name.starts_with("Groove"))
            };
            p.tracks.len() == demo_project().tracks.len()
                && p.arrangement.lanes.iter().any(|l| l.track == keys && l.clips.iter().any(groove))
        })
    };
    assert!(until(&mut harness, landed), "a Groove clip on Keys, no new track");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn system_files_without_a_pointer_position_go_where_enter_puts_them_and_projects_still_open() {
    let (settings, folder) = (temp("system-nopos-settings"), temp("system-nopos"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    // The pointer is outside the window, as during an OS drag that reports no position.
    harness.remove_cursor();
    harness.run();
    let wav = folder.join("Loop 120bpm.wav");
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(wav)));
    harness.step();
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "no track selected: a new track");

    let song = folder.join("Song.suhina");
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(song)));
    harness.step();
    harness.run();
    let asked = harness.state_mut().take_open_request().is_some()
        || harness.query_by_label_contains("Open Song.suhina").is_some();
    assert!(asked, "a project dropped from the system still opens");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_sample_dragged_onto_the_empty_space_makes_an_audio_track() {
    let (settings, folder) = (temp("empty-settings"), temp("empty"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "loop".into();
    harness.run();
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    let from = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    let to = harness.get_by_label("+ Audio track").rect().center() + egui::vec2(0.0, 40.0);
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "a new track");
    let session = harness.state().session().unwrap();
    let track = session.project().tracks.iter().find(|t| t.name == "Loop 120bpm").unwrap();
    assert!(track.content.audio);
    // The loop's tempo is in its name, so it is warped.
    let warped = session
        .project()
        .contents
        .iter()
        .flat_map(|c| &c.lanes)
        .any(|l| matches!(&l.data, LaneData::Audio(r) if r.warped));
    assert!(warped);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_device_dragged_onto_a_track_header_joins_its_chain_and_becomes_recent() {
    let (settings, folder) = (temp("device-settings"), temp("device"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Effects")).click();
    harness.run();
    let from = harness.get_by_label(&row("Overdrive")).rect().center();
    let to = harness.get_by_label("Track Bass").rect().center();
    drag(&mut harness, from, to);
    let bass_devices = |app: &App| {
        app.session().map_or(0, |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channels.iter().find(|c| c.id == channel).map_or(0, |c| c.devices.len())
        })
    };
    assert!(until(&mut harness, |app| bass_devices(app) == 1));
    harness.get_by_label("Place Effects").click();
    harness.run();
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "Recent › Effects lists it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn escape_cancels_a_drag_and_a_project_opens_with_a_double_click() {
    let (settings, folder) = (temp("cancel-settings"), temp("cancel"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "song".into();
    harness.run();
    double_click(&mut harness, &row("Song.suhina"));
    harness.run();
    let asked = harness.state_mut().take_open_request().is_some()
        || harness.query_by_label_contains("Open Song.suhina").is_some();
    assert!(asked, "the project is asked to open");

    harness.state_mut().browser.search = "kick".into();
    harness.run();
    let from = harness.get_by_label(&row("Kick Hard.wav")).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(300.0, 0.0));
    harness.step();
    assert!(harness.state().browser.drag.is_some());
    harness.key_press(Key::Escape);
    harness.step();
    assert!(harness.state().browser.drag.is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_folder_is_added_from_the_sidebar_and_the_panel_moves_to_the_right() {
    let (settings, folder) = (temp("add-settings"), temp("add"));
    let other = temp("add-other");
    pack(&folder);
    wav(&other.join("Clap.wav"), 0.1);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Add folder").click();
    harness.run();
    harness.input_mut().events.push(egui::Event::Text(other.display().to_string()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let name = other.file_name().unwrap().to_string_lossy().to_string();
    assert!(harness.query_by_label(&format!("Place {name}")).is_some(), "the folder is in the sidebar");

    let left = harness.get_by_label("Place Pinned").rect().center().x;
    assert!(left < SIZE.0 / 2.0);
    let mut settings_now = harness.state().settings().clone();
    settings_now.browser_side = suhina_ui::settings::Side::Right;
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings_now);
    harness.run();
    assert!(harness.get_by_label("Place Pinned").rect().center().x > SIZE.0 / 2.0);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

/// Renders the window with the browser open to `target/ui-snapshots/browser.png`, for looking
/// at: `cargo test -p suhina-ui --test browser -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_browser() {
    let (settings, folder) = (temp("render-settings"), temp("render"));
    pack(&folder);
    let mut app = App::new();
    let mut library = Library::new(Some(settings.clone()));
    library.add_folder(&folder).unwrap();
    library.tag(&folder.join("Drums").join("Kick Hard.wav").display().to_string(), "Red", true).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    double_click(&mut harness, &row("Drums"));
    harness.get_by_label(&row("Kick Hard.wav")).click();
    for _ in 0..20 {
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
    image.save(out.join("browser.png")).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_plug_ins_flat_list_sorts_by_its_columns() {
    let (settings, folder) = (temp("flat-settings"), temp("flat"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    // The Grouped view is the default: the vendors sit under the Plug-ins heading, which
    // starts closed.
    harness.get_by_label(&row("Plug-ins")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Alpha")).is_some(), "a vendor heading");
    harness.get_by_label("Show plug-ins All").click();
    harness.run();
    // No headings, the columns' header on top, the rows in name order.
    assert!(harness.query_by_label(&row("Alpha")).is_none(), "no vendor heading in the flat list");
    assert!(harness.query_by_label("Sort by Modified").is_some(), "the columns' header");
    assert!(
        row_y(&harness, "Alpha Delay") < row_y(&harness, "Alpha Verb")
            && row_y(&harness, "Alpha Verb") < row_y(&harness, "Zeta Synth")
    );
    // By type: the instrument first, then the effects in name order; again for the reverse.
    harness.get_by_label("Sort by Type").click();
    harness.run();
    assert!(row_y(&harness, "Zeta Synth") < row_y(&harness, "Alpha Delay"));
    harness.get_by_label("Sort by Type").click();
    harness.run();
    assert!(row_y(&harness, "Alpha Delay") < row_y(&harness, "Zeta Synth"), "the same column again reverses");
    // By the date the scan read the files: the verb's file was read first, so it is first.
    harness.get_by_label("Sort by Modified").click();
    harness.run();
    assert!(row_y(&harness, "Alpha Verb") < row_y(&harness, "Alpha Delay"), "by the date modified");
    // The sort choice is the window's own: it stays across a switch of the view.
    harness.get_by_label("Show plug-ins Grouped").click();
    harness.run();
    harness.get_by_label("Show plug-ins All").click();
    harness.run();
    assert!(row_y(&harness, "Alpha Verb") < row_y(&harness, "Alpha Delay"), "the sort stays across a view switch");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn plug_ins_group_by_type() {
    let (settings, folder) = (temp("type-settings"), temp("type"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    harness.get_by_label("Show plug-ins By type").click();
    harness.run();
    // Two type headings; under each, Suhina's groups from the format's categories.
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Synth")).is_some(), "Zeta Synth's group");
    harness.get_by_label(&row("Synth")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Zeta Synth")).is_some());
    harness.get_by_label(&row("Effects")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Reverb")).is_some() && harness.query_by_label(&row("Delay")).is_some());
    harness.get_by_label(&row("Reverb")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Alpha Verb")).is_some());
    // The choice is kept for the next start: it is a library command, not the window's.
    let mut view = Library::new(Some(settings.to_path_buf()));
    assert_eq!(view.plugins_view(), suhina_library::store::PluginsView::Type);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_plugin_drags_to_a_track_from_every_view() {
    let (settings, folder) = (temp("drag-settings"), temp("drag"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    let devices = |app: &App| {
        app.session().map_or(0, |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channels.iter().find(|c| c.id == channel).map_or(0, |c| c.devices.len())
        })
    };
    let to = harness.get_by_label("Track Bass").rect().center();
    // From the flat list.
    harness.get_by_label("Show plug-ins All").click();
    harness.run();
    assert!(harness.query_by_label(&row("Zeta Synth")).is_some(), "the flat list lists it");
    let before = devices(harness.state());
    let from = harness.get_by_label(&row("Zeta Synth")).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(40.0, 0.0));
    harness.step();
    eprintln!("DRAG CARRIED: {}", harness.state().browser.drag.is_some());
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| devices(app) == before + 1), "from the flat list");
    // From the grouped tree.
    harness.get_by_label("Show plug-ins Grouped").click();
    harness.run();
    harness.get_by_label(&row("Plug-ins")).click();
    harness.run();
    harness.get_by_label(&row("Alpha")).click();
    harness.run();
    let before = devices(harness.state());
    let from = harness.get_by_label(&row("Alpha Verb")).rect().center();
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| devices(app) == before + 1), "from the grouped tree");
    // From the tree by type.
    harness.get_by_label("Show plug-ins By type").click();
    harness.run();
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    harness.get_by_label(&row("Synth")).click();
    harness.run();
    let before = devices(harness.state());
    let from = harness.get_by_label(&row("Zeta Synth")).rect().center();
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| devices(app) == before + 1), "from the tree by type");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}
