//! The Settings page: replaces the main view until it is closed (the design brief keeps one
//! window and no dialogs). Each setting is a row: a label, a short note and its control.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::motion;
use crate::settings::{Graphics, OnOpen, Settings, Side};
use crate::theme::{self, PAD};
use crate::widgets;

const PAGE_WIDTH: f32 = 560.0;
const LABEL_WIDTH: f32 = 180.0;
const SETTING_ROW: f32 = 34.0;
const SECTION_GAP: f32 = 18.0;
const SEGMENT: f32 = 15.0;

/// Facts the page shows but does not change.
#[derive(Debug, Clone, Default)]
pub struct About {
    /// The audio output in use, if any.
    pub audio_output: Option<String>,
    /// How the window is drawn right now.
    pub graphics_now: Option<Graphics>,
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

/// What the user did on the page this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub changed: bool,
    pub close: bool,
    pub plugins: Option<PluginRequest>,
}

/// Draws the page into the space left in `ui`.
pub fn show(ui: &mut Ui, settings: &mut Settings, about: &About, plugins: &mut PluginsView) -> Outcome {
    let mut outcome = Outcome::default();
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::SURFACE_0);

    let page = Rect::from_min_max(
        Pos2::new(area.center().x - PAGE_WIDTH / 2.0, area.top()),
        Pos2::new(area.center().x + PAGE_WIDTH / 2.0, area.bottom()),
    )
    .intersect(area.shrink2(Vec2::new(PAD * 2.0, 0.0)));
    let mut y = area.top() + 24.0;

    // Title and close.
    let title = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), theme::ROW));
    widgets::text_in(
        &painter,
        title,
        Align2::LEFT_CENTER,
        "Settings",
        theme::ui_font(theme::TEXT + 2.0),
        theme::TEXT_PRIMARY,
    );
    let close = Rect::from_min_max(Pos2::new(title.right() - 52.0, title.top()), title.max);
    let response = widgets::button_area(ui, close, Id::new("settings-close"), "Close settings");
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(close, 0.0, motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, hover));
    widgets::text_in(&painter, close, Align2::CENTER_CENTER, "Close", theme::ui_font(theme::TEXT), theme::TEXT_PRIMARY);
    if response.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome.close = true;
    }
    y = title.bottom() + SECTION_GAP;

    let section = |y: &mut f32, name: &str| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), theme::ROW));
        widgets::text_in(
            &painter,
            rect,
            Align2::LEFT_BOTTOM,
            name,
            theme::ui_font(theme::TEXT_SMALL),
            theme::TEXT_TERTIARY,
        );
        painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(theme::LINE));
        *y = rect.bottom() + 8.0;
    };
    let row = |y: &mut f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SETTING_ROW));
        *y += SETTING_ROW;
        rect
    };

    section(&mut y, "Interface");
    let r = row(&mut y);
    label(&painter, r, "Animations", "Short fades and slides when things change");
    if let Some(on) =
        segmented(ui, &painter, control_rect(r, 2), "Animations", &[("On", true), ("Off", false)], settings.animations)
    {
        settings.animations = on;
        outcome.changed = true;
    }
    y += SECTION_GAP;

    section(&mut y, "Projects");
    let r = row(&mut y);
    label(&painter, r, "When opening a project", "While another is open: ask, open it in a new tab, or replace it");
    let options = [("Ask", OnOpen::Ask), ("New tab", OnOpen::NewTab), ("Replace", OnOpen::Replace)];
    if let Some(choice) =
        segmented(ui, &painter, control_rect(r, 3), "When opening a project", &options, settings.on_open)
    {
        settings.on_open = choice;
        outcome.changed = true;
    }
    y += SECTION_GAP;

    section(&mut y, "Arrangement");
    let r = row(&mut y);
    label(&painter, r, "Track headers", "Which side of the timeline they are on");
    if let Some(side) = segmented(
        ui,
        &painter,
        control_rect(r, 2),
        "Track headers",
        &[("Left", Side::Left), ("Right", Side::Right)],
        settings.track_headers,
    ) {
        settings.track_headers = side;
        outcome.changed = true;
    }
    let r = row(&mut y);
    label(&painter, r, "Allow sample stacking", "Audio clips moved over others all play, instead of replacing them");
    let options = [("On", true), ("Off", false)];
    if let Some(on) =
        segmented(ui, &painter, control_rect(r, 2), "Allow sample stacking", &options, settings.sample_stacking)
    {
        settings.sample_stacking = on;
        outcome.changed = true;
    }
    let r = row(&mut y);
    label(&painter, r, "Allow MIDI stacking", "MIDI clips moved over others all play, instead of replacing them");
    if let Some(on) =
        segmented(ui, &painter, control_rect(r, 2), "Allow MIDI stacking", &options, settings.midi_stacking)
    {
        settings.midi_stacking = on;
        outcome.changed = true;
    }
    y += SECTION_GAP;

    section(&mut y, "Graphics");
    let r = row(&mut y);
    let note = match about.graphics_now {
        Some(now) if now != settings.graphics => "Applies the next time Loopwerk starts",
        _ => "How the window is drawn; OpenGL can resize more smoothly on some systems",
    };
    label(&painter, r, "Renderer", note);
    if let Some(graphics) = segmented(
        ui,
        &painter,
        control_rect(r, 2),
        "Renderer",
        &[("GPU", Graphics::Gpu), ("OpenGL", Graphics::OpenGl)],
        settings.graphics,
    ) {
        settings.graphics = graphics;
        outcome.changed = true;
    }
    y += SECTION_GAP;

    section(&mut y, "Audio");
    let r = row(&mut y);
    let output = about.audio_output.as_deref().unwrap_or("None: no project is playing");
    label(&painter, r, "Output", output);
    y += SECTION_GAP;

    section(&mut y, "Plug-ins");
    outcome.plugins = plugin_section(ui, &painter, page, &mut y, area.bottom(), plugins);
    outcome
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
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Folders", theme::ui_font(theme::TEXT), theme::TEXT_PRIMARY);
    for folder in &plugins.standard {
        let r = line(y, 16.0);
        let text = format!("{folder}  (standard)");
        widgets::text_in(painter, r, Align2::LEFT_CENTER, &text, small.clone(), theme::TEXT_TERTIARY);
    }
    for folder in &plugins.added {
        let r = line(y, 18.0);
        let remove =
            Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top() + 1.0), Pos2::new(r.right(), r.bottom() - 1.0));
        widgets::text_in(painter, r, Align2::LEFT_CENTER, folder, small.clone(), theme::TEXT_SECONDARY);
        let response = widgets::button_area(ui, remove, Id::new(("remove-folder", folder)), format!("Remove {folder}"));
        let fill = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response));
        painter.rect_filled(remove, 0.0, fill);
        widgets::text_in(painter, remove, Align2::CENTER_CENTER, "Remove", small.clone(), theme::TEXT_SECONDARY);
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
    let response = widgets::button_area(ui, add, Id::new("add-folder"), "Add folder");
    let fill = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response));
    painter.rect_filled(add, 0.0, fill);
    widgets::text_in(painter, add, Align2::CENTER_CENTER, "Add", small.clone(), theme::TEXT_PRIMARY);
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
        let base = if plugins.scanning { theme::SURFACE_1 } else { theme::SURFACE_2 };
        let fill = motion::mix(
            base,
            theme::HOVER_ON_SURFACE_2,
            if plugins.scanning { 0.0 } else { widgets::hover(ui, &response) },
        );
        painter.rect_filled(cell, 0.0, fill);
        let color = if plugins.scanning { theme::TEXT_TERTIARY } else { theme::TEXT_PRIMARY };
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
                    ui.label(egui::RichText::new(name).font(small.clone()).color(theme::TEXT_PRIMARY));
                    ui.label(
                        egui::RichText::new(format!("{vendor} · {format} {version}"))
                            .font(small.clone())
                            .color(theme::TEXT_TERTIARY),
                    );
                });
            }
            for (file, why) in &plugins.failed {
                ui.label(egui::RichText::new(format!("{file}: {why}")).font(small.clone()).color(theme::RECORDING));
            }
        });
    });
    request
}

/// A setting's name and a note under it, on the left of its row.
fn label(painter: &egui::Painter, row: Rect, name: &str, note: &str) {
    let top = Rect::from_min_size(row.min, Vec2::new(row.width(), 16.0));
    let bottom = Rect::from_min_size(Pos2::new(row.left(), top.bottom()), Vec2::new(row.width() - LABEL_WIDTH, 14.0));
    widgets::text_in(painter, top, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), theme::TEXT_PRIMARY);
    widgets::text_in(
        painter,
        bottom,
        Align2::LEFT_CENTER,
        note,
        theme::ui_font(theme::TEXT_SMALL),
        theme::TEXT_SECONDARY,
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
        let idle = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, hover);
        painter.rect_filled(cell, 0.0, motion::mix(idle, theme::TEXT_VALUE, selected));
        let color = motion::mix(theme::TEXT_SECONDARY, theme::SURFACE_0, selected);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}
