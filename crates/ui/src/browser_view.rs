//! Draws the browser panel (`docs/spec/browser.md` › Layout; sizes from
//! `docs/design/components.md` › Browser): the search, the category filters, the sidebar of
//! places, the results, the preview strip and the footer. Library changes and previews run
//! their commands (`library.*`, `preview.*`); adding to the project is left to the window,
//! which knows the selected track.

use std::path::PathBuf;

use egui::{Align2, Color32, CursorIcon, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use loopwerk_engine::model::TrackId;
use loopwerk_library::{FolderState, Group, Item};
use loopwerk_session::{Category, Projects};
use serde_json::json;

use crate::app::Status;
use crate::browser::{BrowserState, Place, Row, duration_text};
use crate::drop::{BrowserItem, ItemKind};
use crate::settings::Settings;
use crate::theme::{self, PAD};
use crate::{motion, widgets};

pub const SEARCH: f32 = 26.0;
pub const FILTER: f32 = 16.0;
pub const SIDEBAR: f32 = 104.0;
pub const ROW: f32 = 17.0;
pub const INDENT: f32 = 12.0;
pub const PREVIEW: f32 = 54.0;
pub const FOOTER: f32 = 20.0;
const FILTER_PAD: f32 = 7.0;
const FILTER_GAP: f32 = 2.0;
const DOT: f32 = 6.0;
const PLAY: f32 = 22.0;
/// The List | Map switch above the results.
const MODE_SWITCH: f32 = 22.0;

/// What the window has to do for the browser.
#[derive(Debug, Default)]
pub struct BrowserOut {
    /// Items to add to the selected track (Enter, double-click).
    pub add: Vec<BrowserItem>,
    /// A project file to open.
    pub open_project: Option<PathBuf>,
    pub status: Option<Status>,
}

/// Draws the results area another way: the area, the browser's state and the projects.
pub type OtherResults<'a> = dyn FnMut(&mut Ui, Rect, &mut BrowserState, &mut Projects) + 'a;

/// What the browser needs from the window besides its own state.
pub struct Context<'a> {
    pub projects: &'a mut Projects,
    pub settings: &'a Settings,
    /// The track a MIDI file previews through.
    pub selected_track: Option<TrackId>,
    /// The key shown in the search field (Ctrl+F).
    pub search_key: String,
    /// Draws the results area another way while [`BrowserState::mode`] is not the list (a
    /// view of the files laid out by how they sound, for one).
    pub other_results: Option<&'a mut OtherResults<'a>>,
}

fn icon_color(kind: ItemKind) -> Color32 {
    let rgb = |c: loopwerk_engine::model::Rgb| Color32::from_rgb(c.0, c.1, c.2);
    match kind {
        ItemKind::Midi => rgb(crate::grid::TRACK_PALETTE[1]),
        ItemKind::Sample => rgb(crate::grid::TRACK_PALETTE[2]),
        ItemKind::Instrument | ItemKind::Preset => rgb(crate::grid::TRACK_PALETTE[3]),
        ItemKind::Effect => rgb(crate::grid::TRACK_PALETTE[0]),
        ItemKind::Folder | ItemKind::Heading | ItemKind::Project => theme::text_secondary(),
    }
}

/// Our own simple stroke icons, 12 px.
fn icon(painter: &egui::Painter, rect: Rect, kind: ItemKind, color: Color32) {
    let stroke = Stroke::new(1.1, color);
    let c = rect.center();
    match kind {
        ItemKind::Folder | ItemKind::Heading => {
            let body = Rect::from_center_size(c + Vec2::new(0.0, 1.0), Vec2::new(10.0, 7.0));
            painter.rect_stroke(body, 0.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([body.left_top(), body.left_top() + Vec2::new(4.0, 0.0)], stroke);
            painter
                .line_segment([body.left_top() + Vec2::new(0.0, -1.5), body.left_top() + Vec2::new(3.5, -1.5)], stroke);
        }
        ItemKind::Sample => {
            for (dx, h) in [(-4.0, 2.0), (-2.0, 5.0), (0.0, 8.0), (2.0, 4.0), (4.0, 2.5)] {
                painter.line_segment([c + Vec2::new(dx, -h / 2.0), c + Vec2::new(dx, h / 2.0)], stroke);
            }
        }
        ItemKind::Midi => {
            painter.circle_stroke(c + Vec2::new(-1.5, 2.5), 2.0, stroke);
            painter.line_segment([c + Vec2::new(0.5, 2.5), c + Vec2::new(0.5, -4.5)], stroke);
            painter.line_segment([c + Vec2::new(0.5, -4.5), c + Vec2::new(3.5, -3.0)], stroke);
        }
        ItemKind::Instrument | ItemKind::Effect => {
            let body = Rect::from_center_size(c, Vec2::splat(9.0));
            painter.rect_stroke(body, 0.0, stroke, egui::StrokeKind::Inside);
            painter.circle_stroke(c, 2.0, stroke);
        }
        ItemKind::Preset => {
            for (dx, y) in [(-3.0, -1.0), (0.0, 2.0), (3.0, -2.0)] {
                painter.line_segment([c + Vec2::new(dx, -4.5), c + Vec2::new(dx, 4.5)], stroke);
                painter.line_segment([c + Vec2::new(dx - 1.5, y), c + Vec2::new(dx + 1.5, y)], stroke);
            }
        }
        ItemKind::Project => {
            let page = Rect::from_center_size(c, Vec2::new(8.0, 10.0));
            painter.rect_stroke(page, 0.0, stroke, egui::StrokeKind::Inside);
            painter
                .line_segment([page.left_top() + Vec2::new(2.0, 3.5), page.right_top() + Vec2::new(-2.0, 3.5)], stroke);
            painter
                .line_segment([page.left_top() + Vec2::new(2.0, 6.0), page.right_top() + Vec2::new(-2.0, 6.0)], stroke);
        }
    }
}

fn chevron(painter: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![center + Vec2::new(-3.0, -1.5), center + Vec2::new(0.0, 1.5), center + Vec2::new(3.0, -1.5)]
    } else {
        vec![center + Vec2::new(-1.5, -3.0), center + Vec2::new(1.5, 0.0), center + Vec2::new(-1.5, 3.0)]
    };
    painter.add(egui::Shape::line(points, Stroke::new(1.1, color)));
}

fn play_glyph(painter: &egui::Painter, center: Pos2, playing: bool, color: Color32) {
    if playing {
        painter.rect_filled(Rect::from_center_size(center, Vec2::splat(7.0)), 0.0, color);
    } else {
        painter.add(egui::Shape::convex_polygon(
            vec![center + Vec2::new(-3.0, -4.0), center + Vec2::new(4.0, 0.0), center + Vec2::new(-3.0, 4.0)],
            color,
            Stroke::NONE,
        ));
    }
}

fn tag_color(text: &str) -> Color32 {
    theme::parse_hex(text).unwrap_or_else(theme::text_hint)
}

/// Runs a library or preview command for the browser; an error goes to the status bar.
fn run(projects: &mut Projects, id: &str, params: serde_json::Value, out: &mut BrowserOut) -> bool {
    match projects.execute(id, &params) {
        Ok(_) => true,
        Err(e) => {
            out.status = Some(Status::Error(e.0));
            false
        }
    }
}

/// Draws the browser in `area`.
pub fn show(ui: &mut Ui, area: Rect, state: &mut BrowserState, cx: Context<'_>) -> BrowserOut {
    let mut out = BrowserOut::default();
    let Context { projects, settings, selected_track, search_key, other_results } = cx;
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
    state.poll();
    projects.file_library_mut().poll();

    // The search.
    let search = Rect::from_min_size(area.min, Vec2::new(area.width(), SEARCH));
    search_field(ui, search, state, &search_key, &mut out);
    if std::mem::take(&mut state.stop_preview) {
        stop_preview(state, projects);
    }

    // The category filters, wrapping.
    let mut y = search.bottom() + 4.0;
    let mut x = area.left() + PAD;
    let small = theme::ui_font(theme::TEXT_SMALL);
    for category in Category::ALL {
        let width = widgets::text_width(ui, category.title(), small.clone()) + FILTER_PAD * 2.0;
        if x + width > area.right() - PAD && x > area.left() + PAD {
            x = area.left() + PAD;
            y += FILTER + FILTER_GAP;
        }
        let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, FILTER));
        let response = widgets::button_area(
            ui,
            rect,
            Id::new(("browser-filter", category.title())),
            format!("Show {}", category.title()),
        );
        let chosen = state.category == category;
        if response.clicked() && !chosen {
            state.category = category;
            state.scroll = 0.0;
        }
        let lit = motion::toward(ui.ctx(), response.id.with("on"), chosen, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, motion::mix(idle, theme::text_value(), lit));
        let text = motion::mix(theme::text_secondary(), theme::window_background(), lit);
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, category.title(), small.clone(), text);
        x = rect.right() + FILTER_GAP;
    }
    y += FILTER + 6.0;
    painter.hline(area.x_range(), y - 0.5, theme::hairline(theme::border()));

    let bottom = area.bottom() - FOOTER - PREVIEW;
    let sidebar = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.left() + SIDEBAR, bottom));
    let results = Rect::from_min_max(Pos2::new(sidebar.right() + 1.0, y), Pos2::new(area.right(), bottom));
    painter.vline(sidebar.right() + 0.5, sidebar.y_range(), theme::hairline(theme::border()));
    sidebar_view(ui, sidebar, state, projects, &mut out);

    if let Err(e) = state.refresh_rows(projects) {
        out.status = Some(Status::Error(e));
    }
    // With another view to offer, a List | Map switch above the results chooses.
    let results = match &other_results {
        Some(_) => {
            let switch = Rect::from_min_size(results.min, Vec2::new(results.width(), MODE_SWITCH));
            mode_switch(ui, switch, state);
            Rect::from_min_max(Pos2::new(results.left(), switch.bottom()), results.max)
        }
        None => results,
    };
    match (state.mode, other_results) {
        (crate::browser::ResultsMode::Other, Some(draw)) => draw(ui, results, state, projects),
        _ => results_view(ui, results, state, projects, settings, selected_track, &mut out),
    }

    let strip = Rect::from_min_max(Pos2::new(area.left(), bottom), Pos2::new(area.right(), bottom + PREVIEW));
    preview_strip(ui, strip, state, projects, settings, selected_track, &mut out);
    let footer = Rect::from_min_max(Pos2::new(area.left(), strip.bottom()), area.max);
    footer_view(ui, footer, state, projects, &mut out);

    // A selection that changed previews with auto-play.
    if std::mem::take(&mut state.selection_changed) {
        state.forget_unselected();
        let one = match state.selected.as_slice() {
            [one] if matches!(one.kind, ItemKind::Sample | ItemKind::Midi) => Some(one.clone()),
            _ => None,
        };
        match one {
            Some(item) if settings.preview_autoplay => {
                start_preview(state, projects, settings, selected_track, &item, 0.0, &mut out)
            }
            _ => stop_preview(state, projects),
        }
    }
    // A sample decoded for its preview plays now.
    if let Some((path, from, data)) = state.take_ready_preview() {
        if let Ok(session) = projects.session_mut(None) {
            session.provide_audio(&path, data);
        }
        let params = json!({ "path": path.display().to_string(), "from": from, "gain": settings.preview_volume });
        if projects.execute("preview.play", &params).is_ok() {
            state.previewing = Some(path.display().to_string());
        }
    }
    if state.preview_pending() || state.previewing.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    out
}

fn search_field(ui: &mut Ui, rect: Rect, state: &mut BrowserState, key: &str, out: &mut BrowserOut) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::window_background());
    let glass = Pos2::new(rect.left() + PAD + 5.0, rect.center().y);
    painter.circle_stroke(glass + Vec2::new(-1.0, -1.0), 3.5, Stroke::new(1.1, theme::text_hint()));
    painter
        .line_segment([glass + Vec2::new(1.5, 1.5), glass + Vec2::new(4.5, 4.5)], Stroke::new(1.1, theme::text_hint()));
    let hint = theme::mono_font(theme::TEXT_TINY);
    let key_width = widgets::text_width(ui, key, hint.clone());
    let field = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + 14.0, rect.top() + 4.0),
        Pos2::new(rect.right() - PAD - key_width - 6.0, rect.bottom() - 4.0),
    );
    let id = Id::new("browser-search");
    // egui lets go of a field's focus on Escape before the field is drawn, so whether the
    // field had the keyboard is remembered from the frame before.
    let focused = ui.ctx().memory(|m| m.has_focus(id)) || state.search_focused;
    if focused {
        // The list's keys, taken before the field sees them.
        let (down, up, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down || up {
            move_selection(state, if down { 1 } else { -1 });
        }
        if enter && !state.selected.is_empty() {
            out.add = state.selected.clone();
        }
        if escape {
            state.stop_preview = true;
            if state.search.is_empty() {
                ui.ctx().memory_mut(|m| m.surrender_focus(id));
                state.search_focused = false;
            } else {
                state.search.clear();
                // Escape cleared the search; the field keeps the keyboard.
                state.focus_search = true;
            }
        }
    }
    let edit = egui::TextEdit::singleline(&mut state.search)
        .id(id)
        .frame(egui::Frame::NONE)
        .hint_text(egui::RichText::new("Search").color(theme::text_hint()))
        .font(theme::ui_font(theme::TEXT))
        .text_color(theme::text_primary());
    let response = ui.put(field, edit);
    if std::mem::take(&mut state.focus_search) {
        response.request_focus();
    }
    state.search_focused = response.has_focus();
    if response.changed() {
        state.scroll = 0.0;
    }
    widgets::text_in(
        &painter,
        Rect::from_min_max(Pos2::new(field.right(), rect.top()), rect.max - Vec2::new(PAD, 0.0)),
        Align2::RIGHT_CENTER,
        key,
        hint,
        theme::text_hint(),
    );
}

/// Moves the selection `step` rows (from the search field's arrow keys).
fn move_selection(state: &mut BrowserState, step: isize) {
    let rows = state.rows();
    if rows.is_empty() {
        return;
    }
    let current = state.selected.last().and_then(|s| rows.iter().position(|r| r.item.key == s.key));
    let next = match current {
        Some(i) => i.saturating_add_signed(step).min(rows.len() - 1),
        None => 0,
    };
    if let Some(row) = rows.get(next) {
        let item = row.item.clone();
        state.anchor = Some(next);
        state.select(vec![item]);
        state.scroll = state.scroll.min(next as f32 * ROW).max(next as f32 * ROW - ROW * 8.0);
    }
}

fn sidebar_view(ui: &mut Ui, area: Rect, state: &mut BrowserState, projects: &mut Projects, out: &mut BrowserOut) {
    let painter = ui.painter_at(area);
    let title_font = theme::ui_font(theme::TEXT_SMALL);
    let item_font = theme::ui_font(theme::TEXT_SMALL);
    let mut y = area.top();
    let title = |y: &mut f32, text: &str| {
        *y += 8.0;
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, *y), Vec2::new(area.width() - PAD, 12.0));
        widgets::text_in(&painter, rect, Align2::LEFT_CENTER, text, title_font.clone(), theme::text_hint());
        *y += 12.0;
    };
    let mut entry = |ui: &mut Ui, y: &mut f32, place: Place, label: &str, note: Option<&str>| -> egui::Response {
        let rect = Rect::from_min_size(Pos2::new(area.left(), *y), Vec2::new(area.width(), ROW));
        *y += ROW;
        let response = ui.interact(rect.intersect(area), Id::new(("browser-place", place.param())), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Place {label}")));
        let chosen = state.place.as_ref() == Some(&place);
        let fill = if chosen {
            theme::control_active()
        } else {
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response))
        };
        if rect.intersects(area) {
            painter.rect_filled(rect.intersect(area), 0.0, fill);
            let text = Rect::from_min_max(rect.min + Vec2::new(PAD + 2.0, 0.0), rect.max - Vec2::new(PAD, 0.0));
            let color = if chosen { theme::text_primary() } else { theme::text_secondary() };
            widgets::text_in(&painter, text, Align2::LEFT_CENTER, label, item_font.clone(), color);
            if let Some(note) = note {
                widgets::text_in(
                    &painter,
                    text,
                    Align2::RIGHT_CENTER,
                    note,
                    theme::mono_font(theme::TEXT_TINY),
                    theme::text_hint(),
                );
            }
        }
        if response.clicked() {
            state.place = if chosen { None } else { Some(place) };
            state.scroll = 0.0;
        }
        response
    };
    title(&mut y, "Pinned");
    entry(ui, &mut y, Place::Pinned, "Pinned", None);
    title(&mut y, "Recent");
    for group in Group::ALL {
        entry(ui, &mut y, Place::Recent(group), group.title(), None);
    }
    title(&mut y, "Folders");
    let mut menus = Vec::new();
    for folder in projects.file_library_mut().folders() {
        let name = folder
            .path
            .file_name()
            .map_or_else(|| folder.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let note = match folder.state {
            FolderState::Scanning => Some("…"),
            FolderState::Polling => Some("poll"),
            FolderState::Missing => Some("gone"),
            _ => None,
        };
        let response = entry(ui, &mut y, Place::Folder(folder.path.clone()), &name, note);
        menus.push((response, folder.path));
    }
    for (response, folder) in menus {
        let path = folder.display().to_string();
        response.context_menu(|ui| {
            if ui.button("Refresh").clicked() {
                run(projects, "library.refresh", json!({ "folder": path }), out);
                state.relist();
                ui.close();
            }
            if ui.button("Pin").clicked() {
                run(projects, "library.pin", json!({ "item": path }), out);
                ui.close();
            }
            if ui.button("Remove from the browser").clicked() {
                if run(projects, "library.removeFolder", json!({ "folder": path }), out)
                    && state.place == Some(Place::Folder(folder.clone()))
                {
                    state.place = None;
                }
                state.relist();
                ui.close();
            }
        });
    }
    // Adding a folder: a field for its path.
    let rect = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), ROW + 2.0));
    match state.new_folder.as_mut() {
        None => {
            let response = widgets::button_area(ui, rect.intersect(area), Id::new("browser-add-folder"), "Add folder");
            let color = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &response));
            widgets::text_in(
                &painter,
                rect.shrink2(Vec2::new(PAD + 2.0, 0.0)),
                Align2::LEFT_CENTER,
                "+ Add folder",
                item_font.clone(),
                color,
            );
            if response.clicked() {
                state.new_folder = Some(String::new());
            }
        }
        Some(text) => {
            let id = Id::new("browser-new-folder");
            let edit = egui::TextEdit::singleline(text)
                .id(id)
                .hint_text("A folder's path")
                .font(item_font.clone())
                .text_color(theme::text_primary());
            let response = ui.put(rect.shrink2(Vec2::new(4.0, 1.0)).intersect(area), edit);
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if response.lost_focus() || escape {
                let path = text.trim().to_string();
                if enter && !path.is_empty() {
                    if run(projects, "library.addFolder", json!({ "folder": path }), out) {
                        out.status = Some(Status::Done(format!("Added {path} to the browser")));
                    }
                    state.relist();
                }
                state.new_folder = None;
            }
        }
    }
}

fn results_view(
    ui: &mut Ui,
    area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    selected_track: Option<TrackId>,
    out: &mut BrowserOut,
) {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
    let rows: Vec<Row> = state.rows().to_vec();
    if rows.is_empty() {
        let text = if state.search.trim().is_empty() && projects.file_library_mut().folders().is_empty() {
            "Add a folder to find samples"
        } else {
            "Nothing found"
        };
        widgets::text_in(
            &painter,
            area.shrink(PAD),
            Align2::CENTER_TOP,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        return;
    }
    // Scrolling with the wheel over the list.
    let content = rows.len() as f32 * ROW;
    let max_scroll = (content - area.height()).max(0.0);
    if ui.rect_contains_pointer(area) {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        state.scroll -= wheel;
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll);
    let first = (state.scroll / ROW).floor() as usize;
    let visible = (area.height() / ROW).ceil() as usize + 1;
    let tags = projects.file_library_mut().tags();
    let mono = theme::mono_font(theme::TEXT_TINY);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut clicked: Option<(usize, Modifiers)> = None;
    let mut double: Option<usize> = None;
    let mut drag: Option<usize> = None;
    let mut play: Option<usize> = None;
    for index in first..(first + visible).min(rows.len()) {
        let Some(row) = rows.get(index) else { continue };
        let top = area.top() + index as f32 * ROW - state.scroll;
        let rect = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), ROW));
        let shown = rect.intersect(area);
        if shown.height() <= 0.0 {
            continue;
        }
        let response = ui.interact(shown, Id::new(("browser-row", &row.item.key)), Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Browser {}", row.item.name))
        });
        let selected = state.selected.iter().any(|s| s.key == row.item.key);
        let fill = if selected {
            theme::control_active()
        } else {
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response))
        };
        painter.rect_filled(shown, 0.0, fill);
        let mut x = rect.left() + 4.0 + row.depth as f32 * INDENT;
        let lead = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(10.0, ROW));
        let previewable = matches!(row.item.kind, ItemKind::Sample | ItemKind::Midi);
        if row.opens {
            chevron(&painter, lead.center(), row.open, theme::text_hint());
        } else if previewable && (selected || response.hovered()) {
            let button = widgets::button_area(
                ui,
                lead,
                Id::new(("browser-play", &row.item.key)),
                format!("Preview {}", row.item.name),
            );
            let playing = state.previewing.as_deref() == Some(row.item.key.as_str());
            play_glyph(
                &painter,
                lead.center(),
                playing,
                if button.hovered() { theme::text_primary() } else { theme::text_secondary() },
            );
            if button.clicked() {
                play = Some(index);
            }
        }
        x = lead.right() + 2.0;
        let icon_rect = Rect::from_min_size(Pos2::new(x, rect.top() + 2.5), Vec2::splat(12.0));
        icon(&painter, icon_rect, row.item.kind, icon_color(row.item.kind));
        x = icon_rect.right() + 5.0;
        // On the right: a sample's length, a device's kind, where a search found it.
        let meta = match row.item.kind {
            ItemKind::Sample | ItemKind::Midi => {
                row.item.path().and_then(|p| state.info(&p, false).and_then(|i| i.seconds)).map(duration_text)
            }
            ItemKind::Instrument if row.item.plugin => Some("plug-in".into()),
            ItemKind::Effect if row.item.plugin => Some("plug-in".into()),
            _ => None,
        }
        .or_else(|| row.context.clone())
        .unwrap_or_default();
        let meta_width = if meta.is_empty() {
            0.0
        } else {
            widgets::text_width(ui, &meta, mono.clone()).min(area.width() * 0.4) + PAD
        };
        let name_space = (rect.right() - PAD - meta_width - x).max(0.0);
        let item_tags = projects.file_library_mut().item_tags(&row.item.key);
        let dots = item_tags.len() as f32 * (DOT + 2.0);
        let color = if selected { theme::text_primary() } else { theme::text_value() };
        let galley = widgets::fitted(&painter, &row.item.name, font.clone(), color, (name_space - dots).max(0.0));
        let name_pos = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
        let name_end = name_pos.x + galley.size().x;
        painter.galley(name_pos, galley, color);
        for (i, name) in item_tags.iter().enumerate() {
            if let Some(tag) = tags.iter().find(|t| t.name == *name) {
                let center = Pos2::new(name_end + 5.0 + i as f32 * (DOT + 2.0), rect.center().y);
                painter.circle_filled(center, DOT / 2.0, tag_color(&tag.color));
            }
        }
        if !meta.is_empty() {
            let meta_rect =
                Rect::from_min_max(Pos2::new(rect.right() - meta_width, rect.top()), rect.max - Vec2::new(PAD, 0.0));
            widgets::text_in(&painter, meta_rect, Align2::RIGHT_CENTER, &meta, mono.clone(), theme::text_hint());
        }
        if response.clicked() {
            clicked = Some((index, ui.input(|i| i.modifiers)));
        }
        if response.double_clicked() {
            double = Some(index);
        }
        if response.drag_started() {
            drag = Some(index);
        }
        let item = row.item.clone();
        response.context_menu(|ui| row_menu(ui, &item, &tags, state, projects, out));
    }
    if let Some((index, modifiers)) = clicked {
        click_row(state, &rows, index, modifiers);
    }
    if let Some(index) = double
        && let Some(row) = rows.get(index)
    {
        if row.opens {
            state.toggle(&row.item.key);
        } else if row.item.kind == ItemKind::Project {
            out.open_project = row.item.path();
        } else if row.item.kind != ItemKind::Heading && row.item.kind != ItemKind::Folder {
            out.add = vec![row.item.clone()];
        }
    }
    // A click on a chevron opens; a click elsewhere on a folder only selects it.
    if let Some((index, _)) = clicked
        && let Some(row) = rows.get(index)
        && row.opens
        && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < area.left() + 16.0 + row.depth as f32 * INDENT)
    {
        state.toggle(&row.item.key);
    }
    if let Some(index) = play
        && let Some(row) = rows.get(index)
    {
        if state.previewing.as_deref() == Some(row.item.key.as_str()) {
            stop_preview(state, projects);
        } else {
            start_preview(state, projects, settings, selected_track, &row.item.clone(), 0.0, out);
        }
    }
    if let Some(index) = drag
        && let Some(row) = rows.get(index)
    {
        let items = if state.selected.iter().any(|s| s.key == row.item.key) {
            state.selected.clone()
        } else {
            vec![row.item.clone()]
        };
        let items: Vec<BrowserItem> =
            items.into_iter().filter(|i| i.kind != ItemKind::Heading && i.kind != ItemKind::Folder).collect();
        if !items.is_empty() {
            stop_preview(state, projects);
            state.start_drag(items);
        }
    }
    if rows.len() as f32 * ROW > area.height() {
        // A thin bar shows where the list is.
        let share = area.height() / content;
        let bar = Rect::from_min_size(
            Pos2::new(area.right() - 3.0, area.top() + state.scroll * share),
            Vec2::new(2.0, (area.height() * share).max(12.0)),
        );
        painter.rect_filled(bar, 0.0, theme::border_strong());
    }
}

fn click_row(state: &mut BrowserState, rows: &[Row], index: usize, modifiers: Modifiers) {
    let Some(row) = rows.get(index) else { return };
    if row.item.kind == ItemKind::Heading {
        state.toggle(&row.item.key);
        return;
    }
    if modifiers.command {
        let mut selected = state.selected.clone();
        match selected.iter().position(|s| s.key == row.item.key) {
            Some(i) => {
                selected.remove(i);
            }
            None => selected.push(row.item.clone()),
        }
        state.anchor = Some(index);
        state.select(selected);
    } else if modifiers.shift
        && let Some(anchor) = state.anchor
    {
        let (a, b) = (anchor.min(index), anchor.max(index));
        let range: Vec<BrowserItem> = rows
            .get(a..=b)
            .unwrap_or_default()
            .iter()
            .filter(|r| r.item.kind != ItemKind::Heading)
            .map(|r| r.item.clone())
            .collect();
        state.select(range);
    } else {
        state.anchor = Some(index);
        if state.selected.len() != 1 || state.selected.first().map(|s| &s.key) != Some(&row.item.key) {
            state.select(vec![row.item.clone()]);
        }
    }
}

fn row_menu(
    ui: &mut Ui,
    item: &BrowserItem,
    tags: &[loopwerk_library::Tag],
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) {
    if item.kind == ItemKind::Heading {
        return;
    }
    let pinned = projects.file_library_mut().pinned().contains(&item.key);
    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
        run(projects, if pinned { "library.unpin" } else { "library.pin" }, json!({ "item": item.key }), out);
        state.relist();
        ui.close();
    }
    let carried = projects.file_library_mut().item_tags(&item.key);
    ui.menu_button("Tag", |ui| {
        for tag in tags {
            let on = carried.contains(&tag.name);
            let label = format!("{} {}", if on { "✓" } else { "  " }, tag.name);
            if ui.button(label).clicked() {
                run(projects, "library.tag", json!({ "item": item.key, "tag": tag.name, "on": !on }), out);
                state.relist();
                ui.close();
            }
        }
        ui.separator();
        // A new tag, set on the item at once.
        let id = Id::new("browser-new-tag");
        let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let response = ui.add(egui::TextEdit::singleline(&mut name).hint_text("New tag…").desired_width(120.0));
        ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty() {
            let palette = loopwerk_library::store::default_tags();
            let color =
                palette.get(tags.len() % palette.len().max(1)).map_or("#8C8A85".to_string(), |t| t.color.clone());
            let name = name.trim().to_string();
            if run(projects, "library.setTag", json!({ "name": name, "color": color }), out) {
                run(projects, "library.tag", json!({ "item": item.key, "tag": name, "on": true }), out);
            }
            ui.ctx().data_mut(|d| d.remove::<String>(id));
            state.relist();
            ui.close();
        }
    });
    if let Some(Item::File(path)) = item.item()
        && let Some(parent) = path.parent()
        && item.kind != ItemKind::Folder
        && ui.button("Show in folder").clicked()
    {
        state.search.clear();
        state.place = Some(Place::Folder(parent.to_path_buf()));
        ui.close();
    }
    if item.kind == ItemKind::Folder && ui.button("Refresh").clicked() {
        state.relist();
        ui.close();
    }
}

fn start_preview(
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    track: Option<TrackId>,
    item: &BrowserItem,
    from: f64,
    out: &mut BrowserOut,
) {
    let Some(path) = item.path() else { return };
    if active(projects).is_none_or(|s| s.player().is_none()) {
        return;
    }
    match item.kind {
        ItemKind::Sample => {
            state.previewing = None;
            let _ = projects.execute("preview.stop", &json!({}));
            let _ = state.info(&path, true);
            state.want_preview(path, from);
        }
        ItemKind::Midi => {
            state.cancel_preview();
            let mut params =
                json!({ "path": path.display().to_string(), "gain": settings.preview_volume, "from": from });
            if let Some(track) = track {
                params["track"] = json!(track.0);
            }
            if run(projects, "preview.play", params, out) {
                state.previewing = Some(item.key.clone());
            }
        }
        _ => {}
    }
}

/// The List | Map switch.
fn mode_switch(ui: &mut Ui, area: Rect, state: &mut BrowserState) {
    use crate::browser::ResultsMode;
    let painter = ui.painter_at(area);
    painter.hline(area.x_range(), area.bottom() - 0.5, theme::hairline(theme::border()));
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mut x = area.left() + PAD;
    for (mode, name, hint) in [
        (ResultsMode::List, "List", "Show the results as a list"),
        (ResultsMode::Other, "Map", "Show the samples laid out by how they sound"),
    ] {
        let width = widgets::text_width(ui, name, small.clone()) + FILTER_PAD * 2.0;
        let rect = Rect::from_min_size(Pos2::new(x, area.top() + 2.0), Vec2::new(width, FILTER));
        let response = widgets::button_area(ui, rect, Id::new(("browser-mode", name)), hint.to_string());
        let chosen = state.mode == mode;
        if response.clicked() && !chosen {
            state.mode = mode;
        }
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, if chosen { theme::text_value() } else { idle });
        let text = if chosen { theme::window_background() } else { theme::text_secondary() };
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, name, small.clone(), text);
        x = rect.right() + FILTER_GAP;
    }
}

/// Stops any preview.
pub fn stop_preview(state: &mut BrowserState, projects: &mut Projects) {
    let was = state.previewing.is_some() || state.preview_pending();
    state.cancel_preview();
    if was {
        let _ = projects.execute("preview.stop", &json!({}));
    }
}

fn preview_strip(
    ui: &mut Ui,
    area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    track: Option<TrackId>,
    out: &mut BrowserOut,
) {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    let item = match state.selected.as_slice() {
        [one] if matches!(one.kind, ItemKind::Sample | ItemKind::Midi) => one.clone(),
        _ => {
            widgets::text_in(
                &painter,
                area.shrink(PAD),
                Align2::LEFT_CENTER,
                "Select a sample or MIDI file to preview",
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
            return;
        }
    };
    let Some(path) = item.path() else { return };
    let monitor = active(projects)
        .and_then(|s| s.player())
        .map(|p| (p.preview_monitor().playing(), p.preview_monitor().seconds()));
    let playing_this = state.previewing.as_deref() == Some(item.key.as_str()) && monitor.is_some_and(|m| m.0);
    if state.previewing.as_deref() == Some(item.key.as_str())
        && monitor.is_some_and(|m| !m.0)
        && !state.preview_pending()
    {
        state.previewing = None;
    }
    let info = state.info(&path, true).cloned().unwrap_or_default();

    // Top line: play, name, length, tempo, key.
    let top = Rect::from_min_size(area.min + Vec2::new(PAD, 4.0), Vec2::new(area.width() - PAD * 2.0, PLAY));
    let play = Rect::from_min_size(top.min, Vec2::splat(PLAY));
    let response = widgets::button_area(ui, play, Id::new("browser-preview-play"), "Preview");
    painter.rect_filled(
        play,
        0.0,
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
    );
    play_glyph(
        &painter,
        play.center(),
        playing_this,
        if playing_this { theme::accent() } else { theme::text_primary() },
    );
    if response.clicked() {
        if playing_this {
            stop_preview(state, projects);
        } else {
            start_preview(state, projects, settings, track, &item, 0.0, out);
        }
    }
    let mono = theme::mono_font(theme::TEXT_TINY);
    let mut facts = Vec::new();
    if let Some(seconds) = info.seconds {
        facts.push(duration_text(seconds));
    }
    if let Some(tempo) = info.tempo {
        let mark = if tempo.source == loopwerk_media::info::TempoSource::Detected { "~" } else { "" };
        facts.push(format!("{mark}{} BPM", (tempo.bpm * 100.0).round() / 100.0));
    }
    if let Some(key) = &info.key {
        facts.push(key.clone());
    }
    let facts = facts.join("  ");
    let facts_width = widgets::text_width(ui, &facts, mono.clone());
    let text_rect = Rect::from_min_max(Pos2::new(play.right() + PAD, top.top()), top.max);
    widgets::text_in(&painter, text_rect, Align2::RIGHT_CENTER, &facts, mono, theme::text_secondary());
    let name_rect =
        Rect::from_min_max(text_rect.min, Pos2::new(text_rect.right() - facts_width - PAD, text_rect.bottom()));
    widgets::text_in(
        &painter,
        name_rect,
        Align2::LEFT_CENTER,
        &item.name,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );

    // The waveform, or the notes, with the part played brighter and the playhead.
    let wave = Rect::from_min_max(
        Pos2::new(area.left() + PAD, play.bottom() + 4.0),
        Pos2::new(area.right() - PAD, area.bottom() - 4.0),
    );
    let total = info.seconds.unwrap_or(0.0);
    let played = if playing_this { monitor.map_or(0.0, |m| m.1) } else { 0.0 };
    let played_x =
        if total > 0.0 { wave.left() + (played / total).clamp(0.0, 1.0) as f32 * wave.width() } else { wave.left() };
    if let Some(error) = &info.error {
        widgets::text_in(
            &painter,
            wave,
            Align2::LEFT_CENTER,
            &format!("Cannot preview: {error}"),
            theme::ui_font(theme::TEXT_SMALL),
            theme::recording(),
        );
        return;
    }
    if let Some(peaks) = &info.peaks {
        let columns = wave.width().max(1.0) as usize;
        for column in 0..columns {
            let x = wave.left() + column as f32 + 0.5;
            let from = column as f64 / columns as f64 * total;
            let to = (column + 1) as f64 / columns as f64 * total;
            let (low, high) = peaks.range(from, to);
            let mid = wave.center().y;
            let half = wave.height() / 2.0;
            let color = if x <= played_x { theme::text_secondary() } else { theme::text_hint() };
            painter.vline(
                x,
                (mid - high.clamp(-1.0, 1.0) * half)..=(mid - low.clamp(-1.0, 1.0) * half).max(mid - high * half + 1.0),
                Stroke::new(1.0, color),
            );
        }
    } else if let Some(notes) = &info.notes {
        let (low, high) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.2), h.max(n.2)));
        let span = f32::from(high.saturating_sub(low).max(1));
        let beats = info.beats.max(1.0);
        for (start, length, key) in notes.iter() {
            let x0 = wave.left() + (*start / beats) as f32 * wave.width();
            let x1 = wave.left() + ((start + length) / beats) as f32 * wave.width();
            let y = wave.bottom() - f32::from(key - low) / span * (wave.height() - 2.0) - 2.0;
            let color = if x0 <= played_x { theme::text_secondary() } else { theme::text_hint() };
            painter.rect_filled(Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.0), y + 2.0)), 0.0, color);
        }
    } else {
        widgets::text_in(
            &painter,
            wave,
            Align2::LEFT_CENTER,
            "Reading…",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    if playing_this {
        painter.vline(played_x, wave.y_range(), Stroke::new(1.0, theme::accent()));
    }
    // A click on the waveform previews from there.
    let response =
        ui.interact(wave, Id::new("browser-preview-wave"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    if response.clicked()
        && total > 0.0
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let from = f64::from((pointer.x - wave.left()) / wave.width().max(1.0)) * total;
        start_preview(state, projects, settings, track, &item, from.max(0.0), out);
    }
}

fn footer_view(ui: &mut Ui, area: Rect, state: &mut BrowserState, projects: &mut Projects, out: &mut BrowserOut) {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    let mut x = area.left() + PAD;
    for tag in projects.file_library_mut().tags() {
        let rect = Rect::from_center_size(Pos2::new(x + DOT / 2.0 + 2.0, area.center().y), Vec2::splat(DOT + 4.0));
        x = rect.right() + 2.0;
        if x > area.right() - 90.0 {
            break;
        }
        let response = widgets::button_area(ui, rect, Id::new(("browser-tag", &tag.name)), format!("Tag {}", tag.name))
            .on_hover_text(&tag.name);
        let on = state.tag_filter.contains(&tag.name);
        if response.clicked() {
            if on {
                state.tag_filter.retain(|t| *t != tag.name);
            } else {
                state.tag_filter.push(tag.name.clone());
            }
        }
        // Right-click: rename, recolour or remove the tag.
        response.context_menu(|ui| {
            let id = Id::new(("browser-rename-tag", &tag.name));
            let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| tag.name.clone());
            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
            ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && name.trim() != tag.name {
                if run(projects, "library.setTag", json!({ "name": tag.name, "rename": name.trim() }), out) {
                    for filter in state.tag_filter.iter_mut().filter(|f| **f == tag.name) {
                        *filter = name.trim().to_string();
                    }
                }
                ui.ctx().data_mut(|d| d.remove::<String>(id));
                state.relist();
                ui.close();
            }
            let mut color = tag_color(&tag.color);
            if egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::Opaque).changed()
            {
                let hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
                run(projects, "library.setTag", json!({ "name": tag.name, "color": hex }), out);
            }
            if ui.button("Remove tag").clicked() {
                if run(projects, "library.removeTag", json!({ "name": tag.name }), out) {
                    state.tag_filter.retain(|t| *t != tag.name);
                }
                state.relist();
                ui.close();
            }
        });
        painter.circle_filled(rect.center(), DOT / 2.0, tag_color(&tag.color));
        if on {
            painter.circle_stroke(rect.center(), DOT / 2.0 + 1.5, Stroke::new(1.0, theme::text_primary()));
        }
    }
    let count = match state.selected.len() {
        0 | 1 => format!("{} items", state.rows().iter().filter(|r| r.item.kind != ItemKind::Heading).count()),
        n => format!("{n} selected"),
    };
    widgets::text_in(
        &painter,
        area.shrink2(Vec2::new(PAD, 0.0)),
        Align2::RIGHT_CENTER,
        &count,
        theme::mono_font(theme::TEXT_TINY),
        theme::text_hint(),
    );
}

/// The browser's resize handle at its inner edge; returns the new width while dragged.
pub fn resize_handle(ui: &Ui, handle: Rect, width: f32, left: bool) -> Option<f32> {
    let response =
        ui.interact(handle, Id::new("browser-resize"), Sense::drag()).on_hover_cursor(CursorIcon::ResizeHorizontal);
    ui.painter().rect_filled(
        handle,
        0.0,
        if response.hovered() || response.dragged() { theme::border_strong() } else { theme::border() },
    );
    if response.dragged() {
        let dx = response.drag_delta().x;
        return Some(if left { width + dx } else { width - dx });
    }
    None
}

/// The active project's session.
fn active(projects: &Projects) -> Option<&loopwerk_session::Session> {
    projects.active().and_then(|id| projects.get(id))
}
