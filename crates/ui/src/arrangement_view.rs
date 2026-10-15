//! The Arrangement view: a timeline of lanes, one per track, with the track headers on the right
//! (`docs/design/components.md` › Arrangement view). Track headers show more controls as a lane
//! grows. Like the Session view it draws data and turns gestures into [`Action`]s.

use std::collections::HashMap;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{FadeCurve, FadeShape, TrackId};

use crate::automation_lanes::{self as automation, LanesState};
use crate::drop::{DropProbe, DropTarget};
use crate::grid::ColumnKind;
use crate::menus::{self, TrackMenu};
use crate::meter::{self, Meters};
use crate::motion;
use crate::theme::{self, PAD, ROW};
use crate::timeline::{ClipBox, Lane, Timeline};
use crate::waveform::{self, Waveforms};
use crate::widgets::{self, Action, Drag};

/// Width of the track header column: the default and how far it can be dragged.
pub const HEADER_COLUMN: f32 = 232.0;
const HEADER_COLUMN_RANGE: (f32, f32) = (160.0, 520.0);
const RULER: f32 = ROW;
const DEFAULT_LANE: f32 = 48.0;
const MIN_LANE: f32 = 20.0;
const MAX_LANE: f32 = 400.0;
/// How much a point of wheel movement with Alt held grows the lanes (as a power of e).
const LANE_ZOOM_PER_POINT: f32 = 0.004;
/// Heights at which a header shows its next group (components › Track header levels).
const SHOWS_MIXER: f32 = 48.0;
const SHOWS_ROUTING: f32 = 70.0;
const DROPDOWN: f32 = 15.0;
/// Padding inside a header: top and bottom, and the sides.
const HEADER_PAD_Y: f32 = 5.0;
const HEADER_ROW: f32 = 15.0;
const FIELD: f32 = 16.0;
const PAN_FIELD: f32 = 46.0;
const ON_TOGGLE: f32 = 30.0;
const SOLO: f32 = 18.0;
/// The volume field on a return or main row.
const FIXED_FIELD: f32 = 90.0;
/// Points per beat: the default and the zoom range.
const DEFAULT_BEAT_WIDTH: f32 = 8.0;
const BEAT_WIDTH_RANGE: (f32, f32) = (1.0, 400.0);
/// Bars of empty timeline after the last clip.
const TAIL_BARS: f64 = 16.0;
const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);
const CLIP_GAIN_RANGE: (f64, f64) = (-70.0, 24.0);
/// A fade handle: the square drawn at a clip's top corner, and the area that takes the pointer.
const FADE_HANDLE: f32 = 7.0;
const FADE_HANDLE_HIT: f32 = 11.0;
/// A fade's curve handle: a round one half-way along the fade's line, on fades at least this wide.
const CURVE_HANDLE: f32 = 7.0;
const CURVE_HANDLE_MIN_FADE: f32 = 21.0;
/// The gain field at an audio clip's top right, and how far it stays from the clip's right edge
/// so the fade-out handle has room.
const GAIN_FIELD: Vec2 = Vec2::new(44.0, 12.0);
const GAIN_FIELD_INSET: f32 = 12.0;
/// The play start marker on the ruler: a triangle this wide and tall, pointing down.
const MARKER: Vec2 = Vec2::new(9.0, 6.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Volume,
    Pan,
}

/// A clip being dragged: where it came from, its unsnapped start and the lane under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClipDrag {
    track: TrackId,
    index: usize,
    start: f64,
    target: TrackId,
}

/// Which end of a clip a fade belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeEdge {
    In,
    Out,
}

/// A fade handle being dragged: the clip, which fade, and its length so far in beats before
/// snapping.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FadeDrag {
    key: (TrackId, usize),
    edge: FadeEdge,
    raw: f64,
}

/// A fade's curve handle being dragged: the clip, which fade, the curve so far, and how far
/// below the handle's centre the pointer took hold of it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveDrag {
    key: (TrackId, usize),
    edge: FadeEdge,
    curve: f64,
    grab: f32,
}

/// What a clip shows that a drag in progress changes before the project does: its gain and its
/// fades.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    gain_db: f64,
    fade_in: f64,
    fade_out: f64,
    fade_in_curve: FadeCurve,
    fade_out_curve: FadeCurve,
}

impl Shown {
    fn of(clip: &ClipBox) -> Self {
        Self {
            gain_db: clip.audio_view.as_ref().map_or(0.0, |v| v.gain_db),
            fade_in: clip.fade_in,
            fade_out: clip.fade_out,
            fade_in_curve: clip.fade_in_curve,
            fade_out_curve: clip.fade_out_curve,
        }
    }

    fn curve(&self, edge: FadeEdge) -> FadeCurve {
        match edge {
            FadeEdge::In => self.fade_in_curve,
            FadeEdge::Out => self.fade_out_curve,
        }
    }
}

/// A lane on screen this frame: its track, where it is, and whether it can take notes or audio.
#[derive(Debug, Clone, Copy)]
struct Row {
    track: TrackId,
    time: Rect,
    clip: Rect,
}

/// What the view keeps between frames: scrolling, zoom, lane heights, the header column, the
/// selected clip and whatever is being dragged.
#[derive(Debug)]
pub struct ArrangementViewState {
    pub scroll: Vec2,
    /// Points per beat.
    pub beat_width: f32,
    /// Track headers on the left of the timeline instead of the right.
    pub headers_left: bool,
    /// Moved clips stack on others instead of replacing them: audio clips, MIDI clips.
    pub stack_audio: bool,
    pub stack_midi: bool,
    pub header_width: f32,
    heights: HashMap<TrackId, f32>,
    selected: Option<(TrackId, usize)>,
    clip_drag: Option<ClipDrag>,
    value_drag: Option<(TrackId, Control, f64)>,
    /// A clip gain being dragged, and its value so far.
    gain_drag: Option<((TrackId, usize), f64)>,
    fade_drag: Option<FadeDrag>,
    curve_drag: Option<CurveDrag>,
    /// Tracks selected by clicking their headers (Ctrl adds), for grouping.
    pub selected_tracks: Vec<TrackId>,
    /// A track header being dragged to a new place, and where it was let go.
    track_drag: Option<TrackId>,
    track_drop: Option<(TrackId, f32)>,
    /// The middle button is held after being pressed over the timeline: the view follows it.
    panning: bool,
    /// A drag from the browser over the window: the view says which target is under the
    /// pointer (`docs/spec/browser.md` › Dragging into the project).
    pub drop: Option<DropProbe>,
    /// The engine's levels this frame, for the headers' meters.
    pub meters: Meters,
    /// The piano roll is pulled to the top: the view shows only this track, highlighted, and
    /// no return tracks or main track (`docs/spec/ui-clip-editor.md` › Pulled to the top).
    pub focus: Option<TrackId>,
    /// Automation lanes under the tracks: which are shown, selected points, a drag.
    pub automation: LanesState,
}

impl Default for ArrangementViewState {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            beat_width: DEFAULT_BEAT_WIDTH,
            headers_left: false,
            stack_audio: false,
            stack_midi: false,
            header_width: HEADER_COLUMN,
            heights: HashMap::new(),
            selected: None,
            clip_drag: None,
            value_drag: None,
            gain_drag: None,
            fade_drag: None,
            curve_drag: None,
            selected_tracks: Vec::new(),
            track_drag: None,
            track_drop: None,
            panning: false,
            drop: None,
            meters: Meters::default(),
            focus: None,
            automation: LanesState::default(),
        }
    }
}

impl ArrangementViewState {
    /// The selected clip: its track and index.
    pub fn selected_clip(&self) -> Option<(TrackId, usize)> {
        self.selected
    }

    pub fn clear_selected_clip(&mut self) {
        self.selected = None;
    }

    /// Sets a lane's height, as dragging its lower edge does.
    pub fn set_lane_height(&mut self, track: TrackId, height: f32) {
        self.heights.insert(track, height.clamp(MIN_LANE, MAX_LANE));
    }

    /// Scales every track lane's height by `factor`, keeping the content `anchor` points below
    /// the top of the lanes where it is, as Alt and the wheel do.
    fn zoom_lanes(&mut self, lanes: &[Lane], factor: f32, anchor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let lanes: Vec<&Lane> = lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
        // Every lane already at the limit it is zoomed towards: nothing moves.
        let stuck = lanes.iter().all(|l| {
            let h = self.height(l);
            (factor > 1.0 && h >= MAX_LANE) || (factor < 1.0 && h <= MIN_LANE)
        });
        if stuck {
            return;
        }
        let content = |state: &Self| lanes.iter().map(|l| state.height(l) + 1.0).sum::<f32>();
        let before = content(self);
        for lane in &lanes {
            let height = self.height(lane) * factor;
            self.heights.insert(lane.track, height.clamp(MIN_LANE, MAX_LANE));
        }
        let ratio = if before > 0.0 { content(self) / before } else { 1.0 };
        self.scroll.y = ((self.scroll.y + anchor) * ratio - anchor).max(0.0);
    }

    /// What the clip at `key` shows: the project's gain and fades, or the value of a drag in
    /// progress on it (fades on `grid`, or free when it is `None`).
    fn shown(&self, key: (TrackId, usize), clip: &ClipBox, grid: Option<f64>) -> Shown {
        let mut shown = Shown::of(clip);
        if let Some((k, v)) = self.gain_drag
            && k == key
        {
            shown.gain_db = v;
        }
        if let Some(drag) = self.fade_drag.filter(|d| d.key == key) {
            match drag.edge {
                FadeEdge::In => shown.fade_in = fade_length(drag.raw, FadeEdge::In, clip, shown.fade_out, grid),
                FadeEdge::Out => shown.fade_out = fade_length(drag.raw, FadeEdge::Out, clip, shown.fade_in, grid),
            }
        }
        if let Some(drag) = self.curve_drag.filter(|d| d.key == key) {
            match drag.edge {
                FadeEdge::In => shown.fade_in_curve.curve = drag.curve,
                FadeEdge::Out => shown.fade_out_curve.curve = drag.curve,
            }
        }
        shown
    }

    fn height(&self, lane: &Lane) -> f32 {
        self.lane_height(lane.track)
    }

    /// How tall `track`'s lane is.
    pub fn lane_height(&self, track: TrackId) -> f32 {
        self.heights.get(&track).copied().unwrap_or(DEFAULT_LANE)
    }

    /// How tall the view is when it shows only `track` ([`Self::focus`]): the ruler and its lane.
    pub fn focus_height(&self, track: TrackId) -> f32 {
        RULER + 1.0 + self.lane_height(track) + 1.0
    }
}

/// Where a click `raw` beats into the ruler puts the play start marker: on the clip grid, or
/// where it was with `free` (Alt held); never before the start.
fn ruler_beat(raw: f64, bar_beats: f64, beat_width: f32, free: bool) -> f64 {
    let grid = snap(beat_width, bar_beats);
    let beat = if free || grid <= 0.0 { raw } else { (raw / grid).round() * grid };
    // Free clicks keep a thousandth of a beat, as the engine reports it.
    let beat = if free { (beat * 1000.0).round() / 1000.0 } else { beat };
    if beat.is_finite() { beat.max(0.0) } else { 0.0 }
}

/// The grid clips snap to at a zoom level: a sixteenth, a beat or a bar, whichever is at least
/// a few points wide.
pub(crate) fn snap(beat_width: f32, bar_beats: f64) -> f64 {
    if beat_width >= 48.0 {
        0.25
    } else if beat_width >= 12.0 {
        1.0
    } else {
        bar_beats
    }
}

/// A fade's length after a drag of `raw` beats: its end on the grid (unless `grid` is `None`,
/// Alt held), and no longer than the clip leaves beside the other fade. No fade and all the room
/// there is snap too, so a clip that does not start or end on the grid can still be faded over
/// its whole length.
fn fade_length(raw: f64, edge: FadeEdge, clip: &ClipBox, other: f64, grid: Option<f64>) -> f64 {
    if !raw.is_finite() {
        return 0.0;
    }
    let end = clip.start + clip.length;
    let room = (clip.length - other).max(0.0);
    let snapped = match grid.filter(|g| *g > 0.0) {
        Some(g) => {
            let on_grid = match edge {
                FadeEdge::In => ((clip.start + raw) / g).round() * g - clip.start,
                FadeEdge::Out => end - ((end - raw) / g).round() * g,
            };
            [0.0, room].into_iter().fold(on_grid, |best, v| if (v - raw).abs() < (best - raw).abs() { v } else { best })
        }
        None => raw,
    };
    if snapped.is_finite() { snapped.clamp(0.0, room) } else { 0.0 }
}

/// The gain a clip's fades give `beats` into a clip `length` beats long, each fade following its
/// shape and curve, 0 … 1 (`docs/spec/arrangement-editing.md` › Fades).
fn fade_gain(beats: f64, length: f64, shown: &Shown) -> f32 {
    let fade_in = if shown.fade_in > 0.0 { shown.fade_in_curve.gain(beats / shown.fade_in) } else { 1.0 };
    let fade_out =
        if shown.fade_out > 0.0 { shown.fade_out_curve.gain((length - beats) / shown.fade_out) } else { 1.0 };
    (fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32
}

/// What a fade shape is called in its menu.
fn shape_label(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::Linear => "Linear",
        FadeShape::EqualPower => "Equal power",
        FadeShape::Fast => "Fast",
        FadeShape::Slow => "Slow",
        FadeShape::SCurve => "S-curve",
    }
}

/// The curve that puts a fade's curve handle at height `y` in the clip `rect`: the handle sits on
/// the fade's line half-way through it, so it follows the pointer up and down until the curve
/// reaches −1 or +1.
fn curve_at(rect: Rect, shape: FadeShape, y: f32) -> f64 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let gain = if bottom > top { f64::from((bottom - y) / (bottom - top)) } else { f64::NAN };
    FadeCurve::through(shape, gain)
}

/// Most points a fade's line is drawn with, however wide the fade is on screen.
const FADE_LINE_POINTS: usize = 256;

/// The points of a fade's line in the clip `rect`: from the silent bottom corner to full level
/// at the top, following `curve`, from `from` to `to` (left to right; `to` is the silent end for
/// a fade-out). Sampled every 2 points, at most [`FADE_LINE_POINTS`].
fn fade_line(rect: Rect, edge: FadeEdge, from: f32, to: f32, curve: FadeCurve) -> Vec<Pos2> {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let width = (to - from).max(0.0);
    let steps = ((width / 2.0).ceil() as usize).clamp(1, FADE_LINE_POINTS - 1);
    (0..=steps)
        .map(|i| {
            let x = from + width * i as f32 / steps as f32;
            let along = if width > 0.0 { f64::from((x - from) / width) } else { 1.0 };
            let t = if edge == FadeEdge::In { along } else { 1.0 - along };
            Pos2::new(x, bottom - curve.gain(t) as f32 * (bottom - top))
        })
        .collect()
}

/// Where a fade's curve handle sits: on its line half-way through the fade.
fn curve_handle(rect: Rect, edge: FadeEdge, fade: f32, curve: FadeCurve) -> Pos2 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let x = match edge {
        FadeEdge::In => rect.left() + fade / 2.0,
        FadeEdge::Out => rect.right() - fade / 2.0,
    };
    Pos2::new(x, bottom - curve.gain(0.5) as f32 * (bottom - top))
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

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Draws the Arrangement view in the space left in `ui` and adds what the user asked for to
/// `actions`.
pub fn show(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
    // Showing one track, the view is not scrolled down; where it was is kept for later.
    let scrolled = state.scroll.y;
    if state.focus.is_some() {
        state.scroll.y = 0.0;
    }
    show_lanes(ui, timeline, state, actions);
    if state.focus.is_some() {
        state.scroll.y = scrolled;
    }
}

fn show_lanes(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
    // The track lanes shown: every one but the return tracks, or only the focused one.
    let focus = state.focus;
    let in_view = |l: &&Lane| l.kind != ColumnKind::Return && focus.is_none_or(|f| f == l.track);
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    // The header column, on either side, and the timeline beside it.
    let max_header = (area.width() - 120.0).max(HEADER_COLUMN_RANGE.0);
    state.header_width = state.header_width.clamp(HEADER_COLUMN_RANGE.0, HEADER_COLUMN_RANGE.1.min(max_header));
    let (headers, time_x) = if state.headers_left {
        let headers = egui::Rangef::new(area.left(), area.left() + state.header_width);
        (headers, egui::Rangef::new(headers.max + 1.0, area.right()))
    } else {
        let headers = egui::Rangef::new(area.right() - state.header_width, area.right());
        (headers, egui::Rangef::new(area.left(), headers.min - 1.0))
    };
    let time = Rect::from_x_y_ranges(time_x, area.y_range());
    let ruler = Rect::from_min_size(time.min, Vec2::new(time.width(), RULER));
    // Return tracks and the main track are fixed rows along the bottom.
    let returns: Vec<&Lane> =
        timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Return && focus.is_none()).collect();
    let main_volume_db = timeline.main_volume_db.filter(|_| focus.is_none());
    let fixed_rows = returns.len() + usize::from(main_volume_db.is_some());
    let fixed_height = fixed_rows as f32 * (ROW + 1.0);
    let fixed_top = (area.bottom() - fixed_height).max(ruler.bottom() + 1.0);
    let fixed = Rect::from_min_max(Pos2::new(area.left(), fixed_top), area.max);
    let lanes_area =
        Rect::from_min_max(Pos2::new(area.left(), ruler.bottom() + 1.0), Pos2::new(area.right(), fixed_top - 1.0));
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };

    // Zoom around the pointer with Ctrl and the wheel, lane heights with Alt and the wheel; the
    // wheel alone scrolls.
    if let Some(pointer) = widgets::pointer_over(ui, area) {
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 && time.contains(pointer) {
            let beat = (pointer.x - time.left() + state.scroll.x) / state.beat_width;
            state.beat_width = (state.beat_width * zoom).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
            state.scroll.x = beat * state.beat_width - (pointer.x - time.left());
        }
        let (alt, wheel) = ui.input(|i| (i.modifiers.alt, i.smooth_scroll_delta()));
        if alt {
            // Some systems turn Alt and the wheel into a sideways scroll; either axis zooms.
            let steps = if wheel.y == 0.0 { wheel.x } else { wheel.y };
            if steps != 0.0 && pointer.y > lanes_area.top() {
                state.zoom_lanes(&timeline.lanes, (steps * LANE_ZOOM_PER_POINT).exp(), pointer.y - lanes_area.top());
            }
        } else {
            state.scroll -= wheel;
        }
    }
    // Free movement with the middle button held: the timeline follows the pointer both ways.
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Middle),
            i.pointer.button_down(egui::PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = widgets::pointer_over(ui, time).is_some_and(|p| p.y > ruler.top());
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= delta;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let content_width = ((timeline.end + TAIL_BARS * bar_beats) as f32) * state.beat_width;
    let content_height: f32 = timeline
        .lanes
        .iter()
        .filter(in_view)
        .map(|l| state.height(l) + 1.0 + state.automation.height(l.track, &timeline.automation))
        .sum();
    state.scroll.x = state.scroll.x.clamp(0.0, (content_width - time.width()).max(0.0));
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - lanes_area.height()).max(0.0));

    let (scroll_x, beat_width) = (state.scroll.x, state.beat_width);
    let x_of = move |beat: f64| time.left() - scroll_x + beat as f32 * beat_width;
    let first_beat = f64::from(state.scroll.x / state.beat_width);
    let last_beat = f64::from((state.scroll.x + time.width()) / state.beat_width);

    // Ruler: bar numbers, a line at each labelled bar.
    painter.rect_filled(ruler, 0.0, theme::panel_background());
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
            ruler_painter.vline(x, ruler.y_range(), theme::hairline(theme::border()));
            let label = Rect::from_min_max(Pos2::new(x + 3.0, ruler.top()), Pos2::new(x + 40.0, ruler.bottom()));
            widgets::text_in(
                &ruler_painter,
                label,
                Align2::LEFT_CENTER,
                &(bar + 1).to_string(),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
        }
        // Grid in the lanes: every bar, and beats when they are far enough apart.
        for band in grid_bands.iter().filter(|b| b.span() > 0.0) {
            if bar_width >= 6.0 || bar % label_every == 0 {
                grid_painter.vline(x, *band, theme::hairline(theme::border()));
            }
            if state.beat_width >= 10.0 {
                for beat in 1..bar_beats.ceil() as u32 {
                    let x = x_of(f64::from(bar) * bar_beats + f64::from(beat)).round() + 0.5;
                    grid_painter.vline(x, *band, theme::hairline(theme::panel_background()));
                }
            }
        }
    }
    painter.hline(area.x_range(), ruler.bottom() + 0.5, theme::hairline(theme::border_strong()));

    // A click on the ruler moves the play start marker there, on the clip grid (Alt: free); while
    // playing, playback jumps there (`docs/spec/arrangement-editing.md` › Play position).
    let ruler_response = ui.interact(ruler, Id::new("arrangement-ruler"), Sense::click());
    ruler_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Ruler"));
    if ruler_response.clicked()
        && let Some(pointer) = ruler_response.interact_pointer_pos()
    {
        let free = ui.input(|i| i.modifiers.alt);
        let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
        actions.push(Action::new(
            "transport.setPosition",
            json!({ "beats": ruler_beat(raw, bar_beats, state.beat_width, free) }),
        ));
    }
    // The marker: a small triangle hanging from the ruler's lower edge.
    let marker_x = x_of(timeline.start_beat).round() + 0.5;
    if (time.left()..=time.right()).contains(&marker_x) {
        let (top, half) = (ruler.bottom() - MARKER.y, MARKER.x / 2.0);
        ruler_painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(marker_x - half, top), Pos2::new(marker_x + half, top), Pos2::new(marker_x, ruler.bottom())],
            theme::text_primary(),
            egui::Stroke::NONE,
        ));
    }

    // Header column top, beside the ruler: right-click moves the headers to the other side.
    let corner = Rect::from_x_y_ranges(headers, ruler.y_range());
    painter.rect_filled(corner, 0.0, theme::panel_background());
    let corner_response = ui.interact(corner, Id::new("arrangement-header-corner"), Sense::click());
    corner_response.context_menu(|ui| {
        let text = if state.headers_left { "Track headers on the right" } else { "Track headers on the left" };
        if ui.button(text).clicked() {
            state.headers_left = !state.headers_left;
            ui.close();
        }
    });
    let edge_x = if state.headers_left { headers.max + 0.5 } else { headers.min - 0.5 };
    painter.vline(edge_x, area.y_range(), theme::hairline(theme::border_strong()));

    // Where every lane is this frame, so a dragged clip can find the lane under the pointer.
    let mut rows = Vec::new();
    let mut y = lanes_area.top() - state.scroll.y;
    for lane in timeline.lanes.iter().filter(in_view) {
        let height = state.height(lane);
        let lane_time = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(y, y + height));
        rows.push(Row { track: lane.track, time: lane_time, clip: lane_time.intersect(lanes_area) });
        y += height + 1.0 + state.automation.height(lane.track, &timeline.automation);
    }
    let mut y = fixed.top();
    for lane in &returns {
        let lane_time = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(y, y + ROW));
        rows.push(Row { track: lane.track, time: lane_time, clip: lane_time.intersect(fixed) });
        y += ROW + 1.0;
    }
    let row_of = |track: TrackId| rows.iter().find(|r| r.track == track).copied();
    let lane_at = |pos: Pos2| rows.iter().find(|r| r.clip.y_range().contains(pos.y)).map(|r| r.track);

    // Group tracks a track can play into.
    let groups: Vec<(TrackId, String)> =
        timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Group).map(|l| (l.track, l.name.clone())).collect();

    // Lanes.
    state.automation.follow_clip(state.selected);
    let geometry = automation::Geometry {
        time: time.x_range(),
        headers,
        visible: lanes_area,
        scroll_x: state.scroll.x,
        beat_width: state.beat_width,
        grid: snap(state.beat_width, bar_beats),
    };
    let lanes_painter = painter.with_clip_rect(lanes_area);
    let mut track_number = 0;
    let mut ghost = None;
    for lane in timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return) {
        track_number += 1;
        let Some(row) = row_of(lane.track) else { continue };
        let lane_rect = Rect::from_x_y_ranges(area.x_range(), row.time.y_range());
        let below = state.automation.height(lane.track, &timeline.automation);
        if lane_rect.bottom() + below < lanes_area.top() || lane_rect.top() > lanes_area.bottom() {
            continue;
        }
        // The track the piano roll edits, alone in the view: lit, so it is clear which it is.
        if focus == Some(lane.track) {
            lanes_painter.rect_filled(row.time, 0.0, theme::panel_hover());
            let edge = Rect::from_min_size(row.time.min, Vec2::new(2.0, row.time.height()));
            lanes_painter.rect_filled(edge, 0.0, theme::accent());
        }
        for clip in &lane.clips {
            if clip.start + clip.length >= first_beat && clip.start <= last_beat {
                let found = clip_box(
                    ui,
                    &painter,
                    row,
                    lane,
                    clip,
                    &x_of,
                    bar_beats,
                    &lane_at,
                    &timeline.waveforms,
                    state,
                    actions,
                );
                ghost = ghost.or(found);
            }
        }
        let header = Rect::from_x_y_ranges(headers, row.time.y_range());
        track_header(ui, &lanes_painter, header, lanes_area, lane, track_number, &groups, timeline, state, actions);
        lanes_painter.hline(area.x_range(), lane_rect.bottom() + 0.5, theme::hairline(theme::border_strong()));
        // The track's automation lanes, under it.
        let lanes = automation::TrackLanes {
            track: lane.track,
            name: &lane.name,
            color: lane.color,
            top: lane_rect.bottom() + 1.0,
        };
        automation::show(ui, &lanes_painter, &geometry, &lanes, &timeline.automation, &mut state.automation, actions);
    }
    if state.automation.selected_points().is_some() {
        state.selected = None;
    }

    // A track header let go over a lane moves the track (and a group's tracks) there: above the
    // lane under the pointer, or below it from its lower half.
    let tracks: Vec<&Lane> = timeline.lanes.iter().filter(in_view).collect();
    let target_of = |y: f32| {
        let lane = tracks.iter().find(|l| row_of(l.track).is_some_and(|r| r.time.y_range().contains(y)))?;
        let row = row_of(lane.track)?;
        Some((*lane, y > row.time.center().y))
    };
    if let Some(dragged) = state.track_drag
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
        && let Some((lane, below)) = target_of(pointer.y)
        && let Some(row) = row_of(lane.track)
    {
        let y = if below { row.time.bottom() + 0.5 } else { row.time.top() - 0.5 };
        if lane.track != dragged {
            painter.with_clip_rect(lanes_area).hline(area.x_range(), y, egui::Stroke::new(2.0, theme::accent()));
        }
    }
    if let Some((dragged, y)) = state.track_drop.take()
        && let Some((lane, below)) = target_of(y)
        && let Some(source) = tracks.iter().find(|l| l.track == dragged)
        && lane.track != dragged
    {
        // The dragged track's block: it and the lanes nested under it.
        let index = tracks.iter().position(|l| l.track == dragged).unwrap_or_default();
        let block = 1 + tracks.iter().skip(index + 1).take_while(|l| l.depth > source.depth).count();
        let mut to = lane.position + usize::from(below);
        if lane.position > source.position {
            to = to.saturating_sub(block);
        }
        if to != source.position {
            actions.push(Action::new("track.move", json!({ "track": dragged.0, "to": to })));
        }
    }

    // Fixed rows: return tracks, then the main track, each a name and a volume.
    painter.hline(area.x_range(), fixed.top() - 0.5, theme::hairline(theme::border_strong()));
    let fixed_painter = painter.with_clip_rect(fixed);
    for lane in &returns {
        let Some(row) = row_of(lane.track) else { continue };
        for clip in &lane.clips {
            if clip.start + clip.length >= first_beat && clip.start <= last_beat {
                let found = clip_box(
                    ui,
                    &painter,
                    row,
                    lane,
                    clip,
                    &x_of,
                    bar_beats,
                    &lane_at,
                    &timeline.waveforms,
                    state,
                    actions,
                );
                ghost = ghost.or(found);
            }
        }
        let header = Rect::from_x_y_ranges(headers, row.time.y_range());
        fixed_row(ui, &fixed_painter, header, fixed, lane, state, actions);
        fixed_painter.hline(area.x_range(), row.time.bottom() + 0.5, theme::hairline(theme::border()));
    }
    if let Some(db) = main_volume_db {
        let y = fixed.bottom() - ROW;
        let header = Rect::from_x_y_ranges(headers, egui::Rangef::new(y, y + ROW));
        fixed_painter.rect_filled(header, 0.0, theme::panel_background());
        let inner = header.shrink2(Vec2::new(PAD, 2.0));
        let font = theme::ui_font(theme::TEXT);
        widgets::text_in(&fixed_painter, inner, Align2::LEFT_CENTER, "Main", font, theme::text_primary());
        let field = Rect::from_min_max(Pos2::new(inner.right() - FIXED_FIELD, inner.top()), inner.max);
        volume_text(&fixed_painter, field, db, 0.0);
        let at = lying_meter(field.left() - PAD, inner.center().y);
        let level = state.meters.main;
        meter::show(ui, &fixed_painter, at, "Main", meter::Source::Main, &level, actions);
    }

    // A clip dragged onto another lane is drawn there, over everything in that lane.
    if let Some((lane, clip, start)) = ghost
        && let Some(drag) = state.clip_drag
        && let Some(row) = row_of(drag.target)
        && let Some(target) = timeline.lanes.iter().find(|l| l.track == drag.target)
    {
        let rect = clip_rect(row.time, start, clip.length, &x_of);
        let lane_look = Lane { color: lane.color, mute: target.mute, ..target.clone() };
        paint_clip(
            &painter.with_clip_rect(row.clip),
            rect,
            &lane_look,
            clip,
            Shown::of(clip),
            start,
            &x_of,
            state.beat_width,
            &timeline.waveforms,
            true,
        );
    }

    // The empty space under the last lane: buttons that add a track, and new tracks for what
    // the browser drops there, at the beat under the pointer.
    let empty =
        Rect::from_min_max(Pos2::new(area.left(), tracks_bottom + 1.0), Pos2::new(area.right(), lanes_area.bottom()));
    if empty.height() > theme::ROW + 4.0 {
        let buttons = Rect::from_x_y_ranges(time.x_range(), empty.y_range());
        crate::session_view::empty_space(ui, &painter, buttons, actions);
    }
    let beat_at = |x: f32, free: bool| {
        let raw = f64::from((x - time.left() + state.scroll.x) / state.beat_width).max(0.0);
        if free {
            raw
        } else {
            let grid = snap(state.beat_width, bar_beats);
            ((raw / grid).round() * grid).max(0.0)
        }
    };
    let free = ui.input(|i| i.modifiers.alt);
    if let Some(probe) = state.drop.as_mut() {
        for row in rows.iter() {
            let Some(lane) = timeline.lanes.iter().find(|l| l.track == row.track) else { continue };
            if lane.kind != ColumnKind::Track {
                continue;
            }
            probe.offer(Rect::from_x_y_ranges(headers, row.clip.y_range()), DropTarget::Track(row.track));
            if probe.files {
                let beat = beat_at(probe.pointer.x, free);
                let x = x_of(beat);
                let mark =
                    Rect::from_x_y_ranges(egui::Rangef::new(x, (x + 2.0).min(row.clip.right())), row.clip.y_range());
                if row.clip.contains(probe.pointer) && probe.target.is_none() {
                    probe.target = Some(DropTarget::Lane { track: row.track, beat });
                    probe.area = Some(row.clip);
                    painter.rect_filled(mark, 0.0, theme::accent());
                }
            } else {
                probe.offer(row.clip, DropTarget::Track(row.track));
            }
        }
        if empty.height() > 0.0 && empty.contains(probe.pointer) {
            let beat = beat_at(probe.pointer.x.max(time.left()), free);
            probe.offer(empty, DropTarget::NewTracks { beat: Some(beat), scene: None });
        }
        if let Some(target_area) = probe.area {
            painter.rect_stroke(
                target_area.shrink(0.5),
                0.0,
                egui::Stroke::new(1.5, theme::accent()),
                egui::StrokeKind::Inside,
            );
        }
    }

    // The header column's inner edge resizes it.
    let edge = Rect::from_x_y_ranges(egui::Rangef::new(edge_x - 3.0, edge_x + 3.0), area.y_range());
    let response = ui
        .interact(edge, Id::new("arrangement-header-edge"), Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged_by(egui::PointerButton::Primary) {
        let dx = response.drag_delta().x;
        let width = if state.headers_left { state.header_width + dx } else { state.header_width - dx };
        state.header_width = width.clamp(HEADER_COLUMN_RANGE.0, HEADER_COLUMN_RANGE.1.min(max_header));
    }

    // Playhead through ruler and lanes.
    if timeline.running || timeline.beat > 0.0 {
        let x = x_of(timeline.beat).round() + 0.5;
        if x >= time.left() && x <= time.right() {
            painter.vline(x, time.y_range(), theme::hairline(theme::accent()));
        }
    }
}

/// The part of a lane a clip in sub-row `row` of `rows` is drawn in.
fn sub_row(lane_time: Rect, row: usize, rows: usize) -> Rect {
    let rows = rows.max(1) as f32;
    let height = lane_time.height() / rows;
    let top = lane_time.top() + height * row as f32;
    Rect::from_x_y_ranges(lane_time.x_range(), egui::Rangef::new(top.round(), (top + height).round() - 1.0))
}

fn clip_rect(lane_time: Rect, start: f64, length: f64, x_of: &dyn Fn(f64) -> f32) -> Rect {
    let left = x_of(start).round();
    Rect::from_min_max(
        Pos2::new(left, lane_time.top()),
        Pos2::new((x_of(start + length).round() - 1.0).max(left + 1.0), lane_time.bottom()),
    )
}

/// One arrangement clip: draws it and moves it when dragged, along its lane or onto another.
/// Returns the clip and its start when it is being dragged onto another lane, to be drawn there.
#[allow(clippy::too_many_arguments)]
fn clip_box<'a>(
    ui: &Ui,
    painter: &egui::Painter,
    row: Row,
    lane: &'a Lane,
    clip: &'a ClipBox,
    x_of: &dyn Fn(f64) -> f32,
    bar_beats: f64,
    lane_at: &dyn Fn(Pos2) -> Option<TrackId>,
    waveforms: &Waveforms,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) -> Option<(&'a Lane, &'a ClipBox, f64)> {
    let key = (lane.track, clip.index);
    let mine = |drag: &ClipDrag| (drag.track, drag.index) == key;
    let grid = snap(state.beat_width, bar_beats);
    let snapped = |raw: f64| ((raw / grid).round() * grid).max(0.0);
    let at = match state.clip_drag {
        Some(drag) if mine(&drag) => snapped(drag.start),
        _ => clip.start,
    };
    // Where it was last frame takes the pointer; it is drawn where this frame's drag puts it.
    let space = if clip.rows > 1 { sub_row(row.time, clip.row, clip.rows) } else { row.time };
    let visible = clip_rect(space, at, clip.length, x_of).intersect(row.clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return None;
    }
    let bar = (clip.start / bar_beats).floor() as u64 + 1;
    let name = if clip.name.is_empty() { "clip" } else { clip.name.as_str() };
    let response = ui
        .interact(visible, Id::new(("arrangement-clip", lane.track.0, clip.index)), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Grab);
    let label = format!("Clip {name} on {} at bar {bar}", lane.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let stack = if clip.audio { state.stack_audio } else { state.stack_midi };
    response.context_menu(|ui| menus::clip_menu(ui, lane.track, clip.index, stack, actions));
    if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        state.selected = Some(key);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        state.clip_drag =
            Some(ClipDrag { track: lane.track, index: clip.index, start: clip.start, target: lane.track });
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.as_mut()
        && (drag.track, drag.index) == key
    {
        drag.start += f64::from(response.drag_delta().x / state.beat_width);
        if let Some(target) = response.interact_pointer_pos().and_then(lane_at) {
            drag.target = target;
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.take()
        && (drag.track, drag.index) == key
    {
        let start = snapped(drag.start);
        let mut params = json!({ "track": lane.track.0, "index": clip.index, "start": start });
        if drag.target != lane.track {
            params["to_track"] = json!(drag.target.0);
        }
        if (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi) {
            params["stack"] = json!(true);
        }
        if drag.target != lane.track || (start - clip.start).abs() > 1e-9 {
            actions.push(Action::new("clip.move", params));
        }
        // Drawn where it was dropped until the project has moved it.
        let rect = clip_rect(space, start, clip.length, x_of);
        let shown = Shown::of(clip);
        paint_clip(
            &painter.with_clip_rect(row.clip),
            rect,
            lane,
            clip,
            shown,
            start,
            x_of,
            state.beat_width,
            waveforms,
            true,
        );
        return None;
    }

    // What the clip shows: the project's values, or those of a drag in progress, so the
    // waveform follows the gain and the fades while they are dragged.
    let fade_grid = (!ui.input(|i| i.modifiers.alt)).then_some(grid);
    let mut shown = state.shown(key, clip, fade_grid);

    // An audio clip big enough shows its gain, which a vertical drag changes.
    let mut gain_paint = None;
    if let Some(view) = &clip.audio_view
        && state.clip_drag.is_none_or(|d| !mine(&d))
    {
        let rect = clip_rect(space, clip.start, clip.length, x_of);
        let field = Rect::from_min_size(
            Pos2::new(rect.right() - GAIN_FIELD_INSET - GAIN_FIELD.x, rect.top() + 3.0),
            GAIN_FIELD,
        );
        if rect.width() >= 80.0 && rect.height() >= 30.0 && field.intersects(row.clip) {
            let label = format!("Gain of {name} on {}", lane.name);
            let response = widgets::value_area(
                ui,
                field.intersect(row.clip),
                Id::new(("clip-gain", lane.track.0, clip.index)),
                &label,
                widgets::format_db(shown.gain_db),
            );
            let mut preview = match state.gain_drag {
                Some((k, v)) if k == key => Some(v),
                _ => None,
            };
            let mut committed = None;
            match widgets::drag_value(ui, &response, &mut preview, view.gain_db, 0.1, CLIP_GAIN_RANGE, 0.0) {
                Drag::Moving(v) => state.gain_drag = Some((key, v)),
                Drag::Done(v) => {
                    state.gain_drag = None;
                    committed = Some(v);
                    let db = if v <= CLIP_GAIN_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                    actions.push(Action::new(
                        "clip.setGain",
                        json!({ "track": lane.track.0, "index": clip.index, "db": db }),
                    ));
                }
                Drag::None => {}
            }
            shown.gain_db = match state.gain_drag {
                Some((k, v)) if k == key => v,
                _ => committed.unwrap_or(view.gain_db),
            };
            gain_paint = Some((field, shown.gain_db, widgets::hover(ui, &response)));
        }
    }

    // Fade handles at the top corners of an audio clip, while it is hovered or selected: a
    // horizontal drag sets the fade (its end on the grid, free with Alt), a double-click
    // removes it, a right-click chooses its shape.
    let mut handles = Vec::new();
    let mut curves = Vec::new();
    let rect = clip_rect(space, clip.start, clip.length, x_of);
    let selected = state.selected == Some(key);
    let fading = state.fade_drag.is_some_and(|d| d.key == key) || state.curve_drag.is_some_and(|d| d.key == key);
    let pointer_in = widgets::pointer_over(ui, visible).is_some();
    let fits = rect.width() >= 3.0 * FADE_HANDLE && rect.height() >= 2.0 * FADE_HANDLE;
    if clip.audio && fits && state.clip_drag.is_none_or(|d| !mine(&d)) && (pointer_in || selected || fading) {
        let grid = fade_grid;
        for edge in [FadeEdge::In, FadeEdge::Out] {
            let (current, now, other) = match edge {
                FadeEdge::In => (clip.fade_in, shown.fade_in, shown.fade_out),
                FadeEdge::Out => (clip.fade_out, shown.fade_out, shown.fade_in),
            };
            let handle = fade_handle(rect, edge, now as f32 * state.beat_width);
            let hit = handle.expand((FADE_HANDLE_HIT - FADE_HANDLE) / 2.0).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let which = if edge == FadeEdge::In { "Fade-in" } else { "Fade-out" };
            let label = format!("{which} of {name} on {}", lane.name);
            let id = Id::new(("clip-fade", lane.track.0, clip.index, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(now);
                info
            });
            let mine_fade = |d: &FadeDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.fade_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    state.fade_drag = Some(FadeDrag { key, edge, raw: current });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.as_mut().filter(|d| mine_fade(d))
                {
                    let dx = f64::from(response.drag_delta().x / state.beat_width);
                    drag.raw += if edge == FadeEdge::In { dx } else { -dx };
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.take_if(|d| mine_fade(d))
                {
                    done = Some(fade_length(drag.raw, edge, clip, other, grid));
                }
            }
            let length = match (done, state.fade_drag) {
                (Some(v), _) => v,
                (None, Some(d)) if mine_fade(&d) => fade_length(d.raw, edge, clip, other, grid),
                _ => now,
            };
            if let Some(v) = done
                && (v - current).abs() > 1e-9
            {
                let param = if edge == FadeEdge::In { "in" } else { "out" };
                let mut params = json!({ "track": lane.track.0, "index": clip.index });
                params[param] = json!((v * 1e6).round() / 1e6);
                actions.push(Action::new("clip.setFades", params));
            }
            match edge {
                FadeEdge::In => shown.fade_in = length,
                FadeEdge::Out => shown.fade_out = length,
            }
            // A right-click chooses the fade's shape.
            let shape = shown.curve(edge).shape;
            response.context_menu(|ui| {
                for choice in FadeShape::ALL {
                    if ui.radio(choice == shape, shape_label(choice)).clicked() {
                        let param = if edge == FadeEdge::In { "inShape" } else { "outShape" };
                        let mut params = json!({ "track": lane.track.0, "index": clip.index });
                        params[param] = json!(choice.name());
                        actions.push(Action::new("clip.setFades", params));
                        ui.close();
                    }
                }
            });
            handles.push((edge, widgets::hover(ui, &response)));

            // The curve handle, half-way along a fade wide enough: a vertical drag bends the
            // curve, a double-click sets it back to 0.
            let fade_width = length as f32 * state.beat_width;
            if fade_width < CURVE_HANDLE_MIN_FADE || state.fade_drag.is_some_and(|d| mine_fade(&d)) {
                continue;
            }
            let current_curve = shown.curve(edge);
            let center = curve_handle(rect, edge, fade_width, current_curve);
            let hit = Rect::from_center_size(center, Vec2::splat(FADE_HANDLE_HIT)).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let label = format!("{which} curve of {name} on {}", lane.name);
            let id = Id::new(("clip-fade-curve", lane.track.0, clip.index, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(current_curve.curve);
                info
            });
            let mine_curve = |d: &CurveDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.curve_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    // Where the button went down, not where the drag was recognised: the handle
                    // keeps its place under the pointer.
                    let pressed = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                    let grab = pressed.map_or(0.0, |p| p.y - center.y);
                    state.curve_drag = Some(CurveDrag { key, edge, curve: current_curve.curve, grab });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                    && let Some(drag) = state.curve_drag.as_mut().filter(|d| mine_curve(d))
                {
                    drag.curve = curve_at(rect, current_curve.shape, pointer.y - drag.grab);
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.curve_drag.take_if(|d| mine_curve(d))
                {
                    done = Some(drag.curve);
                }
            }
            let stored = match edge {
                FadeEdge::In => clip.fade_in_curve.curve,
                FadeEdge::Out => clip.fade_out_curve.curve,
            };
            if let Some(v) = done {
                let v = (v * 1000.0).round() / 1000.0;
                if (v - stored).abs() > 1e-9 {
                    let param = if edge == FadeEdge::In { "inCurve" } else { "outCurve" };
                    let mut params = json!({ "track": lane.track.0, "index": clip.index });
                    params[param] = json!(v);
                    actions.push(Action::new("clip.setFades", params));
                }
                match edge {
                    FadeEdge::In => shown.fade_in_curve.curve = v,
                    FadeEdge::Out => shown.fade_out_curve.curve = v,
                }
            }
            curves.push((edge, widgets::hover(ui, &response)));
        }
    }

    let result = match state.clip_drag {
        Some(drag) if mine(&drag) && drag.target != lane.track => Some((lane, clip, snapped(drag.start))),
        drag => {
            let start = drag.filter(mine).map_or(clip.start, |d| snapped(d.start));
            let rect = clip_rect(space, start, clip.length, x_of);
            paint_clip(
                &painter.with_clip_rect(row.clip),
                rect,
                lane,
                clip,
                shown,
                start,
                x_of,
                state.beat_width,
                waveforms,
                selected,
            );
            None
        }
    };
    // The gain sits on top of the clip.
    if let Some((field, db, hover)) = gain_paint {
        let gain_painter = painter.with_clip_rect(row.clip);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
        gain_painter.rect_filled(field, 0.0, fill);
        let text = format!("{} dB", widgets::format_db(db));
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &gain_painter,
            field.shrink2(Vec2::new(3.0, 0.0)),
            Align2::RIGHT_CENTER,
            &text,
            font,
            theme::text_value(),
        );
    }
    // The handles sit on top of everything on the clip, where the fades now end.
    let handle_painter = painter.with_clip_rect(row.clip);
    for (edge, hover) in handles {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let handle = fade_handle(rect, edge, length as f32 * state.beat_width);
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.rect_filled(handle, 0.0, fill);
        handle_painter.rect_stroke(handle, 0.0, theme::hairline(theme::window_background()), StrokeKind::Outside);
    }
    for (edge, hover) in curves {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let center = curve_handle(rect, edge, length as f32 * state.beat_width, shown.curve(edge));
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.circle(center, CURVE_HANDLE / 2.0, fill, theme::hairline(theme::window_background()));
    }
    result
}

/// The square of a fade handle: at the clip's top corner with no fade, at the fade's end along
/// the top edge with one, `fade` points from the clip's edge; always inside the clip.
fn fade_handle(clip: Rect, edge: FadeEdge, fade: f32) -> Rect {
    let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
    let left = match edge {
        FadeEdge::In => (clip.left() + fade).min(clip.right() - FADE_HANDLE),
        FadeEdge::Out => (clip.right() - fade - FADE_HANDLE).max(clip.left()),
    };
    Rect::from_min_size(Pos2::new(left.round(), clip.top()), Vec2::splat(FADE_HANDLE))
}

fn view_rect(painter: &egui::Painter) -> Rect {
    painter.clip_rect()
}

/// Draws a clip's body, colour strip, name and the marks of its notes (only those in view).
#[allow(clippy::too_many_arguments)]
fn paint_clip(
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    clip: &ClipBox,
    shown: Shown,
    start: f64,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    waveforms: &Waveforms,
    selected: bool,
) {
    let view = painter.clip_rect();
    if !rect.intersects(view) {
        return;
    }
    let dim = |c: Color32| if lane.mute || !clip.enabled { c.gamma_multiply(0.55) } else { c };
    painter.rect_filled(rect, 0.0, if selected { theme::control_active() } else { theme::control_background() });
    let color = if clip.enabled { rgb(lane.color) } else { theme::text_hint() };
    painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, dim(color));
    let text_color = if clip.enabled && !lane.mute { theme::text_value() } else { theme::text_hint() };
    let has_name = rect.height() >= 20.0;
    if has_name {
        // The name stays in view while the clip's start is scrolled off to the left.
        let left = rect.left().max(view.left()) + 4.0;
        let name_rect =
            Rect::from_min_max(Pos2::new(left, rect.top() + 3.0), Pos2::new(rect.right() - 2.0, rect.top() + 15.0));
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
    if let Some(view) = clip.audio_view.as_ref().filter(|_| marks.height() >= 4.0)
        && let Some(peaks) = waveforms.get(&view.file)
    {
        // One column per point in view: the stretch of the file it shows, scaled by the clip's
        // gain and its fades, drawn as `docs/spec/waveforms.md` says, inside the clip.
        let gain = if shown.gain_db.is_finite() { 10f32.powf(shown.gain_db as f32 / 20.0) } else { 0.0 };
        let from = rect.left().max(view_rect(painter).left()).floor();
        let to = rect.right().min(view_rect(painter).right()).ceil();
        let per_point = 1.0 / f64::from(beat_width);
        let count = (to - from).max(0.0) as usize;
        let columns: Vec<waveform::Column> = (0..count)
            .map(|i| {
                let x = from + i as f32;
                let elapsed = f64::from(x - rect.left()) * per_point;
                let gain = gain * fade_gain(elapsed + per_point / 2.0, clip.length, &shown);
                let (a, b) = (view.seconds(elapsed), view.seconds(elapsed + per_point));
                waveform::Column { x: x + 0.5, from: a.min(b), to: a.max(b), gain }
            })
            .collect();
        let shape = waveform::geometry(peaks, marks, &columns, waveform::scale(painter.ctx()));
        waveform::paint(
            &painter.with_clip_rect(marks.intersect(painter.clip_rect())),
            &shape,
            waveform::Colors::from(dim(color)),
        );
    } else if clip.audio {
        painter.hline(marks.x_range(), marks.center().y.round() + 0.5, theme::hairline(dim(color)));
    } else if !clip.notes.is_empty() && marks.height() >= 4.0 {
        let low = clip.notes.iter().map(|n| n.key).min().unwrap_or(0);
        let high = clip.notes.iter().map(|n| n.key).max().unwrap_or(low);
        let rows = f32::from(high - low) + 1.0;
        // Only the notes in view are drawn; a long looped clip has thousands.
        let first = f64::from((view.left() - x_of(start)) / beat_width) - 1.0;
        let last = f64::from((view.right() - x_of(start)) / beat_width) + 1.0;
        for note in clip.notes.iter().filter(|n| n.start + n.length >= first && n.start <= last) {
            let x = x_of(start + note.start);
            let width = (note.length as f32 * beat_width - 1.0).max(1.0);
            let row = f32::from(note.key - low);
            let y = marks.bottom() - (row + 0.5) / rows * marks.height();
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - 1.0), Vec2::new(width, 2.0)), 0.0, dim(color));
        }
    }
    // A fade is a line over the waveform, from the clip's bottom corner to where it ends at
    // the top, following its shape and curve.
    if clip.audio {
        let fade_stroke = theme::hairline(dim(theme::text_secondary()));
        if shown.fade_in > 0.0 {
            let end = (rect.left() + shown.fade_in as f32 * beat_width).min(rect.right());
            painter.line(fade_line(rect, FadeEdge::In, rect.left(), end, shown.fade_in_curve), fade_stroke);
        }
        if shown.fade_out > 0.0 {
            let from = (rect.right() - shown.fade_out as f32 * beat_width).max(rect.left());
            painter.line(fade_line(rect, FadeEdge::Out, from, rect.right(), shown.fade_out_curve), fade_stroke);
        }
    }
    if selected {
        painter.rect_stroke(rect, 0.0, theme::hairline(theme::text_primary()), StrokeKind::Inside);
    }
}

/// The output dropdown: "→ Main" or a group; picking one runs `track.setOutput`.
fn routing(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    groups: &[(TrackId, String)],
    actions: &mut Vec<Action>,
) {
    let current = lane.output.and_then(|t| groups.iter().find(|(g, _)| *g == t)).map_or("Main", |(_, name)| name);
    let label = Rect::from_min_size(rect.min, Vec2::new(18.0, rect.height()));
    widgets::text_in(painter, label, Align2::LEFT_CENTER, "→", theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let mut picked = None;
    let response =
        widgets::button_area(ui, field, Id::new(("output", lane.track.0)), format!("{} output: {current}", lane.name));
    widgets::dropdown(ui, painter, field, &response, current);
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        if ui.selectable_label(lane.output.is_none(), text("Main")).clicked() && lane.output.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != lane.track) {
            if ui.selectable_label(lane.output == Some(*group), text(name)).clicked() && lane.output != Some(*group) {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": lane.track.0, "to": to })));
    }
}

/// Width of the meter lying in a return or main row, left of its volume.
const LYING_METER: f32 = 48.0;

/// Where a return or main row shows its meter: lying down, ending at `right`, centred on `y`.
fn lying_meter(right: f32, y: f32) -> Rect {
    Rect::from_min_max(Pos2::new(right - LYING_METER, y - meter::WIDTH / 2.0), Pos2::new(right, y + meter::WIDTH / 2.0))
}

/// A return track's row: its name, its meter and its volume.
fn fixed_row(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let inner = rect.shrink2(Vec2::new(PAD, 2.0));
    let field = Rect::from_min_max(Pos2::new(inner.right() - FIXED_FIELD, inner.top()), inner.max);
    let level = state.meters.track(lane.track);
    let at = lying_meter(field.left() - PAD, inner.center().y);
    meter::show(ui, painter, at.intersect(clip), &lane.name, meter::Source::Track(lane.track), &level, actions);
    let name = Rect::from_min_max(inner.min, Pos2::new(at.left() - PAD, inner.bottom()));
    let color = if lane.mute { theme::text_hint() } else { theme::text_primary() };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), color);
    let id = Id::new(("arrangement-header", lane.track.0));
    value_control(ui, painter, field, clip, lane, Control::Volume, true, id, state, actions);
}

#[allow(clippy::too_many_arguments)]
fn track_header(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lanes_area: Rect,
    lane: &Lane,
    track_number: usize,
    groups: &[(TrackId, String)],
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);

    // The header itself: click selects (Ctrl adds), right-click opens the track's menu, a drag
    // moves the track. Its controls, added after it, take their own clicks.
    let body = ui.interact(clip, id.with("body"), Sense::click_and_drag());
    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", lane.name)));
    if body.clicked() {
        if ui.input(|i| i.modifiers.command) {
            if let Some(at) = state.selected_tracks.iter().position(|t| *t == lane.track) {
                state.selected_tracks.remove(at);
            } else {
                state.selected_tracks.push(lane.track);
            }
        } else {
            state.selected_tracks = vec![lane.track];
        }
    }
    if body.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(lane.track);
    }
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = body.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.y));
    }
    let selection = state.selected_tracks.clone();
    body.context_menu(|ui| {
        let menu = TrackMenu {
            track: lane.track,
            name: &lane.name,
            is_group: lane.kind == ColumnKind::Group,
            selection: &selection,
        };
        menus::track_menu(ui, &menu, actions);
    });
    let selected = state.selected_tracks.contains(&lane.track) || state.focus == Some(lane.track);
    painter.rect_filled(rect, 0.0, if selected { theme::control_background() } else { theme::panel_background() });
    // The meter stands at the right edge; the rows keep clear of it.
    let meter_x = rect.right() - PAD - meter::WIDTH;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + lane.depth as f32 * 8.0, rect.top() + HEADER_PAD_Y),
        Pos2::new(meter_x - PAD, rect.bottom() - HEADER_PAD_Y),
    );
    let meter_bottom =
        if rect.height() >= SHOWS_MIXER { inner.top() + HEADER_ROW + PAD + FIELD } else { inner.top() + HEADER_ROW };
    let at = Rect::from_min_max(
        Pos2::new(meter_x, inner.top()),
        Pos2::new(meter_x + meter::WIDTH, meter_bottom.min(inner.bottom())),
    );
    let level = state.meters.track(lane.track);
    meter::show(ui, painter, at.intersect(clip), &lane.name, meter::Source::Track(lane.track), &level, actions);

    // Row 1: track on, colour, name.
    let row = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_ROW));
    let toggle = Rect::from_min_size(row.min, Vec2::new(ON_TOGGLE, HEADER_ROW));
    let solo = Rect::from_min_size(Pos2::new(toggle.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW));
    let square = Rect::from_min_size(Pos2::new(solo.right() + PAD, row.center().y - 3.0), Vec2::splat(6.0));
    painter.rect_filled(square, 0.0, if lane.mute { rgb(lane.color).gamma_multiply(0.55) } else { rgb(lane.color) });
    // The automation button at the end of the row shows and hides the track's lanes.
    let lanes_button = Rect::from_min_size(Pos2::new(row.right() - SOLO, row.top()), Vec2::new(SOLO, HEADER_ROW));
    if lane.kind != ColumnKind::Return && lanes_button.intersects(clip) {
        automation::header_button(
            ui,
            painter,
            lanes_button,
            lane.track,
            &lane.name,
            &mut state.automation,
            &timeline.automation,
        );
    }
    let name = Rect::from_min_max(
        Pos2::new(square.right() + PAD, row.top()),
        Pos2::new(lanes_button.left() - 4.0, row.bottom()),
    );
    let name_color = if lane.mute { theme::text_hint() } else { theme::text_primary() };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), name_color);
    if solo.intersects(clip)
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), lane.track.0, &lane.name, lane.solo)
    {
        actions.push(action);
    }
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

    // Row 3, taller still: where the track's audio goes.
    if rect.height() >= SHOWS_ROUTING {
        let top = row.bottom() + PAD + FIELD + PAD + 4.0;
        let line = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + DROPDOWN));
        if line.intersects(clip) {
            routing(ui, painter, line, lane, groups, actions);
        }
    }

    // The bottom edge resizes the lane.
    let edge =
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 3.0), Pos2::new(rect.right(), rect.bottom() + 2.0));
    if edge.intersects(lanes_area) {
        let response = ui
            .interact(edge.intersect(lanes_area), id.with("resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged_by(egui::PointerButton::Primary) {
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
    let mut committed = None;
    if rect.intersects(clip) {
        let (label, text) = match control {
            Control::Volume => (format!("{} volume", lane.name), widgets::format_db(shown)),
            Control::Pan => (format!("{} pan", lane.name), widgets::format_pan(shown)),
        };
        let response = widgets::value_area(ui, rect.intersect(clip), id.with(label.as_str()), &label, text);
        hover = widgets::hover(ui, &response);
        // Every frame of a drag sets the value, so it is heard while dragging; all but the first
        // continue its undo step, so the drag is one step.
        let continued = preview.is_some();
        let set = |v: f64| match control {
            Control::Volume => {
                let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                Action::new("track.setVolume", json!({ "track": track.0, "db": db, "continue": continued }))
            }
            Control::Pan => Action::new(
                "track.setPan",
                json!({ "track": track.0, "pan": (v * 100.0).round() / 100.0, "continue": continued }),
            ),
        };
        match widgets::drag_value(ui, &response, &mut preview, value, per_point, range, default) {
            Drag::Moving(v) => {
                state.value_drag = Some((track, control, v));
                actions.push(set(v));
            }
            Drag::Done(v) => {
                state.value_drag = None;
                committed = Some(v);
                actions.push(set(v));
            }
            Drag::None => {}
        }
    }
    // A drag (and the value it commits) follows the pointer directly and keeps the animation
    // there, so nothing glides back afterwards; other changes (an undo, say) glide.
    let anim = id.with(("shown", control as u8));
    let dragged = match state.value_drag {
        Some((t, c, v)) if t == track && c == control => Some(v),
        _ => committed,
    };
    let shown = match dragged {
        Some(v) => {
            motion::jump(ui.ctx(), anim, v as f32);
            v
        }
        None if value.is_finite() => f64::from(motion::value(ui.ctx(), anim, value as f32, motion::VALUE)),
        None => value,
    };
    match control {
        Control::Volume if plain => volume_text(painter, rect, shown, hover),
        Control::Volume => volume_field(painter, rect, shown, hover),
        Control::Pan => pan_field(painter, rect, shown, hover),
    }
}

/// A volume field: fill from the left up to the level, a tick at its end, the value on the right.
fn volume_field(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    let x = rect.left() + rect.width() * volume_position(db);
    painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, theme::control_playing());
    let tick = x.round() - 0.5;
    painter.vline(tick, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::text_value()));
    painter.vline(tick, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::text_value()));
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// A volume as a plain value, for the compact return and main rows.
fn volume_text(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let color = motion::mix(theme::text_value(), theme::text_primary(), hover);
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
    painter.rect_filled(rect, 0.0, motion::mix(theme::pan_background(), theme::panel_hover(), hover));
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::pan_centre()));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::pan_centre()));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::text_value());
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
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
    fn alt_and_the_wheel_zoom_every_lane_within_its_limits() {
        let timeline = Timeline::build(&crate::demo::demo_project(), None);
        let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
        let mut state = ArrangementViewState::default();
        state.set_lane_height(tracks[0].track, 100.0);
        state.zoom_lanes(&timeline.lanes, 2.0, 0.0);
        assert_eq!(state.height(tracks[0]), 200.0);
        assert!(tracks[1..].iter().all(|l| state.height(l) == DEFAULT_LANE * 2.0));
        // The content under the pointer stays put: what was 10 points down is now 20.
        state.scroll.y = 0.0;
        state.zoom_lanes(&timeline.lanes, 0.5, 10.0);
        assert_eq!(state.scroll.y, 0.0);
        state.scroll.y = 40.0;
        state.zoom_lanes(&timeline.lanes, 2.0, 10.0);
        assert!((state.scroll.y - 90.0).abs() < 1.0, "{}", state.scroll.y);
        for _ in 0..50 {
            state.zoom_lanes(&timeline.lanes, 0.5, 0.0);
        }
        assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
        state.zoom_lanes(&timeline.lanes, f32::NAN, 0.0);
        assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
    }

    fn audio_clip() -> ClipBox {
        let timeline = Timeline::build(&crate::demo::demo_project(), None);
        let clip = timeline.lanes.iter().flat_map(|l| &l.clips).next().cloned().unwrap();
        ClipBox { start: 2.0, length: 8.0, fade_in: 1.0, fade_out: 0.0, audio: true, ..clip }
    }

    #[test]
    fn a_dragged_gain_or_fade_is_what_the_clip_shows() {
        let clip = audio_clip();
        let key = (TrackId(1), 0);
        let mut state = ArrangementViewState::default();
        let linear = FadeCurve::default();
        assert_eq!(
            state.shown(key, &clip, Some(1.0)),
            Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 0.0, fade_in_curve: linear, fade_out_curve: linear }
        );
        // A curve being dragged bends that fade only.
        state.curve_drag = Some(CurveDrag { key, edge: FadeEdge::In, curve: 0.4, grab: 0.0 });
        assert_eq!(state.shown(key, &clip, Some(1.0)).fade_in_curve.curve, 0.4);
        assert_eq!(state.shown(key, &clip, Some(1.0)).fade_out_curve.curve, 0.0);
        state.curve_drag = None;
        // The waveform is drawn with the gain being dragged, not the one in the project.
        state.gain_drag = Some((key, -12.0));
        assert_eq!(state.shown(key, &clip, Some(1.0)).gain_db, -12.0);
        assert_eq!(state.shown((TrackId(1), 1), &clip, Some(1.0)).gain_db, 0.0, "only on the dragged clip");
        // A fade being dragged: on the grid, or free with Alt.
        state.fade_drag = Some(FadeDrag { key, edge: FadeEdge::Out, raw: 2.6 });
        assert_eq!(state.shown(key, &clip, Some(1.0)).fade_out, 3.0);
        assert!((state.shown(key, &clip, None).fade_out - 2.6).abs() < 1e-9);
    }

    #[test]
    fn a_fade_ends_on_the_grid_and_leaves_room_for_the_other() {
        let clip = audio_clip();
        // The fade-in's end snaps to the grid in project beats: the clip starts at beat 2.
        assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, Some(4.0)), 2.0);
        assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, None), 1.3);
        // The fade-out's start snaps: the clip ends at beat 10.
        assert_eq!(fade_length(1.3, FadeEdge::Out, &clip, 0.0, Some(4.0)), 2.0);
        // Never below 0, never past the other fade.
        assert_eq!(fade_length(-5.0, FadeEdge::In, &clip, 0.0, None), 0.0);
        assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 3.0, None), 5.0);
        assert_eq!(fade_length(f64::NAN, FadeEdge::In, &clip, 0.0, None), 0.0);
        // The clip's own ends snap too: from beat 10 back to beat 2.6, nearer the clip's start
        // (beat 2) than the grid line at beat 4, the fade-out takes the whole clip, and the
        // fade-in from beat 2 to 9.5 takes the room the fade-out leaves.
        assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 0.0, Some(4.0)), 8.0);
        assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 1.0, Some(4.0)), 7.0);
        assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 0.0, Some(4.0)), 8.0);
        assert_eq!(fade_length(0.4, FadeEdge::Out, &clip, 0.0, Some(4.0)), 0.0);
        // Nearer the grid line, the grid wins.
        assert_eq!(fade_length(6.5, FadeEdge::Out, &clip, 0.0, Some(4.0)), 6.0);
    }

    #[test]
    fn fade_handles_follow_the_fade_and_stay_inside_the_clip() {
        let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 40.0));
        assert_eq!(fade_handle(clip, FadeEdge::In, 0.0).min, Pos2::new(100.0, 10.0));
        assert_eq!(fade_handle(clip, FadeEdge::Out, 0.0).max.x, 180.0);
        assert_eq!(fade_handle(clip, FadeEdge::In, 20.0).min.x, 120.0);
        assert_eq!(fade_handle(clip, FadeEdge::Out, 500.0).min.x, 100.0);
        assert_eq!(fade_handle(clip, FadeEdge::In, f32::NAN).min.x, 100.0);
        // Linear: half-way into the fade-in, half the level; full in between the fades.
        let linear = FadeCurve::default();
        let shown = Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 2.0, fade_in_curve: linear, fade_out_curve: linear };
        assert_eq!(fade_gain(0.5, 8.0, &shown), 0.5);
        assert_eq!(fade_gain(4.0, 8.0, &shown), 1.0);
        assert_eq!(fade_gain(7.0, 8.0, &shown), 0.5);
        // Other shapes and curves follow the spec's formula.
        let slow = FadeCurve { shape: FadeShape::Slow, curve: 0.0 };
        let shown = Shown { fade_in_curve: slow, fade_out_curve: FadeCurve { curve: 1.0, ..slow }, ..shown };
        assert_eq!(fade_gain(0.5, 8.0, &shown), 0.25);
        assert!((fade_gain(7.0, 8.0, &shown) - 0.25f32.powf(0.25)).abs() < 1e-6);
    }

    /// A fade's line runs from the silent bottom corner to full level at the top along its
    /// shape, and the curve handle sits on it half-way; dragging up bends towards +1 (#197).
    #[test]
    fn fade_lines_and_curve_handles_follow_the_shape() {
        let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 42.0));
        let (top, bottom) = (12.0, 52.0);
        for shape in FadeShape::ALL {
            let curve = FadeCurve { shape, curve: 0.0 };
            let line = fade_line(clip, FadeEdge::In, 100.0, 140.0, curve);
            assert_eq!(line.first().copied(), Some(Pos2::new(100.0, bottom)), "{shape:?}");
            assert_eq!(line.last().copied(), Some(Pos2::new(140.0, top)), "{shape:?}");
            let out = fade_line(clip, FadeEdge::Out, 150.0, 180.0, curve);
            assert_eq!((out[0], out[out.len() - 1]), (Pos2::new(150.0, top), Pos2::new(180.0, bottom)), "{shape:?}");
            let handle = curve_handle(clip, FadeEdge::In, 40.0, curve);
            assert_eq!(handle.x, 120.0);
            assert!((handle.y - (bottom - curve.gain(0.5) as f32 * 40.0)).abs() < 1e-4, "{shape:?}");
        }
        assert_eq!(curve_handle(clip, FadeEdge::Out, 30.0, FadeCurve::default()).x, 165.0);
        // The clip's height is the whole range: half of it up is +1 from 0.
        // The handle follows the pointer: the curve set by a height puts the handle there.
        for shape in FadeShape::ALL {
            let unbent = curve_handle(clip, FadeEdge::In, 40.0, FadeCurve { shape, curve: 0.0 }).y;
            for y in [unbent - 6.0, unbent - 1.0, unbent, unbent + 1.0, unbent + 6.0] {
                let curve = FadeCurve { shape, curve: curve_at(clip, shape, y) };
                if (-1.0..=1.0).contains(&curve.curve) && curve.curve.abs() < 0.999 {
                    let at = curve_handle(clip, FadeEdge::In, 40.0, curve).y;
                    assert!((at - y).abs() < 0.01, "{shape:?}: handle at {at}, pointer at {y}");
                }
            }
            assert_eq!(curve_at(clip, shape, -500.0), 1.0, "{shape:?}: kept within -1 … +1");
            assert_eq!(curve_at(clip, shape, 500.0), -1.0, "{shape:?}");
        }
        assert!(curve_at(clip, FadeShape::Linear, f32::NAN).is_finite());
        // However wide the fade, its line has a bounded number of points.
        assert!(fade_line(clip, FadeEdge::In, 100.0, 1_000_000.0, FadeCurve::default()).len() <= FADE_LINE_POINTS);
        assert_eq!(shape_label(FadeShape::SCurve), "S-curve");
    }

    #[test]
    fn clips_snap_to_a_grid_that_follows_the_zoom() {
        assert_eq!(snap(60.0, 4.0), 0.25);
        assert_eq!(snap(20.0, 4.0), 1.0);
        assert_eq!(snap(4.0, 3.0), 3.0);
    }

    #[test]
    fn a_ruler_click_lands_on_the_clip_grid_unless_alt_is_held() {
        // 8 points a beat: the grid is a bar.
        assert_eq!(ruler_beat(17.9, 4.0, 8.0, false), 16.0);
        assert_eq!(ruler_beat(18.1, 4.0, 8.0, false), 20.0);
        // 60 points a beat: sixteenths.
        assert_eq!(ruler_beat(2.13, 4.0, 60.0, false), 2.25);
        assert_eq!(ruler_beat(2.1337, 4.0, 60.0, true), 2.134);
        assert_eq!(ruler_beat(-3.0, 4.0, 8.0, true), 0.0);
        assert_eq!(ruler_beat(f64::NAN, 4.0, 8.0, false), 0.0);
    }
}
