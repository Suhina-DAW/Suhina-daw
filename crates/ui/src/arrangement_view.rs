//! The Arrangement view: a timeline of lanes, one per track, with the track headers on the right
//! (`docs/design/components.md` › Arrangement view). Track headers show more controls as a lane
//! grows. Like the Session view it draws data and turns gestures into [`Action`]s.

use std::collections::HashMap;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use loopwerk_engine::model::TrackId;
use serde_json::json;

use crate::grid::ColumnKind;
use crate::motion;
use crate::theme::{self, PAD, ROW};
use crate::timeline::{ClipBox, Lane, Timeline};
use crate::widgets::{self, Action, Drag};

pub const HEADER_COLUMN: f32 = 232.0;
const RULER: f32 = ROW;
const DEFAULT_LANE: f32 = 48.0;
const MIN_LANE: f32 = 20.0;
const MAX_LANE: f32 = 400.0;
/// Heights at which a header shows its next group (components › Track header levels).
const SHOWS_MIXER: f32 = 48.0;
/// Padding inside a header: top and bottom, and the sides.
const HEADER_PAD_Y: f32 = 5.0;
const HEADER_ROW: f32 = 15.0;
const FIELD: f32 = 16.0;
const PAN_FIELD: f32 = 46.0;
const ON_TOGGLE: f32 = 30.0;
/// The volume field on a return or main row.
const FIXED_FIELD: f32 = 90.0;
/// Points per beat: the default and the zoom range.
const DEFAULT_BEAT_WIDTH: f32 = 8.0;
const BEAT_WIDTH_RANGE: (f32, f32) = (1.0, 400.0);
/// Bars of empty timeline after the last clip.
const TAIL_BARS: f64 = 16.0;
const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);

const PAN_BACKGROUND: Color32 = Color32::from_rgb(0x23, 0x22, 0x20);
const PAN_CENTRE: Color32 = Color32::from_rgb(0x55, 0x52, 0x4C);
const BEAT_LINE: Color32 = theme::SURFACE_1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Volume,
    Pan,
}

/// What the view keeps between frames: scrolling, zoom, lane heights, the selected clip and
/// whatever is being dragged.
#[derive(Debug)]
pub struct ArrangementViewState {
    pub scroll: Vec2,
    /// Points per beat.
    pub beat_width: f32,
    heights: HashMap<TrackId, f32>,
    selected: Option<(TrackId, usize)>,
    /// A clip being moved: its track, index and unsnapped start.
    clip_drag: Option<(TrackId, usize, f64)>,
    value_drag: Option<(TrackId, Control, f64)>,
}

impl Default for ArrangementViewState {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            beat_width: DEFAULT_BEAT_WIDTH,
            heights: HashMap::new(),
            selected: None,
            clip_drag: None,
            value_drag: None,
        }
    }
}

impl ArrangementViewState {
    fn height(&self, lane: &Lane) -> f32 {
        self.heights.get(&lane.track).copied().unwrap_or(DEFAULT_LANE)
    }
}

/// The grid clips snap to at a zoom level: a sixteenth, a beat or a bar, whichever is at least
/// a few points wide.
fn snap(beat_width: f32, bar_beats: f64) -> f64 {
    if beat_width >= 48.0 {
        0.25
    } else if beat_width >= 12.0 {
        1.0
    } else {
        bar_beats
    }
}

/// Where a volume sits on a field, 0 … 1, with 0 dB at 85 % (components › Value field).
pub fn volume_position(db: f64) -> f32 {
    if !db.is_finite() || db <= VOLUME_RANGE.0 {
        0.0
    } else if db <= 0.0 {
        let x = (db - VOLUME_RANGE.0) / -VOLUME_RANGE.0;
        (0.85 * x * x) as f32
    } else {
        (0.85 + 0.15 * (db / VOLUME_RANGE.1).min(1.0)) as f32
    }
}

fn rgb(color: loopwerk_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Draws the Arrangement view in the space left in `ui` and adds what the user asked for to
/// `actions`.
pub fn show(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::SURFACE_0);

    let headers_x = (area.right() - HEADER_COLUMN).max(area.left());
    let time = Rect::from_min_max(area.min, Pos2::new(headers_x - 1.0, area.bottom()));
    let ruler = Rect::from_min_size(time.min, Vec2::new(time.width(), RULER));
    // Return tracks and the main track are fixed rows along the bottom.
    let returns: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Return).collect();
    let fixed_rows = returns.len() + usize::from(timeline.main_volume_db.is_some());
    let fixed_height = fixed_rows as f32 * (ROW + 1.0);
    let fixed_top = (area.bottom() - fixed_height).max(ruler.bottom() + 1.0);
    let fixed = Rect::from_min_max(Pos2::new(area.left(), fixed_top), area.max);
    let lanes_area =
        Rect::from_min_max(Pos2::new(area.left(), ruler.bottom() + 1.0), Pos2::new(area.right(), fixed_top - 1.0));
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };

    // Zoom around the pointer with Ctrl and the wheel; the wheel alone scrolls.
    if let Some(pointer) = ui.ctx().pointer_hover_pos().filter(|p| area.contains(*p)) {
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 && time.contains(pointer) {
            let beat = (pointer.x - time.left() + state.scroll.x) / state.beat_width;
            state.beat_width = (state.beat_width * zoom).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
            state.scroll.x = beat * state.beat_width - (pointer.x - time.left());
        }
        state.scroll -= ui.input(|i| i.smooth_scroll_delta());
    }
    let content_width = ((timeline.end + TAIL_BARS * bar_beats) as f32) * state.beat_width;
    let content_height: f32 =
        timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).map(|l| state.height(l) + 1.0).sum();
    state.scroll.x = state.scroll.x.clamp(0.0, (content_width - time.width()).max(0.0));
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - lanes_area.height()).max(0.0));

    let (scroll_x, beat_width) = (state.scroll.x, state.beat_width);
    let x_of = move |beat: f64| time.left() - scroll_x + beat as f32 * beat_width;
    let first_beat = f64::from(state.scroll.x / state.beat_width);
    let last_beat = f64::from((state.scroll.x + time.width()) / state.beat_width);

    // Ruler: bar numbers, a line at each labelled bar.
    painter.rect_filled(ruler, 0.0, theme::SURFACE_1);
    let bar_width = bar_beats as f32 * state.beat_width;
    let label_every = [1u32, 2, 4, 8, 16, 32, 64].into_iter().find(|n| *n as f32 * bar_width >= 40.0).unwrap_or(128);
    let first_bar = (first_beat / bar_beats).floor() as u32;
    let last_bar = (last_beat / bar_beats).ceil() as u32;
    let ruler_painter = painter.with_clip_rect(ruler);
    // The grid covers the tracks and the fixed rows, not the empty space below the tracks.
    let tracks_bottom = (lanes_area.top() - state.scroll.y + content_height).min(lanes_area.bottom());
    let grid_bands =
        [egui::Rangef::new(lanes_area.top(), tracks_bottom), egui::Rangef::new(fixed.top(), fixed.bottom())];
    let grid_painter = painter
        .with_clip_rect(Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(lanes_area.top(), area.bottom())));
    for bar in first_bar..=last_bar {
        let x = x_of(f64::from(bar) * bar_beats).round() + 0.5;
        if bar % label_every == 0 {
            ruler_painter.vline(x, ruler.y_range(), theme::hairline(theme::LINE));
            let label = Rect::from_min_max(Pos2::new(x + 3.0, ruler.top()), Pos2::new(x + 40.0, ruler.bottom()));
            widgets::text_in(
                &ruler_painter,
                label,
                Align2::LEFT_CENTER,
                &(bar + 1).to_string(),
                theme::mono_font(theme::TEXT_SMALL),
                theme::TEXT_SECONDARY,
            );
        }
        // Grid in the lanes: every bar, and beats when they are far enough apart.
        for band in grid_bands.iter().filter(|b| b.span() > 0.0) {
            if bar_width >= 6.0 || bar % label_every == 0 {
                grid_painter.vline(x, *band, theme::hairline(theme::LINE));
            }
            if state.beat_width >= 10.0 {
                for beat in 1..bar_beats.ceil() as u32 {
                    let x = x_of(f64::from(bar) * bar_beats + f64::from(beat)).round() + 0.5;
                    grid_painter.vline(x, *band, theme::hairline(BEAT_LINE));
                }
            }
        }
    }
    painter.hline(area.x_range(), ruler.bottom() + 0.5, theme::hairline(theme::LINE_STRONG));

    // Header column top: above the headers, beside the ruler.
    let corner = Rect::from_min_max(Pos2::new(headers_x, area.top()), Pos2::new(area.right(), ruler.bottom()));
    painter.rect_filled(corner, 0.0, theme::SURFACE_1);
    painter.vline(headers_x - 0.5, area.y_range(), theme::hairline(theme::LINE_STRONG));

    // Lanes.
    let lanes_painter = painter.with_clip_rect(lanes_area);
    let mut y = lanes_area.top() - state.scroll.y;
    let mut track_number = 0;
    for lane in timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return) {
        let height = state.height(lane);
        track_number += 1;
        let lane_rect = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.right(), y + height));
        y += height + 1.0;
        if lane_rect.bottom() < lanes_area.top() || lane_rect.top() > lanes_area.bottom() {
            continue;
        }
        let lane_time = Rect::from_min_max(lane_rect.min, Pos2::new(time.right(), lane_rect.bottom()));
        let clips_painter = painter.with_clip_rect(lane_time.intersect(lanes_area));
        for clip in &lane.clips {
            if clip.start + clip.length < first_beat || clip.start > last_beat {
                continue;
            }
            clip_box(ui, &clips_painter, lane_time, lane, clip, &x_of, bar_beats, state, actions);
        }
        let header = Rect::from_min_max(Pos2::new(headers_x, lane_rect.top()), lane_rect.max);
        track_header(ui, &lanes_painter, header, lanes_area, lane, track_number, state, actions);
        lanes_painter.hline(area.x_range(), lane_rect.bottom() + 0.5, theme::hairline(theme::LINE_STRONG));
    }

    // Fixed rows: return tracks, then the main track, each a name and a volume.
    painter.hline(area.x_range(), fixed.top() - 0.5, theme::hairline(theme::LINE_STRONG));
    let fixed_painter = painter.with_clip_rect(fixed);
    let mut y = fixed.top();
    for lane in &returns {
        let row = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.right(), y + ROW));
        y += ROW + 1.0;
        let lane_time = Rect::from_min_max(row.min, Pos2::new(time.right(), row.bottom()));
        let clips_painter = painter.with_clip_rect(lane_time.intersect(fixed));
        for clip in &lane.clips {
            if clip.start + clip.length >= first_beat && clip.start <= last_beat {
                clip_box(ui, &clips_painter, lane_time, lane, clip, &x_of, bar_beats, state, actions);
            }
        }
        let header = Rect::from_min_max(Pos2::new(headers_x, row.top()), row.max);
        fixed_row(ui, &fixed_painter, header, fixed, lane, state, actions);
        fixed_painter.hline(area.x_range(), row.bottom() + 0.5, theme::hairline(theme::LINE));
    }
    if let Some(db) = timeline.main_volume_db {
        let header = Rect::from_min_max(Pos2::new(headers_x, y), Pos2::new(area.right(), y + ROW));
        fixed_painter.rect_filled(header, 0.0, theme::SURFACE_1);
        let inner = header.shrink2(Vec2::new(PAD, 2.0));
        let font = theme::ui_font(theme::TEXT);
        widgets::text_in(&fixed_painter, inner, Align2::LEFT_CENTER, "Main", font, theme::TEXT_PRIMARY);
        let field = Rect::from_min_max(Pos2::new(inner.right() - FIXED_FIELD, inner.top()), inner.max);
        volume_text(&fixed_painter, field, db, 0.0);
    }

    // Playhead through ruler and lanes.
    if timeline.running || timeline.beat > 0.0 {
        let x = x_of(timeline.beat).round() + 0.5;
        if x >= time.left() && x <= time.right() {
            painter.vline(x, time.y_range(), theme::hairline(theme::ACCENT));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn clip_box(
    ui: &Ui,
    painter: &egui::Painter,
    lane_time: Rect,
    lane: &Lane,
    clip: &ClipBox,
    x_of: &dyn Fn(f64) -> f32,
    bar_beats: f64,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let key = (lane.track, clip.index);
    let grid = snap(state.beat_width, bar_beats);
    let start = match state.clip_drag {
        Some((t, i, raw)) if (t, i) == key => ((raw / grid).round() * grid).max(0.0),
        _ => clip.start,
    };
    let rect = Rect::from_min_max(
        Pos2::new(x_of(start).round(), lane_time.top()),
        Pos2::new((x_of(start + clip.length).round() - 1.0).max(x_of(start).round() + 1.0), lane_time.bottom()),
    );
    let visible = rect.intersect(painter.clip_rect());
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }
    let bar = (start / bar_beats).floor() as u64 + 1;
    let name = if clip.name.is_empty() { "clip" } else { clip.name.as_str() };
    let response = ui
        .interact(visible, Id::new(("arrangement-clip", lane.track.0, clip.index)), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Grab);
    let label = format!("Clip {name} on {} at bar {bar}", lane.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if response.clicked() || response.drag_started() {
        state.selected = Some(key);
    }
    if response.drag_started() {
        state.clip_drag = Some((lane.track, clip.index, clip.start));
    }
    if response.dragged()
        && let Some((t, i, raw)) = state.clip_drag
        && (t, i) == key
    {
        let moved = raw + f64::from(response.drag_delta().x / state.beat_width);
        state.clip_drag = Some((t, i, moved));
    }
    if response.drag_stopped()
        && let Some((t, i, _)) = state.clip_drag.take()
        && (t, i) == key
        && (start - clip.start).abs() > 1e-9
    {
        actions.push(Action::new("clip.move", json!({ "track": lane.track.0, "index": clip.index, "start": start })));
    }

    let selected = state.selected == Some(key);
    let dim = |c: Color32| if lane.mute || !clip.enabled { c.gamma_multiply(0.55) } else { c };
    painter.rect_filled(rect, 0.0, if selected { theme::SURFACE_3 } else { theme::SURFACE_2 });
    let color = if clip.enabled { rgb(lane.color) } else { theme::TEXT_TERTIARY };
    painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, dim(color));
    let text_color = if clip.enabled && !lane.mute { theme::TEXT_VALUE } else { theme::TEXT_TERTIARY };
    let has_name = rect.height() >= 20.0;
    if has_name {
        let name_rect =
            Rect::from_min_max(rect.min + Vec2::new(4.0, 3.0), Pos2::new(rect.right() - 2.0, rect.top() + 15.0));
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &clip.name,
            theme::ui_font(theme::TEXT_SMALL),
            text_color,
        );
    }
    let marks = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + if has_name && rect.height() >= 30.0 { 17.0 } else { 4.0 }),
        Pos2::new(rect.right(), rect.bottom() - 3.0),
    );
    if clip.audio {
        painter.hline(marks.x_range(), marks.center().y.round() + 0.5, theme::hairline(dim(color)));
    } else if !clip.notes.is_empty() && marks.height() >= 4.0 {
        let low = clip.notes.iter().map(|n| n.key).min().unwrap_or(0);
        let high = clip.notes.iter().map(|n| n.key).max().unwrap_or(low);
        let rows = f32::from(high - low) + 1.0;
        for note in &clip.notes {
            let x = x_of(start + note.start);
            let width = (note.length as f32 * state.beat_width - 1.0).max(1.0);
            let row = f32::from(note.key - low);
            let y = marks.bottom() - (row + 0.5) / rows * marks.height();
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - 1.0), Vec2::new(width, 2.0)), 0.0, dim(color));
        }
    }
    if selected {
        painter.rect_stroke(rect, 0.0, theme::hairline(theme::TEXT_PRIMARY), StrokeKind::Inside);
    }
}

/// A return track's row: its name and its volume.
fn fixed_row(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    painter.rect_filled(rect, 0.0, theme::SURFACE_1);
    let inner = rect.shrink2(Vec2::new(PAD, 2.0));
    let field = Rect::from_min_max(Pos2::new(inner.right() - FIXED_FIELD, inner.top()), inner.max);
    let name = Rect::from_min_max(inner.min, Pos2::new(field.left() - PAD, inner.bottom()));
    let color = if lane.mute { theme::TEXT_TERTIARY } else { theme::TEXT_PRIMARY };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), color);
    let id = Id::new(("arrangement-header", lane.track.0));
    value_control(ui, painter, field, clip, lane, Control::Volume, true, id, state, actions);
}

#[allow(clippy::too_many_arguments)]
fn track_header(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    lanes_area: Rect,
    lane: &Lane,
    track_number: usize,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);
    painter.rect_filled(rect, 0.0, theme::SURFACE_1);
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + lane.depth as f32 * 8.0, rect.top() + HEADER_PAD_Y),
        Pos2::new(rect.right() - PAD, rect.bottom() - HEADER_PAD_Y),
    );

    // Row 1: colour, name, track on.
    let row = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_ROW));
    let square = Rect::from_min_size(Pos2::new(row.left(), row.center().y - 3.0), Vec2::splat(6.0));
    painter.rect_filled(square, 0.0, if lane.mute { rgb(lane.color).gamma_multiply(0.55) } else { rgb(lane.color) });
    let toggle = Rect::from_min_max(Pos2::new(row.right() - ON_TOGGLE, row.top()), row.max);
    let name =
        Rect::from_min_max(Pos2::new(square.right() + PAD, row.top()), Pos2::new(toggle.left() - PAD, row.bottom()));
    let name_color = if lane.mute { theme::TEXT_TERTIARY } else { theme::TEXT_PRIMARY };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), name_color);
    if toggle.intersects(clip) {
        let label = if lane.mute { format!("Turn {} on", lane.name) } else { format!("Turn {} off", lane.name) };
        let response = widgets::button_area(ui, toggle.intersect(clip), id.with("on"), label);
        if response.clicked() {
            actions.push(Action::new("track.setMute", json!({ "track": lane.track.0, "mute": !lane.mute })));
        }
        let (fill, text) = widgets::toggle_colors(ui, &response, !lane.mute);
        painter.rect_filled(toggle, 0.0, fill);
        let number = if lane.kind == ColumnKind::Return {
            lane.name.chars().next().map(String::from).unwrap_or_default()
        } else {
            track_number.to_string()
        };
        widgets::text_in(painter, toggle, Align2::CENTER_CENTER, &number, theme::mono_font(theme::TEXT_SMALL), text);
    }

    // Row 2, once the lane is tall enough: volume and pan.
    if rect.height() >= SHOWS_MIXER {
        let top = row.bottom() + PAD;
        let pan = Rect::from_min_max(Pos2::new(inner.right() - PAN_FIELD, top), Pos2::new(inner.right(), top + FIELD));
        let volume = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(pan.left() - 4.0, top + FIELD));
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
        value_control(ui, painter, pan, clip, lane, Control::Pan, false, id, state, actions);
    }

    // The bottom edge resizes the lane.
    let edge =
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 3.0), Pos2::new(rect.right(), rect.bottom() + 2.0));
    if edge.intersects(lanes_area) {
        let response = ui
            .interact(edge.intersect(lanes_area), id.with("resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged() {
            let height = (state.height(lane) + response.drag_delta().y).clamp(MIN_LANE, MAX_LANE);
            state.heights.insert(lane.track, height);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn value_control(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    control: Control,
    plain: bool,
    id: Id,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let track = lane.track;
    let (value, per_point, range, default) = match control {
        Control::Volume => (lane.volume_db, 0.25, VOLUME_RANGE, 0.0),
        Control::Pan => (lane.pan, 0.01, (-1.0, 1.0), 0.0),
    };
    let mut preview = match state.value_drag {
        Some((t, c, v)) if t == track && c == control => Some(v),
        _ => None,
    };
    let shown = preview.unwrap_or(value);
    let mut hover = 0.0;
    if rect.intersects(clip) {
        let (label, text) = match control {
            Control::Volume => (format!("{} volume", lane.name), widgets::format_db(shown)),
            Control::Pan => (format!("{} pan", lane.name), widgets::format_pan(shown)),
        };
        let response = widgets::value_area(ui, rect.intersect(clip), id.with(label.as_str()), &label, text);
        hover = widgets::hover(ui, &response);
        match widgets::drag_value(ui, &response, &mut preview, value, per_point, range, default) {
            Drag::Moving(v) => state.value_drag = Some((track, control, v)),
            Drag::Done(v) => {
                state.value_drag = None;
                actions.push(match control {
                    Control::Volume => {
                        let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                        Action::new("track.setVolume", json!({ "track": track.0, "db": db }))
                    }
                    Control::Pan => {
                        Action::new("track.setPan", json!({ "track": track.0, "pan": (v * 100.0).round() / 100.0 }))
                    }
                });
            }
            Drag::None => {}
        }
    }
    let shown = match state.value_drag {
        Some((t, c, v)) if t == track && c == control => v,
        _ => value,
    };
    // The fill glides to a new value (after an undo, say); a drag follows the pointer directly.
    let dragging = matches!(state.value_drag, Some((t, c, _)) if t == track && c == control);
    let shown = if dragging || !shown.is_finite() {
        shown
    } else {
        f64::from(motion::value(ui.ctx(), id.with(("shown", control as u8)), shown as f32, motion::VALUE))
    };
    match control {
        Control::Volume if plain => volume_text(painter, rect, shown, hover),
        Control::Volume => volume_field(painter, rect, shown, hover),
        Control::Pan => pan_field(painter, rect, shown, hover),
    }
}

/// A volume field: fill from the left up to the level, a tick at its end, the value on the right.
fn volume_field(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, hover));
    let x = rect.left() + rect.width() * volume_position(db);
    painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, theme::SURFACE_3_BRIGHT);
    let tick = x.round() - 0.5;
    painter.vline(tick, egui::Rangef::new(rect.top(), rect.top() + 3.0), theme::hairline(theme::TEXT_VALUE));
    painter.vline(tick, egui::Rangef::new(rect.bottom() - 3.0, rect.bottom()), theme::hairline(theme::TEXT_VALUE));
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::TEXT_PRIMARY,
    );
}

/// A volume as a plain value, for the compact return and main rows.
fn volume_text(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let color = motion::mix(theme::TEXT_VALUE, theme::TEXT_PRIMARY, hover);
    widgets::text_in(
        painter,
        rect,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        color,
    );
}

/// A pan field: a bar along the bottom from the centre, a centre tick, the value in the middle.
fn pan_field(painter: &egui::Painter, rect: Rect, pan: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(PAN_BACKGROUND, theme::HOVER_ON_SURFACE_1, hover));
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 3.0), theme::hairline(PAN_CENTRE));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 3.0, rect.bottom()), theme::hairline(PAN_CENTRE));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::TEXT_VALUE);
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::TEXT_PRIMARY,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_sits_at_85_percent_for_0_db() {
        assert_eq!(volume_position(f64::NEG_INFINITY), 0.0);
        assert_eq!(volume_position(-70.0), 0.0);
        assert!((volume_position(0.0) - 0.85).abs() < 1e-6);
        assert!((volume_position(6.0) - 1.0).abs() < 1e-6);
        assert!(volume_position(-6.0) < volume_position(-3.0));
    }

    #[test]
    fn clips_snap_to_a_grid_that_follows_the_zoom() {
        assert_eq!(snap(60.0, 4.0), 0.25);
        assert_eq!(snap(20.0, 4.0), 1.0);
        assert_eq!(snap(4.0, 3.0), 3.0);
    }
}
