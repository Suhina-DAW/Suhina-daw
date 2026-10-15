//! The Session view: tracks as columns, scenes as rows, scene launch on the right, the mixer
//! along the bottom (`docs/spec/ui-session-view.md`, sizes in `docs/design/components.md`).
//! It draws a [`Grid`] and turns clicks into [`Action`]s; it never changes the project itself.

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;

use crate::drop::{DropProbe, DropTarget};
use crate::grid::{Column, ColumnKind, Grid, SlotState};
use crate::menus::{self, TrackMenu};
use crate::motion;
use crate::theme::{self, PAD, ROW};
use crate::widgets::{self, Action, Drag};

/// One row: a slot and the line under it.
const PITCH: f32 = ROW + 1.0;
/// Mark sizes inside a slot.
const CLIP_MARK: f32 = 6.0;
const EMPTY_MARK: f32 = 3.0;
/// How dim a muted track's lane is.
const MUTED_ALPHA: f32 = 0.55;
/// Volume range of the mixer's value, in dB.
const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);
/// Space between the knobs of a mixer strip.
const SEND_GAP: f32 = 4.0;

/// What the view keeps between frames: scrolling, and a value being dragged.
#[derive(Debug, Default)]
pub struct SessionViewState {
    pub scroll: Vec2,
    /// The track whose header was clicked; the detail area shows its devices.
    pub selected: Option<TrackId>,
    /// A column header being dragged to move its track, and where it was let go.
    track_drag: Option<TrackId>,
    track_drop: Option<(TrackId, f32)>,
    drag: Option<(TrackId, Control, f64)>,
    /// A drag from the browser over the window: the view says which target is under the
    /// pointer (`docs/spec/browser.md` › Dragging into the project).
    pub drop: Option<DropProbe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Volume,
    Pan,
    /// The send to this return track.
    Send(TrackId),
}

fn column_width(column: &Column) -> f32 {
    if column.kind == ColumnKind::Return { theme::RETURN_COLUMN } else { theme::TRACK_COLUMN }
}

/// Fill for a muted track's lane: the same colour, dimmer.
fn lane(color: Color32, muted: bool) -> Color32 {
    if muted { color.gamma_multiply(MUTED_ALPHA) } else { color }
}

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Draws the Session view in the space left in `ui` and adds what the user asked for to
/// `actions`. `progress` is how far each column's playing clip is (0 … 1), by column index.
pub fn show(
    ui: &mut Ui,
    grid: &Grid,
    progress: &[Option<f32>],
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    // Fixed bands: header on top, mixer at the bottom, scene column on the right.
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), ROW));
    let mixer =
        Rect::from_min_max(Pos2::new(area.left(), (area.bottom() - theme::MIXER).max(header.bottom())), area.max);
    let body =
        Rect::from_min_max(Pos2::new(area.left(), header.bottom() + 1.0), Pos2::new(area.right(), mixer.top() - 1.0));
    let scenes_x = (area.right() - theme::SCENE_COLUMN).max(area.left());
    // Return tracks sit against the scene column; regular tracks fill the space left of them.
    let returns_width: f32 =
        grid.columns.iter().filter(|c| c.kind == ColumnKind::Return).map(|c| column_width(c) + 1.0).sum();
    let returns_x = (scenes_x - returns_width).max(area.left());
    let tracks_clip = Rect::from_min_max(area.min, Pos2::new(returns_x - 1.0, area.bottom()));
    let returns_clip = Rect::from_min_max(Pos2::new(returns_x, area.top()), Pos2::new(scenes_x - 1.0, area.bottom()));

    // Scrolling: the body scrolls both ways; header and mixer follow the tracks sideways.
    let content_width: f32 =
        grid.columns.iter().filter(|c| c.kind != ColumnKind::Return).map(|c| column_width(c) + 1.0).sum();
    let content_height = (grid.scenes.len() as f32 + 1.0) * PITCH;
    if ui.rect_contains_pointer(area) {
        let delta = ui.input(|i| i.smooth_scroll_delta());
        state.scroll -= delta;
    }
    state.scroll.x = state.scroll.x.clamp(0.0, (content_width - tracks_clip.width()).max(0.0));
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - body.height()).max(0.0));

    // Lines between the bands.
    painter.hline(area.x_range(), header.bottom(), theme::hairline(theme::border_strong()));
    painter.hline(area.x_range(), mixer.top() - 1.0, theme::hairline(theme::border_strong()));

    let bands = Bands { header, body, mixer, top: body.top() - state.scroll.y, scenes: grid.scenes.len() };
    // Where each track column is this frame, for a dragged header to find its place.
    let mut spans: Vec<(&Column, egui::Rangef)> = Vec::new();
    let mut track_x = area.left() - state.scroll.x;
    let mut return_x = returns_x;
    let mut track_number = 0;
    for (index, column) in grid.columns.iter().enumerate() {
        let width = column_width(column);
        let (x, clip) = if column.kind == ColumnKind::Return {
            (&mut return_x, returns_clip)
        } else {
            track_number += 1;
            (&mut track_x, tracks_clip)
        };
        let span = egui::Rangef::new(*x, *x + width);
        *x += width + 1.0;
        if column.kind != ColumnKind::Return {
            spans.push((column, span));
        }
        if span.max < clip.left() || span.min > clip.right() {
            continue;
        }
        let progress = progress.get(index).copied().flatten();
        draw_column(ui, &painter, &bands, clip, span, column, track_number, progress, state, actions);
    }
    // The empty space right of the last track: buttons that add a track, and new tracks for
    // what the browser drops there.
    let empty = Rect::from_min_max(
        Pos2::new(track_x.max(area.left()), area.top()),
        Pos2::new(returns_x - 1.0, mixer.top() - 1.0),
    );
    if empty.width() > 24.0 {
        empty_space(ui, &painter, empty, actions);
    }
    if let Some(probe) = state.drop.as_mut() {
        let row_of = |y: f32| ((y - bands.top) / PITCH).floor().max(0.0) as usize;
        for (column, span) in &spans {
            if column.kind != ColumnKind::Track {
                continue;
            }
            let header_rect = Rect::from_x_y_ranges(*span, header.y_range());
            probe.offer(header_rect, DropTarget::Track(column.track));
            let column_body = Rect::from_x_y_ranges(*span, body.y_range()).intersect(tracks_clip);
            if column_body.contains(probe.pointer) {
                let scene = row_of(probe.pointer.y).min(grid.scenes.len());
                let top = bands.top + scene as f32 * PITCH;
                let cell = Rect::from_x_y_ranges(*span, egui::Rangef::new(top, top + ROW)).intersect(column_body);
                let target = if probe.files {
                    DropTarget::Slot { track: column.track, scene }
                } else {
                    DropTarget::Track(column.track)
                };
                probe.offer(if probe.files { cell } else { column_body }, target);
            }
        }
        if empty.width() > 0.0 && empty.contains(probe.pointer) {
            let scene = if probe.pointer.y > body.top() { row_of(probe.pointer.y).min(grid.scenes.len()) } else { 0 };
            probe.offer(empty, DropTarget::NewTracks { beat: None, scene: Some(scene) });
        }
        if let Some(target_area) = probe.area {
            painter.with_clip_rect(area).rect_stroke(
                target_area.shrink(0.5),
                0.0,
                egui::Stroke::new(1.5, theme::accent()),
                StrokeKind::Inside,
            );
        }
    }
    // A header let go over a column moves its track there: before the column under the
    // pointer, or after it from its right half; a group moves with its tracks.
    let target_of = |x: f32| spans.iter().find(|(_, span)| span.contains(x)).map(|(c, span)| (*c, x > span.center()));
    if let Some(dragged) = state.track_drag
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
        && let Some((column, after)) = target_of(pointer.x)
        && column.track != dragged
        && let Some((_, span)) = spans.iter().find(|(c, _)| c.track == column.track)
    {
        let x = if after { span.max + 0.5 } else { span.min - 0.5 };
        painter.with_clip_rect(tracks_clip).vline(x, area.y_range(), egui::Stroke::new(2.0, theme::accent()));
    }
    if let Some((dragged, x)) = state.track_drop.take()
        && let Some((target, after)) = target_of(x)
        && let Some(index) = spans.iter().position(|(c, _)| c.track == dragged)
        && let Some((source, _)) = spans.get(index)
        && target.track != dragged
    {
        let block = 1 + spans.iter().skip(index + 1).take_while(|(c, _)| c.depth > source.depth).count();
        let mut to = target.position + usize::from(after);
        if target.position > source.position {
            to = to.saturating_sub(block);
        }
        if to != source.position {
            actions.push(Action::new("track.move", json!({ "track": dragged.0, "to": to })));
        }
    }

    if returns_width > 0.0 {
        painter.vline(returns_x - 0.5, area.y_range(), theme::hairline(theme::border_strong()));
    }

    // Scene column, fixed on the right.
    painter.vline(scenes_x - 0.5, area.y_range(), theme::hairline(theme::border_strong()));
    let scene_span = egui::Rangef::new(scenes_x, area.right());
    let header_rect = Rect::from_x_y_ranges(scene_span, header.y_range());
    painter.rect_filled(header_rect, 0.0, theme::control_background());
    widgets::text_in(
        &painter,
        header_rect.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        "Main",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    scene_column(
        ui,
        &painter.with_clip_rect(Rect::from_x_y_ranges(scene_span, body.y_range())),
        scene_span,
        body.top() - state.scroll.y,
        grid,
        actions,
    );
    let main_mixer = Rect::from_x_y_ranges(scene_span, mixer.y_range());
    painter.rect_filled(main_mixer, 0.0, theme::panel_background());
}

/// The two buttons that add an empty track, at the top of the empty space after the tracks;
/// the space below takes drops (`docs/spec/browser.md` › Files).
pub(crate) fn empty_space(ui: &Ui, painter: &egui::Painter, area: Rect, actions: &mut Vec<Action>) {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut y = area.top() + PAD;
    for (kind, label) in [("midi", "+ MIDI track"), ("audio", "+ Audio track")] {
        let width = (widgets::text_width(ui, label, font.clone()) + PAD * 2.0).min(area.width() - PAD * 2.0);
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, y), Vec2::new(width, ROW));
        y = rect.bottom() + 2.0;
        if rect.bottom() > area.bottom() || width <= 0.0 {
            break;
        }
        let response = widgets::button_area(ui, rect, Id::new(("add-track", kind, area.min.x as i32)), label);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, fill);
        widgets::text_in(painter, rect, Align2::CENTER_CENTER, label, font.clone(), theme::text_secondary());
        if response.clicked() {
            actions.push(Action::new("track.add", json!({ "kind": kind })));
        }
    }
    if area.height() > y - area.top() + ROW * 2.0 {
        let hint = Rect::from_min_max(Pos2::new(area.left(), y), area.max);
        widgets::text_in(
            painter,
            hint.shrink(PAD),
            Align2::CENTER_CENTER,
            "Drop files or devices to add a track",
            font,
            theme::text_hint(),
        );
    }
}

/// The view's horizontal bands, and where the first scene row is after scrolling.
struct Bands {
    header: Rect,
    body: Rect,
    mixer: Rect,
    top: f32,
    scenes: usize,
}

/// One column: header, slots and stop button, mixer strip, and its separator, inside `clip`.
#[allow(clippy::too_many_arguments)]
fn draw_column(
    ui: &Ui,
    painter: &egui::Painter,
    bands: &Bands,
    clip: Rect,
    span: egui::Rangef,
    column: &Column,
    track_number: usize,
    progress: Option<f32>,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("session-column", column.track.0));
    let header = Rect::from_x_y_ranges(span, bands.header.y_range()).intersect(clip);
    column_header(ui, painter, header, column, state, actions);
    let body_clip = bands.body.intersect(clip);
    column_body(
        ui,
        &painter.with_clip_rect(body_clip),
        body_clip,
        span,
        bands.top,
        bands.scenes,
        column,
        progress,
        id,
        actions,
    );
    let mixer_rect = Rect::from_x_y_ranges(span, bands.mixer.y_range());
    let mixer_painter = painter.with_clip_rect(bands.mixer.intersect(clip));
    mixer_strip(ui, &mixer_painter, mixer_rect, column, track_number, state, id, actions);
    // Column separator through header, grid and mixer.
    let full = egui::Rangef::new(bands.header.top(), bands.mixer.bottom());
    painter.with_clip_rect(clip).vline(span.max + 0.5, full, theme::hairline(theme::border_strong()));
}

fn column_header(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    if rect.width() <= 0.0 {
        return;
    }
    // Right-click opens the track's menu.
    // A click selects the track, a drag moves it, a right-click opens its menu.
    let response = ui.interact(rect, Id::new(("session-header", column.track.0)), Sense::click_and_drag());
    if response.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(column.track);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.x));
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", column.name)));
    response.context_menu(|ui| {
        let menu = TrackMenu {
            track: column.track,
            name: &column.name,
            is_group: column.kind == ColumnKind::Group,
            selection: &[],
        };
        menus::track_menu(ui, &menu, actions);
    });
    if response.clicked() {
        state.selected = Some(column.track);
    }
    let fill = if state.selected == Some(column.track) { theme::control_active() } else { theme::control_background() };
    painter.rect_filled(rect, 0.0, fill);
    let strip = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 2.0), rect.max);
    painter.rect_filled(strip, 0.0, lane(rgb(column.color), column.mute));
    let color = if column.mute { theme::text_hint() } else { theme::text_primary() };
    widgets::text_in(
        painter,
        rect.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &column.name,
        theme::ui_font(theme::TEXT),
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn column_body(
    ui: &Ui,
    painter: &egui::Painter,
    clip: Rect,
    span: egui::Rangef,
    top: f32,
    scenes: usize,
    column: &Column,
    progress: Option<f32>,
    id: Id,
    actions: &mut Vec<Action>,
) {
    for (scene, slot) in column.slots.iter().enumerate() {
        let rect = Rect::from_x_y_ranges(
            span,
            egui::Rangef::new(top + scene as f32 * PITCH, top + scene as f32 * PITCH + ROW),
        );
        if !rect.intersects(clip) {
            continue;
        }
        let number = scene + 1;
        let response = match slot.state {
            SlotState::Empty => None,
            SlotState::EmptyStop => Some(widgets::button_area(
                ui,
                rect.intersect(clip),
                id.with(scene),
                format!("Stop {} (scene {number})", column.name),
            )),
            _ => {
                let name = if slot.name.is_empty() { "clip" } else { slot.name.as_str() };
                Some(widgets::button_area(
                    ui,
                    rect.intersect(clip),
                    id.with(scene),
                    format!("Launch {name} on {} (scene {number})", column.name),
                ))
            }
        };
        let hover = response.as_ref().map_or(0.0, |r| widgets::hover(ui, r));
        let pulse = if slot.state == SlotState::Queued { motion::pulse(ui.ctx()) } else { 1.0 };
        if response.is_some_and(|r| r.clicked()) {
            actions.push(Action::new("clip.launch", json!({ "track": column.track.0, "scene": number })));
        }
        slot_cell(painter, rect, column, slot, hover, pulse, progress);
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::border()));
    }

    if column.kind == ColumnKind::Return {
        // Returns have no slots, but the scene lines run through them.
        for scene in 0..scenes {
            painter.hline(span, top + scene as f32 * PITCH + ROW + 0.5, theme::hairline(theme::border()));
        }
        return;
    }
    // Under the last scene: the track's stop button.
    let stop_top = top + scenes as f32 * PITCH;
    let stop_rect = Rect::from_x_y_ranges(span, egui::Rangef::new(stop_top, stop_top + ROW));
    if !stop_rect.intersects(clip) {
        return;
    }
    let response =
        widgets::button_area(ui, stop_rect.intersect(clip), id.with("stop"), format!("Stop {}", column.name));
    if response.clicked() {
        actions.push(Action::new("track.stop", json!({ "track": column.track.0 })));
    }
    let fill = motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response));
    painter.rect_filled(stop_rect, 0.0, fill);
    let square = Rect::from_center_size(stop_rect.center(), Vec2::splat(7.0));
    let color = if column.stop_queued { theme::accent() } else { theme::text_secondary() };
    painter.rect_stroke(square, 0.0, theme::hairline(color), StrokeKind::Inside);
}

fn slot_cell(
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    slot: &crate::grid::Slot,
    hover: f32,
    pulse: f32,
    progress: Option<f32>,
) {
    let track_color = rgb(column.color);
    let mark_rect =
        |size: f32| Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - size / 2.0), Vec2::splat(size));
    let name_rect = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + CLIP_MARK + PAD, rect.top()),
        Pos2::new(rect.right() - PAD, rect.bottom()),
    );
    let font = theme::ui_font(theme::TEXT);
    match slot.state {
        SlotState::Empty => {
            painter.rect_filled(rect, 0.0, theme::window_background());
        }
        SlotState::EmptyStop => {
            painter.rect_filled(rect, 0.0, motion::mix(theme::window_background(), theme::panel_background(), hover));
            painter.rect_filled(mark_rect(EMPTY_MARK), 0.0, theme::slot_empty());
        }
        SlotState::Stopped | SlotState::Queued => {
            let fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
            painter.rect_filled(rect, 0.0, lane(fill, column.mute));
            let (mark, text) = if slot.enabled {
                (track_color, theme::text_value())
            } else {
                (theme::text_hint(), theme::text_hint())
            };
            if slot.state == SlotState::Queued {
                painter.rect_filled(mark_rect(CLIP_MARK), 0.0, mark.gamma_multiply(0.5));
                // The outline breathes while the clip waits for its launch.
                let accent = theme::accent().gamma_multiply(0.45 + 0.55 * pulse);
                painter.rect_stroke(mark_rect(CLIP_MARK), 0.0, theme::hairline(accent), StrokeKind::Outside);
                painter.rect_stroke(rect, 0.0, theme::hairline(accent), StrokeKind::Inside);
            } else {
                painter.rect_filled(mark_rect(CLIP_MARK), 0.0, lane(mark, column.mute));
            }
            let text = if column.mute { theme::text_hint() } else { text };
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, text);
        }
        SlotState::Playing => {
            painter.rect_filled(rect, 0.0, lane(theme::control_playing(), column.mute));
            painter.rect_filled(mark_rect(CLIP_MARK), 0.0, lane(track_color, column.mute));
            let text = if column.mute { theme::text_hint() } else { theme::text_playing() };
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, text);
            let done = progress.unwrap_or(1.0).clamp(0.0, 1.0);
            let line =
                Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 2.0), Vec2::new(rect.width() * done, 2.0));
            painter.rect_filled(line, 0.0, theme::accent());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mixer_strip(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    track_number: usize,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let inner = rect.shrink(PAD);
    let track = column.track;

    // Knob row: pan.
    let label_pan = match state.drag {
        Some((t, Control::Pan, v)) if t == track => v,
        _ => column.pan,
    };
    let knob_rect = Rect::from_min_size(inner.min, Vec2::splat(theme::KNOB));
    let pan_response = widgets::value_area(
        ui,
        knob_rect,
        id.with("pan"),
        format!("{} pan", column.name),
        widgets::format_pan(label_pan),
    );
    let mut committed_pan = None;
    let mut preview = match state.drag {
        Some((t, Control::Pan, v)) if t == track => Some(v),
        _ => None,
    };
    match widgets::drag_value(ui, &pan_response, &mut preview, column.pan, 0.01, (-1.0, 1.0), 0.0) {
        Drag::Moving(v) => state.drag = Some((track, Control::Pan, v)),
        Drag::Done(v) => {
            state.drag = None;
            committed_pan = Some(v);
            actions.push(Action::new("track.setPan", json!({ "track": track.0, "pan": (v * 100.0).round() / 100.0 })));
        }
        Drag::None => {}
    }
    // Shown after this frame's drag, so the knob never trails the pointer.
    let pan_shown = match state.drag {
        Some((t, Control::Pan, v)) if t == track => v,
        _ => committed_pan.unwrap_or(column.pan),
    };
    widgets::knob(painter, knob_rect.center(), theme::KNOB, ((pan_shown + 1.0) / 2.0) as f32, theme::text_primary());
    // Sends: one knob per return, beside the pan knob, as many as the row holds.
    let sending = send_knobs(ui, painter, knob_rect, inner, column, state, id, actions);

    // Values row: pan left, volume right (the volume is the draggable value).
    let label_volume = match state.drag {
        Some((t, Control::Volume, v)) if t == track => v,
        _ => column.volume_db,
    };
    let values = Rect::from_min_size(Pos2::new(inner.left(), knob_rect.bottom() + 4.0), Vec2::new(inner.width(), 12.0));
    let mono = theme::mono_font(theme::TEXT_SMALL);
    // While a send is dragged, its level stands where the pan is shown.
    let left_value = sending
        .map_or_else(|| widgets::format_pan(pan_shown), |(letter, db)| format!("{letter} {}", widgets::format_db(db)));
    widgets::text_in(painter, values, Align2::LEFT_CENTER, &left_value, mono.clone(), theme::text_secondary());
    // The volume is dragged from the right half of the strip, or from its value alone when
    // send knobs take the row above.
    let volume_top = if column.sends.is_empty() { inner.top() } else { values.top() };
    let volume_rect =
        Rect::from_min_max(Pos2::new(inner.center().x, volume_top), Pos2::new(inner.right(), values.bottom()));
    let volume_response = widgets::value_area(
        ui,
        volume_rect,
        id.with("volume"),
        format!("{} volume", column.name),
        widgets::format_db(label_volume),
    );
    let mut committed_volume = None;
    let mut preview = match state.drag {
        Some((t, Control::Volume, v)) if t == track => Some(v),
        _ => None,
    };
    match widgets::drag_value(ui, &volume_response, &mut preview, column.volume_db, 0.25, VOLUME_RANGE, 0.0) {
        Drag::Moving(v) => state.drag = Some((track, Control::Volume, v)),
        Drag::Done(v) => {
            state.drag = None;
            committed_volume = Some(v);
            let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
            actions.push(Action::new("track.setVolume", json!({ "track": track.0, "db": db })));
        }
        Drag::None => {}
    }
    let volume_shown = match state.drag {
        Some((t, Control::Volume, v)) if t == track => v,
        _ => committed_volume.unwrap_or(column.volume_db),
    };
    let volume_color = motion::mix(theme::text_value(), theme::text_primary(), widgets::hover(ui, &volume_response));
    widgets::text_in(painter, values, Align2::RIGHT_CENTER, &widgets::format_db(volume_shown), mono, volume_color);

    // Toggle row: track on, showing the track number.
    let toggle = Rect::from_min_size(
        Pos2::new(inner.left(), inner.bottom() - theme::TOGGLE),
        Vec2::new(inner.width(), theme::TOGGLE),
    );
    let label = if column.mute { format!("Turn {} on", column.name) } else { format!("Turn {} off", column.name) };
    let solo = Rect::from_min_max(Pos2::new(toggle.right() - (toggle.width() - 2.0) / 3.0, toggle.top()), toggle.max);
    let toggle = Rect::from_min_max(toggle.min, Pos2::new(solo.left() - 2.0, toggle.bottom()));
    if let Some(action) = widgets::solo_button(ui, painter, solo, id.with("solo"), track.0, &column.name, column.solo) {
        actions.push(action);
    }
    let response = widgets::button_area(ui, toggle, id.with("on"), label);
    if response.clicked() {
        actions.push(Action::new("track.setMute", json!({ "track": track.0, "mute": !column.mute })));
    }
    let (fill, text) = widgets::toggle_colors(ui, &response, !column.mute);
    painter.rect_filled(toggle, 0.0, fill);
    let number = if column.kind == ColumnKind::Return {
        column.name.chars().next().map(String::from).unwrap_or_default()
    } else {
        track_number.to_string()
    };
    widgets::text_in(painter, toggle, Align2::CENTER_CENTER, &number, theme::mono_font(theme::TEXT_SMALL), text);
}

/// The send knobs of a mixer strip, right of the pan knob at `pan`: dragged up or down for the
/// level (`track.setSend`), double-click for none, right-click for before or after the fader
/// (`track.setSendMode`). Returns the return's letter and the level while one is dragged.
#[allow(clippy::too_many_arguments)]
fn send_knobs(
    ui: &Ui,
    painter: &egui::Painter,
    pan: Rect,
    inner: Rect,
    column: &Column,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) -> Option<(String, f64)> {
    let track = column.track;
    let step = theme::KNOB + SEND_GAP;
    let fit = ((inner.right() - pan.right()) / step).floor().max(0.0) as usize;
    let mut sending = None;
    for (index, send) in column.sends.iter().take(fit).enumerate() {
        let rect = pan.translate(Vec2::new(step * (index + 1) as f32, 0.0));
        let letter = send.name.chars().next().map(String::from).unwrap_or_default();
        let control = Control::Send(send.target);
        let mode = if send.pre_fader { "pre" } else { "post" };
        let response = widgets::value_area(
            ui,
            rect,
            id.with(("send", send.target.0)),
            format!("{} send to {}", column.name, send.name),
            format!("{} {mode}", widgets::format_db(send.level_db)),
        );
        let mut preview = match state.drag {
            Some((t, c, v)) if t == track && c == control => Some(v),
            _ => None,
        };
        let level = if send.level_db.is_finite() { send.level_db } else { VOLUME_RANGE.0 };
        let mut committed = None;
        match widgets::drag_value(ui, &response, &mut preview, level, 0.25, VOLUME_RANGE, VOLUME_RANGE.0) {
            Drag::Moving(v) => state.drag = Some((track, control, v)),
            Drag::Done(v) => {
                state.drag = None;
                committed = Some(v);
                let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                actions.push(Action::new(
                    "track.setSend",
                    json!({ "track": track.0, "return": send.target.0, "level": db }),
                ));
            }
            Drag::None => {}
        }
        response.context_menu(|ui| {
            for (pre, label) in [(true, "Before the fader (pre)"), (false, "After the fader (post)")] {
                if ui.radio(send.pre_fader == pre, label).clicked() {
                    let mode = if pre { "pre" } else { "post" };
                    actions.push(Action::new(
                        "track.setSendMode",
                        json!({ "track": track.0, "return": send.target.0, "mode": mode }),
                    ));
                    ui.close();
                }
            }
        });
        let shown = match state.drag {
            Some((t, c, v)) if t == track && c == control => {
                sending = Some((letter.clone(), v));
                v
            }
            _ => committed.unwrap_or(level),
        };
        let position = ((shown - VOLUME_RANGE.0) / (VOLUME_RANGE.1 - VOLUME_RANGE.0)) as f32;
        let pointer = motion::mix(theme::text_value(), theme::text_primary(), widgets::hover(ui, &response));
        widgets::knob(painter, rect.center(), theme::KNOB, position, pointer);
        // The return's letter in the knob, and a dot under it when the send is before the fader.
        let font = theme::mono_font(theme::TEXT_SMALL - 2.0);
        widgets::text_in(painter, rect, Align2::CENTER_CENTER, &letter, font, theme::text_hint());
        if send.pre_fader {
            painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, theme::text_value());
        }
    }
    sending
}

fn scene_column(
    ui: &Ui,
    painter: &egui::Painter,
    span: egui::Rangef,
    top: f32,
    grid: &Grid,
    actions: &mut Vec<Action>,
) {
    let clip = painter.clip_rect();
    for scene in &grid.scenes {
        let y = top + (scene.number - 1) as f32 * PITCH;
        let rect = Rect::from_x_y_ranges(span, egui::Rangef::new(y, y + ROW));
        if !rect.intersects(clip) {
            continue;
        }
        let label = if scene.name.is_empty() {
            format!("Launch scene {}", scene.number)
        } else {
            format!("Launch scene {} {}", scene.number, scene.name)
        };
        let response = widgets::button_area(ui, rect.intersect(clip), Id::new(("scene", scene.number)), label);
        if response.clicked() {
            actions.push(Action::new("scene.launch", json!({ "scene": scene.number })));
        }
        let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        let playing = motion::toward(ui.ctx(), response.id.with("playing"), scene.playing, motion::VALUE);
        let fill = motion::mix(hover, theme::control_playing(), playing);
        painter.rect_filled(rect, 0.0, fill);
        let inner = rect.shrink2(Vec2::new(PAD, 0.0));
        let mut name = scene.name.clone();
        if let Some(tempo) = scene.tempo_bpm {
            name = format!("{name} {tempo:.0}").trim().to_string();
        }
        let number = scene.number.to_string();
        widgets::text_in(
            painter,
            inner,
            Align2::RIGHT_CENTER,
            &number,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        let name_rect = Rect::from_min_max(inner.min, Pos2::new(inner.right() - 18.0, inner.bottom()));
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &name,
            theme::ui_font(theme::TEXT),
            theme::text_primary(),
        );
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::border()));
    }
    let stop_top = top + grid.scenes.len() as f32 * PITCH;
    let rect = Rect::from_x_y_ranges(span, egui::Rangef::new(stop_top, stop_top + ROW));
    if rect.intersects(clip) {
        let response = widgets::button_area(ui, rect.intersect(clip), Id::new("stop-all"), "Stop all clips");
        if response.clicked() {
            actions.push(Action::new("launcher.stopAll", json!({})));
        }
        painter.rect_filled(
            rect,
            0.0,
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response)),
        );
        let square = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - 3.5), Vec2::splat(7.0));
        painter.rect_stroke(square, 0.0, theme::hairline(theme::text_secondary()), StrokeKind::Inside);
        let text = Rect::from_min_max(Pos2::new(square.right() + PAD, rect.top()), rect.max);
        widgets::text_in(
            painter,
            text,
            Align2::LEFT_CENTER,
            "Stop all",
            theme::ui_font(theme::TEXT),
            theme::text_secondary(),
        );
    }
}
