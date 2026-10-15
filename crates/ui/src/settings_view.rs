//! The Settings page: replaces the main view until it is closed (the design brief keeps one
//! window and no dialogs). Its pages are listed down the left; each setting is a row: a label, a
//! short note and its control.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::keymap::{Keymap, Shortcut, actions};
use crate::motion;
use crate::settings::{DeviceLayout, EditorPlacement, Graphics, MixerControls, OnOpen, Settings, Side, StopReturn};
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
/// Themes shown before their list scrolls.
const THEME_ROWS: usize = 8;
/// A dropdown on the right of its row (Settings › Audio).
const DROPDOWN_FIELD: f32 = 260.0;

/// Facts the page shows but does not change.
#[derive(Debug, Clone, Default)]
pub struct About {
    /// What the active project plays on now, read every frame (`audio.info`).
    pub audio_live: Option<suhina_session::OutputInfo>,
    /// The audio hosts and their output devices, listed when Settings › Audio opens or is
    /// refreshed (`audio.devices`).
    pub audio_hosts: Vec<suhina_session::audio::HostInfo>,
    /// How the window is drawn right now.
    pub graphics_now: Option<Graphics>,
    /// Plug-in editors can open inside the device panel here (the window is a Win32 or X11
    /// one); elsewhere the setting for it is shown but does nothing.
    pub editors_embeddable: bool,
    /// The folders projects are saved in and templates kept in when the settings leave them
    /// empty.
    pub projects_default: String,
    pub templates_default: String,
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
            let folders = [
                (
                    "Projects folder",
                    "Where a new project is saved first",
                    &mut settings.projects_folder,
                    &about.projects_default,
                ),
                (
                    "Templates folder",
                    "Projects listed under New › From a template, and where Save as template puts one",
                    &mut settings.templates_folder,
                    &about.templates_default,
                ),
            ];
            for (name, note, value, default) in folders {
                let r = row(&mut y);
                // The note is cut where the field starts.
                let left =
                    Rect::from_min_size(r.min, Vec2::new(r.width() - FOLDER_FIELD - 8.0 + LABEL_WIDTH, r.height()));
                label(&painter, left, name, note);
                let field = Rect::from_min_size(
                    Pos2::new(r.right() - FOLDER_FIELD, r.top()),
                    Vec2::new(FOLDER_FIELD, SEGMENT + 2.0),
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
                "Stop",
                "Back to the play start marker, or stay where it stopped; stop again for the start",
            );
            let options = [("Marker", StopReturn::Marker), ("Stay", StopReturn::Here)];
            if let Some(choice) = segmented(ui, &painter, control_rect(r, 2), "Stop", &options, settings.stop) {
                settings.stop = choice;
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
            let r = row(&mut y);
            label(
                &painter,
                r,
                "Mixer controls",
                "A track's volume and pan as a field, or as a knob: the wheel over either changes it",
            );
            let options = [("Field", MixerControls::Field), ("Knob", MixerControls::Knob)];
            if let Some(controls) =
                segmented(ui, &painter, control_rect(r, 2), "Mixer controls", &options, settings.mixer_controls)
            {
                settings.mixer_controls = controls;
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
    let mut top = head.bottom() + 4.0;
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
    // Buffer size.
    let r = row(y);
    let rate = f64::from(choice.sample_rate.or(default_rate).unwrap_or(48_000));
    label(painter, r, "Buffer size", "The engine renders in blocks of it; smaller is quicker, larger is safer");
    let mut options: Vec<(String, Option<u32>)> = vec![("Default (1024 frames)".to_string(), None)];
    let sizes = device.map_or_else(Vec::new, |d| d.buffer_sizes.clone());
    options.extend(sizes.iter().map(|n| (format!("{n} frames ({:.1} ms)", f64::from(*n) * 1000.0 / rate), Some(*n))));
    if let Some(frames) = choice_dropdown(ui, painter, r, "Buffer size", &options, &choice.buffer_frames) {
        picked = choice.with_params(&serde_json::json!({ "buffer_frames": frames.unwrap_or(0) })).ok();
    }
    // Exclusive mode, WASAPI only.
    let wasapi = host.is_some_and(|h| h.name == "WASAPI");
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

    *y += 8.0;
    section(y, "Now");
    let r = row(y);
    let (output, latency) = match &about.audio_live {
        Some(info) => (
            info.describe(),
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
    (picked, response.clicked())
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
