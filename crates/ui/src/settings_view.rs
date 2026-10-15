//! The Settings page: replaces the main view until it is closed (the design brief keeps one
//! window and no dialogs). Its pages are listed down the left; each setting is a row: a label, a
//! short note and its control.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::keymap::{Keymap, Preset, Shortcut, actions};
use crate::motion;
use crate::pick::PickField;
use crate::settings::{
    AtStart, DeviceLayout, EditorPlacement, Graphics, OnOpen, PianoRollTop, PlayFrom, Settings, Side, StopReturn,
};
use crate::theme::{self, PAD, Palette};
use crate::widgets;

const PAGE_WIDTH: f32 = 560.0;
/// The list of pages on the left, and the gap between it and the page.
const SIDEBAR: f32 = 140.0;
const SIDEBAR_GAP: f32 = 24.0;
const SIDEBAR_ROW: f32 = 20.0;
/// How far a sub-page is indented under its page.
const SUB_INDENT: f32 = 12.0;
const LABEL_WIDTH: f32 = 180.0;
const SETTING_ROW: f32 = 34.0;
const SEGMENT: f32 = 15.0;
/// A folder field on the right of its row; empty shows the default folder.
const FOLDER_FIELD: f32 = 300.0;
/// The Browse… button at the end of a path field.
const BROWSE_WIDTH: f32 = 60.0;
/// Themes shown before their list scrolls.
const THEME_ROWS: usize = 8;
/// A dropdown on the right of its row (Settings › Audio).
const DROPDOWN_FIELD: f32 = 260.0;

/// Facts the page shows but does not change.
#[derive(Debug, Clone)]
pub struct About {
    /// What the active project plays on now, read every frame (`audio.info`).
    pub audio_live: Option<suhina_session::OutputInfo>,
    /// The audio hosts and their output devices, listed when Settings › Audio opens or is
    /// refreshed (`audio.devices`).
    pub audio_hosts: Vec<suhina_session::audio::HostInfo>,
    /// The MIDI input ports, listed with the audio devices (`midi.inputs`), or why they could
    /// not be listed.
    pub midi_inputs: Result<Vec<MidiPort>, String>,
    /// How the window is drawn right now.
    pub graphics_now: Option<Graphics>,
    /// Plug-in editors can open inside the device panel here (the window is a Win32 or X11
    /// one); elsewhere the setting for it is shown but does nothing.
    pub editors_embeddable: bool,
    /// The folders projects are saved in and templates kept in when the settings leave them
    /// empty.
    pub projects_default: String,
    pub templates_default: String,
    /// The active project's newest backup, as Settings › General shows it: when and where.
    pub last_backup: String,
}

impl Default for About {
    fn default() -> Self {
        Self {
            audio_live: None,
            audio_hosts: Vec::new(),
            midi_inputs: Ok(Vec::new()),
            graphics_now: None,
            editors_embeddable: false,
            projects_default: String::new(),
            templates_default: String::new(),
            last_backup: Default::default(),
        }
    }
}

/// A MIDI input port as Settings › Audio shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MidiPort {
    pub name: String,
    pub enabled: bool,
    pub open: bool,
    /// Why it could not be opened.
    pub error: Option<String>,
}

/// The plug-in library as the page shows it, read through the `plugins.*` commands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginsView {
    /// The system's standard folders, and the ones the user added.
    pub standard: Vec<String>,
    pub added: Vec<String>,
    /// Name, vendor, format and version of each plug-in found.
    pub found: Vec<[String; 4]>,
    /// Files that failed to load, and why.
    pub failed: Vec<(String, String)>,
    /// A scan is running in the background.
    pub scanning: bool,
    /// The folder typed into the add field.
    pub new_folder: String,
}

/// What the page asks the app to do with the plug-in library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRequest {
    /// Scan new and changed files, or every file again.
    Scan {
        all: bool,
    },
    AddFolder(String),
    RemoveFolder(String),
}

/// The themes as the page shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemesView {
    /// Each theme's id, name and whether it comes with Suhina: the user's own first.
    pub list: Vec<(String, String, bool)>,
    /// The id of the theme in use, whether it comes with Suhina, its name and its colours.
    pub current: String,
    pub built_in: bool,
    pub name: String,
    pub colors: Palette,
    /// The folder theme files are kept in, if there is one.
    pub folder: Option<String>,
}

/// What the page asks the app to do with the themes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeRequest {
    /// Use the theme with this id.
    Use(String),
    /// Use these colours; a built-in theme is copied first, since it does not change.
    Edit(Palette),
    /// Give the theme in use this name (and its file a name made from it).
    Rename(String),
    /// A copy of the theme in use, to change.
    Duplicate,
    /// Read the theme files again.
    Reload,
    /// Show the themes folder in the file manager.
    OpenFolder,
    /// Write every theme file into one zip.
    Backup,
    /// Write a theme template and a README on the format into a zip.
    ExportTemplate,
}

/// The pages of Settings, in the order the list on the left shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Interface,
    Theme,
    Arrangement,
    DetailArea,
    Browser,
    Graphics,
    Audio,
    Shortcuts,
    Plugins,
}

impl Page {
    /// Every page with its name, and whether it belongs to the page above it (Interface).
    pub const ALL: [(Page, &'static str, bool); 10] = [
        (Page::General, "General", false),
        (Page::Interface, "Interface", false),
        (Page::Theme, "Theme", true),
        (Page::Arrangement, "Arrangement", true),
        (Page::DetailArea, "Detail area", true),
        (Page::Browser, "Browser", true),
        (Page::Graphics, "Graphics", false),
        (Page::Audio, "Audio", false),
        (Page::Shortcuts, "Shortcuts", false),
        (Page::Plugins, "Plug-ins", false),
    ];
}

/// What the Settings page keeps between frames: the page shown, and the action whose keys are
/// being set (the next key pressed becomes its key).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageState {
    pub page: Page,
    pub capture: Option<String>,
    /// The name being typed for a theme, with the theme's id.
    pub theme_name: Option<(String, String)>,
    /// The theme the list was last scrolled to, so it shows the one in use when that changes.
    pub theme_shown: Option<String>,
    /// The audio devices were listed since the Audio page was opened.
    pub audio_listed: Option<bool>,
}

/// What the user did on the page this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub changed: bool,
    pub close: bool,
    pub plugins: Option<PluginRequest>,
    pub themes: Option<ThemeRequest>,
    pub keymap_changed: bool,
    /// The most colour tags the browser may have, as chosen.
    pub tag_limit: Option<usize>,
    /// The audio output was chosen anew: the app runs `audio.setOutput` with it.
    pub audio_output: Option<suhina_session::OutputChoice>,
    /// The audio devices should be listed again.
    pub audio_refresh: bool,
    /// A MIDI input port was turned on or off: the app runs `midi.setInput`.
    pub midi_input: Option<(String, bool)>,
    /// A field's Browse… was clicked: the app shows the system's dialog for it.
    pub pick: Option<PickField>,
}

/// What the page shows and changes besides the settings.
pub struct Sources<'a> {
    pub about: &'a About,
    pub plugins: &'a mut PluginsView,
    pub themes: &'a ThemesView,
    pub keymap: &'a mut Keymap,
    /// The most colour tags the browser may have now.
    pub tag_limit: usize,
}

/// Draws the page into the space left in `ui`.
pub fn show(ui: &mut Ui, settings: &mut Settings, sources: Sources<'_>, state: &mut PageState) -> Outcome {
    let Sources { about, plugins, themes, keymap, tag_limit } = sources;
    let mut outcome = Outcome::default();
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    let width = SIDEBAR + SIDEBAR_GAP + PAGE_WIDTH;
    let whole = Rect::from_min_max(
        Pos2::new(area.center().x - width / 2.0, area.top()),
        Pos2::new(area.center().x + width / 2.0, area.bottom()),
    )
    .intersect(area.shrink2(Vec2::new(PAD * 2.0, 0.0)));
    let y = area.top() + 24.0;

    // Title and close.
    let title = Rect::from_min_size(Pos2::new(whole.left(), y), Vec2::new(whole.width(), theme::ROW));
    widgets::text_in(
        &painter,
        title,
        Align2::LEFT_CENTER,
        "Settings",
        theme::ui_font(theme::TEXT + 2.0),
        theme::text_primary(),
    );
    let close = Rect::from_min_max(Pos2::new(title.right() - 52.0, title.top()), title.max);
    let response = widgets::button_area(ui, close, Id::new("settings-close"), "Close settings");
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(close, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    widgets::text_in(
        &painter,
        close,
        Align2::CENTER_CENTER,
        "Close",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    // Escape closes the page, unless it is cancelling a key being set.
    if response.clicked() || (state.capture.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        outcome.close = true;
    }
    let top = title.bottom() + 16.0;

    // The pages down the left, the one shown beside them.
    let sidebar = Rect::from_min_max(Pos2::new(whole.left(), top), Pos2::new(whole.left() + SIDEBAR, whole.bottom()));
    if let Some(picked) = page_list(ui, &painter, sidebar, state.page) {
        state.page = picked;
        state.capture = None;
        state.audio_listed = None;
        state.theme_shown = None;
    }
    let page = Rect::from_min_max(Pos2::new(sidebar.right() + SIDEBAR_GAP, top), whole.max);
    let bottom = area.bottom();

    // A section title over a hairline; the first sits level with the top of the list.
    let section = |y: &mut f32, name: &str| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SIDEBAR_ROW));
        widgets::text_in(
            &painter,
            rect,
            Align2::LEFT_CENTER,
            name,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(theme::border()));
        *y = rect.bottom() + 8.0;
    };
    let row = |y: &mut f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SETTING_ROW));
        *y += SETTING_ROW;
        rect
    };
    let mut y = top;
    let on_off = [("On", true), ("Off", false)];

    match state.page {
        Page::General => {
            section(&mut y, "Projects");
            let r = row(&mut y);
            label(
                &painter,
                r,
                "When opening a project",
                "While another is open: ask, open it in a new tab, or replace it",
            );
            let options = [("Ask", OnOpen::Ask), ("New tab", OnOpen::NewTab), ("Replace", OnOpen::Replace)];
            if let Some(choice) =
                segmented(ui, &painter, control_rect(r, 3), "When opening a project", &options, settings.on_open)
            {
                settings.on_open = choice;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(
                &painter,
                r,
                "At start",
                "With no project given: the start panel, a new one, one from a template, or the last",
            );
            let options = [
                ("Panel", AtStart::Panel),
                ("New", AtStart::Blank),
                ("Template", AtStart::Template),
                ("Last", AtStart::Last),
            ];
            if let Some(choice) = segmented(ui, &painter, control_rect(r, 4), "At start", &options, settings.at_start) {
                settings.at_start = choice;
                outcome.changed = true;
            }
            let template_hint = String::from("A template's file name, or a path");
            let starts_from_template = settings.at_start == AtStart::Template;
            let mut folders = vec![
                (
                    "Projects folder",
                    "Where a new project is saved first",
                    &mut settings.projects_folder,
                    &about.projects_default,
                    PickField::ProjectsFolder,
                ),
                (
                    "Templates folder",
                    "Projects listed under New › From a template, and where Save as template puts one",
                    &mut settings.templates_folder,
                    &about.templates_default,
                    PickField::TemplatesFolder,
                ),
            ];
            if starts_from_template {
                folders.push((
                    "Start template",
                    "The template At start makes a new project from",
                    &mut settings.start_template,
                    &template_hint,
                    PickField::StartTemplate,
                ));
            }
            for (name, note, value, default, pick) in folders {
                let r = row(&mut y);
                // The note is cut where the field starts.
                let left =
                    Rect::from_min_size(r.min, Vec2::new(r.width() - FOLDER_FIELD - 8.0 + LABEL_WIDTH, r.height()));
                label(&painter, left, name, note);
                let browse = Rect::from_min_size(
                    Pos2::new(r.right() - BROWSE_WIDTH, r.top() + 1.0),
                    Vec2::new(BROWSE_WIDTH, SEGMENT),
                );
                if crate::project_dialog::button(ui, &painter, browse, "Browse…", &format!("Browse for {name}")) {
                    outcome.pick = Some(pick);
                }
                let field = Rect::from_min_size(
                    Pos2::new(r.right() - FOLDER_FIELD, r.top()),
                    Vec2::new(FOLDER_FIELD - BROWSE_WIDTH - 4.0, SEGMENT + 2.0),
                );
                let edit = ui.put(
                    field,
                    egui::TextEdit::singleline(value)
                        .hint_text(default.as_str())
                        .font(theme::ui_font(theme::TEXT_SMALL)),
                );
                edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
                if edit.changed() {
                    outcome.changed = true;
                }
            }
            y += 8.0;
            section(&mut y, "Backups");
            let r = row(&mut y);
            label(&painter, r, "Backups", "When a copy of each changed project is written to .backup beside it");
            let options: Vec<(String, suhina_session::backup::Interval)> =
                suhina_session::backup::Interval::ALL.iter().map(|i| (i.title(), *i)).collect();
            if let Some(choice) = choice_dropdown(ui, &painter, r, "Backups", &options, &settings.backup) {
                settings.backup = choice;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Clean up backups", "Keep the newest 20 backups of each project and delete older ones");
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Clean up backups", &on_off, settings.backup_clean_up)
            {
                settings.backup_clean_up = on;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Last backup", &about.last_backup);
            y += 8.0;
            section(&mut y, "Dropping files");
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Several files",
                "Onto one track, or one track each; Ctrl while dropping does the other",
            );
            let options = [("One track", false), ("One each", true)];
            if let Some(choice) =
                segmented(ui, &painter, control_rect(r, 2), "Several files", &options, settings.drop_separate)
            {
                settings.drop_separate = choice;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Warp mode", "How a sample with a known tempo is stretched to the project's");
            let options: Vec<(&str, crate::settings::WarpMode)> =
                crate::settings::WarpMode::ALL.iter().map(|m| (m.title(), *m)).collect();
            if let Some(choice) = segmented(ui, &painter, control_rect(r, 6), "Warp mode", &options, settings.warp_mode)
            {
                settings.warp_mode = choice;
                outcome.changed = true;
            }
            y += 8.0;
            section(&mut y, "Transport");
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Play from",
                "Where Play starts: the play start marker, or the start of the clip selected in the Arrangement view",
            );
            let options = [("Marker", PlayFrom::Marker), ("Selected clip", PlayFrom::Clip)];
            if let Some(choice) = segmented(ui, &painter, control_rect(r, 2), "Play from", &options, settings.play_from)
            {
                settings.play_from = choice;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Stop",
                "Back to the play start marker, or stay where it stopped; stop again for the start",
            );
            let options = [("Marker", StopReturn::Marker), ("Stay", StopReturn::Here)];
            if let Some(choice) = segmented(ui, &painter, control_rect(r, 2), "Stop", &options, settings.stop) {
                settings.stop = choice;
                outcome.changed = true;
            }
            y += 8.0;
            section(&mut y, "Piano roll");
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Pulled to the top",
                "What stays above the piano roll when its splitter is dragged all the way up",
            );
            let options = [("Edited track", PianoRollTop::Track), ("Whole window", PianoRollTop::Window)];
            if let Some(choice) =
                segmented(ui, &painter, control_rect(r, 2), "Pulled to the top", &options, settings.piano_roll_top)
            {
                settings.piano_roll_top = choice;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Piano roll grid",
                "Its own grid value, or the one in the control bar, which otherwise sets the Arrangement view's only",
            );
            let options = [("Its own", false), ("Control bar", true)];
            if let Some(choice) =
                segmented(ui, &painter, control_rect(r, 2), "Piano roll grid", &options, settings.piano_grid_follows)
            {
                settings.piano_grid_follows = choice;
                outcome.changed = true;
            }
        }
        Page::Interface => {
            section(&mut y, "Interface");
            let r = row(&mut y);
            label(&painter, r, "Animations", "Short fades and slides when things change");
            if let Some(on) = segmented(ui, &painter, control_rect(r, 2), "Animations", &on_off, settings.animations) {
                settings.animations = on;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Zoom", "The size of the whole window; Ctrl and + or -, Ctrl and 0 for 100 %");
            let actual = format!("{} %", settings.zoom);
            // Three buttons, none of them chosen: smaller, back to 100 %, larger.
            let options = [("-", Some(-1)), (actual.as_str(), Some(0)), ("+", Some(1))];
            match segmented(ui, &painter, control_rect(r, 3), "Zoom", &options, None) {
                Some(Some(0)) if settings.zoom != crate::settings::ZOOM => {
                    settings.zoom = crate::settings::ZOOM;
                    outcome.changed = true;
                }
                Some(Some(steps)) if steps != 0 => outcome.changed |= settings.step_zoom(steps),
                _ => {}
            }
        }
        Page::Browser => {
            section(&mut y, "Browser");
            let r = row(&mut y);
            label(&painter, r, "Side", "Which side of the window the browser is on");
            let options = [("Left", Side::Left), ("Right", Side::Right)];
            if let Some(side) =
                segmented(ui, &painter, control_rect(r, 2), "Browser side", &options, settings.browser_side)
            {
                settings.browser_side = side;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Auto-play", "Selecting a sample or MIDI file previews it");
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Auto-play", &on_off, settings.preview_autoplay)
            {
                settings.preview_autoplay = on;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Preview volume", "How loud previews play, after the main channel");
            let options = [("-18 dB", -18), ("-12 dB", -12), ("-6 dB", -6), ("0 dB", 0)];
            if let Some(db) =
                segmented(ui, &painter, control_rect(r, 4), "Preview volume", &options, settings.preview_volume)
            {
                settings.preview_volume = db;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Map layout", "The sample map as a free cloud or snapped to a grid");
            let options = [("Cloud", false), ("Grid", true)];
            if let Some(grid) = segmented(ui, &painter, control_rect(r, 2), "Map layout", &options, settings.map_grid) {
                settings.map_grid = grid;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Map preview on hover", "Pointing at a sample on the map previews it");
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Map preview on hover", &on_off, settings.map_hover_preview)
            {
                settings.map_hover_preview = on;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Map: longest sample", "Longer samples are left off the map");
            let options = [("5 s", 5u32), ("10 s", 10), ("30 s", 30), ("60 s", 60)];
            if let Some(seconds) =
                segmented(ui, &painter, control_rect(r, 4), "Map longest sample", &options, settings.map_max_seconds)
            {
                settings.map_max_seconds = seconds;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Tag limit", "The most colour tags there may be");
            let options = [("64", 64usize), ("128", 128), ("256", 256), ("1024", 1024)];
            if let Some(limit) = segmented(ui, &painter, control_rect(r, 4), "Tag limit", &options, tag_limit) {
                outcome.tag_limit = Some(limit);
            }
        }
        Page::Theme => {
            section(&mut y, "Theme");
            outcome.themes = theme_page(ui, &painter, page, y, bottom, themes, state);
        }
        Page::Arrangement => {
            section(&mut y, "Arrangement");
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Waveform scale",
                "Linear, or decibels so quiet parts show; clips, previews and the map",
            );
            let options = [("Linear", false), ("dB", true)];
            if let Some(db) =
                segmented(ui, &painter, control_rect(r, 2), "Waveform scale", &options, settings.waveform_db)
            {
                settings.waveform_db = db;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Track headers", "Which side of the timeline they are on");
            let options = [("Left", Side::Left), ("Right", Side::Right)];
            if let Some(side) =
                segmented(ui, &painter, control_rect(r, 2), "Track headers", &options, settings.track_headers)
            {
                settings.track_headers = side;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Allow sample stacking",
                "Audio clips moved over others all play, instead of replacing them",
            );
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Allow sample stacking", &on_off, settings.sample_stacking)
            {
                settings.sample_stacking = on;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Allow MIDI stacking",
                "MIDI clips moved over others all play, instead of replacing them",
            );
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Allow MIDI stacking", &on_off, settings.midi_stacking)
            {
                settings.midi_stacking = on;
                outcome.changed = true;
            }
        }
        Page::DetailArea => {
            section(&mut y, "Detail area");
            let r = row(&mut y);
            label(&painter, r, "Device chain", "In a row under the view, or stacked in a column beside it");
            let options = [("Row", DeviceLayout::Row), ("Stack", DeviceLayout::Stack)];
            if let Some(layout) =
                segmented(ui, &painter, control_rect(r, 2), "Device chain", &options, settings.device_layout)
            {
                settings.device_layout = layout;
                outcome.changed = true;
            }
            let r = row(&mut y);
            label(&painter, r, "Select on launch", "Launching a clip in the Session view shows it here");
            if let Some(on) =
                segmented(ui, &painter, control_rect(r, 2), "Select on launch", &on_off, settings.select_on_launch)
            {
                settings.select_on_launch = on;
                outcome.changed = true;
            }
        }
        Page::Graphics => {
            section(&mut y, "Graphics");
            let r = row(&mut y);
            let note = match about.graphics_now {
                Some(now) if now != settings.graphics => "Applies the next time Suhina starts",
                _ => "How the window is drawn; OpenGL can resize more smoothly on some systems",
            };
            label(&painter, r, "Renderer", note);
            let options = [("GPU", Graphics::Gpu), ("OpenGL", Graphics::OpenGl)];
            if let Some(graphics) = segmented(ui, &painter, control_rect(r, 2), "Renderer", &options, settings.graphics)
            {
                settings.graphics = graphics;
                outcome.changed = true;
            }
        }
        Page::Audio => {
            if state.audio_listed != Some(true) {
                outcome.audio_refresh = true;
                state.audio_listed = Some(true);
            }
            let (picked, refresh) = audio_page(ui, &painter, page, &mut y, settings, about);
            outcome.audio_refresh |= refresh;
            y += 8.0;
            outcome.midi_input = midi_section(ui, &painter, page, &mut y, about);
            if let Some(choice) = picked {
                settings.set_output_choice(&choice);
                outcome.audio_output = Some(choice);
                outcome.changed = true;
            }
        }
        Page::Shortcuts => {
            outcome.keymap_changed = shortcuts_page(ui, &painter, page, y, bottom, keymap, state);
        }
        Page::Plugins => {
            section(&mut y, "Plug-ins");
            let r = row(&mut y);
            let note = if about.editors_embeddable {
                "In a window above Suhina, or inside the device panel as part of the chain"
            } else {
                "Editors need Windows or an X server (DISPLAY); they do not open on this system yet"
            };
            label(&painter, r, "Editors", note);
            let options = [("Window", EditorPlacement::Window), ("Device view", EditorPlacement::DeviceView)];
            if let Some(placement) =
                segmented(ui, &painter, control_rect(r, 2), "Editors", &options, settings.plugin_editors)
            {
                settings.plugin_editors = placement;
                outcome.changed = true;
            }
            y += PAD;
            outcome.plugins = plugin_section(ui, &painter, page, &mut y, bottom, plugins);
        }
    }
    outcome
}

/// The list of pages: Interface's own pages indented under it, the page shown filled. Returns
/// the page clicked, if it is not the one shown.
fn page_list(ui: &Ui, painter: &egui::Painter, rect: Rect, shown: Page) -> Option<Page> {
    let mut picked = None;
    for (index, (page, name, sub)) in Page::ALL.into_iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + index as f32 * SIDEBAR_ROW),
            Vec2::new(rect.width(), SIDEBAR_ROW),
        );
        let response =
            widgets::button_area(ui, row, Id::new(("settings-page", index)), format!("Settings page: {name}"));
        if response.clicked() && page != shown {
            picked = Some(page);
        }
        let selected = motion::toward(ui.ctx(), response.id.with("selected"), page == shown, motion::VALUE);
        let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
        painter.rect_filled(row, 0.0, motion::mix(idle, theme::control_background(), selected));
        let indent = if sub { SUB_INDENT } else { 0.0 };
        let text = Rect::from_min_max(Pos2::new(row.left() + PAD + indent, row.top()), row.max);
        let color = motion::mix(theme::text_secondary(), theme::text_primary(), selected);
        widgets::text_in(painter, text, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), color);
    }
    picked
}

/// Interface › Theme: the themes to pick from, what can be done with them, and the colours of the
/// one in use. Returns what the user asked for.
fn theme_page(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    mut y: f32,
    bottom: f32,
    themes: &ThemesView,
    state: &mut PageState,
) -> Option<ThemeRequest> {
    let mut request = None;
    let small = theme::ui_font(theme::TEXT_SMALL);

    // The themes, in a list that scrolls once it is taller than eight rows: a click uses one.
    let rows = themes.list.len().clamp(1, THEME_ROWS) as f32;
    let list = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), rows * theme::ROW));
    y = list.bottom() + 8.0;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("themes").auto_shrink([false, false]).show(ui, |ui| {
            for (id, name, built_in) in &themes.list {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), theme::ROW), Sense::hover());
                let response = widgets::button_area(ui, r, Id::new(("theme", id)), format!("Theme: {name}"));
                let current = *id == themes.current;
                let selected = motion::toward(ui.ctx(), response.id.with("selected"), current, motion::VALUE);
                let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
                let painter = ui.painter();
                painter.rect_filled(r, 0.0, motion::mix(idle, theme::control_background(), selected));
                let inner = r.shrink2(Vec2::new(PAD, 0.0));
                let color = motion::mix(theme::text_secondary(), theme::text_primary(), selected);
                widgets::text_in(painter, inner, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), color);
                let file = if *built_in { "built-in".to_string() } else { format!("{id}.json") };
                widgets::text_in(painter, inner, Align2::RIGHT_CENTER, &file, small.clone(), theme::text_hint());
                if response.clicked() && !current {
                    request = Some(ThemeRequest::Use(id.clone()));
                }
                if current && state.theme_shown.as_ref() != Some(id) {
                    response.scroll_to_me(None);
                    state.theme_shown = Some(id.clone());
                }
            }
        });
    });

    // The theme in use: its name, typed over and kept on Enter or leaving the field (a built-in
    // theme's name does not change).
    let r = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), SETTING_ROW));
    y += SETTING_ROW;
    let note = match &themes.folder {
        Some(folder) => format!("Kept in {folder}; drop a theme file or a zip of them on the window to add it"),
        None => "There is no folder to keep theme files in".to_string(),
    };
    if themes.built_in {
        label(painter, r, &format!("{} (built-in; changing a colour makes a copy)", themes.name), &note);
    } else {
        if state.theme_name.as_ref().is_none_or(|(id, _)| *id != themes.current) {
            state.theme_name = Some((themes.current.clone(), themes.name.clone()));
        }
        if let Some((_, name)) = state.theme_name.as_mut() {
            let field = Rect::from_min_size(r.min, Vec2::new(r.width() / 2.0, 16.0));
            let edit = ui.put(field, egui::TextEdit::singleline(name).font(theme::ui_font(theme::TEXT)));
            edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Theme name"));
            if edit.lost_focus() && name.trim() != themes.name && !name.trim().is_empty() {
                request = Some(ThemeRequest::Rename(name.trim().to_string()));
            }
        }
        let line = Rect::from_min_size(Pos2::new(r.left(), r.top() + 18.0), Vec2::new(r.width(), 14.0));
        widgets::text_in(painter, line, Align2::LEFT_CENTER, &note, small.clone(), theme::text_secondary());
    }

    // What can be done with the themes.
    let actions = [
        ("Duplicate", "Duplicate theme", ThemeRequest::Duplicate),
        ("Reload", "Reload themes", ThemeRequest::Reload),
        ("Open folder", "Open the themes folder", ThemeRequest::OpenFolder),
        ("Back up", "Back up themes", ThemeRequest::Backup),
        ("Export template", "Export the theme template", ThemeRequest::ExportTemplate),
    ];
    let mut x = page.left();
    for (index, (text, name, asked)) in actions.into_iter().enumerate() {
        let width = widgets::text_width(ui, text, small.clone()) + 2.0 * PAD + 8.0;
        let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, SEGMENT + 2.0));
        x = cell.right() + 1.0;
        let response = widgets::button_area(ui, cell, Id::new(("theme-action", index)), name);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(cell, 0.0, fill);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, small.clone(), theme::text_primary());
        if response.clicked() {
            request = Some(asked);
        }
    }
    y += SEGMENT + 2.0 + 12.0;

    // Its colours, in the space left.
    let r = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), 16.0));
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Colours", small.clone(), theme::text_hint());
    painter.hline(page.x_range(), r.bottom() + 2.5, theme::hairline(theme::border()));
    y = r.bottom() + 6.0;
    let list = Rect::from_min_max(Pos2::new(page.left(), y), Pos2::new(page.right(), (bottom - 12.0).max(y + 60.0)));
    let colors = themes.colors;
    let mut edited = colors;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("colours").auto_shrink([false, false]).show(ui, |ui| {
            let left = egui::Layout::left_to_right(egui::Align::Center);
            for (key, purpose, color) in colors.tokens() {
                ui.horizontal(|ui| {
                    let mut picked = color;
                    let swatch =
                        egui::color_picker::color_edit_button_srgba(ui, &mut picked, egui::color_picker::Alpha::Opaque);
                    // An outline, so a swatch the colour of the page still shows where it is.
                    ui.painter().rect_stroke(
                        swatch.rect,
                        0.0,
                        theme::hairline(theme::border_strong()),
                        egui::StrokeKind::Inside,
                    );
                    let name = format!("Colour {key}");
                    swatch.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, &name));
                    if picked != color
                        && let Some(slot) = edited.token_mut(key)
                    {
                        *slot = picked;
                    }
                    ui.allocate_ui_with_layout(Vec2::new(64.0, 18.0), left, |ui| {
                        ui.set_min_width(64.0);
                        let text = egui::RichText::new(theme::hex(picked))
                            .font(theme::mono_font(theme::TEXT_SMALL))
                            .color(theme::text_value());
                        ui.label(text);
                    });
                    ui.allocate_ui_with_layout(Vec2::new(170.0, 18.0), left, |ui| {
                        ui.set_min_width(170.0);
                        let text = egui::RichText::new(key).font(theme::mono_font(theme::TEXT_SMALL));
                        ui.label(text.color(theme::text_primary()));
                    });
                    let text = egui::RichText::new(purpose).font(small.clone()).color(theme::text_hint());
                    ui.add(egui::Label::new(text).truncate());
                });
            }
        });
    });
    if edited != colors {
        request = Some(ThemeRequest::Edit(edited));
    }
    request
}

/// Every action with its keys: Change takes the next key pressed, Clear unbinds, Reset all
/// goes back to the defaults. Returns whether the keymap changed.
fn shortcuts_page(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    y: f32,
    bottom: f32,
    keymap: &mut Keymap,
    state: &mut PageState,
) -> bool {
    let mut changed = false;
    let small = theme::ui_font(theme::TEXT_SMALL);

    // A key being set: the next key pressed (with its modifiers) is the action's key.
    if let Some(action) = state.capture.clone() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some(Shortcut::new(*modifiers, *key)),
                _ => None,
            })
        });
        match pressed {
            Some(s) if s.key == egui::Key::Escape && s.modifiers.is_none() => state.capture = None,
            Some(s) => {
                // Kept portable: Ctrl on Windows and Linux, Cmd on macOS, both written "Ctrl".
                let mut modifiers = egui::Modifiers::NONE;
                if s.modifiers.command || s.modifiers.ctrl || s.modifiers.mac_cmd {
                    modifiers = modifiers.plus(egui::Modifiers::COMMAND);
                }
                if s.modifiers.shift {
                    modifiers = modifiers.plus(egui::Modifiers::SHIFT);
                }
                if s.modifiers.alt {
                    modifiers = modifiers.plus(egui::Modifiers::ALT);
                }
                let shortcut = Shortcut::new(modifiers, s.key);
                keymap.set(&action, vec![shortcut]);
                state.capture = None;
                changed = true;
            }
            None => {}
        }
    }

    let head = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), SETTING_ROW));
    label(painter, head, "Shortcuts", "Change, then press the keys; Escape cancels. Kept in keymap.json.");
    let reset = Rect::from_min_max(
        Pos2::new(head.right() - 80.0, head.top() + 1.0),
        Pos2::new(head.right(), head.top() + 1.0 + SEGMENT),
    );
    let response = widgets::button_area(ui, reset, Id::new("reset-keys"), "Reset all keys");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(reset, 0.0, fill);
    widgets::text_in(painter, reset, Align2::CENTER_CENTER, "Reset all", small.clone(), theme::text_primary());
    if response.clicked() {
        *keymap = Keymap::default();
        state.capture = None;
        changed = true;
    }

    // The preset: two sets of keys, one for drawing notes first and one for selecting first. A
    // choice sets the keys it names; single keys can still be changed below.
    let mut top = head.bottom() + 4.0;
    let preset_row = Rect::from_min_size(Pos2::new(page.left(), top), Vec2::new(page.width(), 24.0));
    let preset_label = Rect::from_min_size(preset_row.min, Vec2::new(LABEL_WIDTH, 20.0));
    widgets::text_in(
        painter,
        preset_label,
        Align2::LEFT_CENTER,
        "Preset",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let current = keymap.preset();
    let control =
        Rect::from_min_size(Pos2::new(preset_row.left() + LABEL_WIDTH, preset_row.top()), Vec2::new(160.0, SEGMENT));
    let options: Vec<(&str, Preset)> = Preset::ALL.iter().map(|p| ((*p).name(), *p)).collect();
    if let Some(picked) = preset_segmented(ui, painter, control, &options, current) {
        keymap.set_preset(picked);
        state.capture = None;
        changed = true;
    }
    let note = Rect::from_min_size(
        Pos2::new(preset_row.left(), preset_row.top() + SEGMENT + 3.0),
        Vec2::new(page.width(), 16.0),
    );
    widgets::text_in(painter, note, Align2::LEFT_CENTER, current.note(), small.clone(), theme::text_secondary());
    top += 30.0;
    for (key, ids) in keymap.conflicts() {
        let r = Rect::from_min_size(Pos2::new(page.left(), top), Vec2::new(page.width(), 16.0));
        let text = format!("{} runs {} at once", key.text(), ids.join(" and "));
        widgets::text_in(painter, r, Align2::LEFT_CENTER, &text, small.clone(), theme::recording());
        top += 16.0;
    }

    let list =
        Rect::from_min_max(Pos2::new(page.left(), top + 4.0), Pos2::new(page.right(), (bottom - 12.0).max(top + 60.0)));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut group = "";
            for action in actions() {
                if action.group != group {
                    group = action.group;
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(group).font(small.clone()).color(theme::text_hint()));
                }
                ui.horizontal(|ui| {
                    let keys: Vec<String> = keymap.keys(&action.id).iter().map(Shortcut::text).collect();
                    let capturing = state.capture.as_deref() == Some(action.id.as_str());
                    let left = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(Vec2::new(300.0, 18.0), left, |ui| {
                        ui.set_min_width(300.0);
                        let text = egui::RichText::new(&action.label).font(small.clone()).color(theme::text_primary());
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let shown = if capturing {
                        "Press keys".to_string()
                    } else if keys.is_empty() {
                        "-".into()
                    } else {
                        keys.join(", ")
                    };
                    let color = if capturing { theme::accent() } else { theme::text_value() };
                    ui.allocate_ui_with_layout(Vec2::new(130.0, 18.0), left, |ui| {
                        ui.set_min_width(130.0);
                        let text = egui::RichText::new(shown).font(theme::mono_font(theme::TEXT_SMALL)).color(color);
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let change = ui.add(egui::Button::new(egui::RichText::new("Change").font(small.clone())));
                    let label = format!("Change keys of {}", action.id);
                    change.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                    if change.clicked() {
                        state.capture = Some(action.id.clone());
                    }
                    let clear = ui.add(egui::Button::new(egui::RichText::new("Clear").font(small.clone())));
                    let label = format!("Clear keys of {}", action.id);
                    clear.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                    if clear.clicked() && !keys.is_empty() {
                        keymap.set(&action.id, Vec::new());
                        changed = true;
                    }
                });
            }
        });
    });
    changed
}

/// Folders, scanning and what was found. Returns what the user asked for.
fn plugin_section(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    bottom: f32,
    plugins: &mut PluginsView,
) -> Option<PluginRequest> {
    let mut request = None;
    let small = theme::ui_font(theme::TEXT_SMALL);
    let line = |y: &mut f32, height: f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), height));
        *y += height;
        rect
    };

    // Folders: the standard ones, then the added ones with a way to remove each.
    let r = line(y, 18.0);
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Folders", theme::ui_font(theme::TEXT), theme::text_primary());
    for folder in &plugins.standard {
        let r = line(y, 16.0);
        let text = format!("{folder}  (standard)");
        widgets::text_in(painter, r, Align2::LEFT_CENTER, &text, small.clone(), theme::text_hint());
    }
    for folder in &plugins.added {
        let r = line(y, 18.0);
        let remove =
            Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top() + 1.0), Pos2::new(r.right(), r.bottom() - 1.0));
        widgets::text_in(painter, r, Align2::LEFT_CENTER, folder, small.clone(), theme::text_secondary());
        let response = widgets::button_area(ui, remove, Id::new(("remove-folder", folder)), format!("Remove {folder}"));
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(remove, 0.0, fill);
        widgets::text_in(painter, remove, Align2::CENTER_CENTER, "Remove", small.clone(), theme::text_secondary());
        if response.clicked() {
            request = Some(PluginRequest::RemoveFolder(folder.clone()));
        }
    }
    *y += 4.0;
    let r = line(y, 20.0);
    let add = Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top()), r.max);
    let field = Rect::from_min_max(r.min, Pos2::new(add.left() - 4.0, r.bottom()));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut plugins.new_folder)
            .hint_text("A folder to look in as well")
            .font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Plug-in folder to add"));
    let response = widgets::button_area(ui, add, Id::new("add-folder"), "Add plug-in folder");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(add, 0.0, fill);
    widgets::text_in(painter, add, Align2::CENTER_CENTER, "Add", small.clone(), theme::text_primary());
    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if (response.clicked() || entered) && !plugins.new_folder.trim().is_empty() {
        request = Some(PluginRequest::AddFolder(plugins.new_folder.trim().to_string()));
    }
    *y += 8.0;

    // Scanning.
    let r = line(y, SETTING_ROW);
    let summary = if plugins.scanning {
        "Scanning: each file is loaded in a separate process, so a plug-in that crashes is only reported".to_string()
    } else {
        format!("{} plug-ins found, {} files failed to load", plugins.found.len(), plugins.failed.len())
    };
    label(painter, r, "Scan", &summary);
    let buttons = control_rect(r, 2);
    let half = (buttons.width() - 1.0) / 2.0;
    for (index, (text, all)) in [("Scan", false), ("Rescan all", true)].into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(buttons.left() + index as f32 * (half + 1.0), buttons.top()),
            Vec2::new(half, buttons.height()),
        );
        let response = widgets::button_area(ui, cell, Id::new(("scan", index)), text);
        let base = if plugins.scanning { theme::panel_background() } else { theme::control_background() };
        let fill = motion::mix(
            base,
            theme::control_hover(),
            if plugins.scanning { 0.0 } else { widgets::hover(ui, &response) },
        );
        painter.rect_filled(cell, 0.0, fill);
        let color = if plugins.scanning { theme::text_hint() } else { theme::text_primary() };
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, small.clone(), color);
        if response.clicked() && !plugins.scanning {
            request = Some(PluginRequest::Scan { all });
        }
    }

    // What was found, in the space left.
    let list =
        Rect::from_min_max(Pos2::new(page.left(), *y + 4.0), Pos2::new(page.right(), (bottom - 12.0).max(*y + 40.0)));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for [name, vendor, format, version] in &plugins.found {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).font(small.clone()).color(theme::text_primary()));
                    ui.label(
                        egui::RichText::new(format!("{vendor} · {format} {version}"))
                            .font(small.clone())
                            .color(theme::text_hint()),
                    );
                });
            }
            for (file, why) in &plugins.failed {
                ui.label(egui::RichText::new(format!("{file}: {why}")).font(small.clone()).color(theme::recording()));
            }
        });
    });
    request
}

/// Settings › Audio: host, device, sample rate and buffer size as dropdowns of what the driver
/// allows, exclusive mode, and what plays now with its latency and underruns. Returns a new
/// choice, and whether the devices should be listed again.
fn audio_page(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    settings: &Settings,
    about: &About,
) -> (Option<suhina_session::OutputChoice>, bool) {
    use suhina_session::audio::{DeviceInfo, HostInfo};
    let section = |y: &mut f32, name: &str| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SIDEBAR_ROW));
        widgets::text_in(
            painter,
            rect,
            Align2::LEFT_CENTER,
            name,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(theme::border()));
        *y = rect.bottom() + 8.0;
    };
    let row = |y: &mut f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SETTING_ROW));
        *y += SETTING_ROW;
        rect
    };
    let choice = settings.output_choice();
    let mut picked: Option<suhina_session::OutputChoice> = None;
    let default_host = about.audio_hosts.iter().find(|h| h.default).or(about.audio_hosts.first());
    let host: Option<&HostInfo> = match &choice.host {
        Some(name) => about.audio_hosts.iter().find(|h| h.name.eq_ignore_ascii_case(name)),
        None => default_host,
    };
    let device: Option<&DeviceInfo> = host.and_then(|h| match &choice.device {
        Some(name) => h.devices.iter().find(|d| d.name.eq_ignore_ascii_case(name)),
        None => h.devices.iter().find(|d| d.default).or(h.devices.first()),
    });

    section(y, "Output");
    // Host.
    let r = row(y);
    label(painter, r, "Driver", "WASAPI is Windows' own; ASIO talks to the interface's driver directly");
    let mut options: Vec<(String, Option<String>)> =
        vec![(format!("Default ({})", default_host.map_or("none", |h| h.name.as_str())), None)];
    options.extend(about.audio_hosts.iter().map(|h| (h.name.clone(), Some(h.name.clone()))));
    if let Some(host) = choice_dropdown(ui, painter, r, "Driver", &options, &choice.host) {
        picked = choice.with_params(&serde_json::json!({ "host": host.unwrap_or_default() })).ok();
    }
    // Device.
    let r = row(y);
    let note = match (&choice.device, device) {
        (Some(name), None) => format!("{name} is not there; the default output plays"),
        _ => "The output device on that driver".to_string(),
    };
    label(painter, r, "Device", &note);
    let default_device = host.and_then(|h| h.devices.iter().find(|d| d.default)).map_or("none", |d| d.name.as_str());
    let mut options: Vec<(String, Option<String>)> = vec![(format!("Default ({default_device})"), None)];
    if let Some(host) = host {
        options.extend(host.devices.iter().map(|d| (d.name.clone(), Some(d.name.clone()))));
    }
    if let Some(name) = choice_dropdown(ui, painter, r, "Device", &options, &choice.device) {
        picked = choice.with_params(&serde_json::json!({ "device": name.unwrap_or_default() })).ok();
    }
    // Sample rate.
    let r = row(y);
    label(painter, r, "Sample rate", "The rates the device offers");
    let default_rate = device.and_then(|d| d.default_sample_rate);
    let mut options: Vec<(String, Option<u32>)> =
        vec![(default_rate.map_or_else(|| "Device's own".to_string(), |r| format!("Device's own ({r} Hz)")), None)];
    if let Some(device) = device {
        options.extend(device.sample_rates.iter().map(|r| (format!("{r} Hz"), Some(*r))));
    }
    if let Some(rate) = choice_dropdown(ui, painter, r, "Sample rate", &options, &choice.sample_rate) {
        picked = choice.with_params(&serde_json::json!({ "sample_rate": rate.unwrap_or(0) })).ok();
    }
    // The buffer row says what the number is, and it is the device's own buffer only where it is
    // asked of the driver: ASIO (when its driver declared a size the engine can render in) and
    // WASAPI exclusive mode. Everywhere else it is Suhina's render block alone: WASAPI shared
    // mode plays the period Windows gives the device, whatever is asked, and a driver that
    // declared no size Suhina can render in keeps its own (ADR 0006). The exclusive row below is
    // WASAPI's alone.
    let wasapi = host.is_some_and(|h| h.name == "WASAPI");
    let sizes = device.map_or_else(Vec::new, |d| d.buffer_sizes.clone());
    let declares = device.and_then(|d| d.buffer_range);
    let exclusive = wasapi && choice.exclusive && device.is_some_and(|d| d.exclusive);
    // The number is the device's own only where one of its declared sizes is asked of it.
    let device_sets = exclusive || (!wasapi && declares.is_some() && !sizes.is_empty());
    // The period Windows gives the device is a duration, but WASAPI reports it in frames at the
    // device's own rate, so it is said in milliseconds: the same duration at any rate.
    let period_ms = device
        .and_then(|d| Some((d.buffer_range?, d.default_sample_rate?)))
        .filter(|((min, max), _)| min == max)
        .map(|((min, _), rate)| f64::from(min) * 1000.0 / f64::from(rate.max(1)));
    let r = row(y);
    let rate = f64::from(choice.sample_rate.or(default_rate).unwrap_or(48_000));
    let (name, note) = if exclusive {
        ("Buffer size", "The device's own buffer; Windows asks the device for it".to_string())
    } else if device_sets {
        let note = match declares {
            Some((min, max)) if min < max => {
                format!("The device's own buffer, from its driver: {min} to {max} frames")
            }
            Some((min, _)) => format!("The device's own buffer; its driver allows {min} frames only"),
            None => "The device's own buffer; its driver sets the size".to_string(),
        };
        ("Buffer size", note)
    } else if wasapi {
        let note = match period_ms {
            Some(ms) => format!("Suhina's render block; Windows plays this device in {ms:.1} ms periods"),
            None => "Suhina's render block; Windows sets the device's buffer".to_string(),
        };
        ("Engine block", note)
    } else {
        ("Engine block", "Suhina's render block; the driver keeps its own buffer size".to_string())
    };
    label(painter, r, name, &note);
    let mut options: Vec<(String, Option<u32>)> = vec![("Default (1024 frames)".to_string(), None)];
    options.extend(sizes.iter().map(|n| (format!("{n} frames ({:.1} ms)", f64::from(*n) * 1000.0 / rate), Some(*n))));
    if let Some(frames) = choice_dropdown(ui, painter, r, name, &options, &choice.buffer_frames) {
        picked = choice.with_params(&serde_json::json!({ "buffer_frames": frames.unwrap_or(0) })).ok();
    }
    // Exclusive mode, WASAPI only.
    let r = row(y);
    let note = if !wasapi {
        "WASAPI only; ASIO always has the device to itself"
    } else if device.is_some_and(|d| !d.exclusive) {
        "This device does not allow it; it plays shared"
    } else {
        "The device plays only Suhina, with less delay; other programs are not heard on it"
    };
    label(painter, r, "Exclusive mode", note);
    let on_off = [("On", true), ("Off", false)];
    if let Some(on) = segmented(ui, painter, control_rect(r, 2), "Exclusive mode", &on_off, choice.exclusive) {
        picked = choice.with_params(&serde_json::json!({ "exclusive": on })).ok();
    }

    // Input device (ADR 0017): off until chosen, so nothing opens a microphone unasked.
    let r = row(y);
    let inputs = host.map_or(&[][..], |h| h.inputs.as_slice());
    let note = match &choice.input {
        suhina_session::InputChoice::Named(name)
            if !inputs.iter().any(|i| suhina_session::InputChoice::Named(i.name.clone()) == choice.input) =>
        {
            format!("{name} is not there on this driver")
        }
        _ => "What tracks with an audio input hear: microphones, an interface's inputs".to_string(),
    };
    label(painter, r, "Input", &note);
    let default_input = inputs.iter().find(|i| i.default).map_or("none", |i| i.name.as_str());
    let mut options: Vec<(String, suhina_session::InputChoice)> = vec![
        ("Off".to_string(), suhina_session::InputChoice::Off),
        (format!("Default ({default_input})"), suhina_session::InputChoice::Default),
    ];
    options.extend(inputs.iter().map(|i| {
        let label = format!("{} ({} in)", i.name, i.channels);
        (label, suhina_session::InputChoice::Named(i.name.clone()))
    }));
    if let Some(input) = choice_dropdown(ui, painter, r, "Input", &options, &choice.input) {
        picked = choice.with_params(&serde_json::json!({ "input": input.as_text() })).ok();
    }

    *y += 8.0;
    section(y, "Now");
    let r = row(y);
    let (output, latency) = match &about.audio_live {
        Some(info) => (
            now_playing(info, &choice),
            format!(
                "{:.1} ms, {} underrun{}{}",
                info.latency_ms,
                info.underruns,
                if info.underruns == 1 { "" } else { "s" },
                if info.failed { "; the device reported an error" } else { "" }
            ),
        ),
        None => ("None: no project is playing".to_string(), "-".to_string()),
    };
    label(painter, r, "Output", &output);
    let refresh = Rect::from_min_size(Pos2::new(r.right() - 120.0, r.top() + 1.0), Vec2::new(120.0, SEGMENT));
    let response = widgets::button_area(ui, refresh, Id::new("audio-refresh"), "List the audio devices again");
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(refresh, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    widgets::text_in(
        painter,
        refresh,
        Align2::CENTER_CENTER,
        "List devices again",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    let r = row(y);
    label(painter, r, "Latency", &format!("Engine to speaker, measured: {latency}"));
    // A key on a MIDI controller (ADR 0017): the engine keeps less ahead while a track is armed.
    let r = row(y);
    let key = match &about.audio_live {
        Some(info) if info.live => {
            format!("Up to {:.1} ms; a track is armed, so the engine keeps one block ahead", info.live_latency_ms)
        }
        Some(info) => format!(
            "Up to {:.1} ms now; arming a track keeps one block ahead, so less. Smaller buffers are quicker",
            info.live_latency_ms
        ),
        None => "-".to_string(),
    };
    label(painter, r, "Key to sound", &key);
    let r = row(y);
    let input = match &about.audio_live {
        Some(info) => match &info.input {
            Some(name) => format!(
                "{name}, {} channel{}; {} underrun{}",
                info.input_channels,
                if info.input_channels == 1 { "" } else { "s" },
                info.input_underruns,
                if info.input_underruns == 1 { "" } else { "s" }
            ),
            None => "None: no input device is open".to_string(),
        },
        None => "-".to_string(),
    };
    label(painter, r, "Input now", &input);
    (picked, response.clicked())
}

/// A segmented control for the keymap preset: the two sets of keys, with the chosen one lit.
/// Returns the preset clicked, if it is not the current one.
fn preset_segmented(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    options: &[(&str, Preset)],
    current: Preset,
) -> Option<Preset> {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let count = options.len().max(1) as f32;
    let part = (rect.width() - (count - 1.0)) / count;
    let mut picked = None;
    for (index, (text, value)) in options.iter().enumerate() {
        let x = rect.left() + index as f32 * (part + 1.0);
        let segment = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(part, rect.height()));
        let chosen = *value == current;
        let response = widgets::button_area(ui, segment, Id::new(("keymap-preset", *text)), format!("Preset: {text}"));
        let fill = if chosen { theme::control_active() } else { theme::control_background() };
        painter.rect_filled(segment, 0.0, motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)));
        let color = if chosen { theme::text_primary() } else { theme::text_secondary() };
        widgets::text_in(painter, segment, Align2::CENTER_CENTER, text, small.clone(), color);
        if response.clicked() && !chosen {
            picked = Some(*value);
        }
    }
    picked
}

/// Settings › Audio › Now › Output: the device that plays, and, when it plays in place of the
/// chosen one that is not there, which device it goes back to once it is.
fn now_playing(info: &suhina_session::OutputInfo, choice: &suhina_session::OutputChoice) -> String {
    let wanted = choice.device.as_deref().or(choice.host.as_deref());
    match wanted {
        Some(wanted) if info.fallback => format!("{}, until {wanted} is back", info.describe()),
        _ => info.describe(),
    }
}

/// Settings › Audio › MIDI inputs: each port with On and Off. What the ports that are on play
/// goes to the armed tracks. Returns a port turned on or off.
fn midi_section(ui: &Ui, painter: &egui::Painter, page: Rect, y: &mut f32, about: &About) -> Option<(String, bool)> {
    section_title(painter, page, y, "MIDI inputs");
    let mut picked = None;
    let ports = match &about.midi_inputs {
        Ok(ports) if ports.is_empty() => {
            let r = setting_row(page, y);
            label(painter, r, "None", "No MIDI controller is connected; one plugged in is found within seconds");
            return None;
        }
        Ok(ports) => ports,
        Err(e) => {
            let r = setting_row(page, y);
            label(painter, r, "Not available", e);
            return None;
        }
    };
    for port in ports {
        let r = setting_row(page, y);
        let note = match (&port.error, port.enabled, port.open) {
            (Some(e), true, _) => format!("Could not be opened: {e}"),
            (_, true, true) => "Plays the armed tracks".to_string(),
            (_, true, false) => "Opening…".to_string(),
            (_, false, _) => "Off".to_string(),
        };
        label(painter, r, &port.name, &note);
        let on_off = [("On", true), ("Off", false)];
        let name = format!("MIDI input {}", port.name);
        if let Some(on) = segmented(ui, painter, control_rect(r, 2), &name, &on_off, port.enabled) {
            picked = Some((port.name.clone(), on));
        }
    }
    picked
}

/// A section's title over a hairline, as the other pages draw it.
fn section_title(painter: &egui::Painter, page: Rect, y: &mut f32, name: &str) {
    let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SIDEBAR_ROW));
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
    painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(theme::border()));
    *y = rect.bottom() + 8.0;
}

/// A setting's row under `y`, moving `y` past it.
fn setting_row(page: Rect, y: &mut f32) -> Rect {
    let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SETTING_ROW));
    *y += SETTING_ROW;
    rect
}

/// A dropdown on the right of `row` listing `options` (text, value). Returns the value picked,
/// if it is not the current one.
fn choice_dropdown<T: Clone + PartialEq>(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    name: &str,
    options: &[(String, T)],
    current: &T,
) -> Option<T> {
    let field = Rect::from_min_size(
        Pos2::new(row.right() - DROPDOWN_FIELD, row.top() + 1.0),
        Vec2::new(DROPDOWN_FIELD, SEGMENT + 2.0),
    );
    let shown = options.iter().find(|(_, v)| v == current).map_or("-", |(text, _)| text.as_str());
    let response = widgets::button_area(ui, field, Id::new(("audio-setting", name)), format!("{name}: {shown}"));
    widgets::dropdown(ui, painter, field, &response, shown);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        for (label, value) in options {
            if ui.selectable_label(value == current, text(label)).clicked() && value != current {
                picked = Some(value.clone());
            }
        }
    });
    picked
}

/// A setting's name and a note under it, on the left of its row.
fn label(painter: &egui::Painter, row: Rect, name: &str, note: &str) {
    let top = Rect::from_min_size(row.min, Vec2::new(row.width(), 16.0));
    let bottom = Rect::from_min_size(Pos2::new(row.left(), top.bottom()), Vec2::new(row.width() - LABEL_WIDTH, 14.0));
    widgets::text_in(painter, top, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), theme::text_primary());
    widgets::text_in(
        painter,
        bottom,
        Align2::LEFT_CENTER,
        note,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// Where a segmented control with `parts` parts sits on the right of a row.
fn control_rect(row: Rect, parts: usize) -> Rect {
    let width = 64.0 * parts as f32;
    Rect::from_min_size(Pos2::new(row.right() - width, row.top() + 1.0), Vec2::new(width, SEGMENT))
}

/// A segmented control (components › Dropdown and segmented control). Returns the option
/// clicked, if it is not the current one.
fn segmented<T: Copy + PartialEq>(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    options: &[(&str, T)],
    current: T,
) -> Option<T> {
    let count = options.len().max(1) as f32;
    let part = (rect.width() - (count - 1.0)) / count;
    let mut picked = None;
    for (index, (text, value)) in options.iter().enumerate() {
        let x = rect.left() + index as f32 * (part + 1.0);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(part, rect.height()));
        let response = widgets::button_area(ui, cell, Id::new(("setting", name, index)), format!("{name}: {text}"));
        if response.clicked() && *value != current {
            picked = Some(*value);
        }
        let selected = motion::toward(ui.ctx(), response.id.with("selected"), *value == current, motion::VALUE);
        let hover = widgets::hover(ui, &response);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), hover);
        painter.rect_filled(cell, 0.0, motion::mix(idle, theme::text_value(), selected));
        let color = motion::mix(theme::text_secondary(), theme::window_background(), selected);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_now_says_which_chosen_device_it_waits_for() {
        let info = suhina_session::OutputInfo {
            host: "WASAPI".into(),
            device: "Speakers".into(),
            sample_rate: 48_000,
            engine_block: 512,
            ..Default::default()
        };
        let usb = suhina_session::OutputChoice { device: Some("USB Audio".into()), ..Default::default() };
        assert_eq!(now_playing(&info, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames)");
        let waiting = suhina_session::OutputInfo { fallback: true, ..info };
        assert_eq!(now_playing(&waiting, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames), until USB Audio is back");
        let asio = suhina_session::OutputChoice { host: Some("ASIO".into()), ..Default::default() };
        assert!(now_playing(&waiting, &asio).ends_with("until ASIO is back"));
    }
}
