//! The Session view: tracks as columns, scenes as rows, scene launch on the right, the mixer
//! along the bottom (`docs/spec/ui-session-view.md`, sizes in `docs/design/components.md`).
//! It draws a [`Grid`] and turns clicks into [`Action`]s; it never changes the project itself.

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use loopwerk_engine::model::TrackId;
use serde_json::json;

use crate::grid::{Column, ColumnKind, Grid, SlotState};
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

/// What the view keeps between frames: scrolling, and a value being dragged.
#[derive(Debug, Default)]
pub struct SessionViewState {
    pub scroll: Vec2,
    drag: Option<(TrackId, Control, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Volume,
    Pan,
}

fn column_width(column: &Column) -> f32 {
    if column.kind == ColumnKind::Return { theme::RETURN_COLUMN } else { theme::TRACK_COLUMN }
}

/// Fill for a muted track's lane: the same colour, dimmer.
fn lane(color: Color32, muted: bool) -> Color32 {
    if muted { color.gamma_multiply(MUTED_ALPHA) } else { color }
}

fn rgb(color: loopwerk_engine::model::Rgb) -> Color32 {
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
    painter.rect_filled(area, 0.0, theme::SURFACE_0);

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
    painter.hline(area.x_range(), header.bottom(), theme::hairline(theme::LINE_STRONG));
    painter.hline(area.x_range(), mixer.top() - 1.0, theme::hairline(theme::LINE_STRONG));

    let bands = Bands { header, body, mixer, top: body.top() - state.scroll.y, scenes: grid.scenes.len() };
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
        if span.max < clip.left() || span.min > clip.right() {
            continue;
        }
        let progress = progress.get(index).copied().flatten();
        draw_column(ui, &painter, &bands, clip, span, column, track_number, progress, state, actions);
    }
    if returns_width > 0.0 {
        painter.vline(returns_x - 0.5, area.y_range(), theme::hairline(theme::LINE_STRONG));
    }

    // Scene column, fixed on the right.
    painter.vline(scenes_x - 0.5, area.y_range(), theme::hairline(theme::LINE_STRONG));
    let scene_span = egui::Rangef::new(scenes_x, area.right());
    let header_rect = Rect::from_x_y_ranges(scene_span, header.y_range());
    painter.rect_filled(header_rect, 0.0, theme::SURFACE_2);
    widgets::text_in(
        &painter,
        header_rect.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        "Main",
        theme::ui_font(theme::TEXT),
        theme::TEXT_PRIMARY,
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
    painter.rect_filled(main_mixer, 0.0, theme::SURFACE_1);
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
    column_header(painter, Rect::from_x_y_ranges(span, bands.header.y_range()).intersect(clip), column);
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
    painter.with_clip_rect(clip).vline(span.max + 0.5, full, theme::hairline(theme::LINE_STRONG));
}

fn column_header(painter: &egui::Painter, rect: Rect, column: &Column) {
    if rect.width() <= 0.0 {
        return;
    }
    painter.rect_filled(rect, 0.0, theme::SURFACE_2);
    let strip = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 2.0), rect.max);
    painter.rect_filled(strip, 0.0, lane(rgb(column.color), column.mute));
    let color = if column.mute { theme::TEXT_TERTIARY } else { theme::TEXT_PRIMARY };
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
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::LINE));
    }

    if column.kind == ColumnKind::Return {
        // Returns have no slots, but the scene lines run through them.
        for scene in 0..scenes {
            painter.hline(span, top + scene as f32 * PITCH + ROW + 0.5, theme::hairline(theme::LINE));
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
    let fill = motion::mix(theme::SURFACE_1, theme::HOVER_ON_SURFACE_1, widgets::hover(ui, &response));
    painter.rect_filled(stop_rect, 0.0, fill);
    let square = Rect::from_center_size(stop_rect.center(), Vec2::splat(7.0));
    let color = if column.stop_queued { theme::ACCENT } else { theme::TEXT_SECONDARY };
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
            painter.rect_filled(rect, 0.0, theme::SURFACE_0);
        }
        SlotState::EmptyStop => {
            painter.rect_filled(rect, 0.0, motion::mix(theme::SURFACE_0, theme::SURFACE_1, hover));
            painter.rect_filled(mark_rect(EMPTY_MARK), 0.0, theme::EMPTY_MARK);
        }
        SlotState::Stopped | SlotState::Queued => {
            let fill = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, hover);
            painter.rect_filled(rect, 0.0, lane(fill, column.mute));
            let (mark, text) = if slot.enabled {
                (track_color, theme::TEXT_VALUE)
            } else {
                (theme::TEXT_TERTIARY, theme::TEXT_TERTIARY)
            };
            if slot.state == SlotState::Queued {
                painter.rect_filled(mark_rect(CLIP_MARK), 0.0, mark.gamma_multiply(0.5));
                // The outline breathes while the clip waits for its launch.
                let accent = theme::ACCENT.gamma_multiply(0.45 + 0.55 * pulse);
                painter.rect_stroke(mark_rect(CLIP_MARK), 0.0, theme::hairline(accent), StrokeKind::Outside);
                painter.rect_stroke(rect, 0.0, theme::hairline(accent), StrokeKind::Inside);
            } else {
                painter.rect_filled(mark_rect(CLIP_MARK), 0.0, lane(mark, column.mute));
            }
            let text = if column.mute { theme::TEXT_TERTIARY } else { text };
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, text);
        }
        SlotState::Playing => {
            painter.rect_filled(rect, 0.0, lane(theme::SURFACE_3_BRIGHT, column.mute));
            painter.rect_filled(mark_rect(CLIP_MARK), 0.0, lane(track_color, column.mute));
            let text = if column.mute { theme::TEXT_TERTIARY } else { Color32::WHITE };
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, text);
            let done = progress.unwrap_or(1.0).clamp(0.0, 1.0);
            let line =
                Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 2.0), Vec2::new(rect.width() * done, 2.0));
            painter.rect_filled(line, 0.0, theme::ACCENT);
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
    painter.rect_filled(rect, 0.0, theme::SURFACE_1);
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
    widgets::knob(painter, knob_rect.center(), theme::KNOB, ((pan_shown + 1.0) / 2.0) as f32, theme::TEXT_PRIMARY);

    // Values row: pan left, volume right (the volume is the draggable value).
    let label_volume = match state.drag {
        Some((t, Control::Volume, v)) if t == track => v,
        _ => column.volume_db,
    };
    let values = Rect::from_min_size(Pos2::new(inner.left(), knob_rect.bottom() + 4.0), Vec2::new(inner.width(), 12.0));
    let mono = theme::mono_font(theme::TEXT_SMALL);
    widgets::text_in(
        painter,
        values,
        Align2::LEFT_CENTER,
        &widgets::format_pan(pan_shown),
        mono.clone(),
        theme::TEXT_SECONDARY,
    );
    let volume_rect =
        Rect::from_min_max(Pos2::new(inner.center().x, inner.top()), Pos2::new(inner.right(), values.bottom()));
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
    let volume_color = motion::mix(theme::TEXT_VALUE, theme::TEXT_PRIMARY, widgets::hover(ui, &volume_response));
    widgets::text_in(painter, values, Align2::RIGHT_CENTER, &widgets::format_db(volume_shown), mono, volume_color);

    // Toggle row: track on, showing the track number.
    let toggle = Rect::from_min_size(
        Pos2::new(inner.left(), inner.bottom() - theme::TOGGLE),
        Vec2::new(inner.width(), theme::TOGGLE),
    );
    let label = if column.mute { format!("Turn {} on", column.name) } else { format!("Turn {} off", column.name) };
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
        let hover = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response));
        let playing = motion::toward(ui.ctx(), response.id.with("playing"), scene.playing, motion::VALUE);
        let fill = motion::mix(hover, theme::SURFACE_3_BRIGHT, playing);
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
            theme::TEXT_TERTIARY,
        );
        let name_rect = Rect::from_min_max(inner.min, Pos2::new(inner.right() - 18.0, inner.bottom()));
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &name,
            theme::ui_font(theme::TEXT),
            theme::TEXT_PRIMARY,
        );
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::LINE));
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
            motion::mix(theme::SURFACE_1, theme::HOVER_ON_SURFACE_1, widgets::hover(ui, &response)),
        );
        let square = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - 3.5), Vec2::splat(7.0));
        painter.rect_stroke(square, 0.0, theme::hairline(theme::TEXT_SECONDARY), StrokeKind::Inside);
        let text = Rect::from_min_max(Pos2::new(square.right() + PAD, rect.top()), rect.max);
        widgets::text_in(
            painter,
            text,
            Align2::LEFT_CENTER,
            "Stop all",
            theme::ui_font(theme::TEXT),
            theme::TEXT_SECONDARY,
        );
    }
}
