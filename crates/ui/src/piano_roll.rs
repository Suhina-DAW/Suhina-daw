//! The piano roll: the MIDI clip editor in the detail area (`docs/spec/ui-clip-editor.md`). A key
//! column, a ruler, the note grid and a velocity lane. Every edit is a `note.*` command; the view
//! keeps only what is on screen: zoom, scroll, the selected notes and a gesture in progress.

use std::collections::BTreeSet;

use egui::{Align2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::Quantization;
use suhina_engine::model::{LaneData, Note, Project, TrackId};

use crate::grid::default_track_colour;
use crate::theme;
use crate::widgets::{self, Action};

/// Width of the key column, height of the ruler and the default height of the velocity lane.
const KEYS: f32 = 36.0;
const RULER: f32 = 16.0;
const VELOCITY_LANE: f32 = 72.0;
/// Key row heights and points per beat: the default and the zoom ranges.
const DEFAULT_KEY_HEIGHT: f32 = 12.0;
const KEY_HEIGHT_RANGE: (f32, f32) = (6.0, 32.0);
const BEAT_WIDTH_RANGE: (f32, f32) = (4.0, 400.0);
/// Grid lines closer than this, in points, are thinned out.
const MIN_LINE_SPACING: f32 = 6.0;
/// The most grid lines drawn in a frame, so a bad zoom can never draw without end.
const MAX_LINES: usize = 4000;
/// How close to a note's right end, in points, the pointer resizes it instead of moving it.
const EDGE: f32 = 5.0;
/// How close to a velocity stem, in points, the pointer grabs it.
const STEM_REACH: f32 = 4.0;
/// The shortest note a resize leaves, in beats.
const MIN_LENGTH: f64 = 0.01;
/// The length of a note drawn without a grid, in beats, and its velocity.
const FREE_LENGTH: f64 = 0.25;
const NEW_VELOCITY: u8 = 100;
const EPSILON: f64 = 1e-9;

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    let name = NAMES.get(usize::from(key % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(key / 12) - 2)
}

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// A MIDI clip as the piano roll shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoClip {
    /// The params that name the clip in a `note.*` command: `track` and `index` or `scene`.
    pub address: Value,
    pub notes: Vec<Note>,
    /// Where the part of the content the clip plays ends, in beats (its loop's end when it
    /// loops): the grid past it is shaded.
    pub length: f64,
    pub color: Color32,
    /// Beats in a bar.
    pub bar_beats: f64,
    /// The grid step in beats (the control bar's grid value); 0 is none.
    pub grid: f64,
}

/// The piano roll for the arrangement clip `index` on `track`, with the grid `quantization`;
/// `None` when the clip is missing or plays audio.
pub fn arrangement_clip(
    project: &Project,
    track: TrackId,
    index: usize,
    quantization: Quantization,
) -> Option<PianoClip> {
    let clip = project.arrangement.lanes.iter().find(|l| l.track == Some(track))?.clips.get(index)?;
    let content = project.content(clip.content)?;
    let mut notes = None;
    for lane in content.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)) {
        match &lane.data {
            LaneData::Audio(_) => return None,
            LaneData::Notes(n) if notes.is_none() => notes = Some(n.clone()),
            LaneData::Notes(_) => {}
        }
    }
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return None;
    }
    let position = project.tracks.iter().position(|t| t.id == track)?;
    let color = project.tracks.get(position).and_then(|t| t.color).unwrap_or_else(|| default_track_colour(position));
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    Some(PianoClip {
        address: json!({ "track": track.0, "index": index }),
        notes: notes.unwrap_or_default(),
        length: content_end(clip.loop_range.map(|l| l.end), clip.offset, clip.length, content.length),
        color: Color32::from_rgb(color.0, color.1, color.2),
        bar_beats,
        grid: grid_step(quantization, bar_beats),
    })
}

/// Where the part of the content a clip plays ends, in content beats: its loop's end when it
/// loops, else its offset and length; at least a beat.
fn content_end(loop_end: Option<f64>, offset: f64, length: f64, content_length: f64) -> f64 {
    let end = loop_end.unwrap_or(offset + length);
    let end = if end.is_finite() && end > 0.0 { end } else { content_length };
    if end.is_finite() { end.clamp(1.0, f64::from(BEAT_LIMIT)) } else { 4.0 }
}

/// The furthest the piano roll reaches, in beats: the engine's furthest start.
const BEAT_LIMIT: f32 = 120_000.0;

/// The grid step in beats for the control bar's value; 0 for none.
pub fn grid_step(quantization: Quantization, bar_beats: f64) -> f64 {
    let step = match quantization {
        Quantization::None => 0.0,
        Quantization::Bars(bars) => f64::from(bars) * bar_beats,
        Quantization::Beats(beats) => beats,
    };
    if step.is_finite() && step > 0.0 { step } else { 0.0 }
}

/// The grids quantise offers, as `note.quantize` takes them.
pub const QUANTIZE_GRIDS: [&str; 13] =
    ["1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32", "2 bars", "4 bars", "8 bars"];

/// What Ctrl+U and the Quantise panel quantise with; kept until changed, for the whole session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantizeSettings {
    /// One of [`QUANTIZE_GRIDS`].
    pub grid: &'static str,
    /// How far towards the grid, 0 to 100 percent.
    pub amount: f64,
    /// Whether the notes' ends move to the grid too.
    pub ends: bool,
}

impl Default for QuantizeSettings {
    fn default() -> Self {
        Self { grid: "1/16", amount: 100.0, ends: false }
    }
}

/// A drag in progress, from where it started.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    /// The selection follows the grabbed note.
    Move { from: Pos2, grabbed: usize },
    /// The selection's lengths follow the grabbed note's right end.
    Resize { from: Pos2, grabbed: usize },
    /// A rectangle selects the notes it touches; `add` keeps the selection.
    Select { from: Pos2, add: bool },
    /// The selection's velocities follow the grabbed stem.
    Velocity { from: Pos2, grabbed: usize },
}

/// What the piano roll keeps between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoRollState {
    /// The clip shown last, as its address: another clip resets the view.
    shown: Option<Value>,
    /// Points scrolled from beat 0 and from the top of the highest key.
    pub scroll: Vec2,
    pub beat_width: f32,
    pub key_height: f32,
    /// Indices of the selected notes.
    pub selected: BTreeSet<usize>,
    /// The last press was in the piano roll, so Delete deletes notes there.
    pub focused: bool,
    gesture: Option<Gesture>,
    panning: bool,
    /// The settings quantise uses, and whether the Quantise panel is open.
    pub quantize: QuantizeSettings,
    pub quantize_open: bool,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            shown: None,
            scroll: Vec2::ZERO,
            beat_width: 40.0,
            key_height: DEFAULT_KEY_HEIGHT,
            selected: BTreeSet::new(),
            focused: false,
            gesture: None,
            panning: false,
            quantize: QuantizeSettings::default(),
            quantize_open: false,
        }
    }
}

impl PianoRollState {
    /// `note.delete` for the selected notes, when the piano roll has the keyboard and notes are
    /// selected; the selection is then cleared.
    pub fn delete_selected(&mut self) -> Option<Action> {
        if !self.focused || self.selected.is_empty() {
            return None;
        }
        let mut params = self.shown.clone()?;
        params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        self.selected.clear();
        Some(Action::new("note.delete", params))
    }

    /// `note.quantize` for the selected notes, or every note when none is selected, with the
    /// last settings; `None` when no clip is shown.
    pub fn quantize_selected(&self) -> Option<Action> {
        let mut params = self.shown.clone()?;
        if !self.selected.is_empty() {
            params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        }
        params["grid"] = json!(self.quantize.grid);
        params["amount"] = json!(self.quantize.amount);
        params["ends"] = json!(self.quantize.ends);
        Some(Action::new("note.quantize", params))
    }

    /// Forgets the clip, so the next one shown is fitted to the view again.
    pub fn hide(&mut self) {
        self.shown = None;
        self.selected.clear();
        self.gesture = None;
        self.focused = false;
        self.quantize_open = false;
    }
}

/// Where things are this frame.
#[derive(Debug, Clone, Copy)]
struct Layout {
    keys: Rect,
    ruler: Rect,
    grid: Rect,
    lane: Rect,
    scroll: Vec2,
    beat_width: f32,
    key_height: f32,
}

impl Layout {
    fn x_of(&self, beat: f64) -> f32 {
        self.grid.left() - self.scroll.x + beat as f32 * self.beat_width
    }

    fn beat_at(&self, x: f32) -> f64 {
        f64::from((x - self.grid.left() + self.scroll.x) / self.beat_width)
    }

    fn y_of(&self, key: u8) -> f32 {
        self.grid.top() - self.scroll.y + f32::from(127 - key.min(127)) * self.key_height
    }

    /// The key whose row is at `y`, if any.
    fn key_at(&self, y: f32) -> Option<u8> {
        let row = ((y - self.grid.top() + self.scroll.y) / self.key_height).floor();
        (0.0..=127.0).contains(&row).then(|| 127 - row as u8)
    }

    fn note_rect(&self, note: &Note) -> Rect {
        let left = self.x_of(note.time);
        let right = self.x_of(note.time + note.duration.max(0.0)).max(left + 2.0);
        let top = self.y_of(note.key);
        Rect::from_min_max(Pos2::new(left, top + 1.0), Pos2::new(right - 1.0, top + self.key_height))
    }
}

/// The grid as drawn at this zoom: its step (the chosen one, doubled until lines are far
/// enough apart) and whether things snap to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteGrid {
    pub step: f64,
    pub snaps: bool,
}

impl NoteGrid {
    pub fn new(chosen: f64, beat_width: f32) -> Self {
        let snaps = chosen.is_finite() && chosen > 0.0;
        let mut step = if snaps { chosen } else { 1.0 };
        let points = f64::from(beat_width.max(f32::MIN_POSITIVE));
        while step * points < f64::from(MIN_LINE_SPACING) && step < 1e6 {
            step *= 2.0;
        }
        Self { step, snaps }
    }

    /// `beat` on the nearest line (or the line before it with `down`), unless `free` or there is
    /// no snapping.
    fn place(&self, beat: f64, free: bool, down: bool) -> f64 {
        if free || !self.snaps {
            return beat;
        }
        let lines = beat / self.step;
        (if down { (lines + EPSILON).floor() } else { lines.round() }) * self.step
    }
}

/// How far a drag of `raw` beats and `raw_keys` semitones moves the selection: by whole grid
/// steps unless `free`, and never past the content's start or the keys 0 to 127.
pub fn move_delta(raw: f64, raw_keys: i32, moved: &[Note], grid: NoteGrid, free: bool) -> (f64, i32) {
    let mut beats = if free || !grid.snaps { raw } else { (raw / grid.step).round() * grid.step };
    let earliest = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    if earliest.is_finite() {
        beats = beats.max(-earliest);
    }
    let lowest = moved.iter().map(|n| i32::from(n.key)).min().unwrap_or(0);
    let highest = moved.iter().map(|n| i32::from(n.key)).max().unwrap_or(127);
    let keys = raw_keys.clamp(-lowest, 127 - highest);
    (if beats.is_finite() { beats } else { 0.0 }, keys)
}

/// How much a drag of the grabbed note's right end by `raw` beats changes the selection's
/// lengths: the end on the grid unless `free`, and no note shorter than [`MIN_LENGTH`].
pub fn resize_by(raw: f64, grabbed: &Note, resized: &[Note], grid: NoteGrid, free: bool) -> f64 {
    let end = grabbed.time + grabbed.duration;
    let mut new_end = grid.place(end + raw, free, false);
    if grid.snaps && !free && new_end <= grabbed.time + EPSILON {
        // Snapped onto or before its start: the end goes to the first line after the start.
        new_end = ((grabbed.time / grid.step + EPSILON).floor() + 1.0) * grid.step;
    }
    let shortest = resized.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
    let by = new_end - end;
    let by = if shortest.is_finite() { by.max(MIN_LENGTH - shortest) } else { by };
    if by.is_finite() { by } else { 0.0 }
}

/// The note at `pos`, the topmost (latest) first, and whether `pos` is on its right end.
fn note_at(layout: &Layout, notes: &[Note], pos: Pos2) -> Option<(usize, bool)> {
    notes.iter().enumerate().rev().find_map(|(i, n)| {
        let rect = layout.note_rect(n).expand2(Vec2::new(0.0, 0.5));
        rect.contains(pos).then(|| (i, pos.x >= rect.right() - EDGE.min(rect.width() / 3.0)))
    })
}

/// The velocity stem nearest to `pos` within reach, a selected one first.
fn stem_at(layout: &Layout, notes: &[Note], selected: &BTreeSet<usize>, pos: Pos2) -> Option<usize> {
    let near = |(i, n): &(usize, &Note)| {
        let dx = (layout.x_of(n.time) - pos.x).abs();
        (dx <= STEM_REACH).then_some((*i, dx))
    };
    let candidates: Vec<(usize, f32)> = notes.iter().enumerate().filter_map(|e| near(&e)).collect();
    let best = |list: &mut dyn Iterator<Item = (usize, f32)>| list.min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| c.0);
    best(&mut candidates.iter().copied().filter(|(i, _)| selected.contains(i)))
        .or_else(|| best(&mut candidates.iter().copied()))
}

fn velocity_height(velocity: f32, lane: Rect) -> f32 {
    let v = f32::from(suhina_session::midi_velocity(velocity));
    (lane.height() - 4.0).max(0.0) * v / 127.0
}

/// Draws the piano roll for `clip` in `area` and adds the edits made in it to `actions`.
pub fn show(ui: &mut Ui, area: Rect, clip: &PianoClip, state: &mut PianoRollState, actions: &mut Vec<Action>) {
    if state.shown.as_ref() != Some(&clip.address) {
        state.shown = Some(clip.address.clone());
        state.selected.clear();
        state.gesture = None;
        fit(state, area, clip);
    }
    state.selected.retain(|i| *i < clip.notes.len());
    let lane_height = VELOCITY_LANE.min((area.height() - RULER) * 0.3).max(0.0);
    let keys = Rect::from_min_max(
        Pos2::new(area.left(), area.top() + RULER),
        Pos2::new(area.left() + KEYS, area.bottom() - lane_height),
    );
    let ruler = Rect::from_min_max(Pos2::new(keys.right(), area.top()), Pos2::new(area.right(), keys.top()));
    let grid_rect = Rect::from_min_max(Pos2::new(keys.right(), keys.top()), Pos2::new(area.right(), keys.bottom()));
    let lane = Rect::from_min_max(Pos2::new(keys.right(), keys.bottom()), area.max);
    if grid_rect.width() <= 1.0 || grid_rect.height() <= 1.0 {
        return;
    }

    let (pressed, alt, shift, command) = ui.input(|i| {
        (i.pointer.any_pressed(), i.modifiers.alt, i.modifiers.shift, i.modifiers.command || i.modifiers.ctrl)
    });
    if pressed {
        state.focused = widgets::pointer_over(ui, area).is_some();
    }
    scroll_and_zoom(ui, state, area, grid_rect, shift);
    state.scroll.y = state.scroll.y.clamp(0.0, (128.0 * state.key_height - grid_rect.height()).max(0.0));
    let width = ((clip.length + clip.bar_beats * 4.0) as f32 * state.beat_width).max(grid_rect.width());
    state.scroll.x = state.scroll.x.clamp(0.0, (width - grid_rect.width()).max(0.0));
    let layout = Layout {
        keys,
        ruler,
        grid: grid_rect,
        lane,
        scroll: state.scroll,
        beat_width: state.beat_width,
        key_height: state.key_height,
    };
    let grid = NoteGrid::new(clip.grid, state.beat_width);

    // The grid's gestures: one widget for the whole grid, so a note under the pointer and the
    // empty grid around it are told apart by where the press was.
    let response = ui.interact(grid_rect, Id::new("piano-roll-grid"), Sense::click_and_drag());
    let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
    if let Some(pos) = ui.ctx().pointer_hover_pos().filter(|p| grid_rect.contains(*p)) {
        let cursor = match (state.gesture, note_at(&layout, &clip.notes, pos)) {
            (Some(Gesture::Resize { .. }), _) | (None, Some((_, true))) => Some(CursorIcon::ResizeHorizontal),
            (Some(Gesture::Move { .. }), _) => Some(CursorIcon::Grabbing),
            _ => None,
        };
        if let Some(cursor) = cursor {
            ui.ctx().set_cursor_icon(cursor);
        }
    }
    if response.double_clicked()
        && let Some(pos) = pointer
    {
        match note_at(&layout, &clip.notes, pos) {
            Some((index, _)) => {
                state.selected.clear();
                let mut params = clip.address.clone();
                params["notes"] = json!([index]);
                actions.push(Action::new("note.delete", params));
            }
            None => {
                if let Some(key) = layout.key_at(pos.y) {
                    let time = grid.place(layout.beat_at(pos.x).max(0.0), alt, true).max(0.0);
                    let length = if grid.snaps && !alt { clip.grid.max(MIN_LENGTH) } else { FREE_LENGTH };
                    let mut params = clip.address.clone();
                    params["notes"] =
                        json!([{ "time": round_beat(time), "length": length, "key": key, "velocity": NEW_VELOCITY }]);
                    actions.push(Action::new("note.add", params));
                    // `note.add` appends: the new note is the next index.
                    state.selected = BTreeSet::from([clip.notes.len()]);
                }
            }
        }
    } else if response.clicked()
        && let Some(pos) = pointer
    {
        match note_at(&layout, &clip.notes, pos) {
            Some((index, _)) if shift || command => {
                if !state.selected.remove(&index) {
                    state.selected.insert(index);
                }
            }
            Some((index, _)) => state.selected = BTreeSet::from([index]),
            None => state.selected.clear(),
        }
    }
    if response.drag_started_by(PointerButton::Primary) {
        let from = ui.input(|i| i.pointer.press_origin()).or(pointer).unwrap_or(grid_rect.center());
        state.gesture = Some(match note_at(&layout, &clip.notes, from) {
            Some((grabbed, edge)) => {
                if !state.selected.contains(&grabbed) {
                    if !(shift || command) {
                        state.selected.clear();
                    }
                    state.selected.insert(grabbed);
                }
                if edge { Gesture::Resize { from, grabbed } } else { Gesture::Move { from, grabbed } }
            }
            None => Gesture::Select { from, add: shift || command },
        });
    }

    // The velocity lane's gestures.
    let lane_response =
        (lane.height() > 4.0).then(|| ui.interact(lane, Id::new("piano-roll-velocity"), Sense::click_and_drag()));
    if let Some(lane_response) = &lane_response {
        let at = lane_response.interact_pointer_pos();
        if lane_response.clicked()
            && let Some(pos) = at
        {
            match stem_at(&layout, &clip.notes, &BTreeSet::new(), pos) {
                Some(index) if shift || command => {
                    if !state.selected.remove(&index) {
                        state.selected.insert(index);
                    }
                }
                Some(index) => state.selected = BTreeSet::from([index]),
                None => {}
            }
        }
        if lane_response.drag_started_by(PointerButton::Primary) {
            let from = ui.input(|i| i.pointer.press_origin()).or(at).unwrap_or(lane.center());
            if let Some(grabbed) = stem_at(&layout, &clip.notes, &state.selected, from) {
                if !state.selected.contains(&grabbed) {
                    state.selected = BTreeSet::from([grabbed]);
                }
                state.gesture = Some(Gesture::Velocity { from, grabbed });
            }
        }
    }

    // The drag's effect, shown while it lasts and made a command when it ends.
    let now = ui.input(|i| i.pointer.latest_pos());
    let dragging = response.dragged_by(PointerButton::Primary)
        || lane_response.as_ref().is_some_and(|r| r.dragged_by(PointerButton::Primary));
    let stopped = response.drag_stopped_by(PointerButton::Primary)
        || lane_response.as_ref().is_some_and(|r| r.drag_stopped_by(PointerButton::Primary));
    let chosen: Vec<Note> = state.selected.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
    let preview = match (state.gesture, now) {
        (Some(gesture), Some(now)) if dragging || stopped => {
            Some(effect(gesture, now, &layout, clip, &chosen, grid, alt))
        }
        _ => None,
    };
    if stopped {
        if let (Some(effect), Some(gesture)) = (preview, state.gesture) {
            commit(gesture, effect, clip, state, actions, &layout);
        }
        state.gesture = None;
    } else if !dragging && !ui.input(|i| i.pointer.primary_down()) {
        state.gesture = None;
    }

    draw(ui, &layout, clip, state, grid, preview);
    for_readers(ui, &layout, clip, state);
    quantize_panel(ui, Rect::from_min_max(area.min, Pos2::new(keys.right(), ruler.bottom())), state, actions);
}

/// The Quantise button in the corner over the keys, and the panel it opens: the grid, the amount
/// and whether ends move, kept for Ctrl+U, and a button that quantises now.
fn quantize_panel(ui: &mut Ui, corner: Rect, state: &mut PianoRollState, actions: &mut Vec<Action>) {
    let response = widgets::button_area(ui, corner, Id::new("piano-roll-quantize"), "Quantise");
    if response.clicked() {
        state.quantize_open = !state.quantize_open;
    }
    let painter = ui.painter_at(corner);
    let hover = widgets::hover(ui, &response);
    let fill = if state.quantize_open { theme::control_hover() } else { theme::control_background() };
    painter.rect_filled(corner, 0.0, crate::motion::mix(fill, theme::control_hover(), hover));
    widgets::text_in(
        &painter,
        corner,
        Align2::CENTER_CENTER,
        "Q",
        theme::mono_font(theme::TEXT_TINY),
        theme::text_value(),
    );
    if !state.quantize_open {
        return;
    }
    let mut settings = state.quantize;
    let mut apply = false;
    let area = egui::Area::new(Id::new("piano-roll-quantize-panel"))
        .order(egui::Order::Foreground)
        .fixed_pos(corner.left_bottom() + Vec2::new(0.0, 2.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Quantise");
                egui::ComboBox::from_label("Grid").selected_text(settings.grid).show_ui(ui, |ui| {
                    for grid in QUANTIZE_GRIDS {
                        ui.selectable_value(&mut settings.grid, grid, grid);
                    }
                });
                ui.add(egui::Slider::new(&mut settings.amount, 0.0..=100.0).suffix(" %").integer().text("Amount"));
                ui.checkbox(&mut settings.ends, "Move ends too");
                ui.horizontal(|ui| {
                    apply = ui.button("Quantise notes").clicked();
                    if ui.button("Close").clicked() {
                        state.quantize_open = false;
                    }
                });
            });
        });
    state.quantize = settings;
    if apply && let Some(action) = state.quantize_selected() {
        actions.push(action);
    }
    // A press outside the panel and its button, or Escape, closes it.
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let outside = ui.input(|i| i.pointer.any_pressed())
        && ui.ctx().pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !corner.contains(p))
        && !egui::Popup::is_any_open(ui.ctx());
    if escape || outside {
        state.quantize_open = false;
    }
}

/// What a gesture does with the pointer at `now`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Effect {
    Move(f64, i32),
    Resize(f64),
    Select(Rect),
    Velocity(f64),
}

fn effect(
    gesture: Gesture,
    now: Pos2,
    layout: &Layout,
    clip: &PianoClip,
    chosen: &[Note],
    grid: NoteGrid,
    free: bool,
) -> Effect {
    match gesture {
        Gesture::Move { from, .. } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            let keys = (-(now.y - from.y) / layout.key_height).round() as i32;
            let (beats, keys) = move_delta(raw, keys, chosen, grid, free);
            Effect::Move(beats, keys)
        }
        Gesture::Resize { from, grabbed } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            clip.notes
                .get(grabbed)
                .map_or(Effect::Resize(0.0), |g| Effect::Resize(resize_by(raw, g, chosen, grid, free)))
        }
        Gesture::Select { from, .. } => Effect::Select(Rect::from_two_pos(from, now)),
        Gesture::Velocity { from, .. } => {
            let per_point = 127.0 / (layout.lane.height() - 4.0).max(1.0);
            Effect::Velocity(f64::from(-(now.y - from.y) * per_point).round())
        }
    }
}

fn commit(
    gesture: Gesture,
    effect: Effect,
    clip: &PianoClip,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    layout: &Layout,
) {
    let notes: Vec<usize> = state.selected.iter().copied().collect();
    let mut params = clip.address.clone();
    params["notes"] = json!(notes);
    match (gesture, effect) {
        (Gesture::Move { .. }, Effect::Move(beats, keys)) if !notes.is_empty() && (beats != 0.0 || keys != 0) => {
            params["beats"] = json!(round_beat(beats));
            params["semitones"] = json!(keys);
            actions.push(Action::new("note.move", params));
        }
        (Gesture::Resize { .. }, Effect::Resize(by)) if !notes.is_empty() && by != 0.0 => {
            params["by"] = json!(round_beat(by));
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::Velocity { .. }, Effect::Velocity(by)) if !notes.is_empty() && by != 0.0 => {
            params["by"] = json!(by);
            actions.push(Action::new("note.setVelocity", params));
        }
        (Gesture::Select { add, .. }, Effect::Select(rect)) => {
            if !add {
                state.selected.clear();
            }
            for (i, note) in clip.notes.iter().enumerate() {
                if layout.note_rect(note).intersects(rect) {
                    state.selected.insert(i);
                }
            }
        }
        _ => {}
    }
}

/// Beats rounded to a millionth, so a value made of steps such as thirds sends no float noise.
fn round_beat(beat: f64) -> f64 {
    (beat * 1e6).round() / 1e6
}

/// A newly shown clip: the whole clip fits the width, and its notes are in the middle.
fn fit(state: &mut PianoRollState, area: Rect, clip: &PianoClip) {
    let width = (area.width() - KEYS).max(1.0);
    state.beat_width = (width / clip.length.max(1.0) as f32).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
    state.scroll.x = 0.0;
    let (low, high) = clip.notes.iter().fold((u8::MAX, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
    let middle = if low <= high { (f32::from(low) + f32::from(high)) / 2.0 } else { 60.0 };
    let height = (area.height() - RULER - VELOCITY_LANE.min((area.height() - RULER) * 0.3)).max(1.0);
    state.scroll.y = (127.0 - middle) * state.key_height + state.key_height / 2.0 - height / 2.0;
}

/// The wheel scrolls, with Ctrl it zooms time and with Ctrl+Shift key height, around the
/// pointer; the middle button pans.
fn scroll_and_zoom(ui: &Ui, state: &mut PianoRollState, area: Rect, grid: Rect, shift: bool) {
    if let Some(pointer) = widgets::pointer_over(ui, area) {
        let (zoom, wheel) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta()));
        if zoom != 1.0 {
            if shift {
                let key = (pointer.y - grid.top() + state.scroll.y) / state.key_height;
                state.key_height = (state.key_height * zoom).clamp(KEY_HEIGHT_RANGE.0, KEY_HEIGHT_RANGE.1);
                state.scroll.y = key * state.key_height - (pointer.y - grid.top());
            } else {
                let beat = (pointer.x - grid.left() + state.scroll.x) / state.beat_width;
                state.beat_width = (state.beat_width * zoom).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
                state.scroll.x = beat * state.beat_width - (pointer.x - grid.left());
            }
        } else {
            state.scroll -= wheel;
        }
    }
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Middle),
            i.pointer.button_down(PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = widgets::pointer_over(ui, grid).is_some();
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= delta;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
}

fn draw(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState, grid: NoteGrid, preview: Option<Effect>) {
    let painter = ui.painter_at(layout.keys.union(layout.ruler).union(layout.grid).union(layout.lane));
    let grid_painter = painter.with_clip_rect(layout.grid);
    let first_key = layout.key_at(layout.grid.bottom() - 0.5).unwrap_or(0);
    let last_key = layout.key_at(layout.grid.top() + 0.5).unwrap_or(127);

    // Rows: black keys darker, a stronger line under each C.
    painter.rect_filled(layout.grid, 0.0, theme::window_background());
    for key in first_key..=last_key {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.grid.left(), top),
            Pos2::new(layout.grid.right(), top + layout.key_height),
        );
        let fill = if is_black(key) { theme::window_background() } else { theme::panel_background() };
        grid_painter.rect_filled(row, 0.0, fill);
        let line = if key % 12 == 0 { theme::border_strong() } else { theme::border() };
        grid_painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(line));
    }

    // Vertical lines: the grid, bars stronger; the ruler's bar numbers.
    let first_beat = layout.beat_at(layout.grid.left()).max(0.0);
    let last_beat = layout.beat_at(layout.grid.right());
    let bar = clip.bar_beats.max(EPSILON);
    painter.rect_filled(layout.ruler, 0.0, theme::panel_background());
    let lane_painter = painter.with_clip_rect(layout.lane);
    let mut beat = (first_beat / grid.step).floor() * grid.step;
    let mut drawn = 0;
    while beat <= last_beat && drawn < MAX_LINES {
        let x = layout.x_of(beat).round() + 0.5;
        let on_bar = ((beat / bar) - (beat / bar).round()).abs() < 1e-6;
        let color = if on_bar { theme::border_strong() } else { theme::border() };
        grid_painter.vline(x, layout.grid.y_range(), theme::hairline(color));
        lane_painter.vline(x, layout.lane.y_range(), theme::hairline(color));
        beat += grid.step;
        drawn += 1;
    }
    let ruler_painter = painter.with_clip_rect(layout.ruler);
    let bar_points = (clip.bar_beats as f32 * layout.beat_width).max(1.0);
    let label_every = (36.0 / bar_points).ceil().max(1.0) as u64;
    let first_bar = (first_beat / bar).floor() as u64;
    let last_bar = (last_beat / bar).ceil().max(0.0) as u64;
    for n in (first_bar..=last_bar).filter(|n| n % label_every == 0).take(MAX_LINES) {
        let x = layout.x_of(n as f64 * bar).round() + 0.5;
        ruler_painter.vline(x, layout.ruler.y_range(), theme::hairline(theme::border_strong()));
        let label =
            Rect::from_min_max(Pos2::new(x + 3.0, layout.ruler.top()), Pos2::new(x + 40.0, layout.ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &(n + 1).to_string(),
            theme::mono_font(theme::TEXT_TINY),
            theme::text_value(),
        );
    }

    // Past the clip's end: shaded.
    let end = layout.x_of(clip.length);
    if end < layout.grid.right() {
        let past = Rect::from_min_max(Pos2::new(end.max(layout.grid.left()), layout.grid.top()), layout.grid.max);
        grid_painter.rect_filled(past, 0.0, Color32::from_black_alpha(90));
    }

    // Notes, where a drag in progress puts them.
    let shown = |i: usize, note: &Note| -> Note {
        if !state.selected.contains(&i) {
            return *note;
        }
        match preview {
            Some(Effect::Move(beats, keys)) => Note {
                time: note.time + beats,
                key: u8::try_from(i32::from(note.key) + keys).unwrap_or(note.key).min(127),
                ..*note
            },
            Some(Effect::Resize(by)) => Note { duration: (note.duration + by).max(MIN_LENGTH), ..*note },
            Some(Effect::Velocity(by)) => {
                let midi = (f64::from(suhina_session::midi_velocity(note.velocity)) + by).clamp(1.0, 127.0);
                Note { velocity: (midi / 127.0) as f32, ..*note }
            }
            _ => *note,
        }
    };
    let selecting = match preview {
        Some(Effect::Select(rect)) => Some(rect),
        _ => None,
    };
    for (i, note) in clip.notes.iter().enumerate() {
        let note = shown(i, note);
        let rect = layout.note_rect(&note);
        if !rect.intersects(layout.grid) {
            continue;
        }
        let velocity = f32::from(suhina_session::midi_velocity(note.velocity));
        let alpha = 0.4 + 0.6 * velocity / 127.0;
        grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(alpha));
        let selected = state.selected.contains(&i) || selecting.is_some_and(|r| r.intersects(rect));
        if selected {
            grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
        }
        if rect.width() >= 28.0 && rect.height() >= 10.0 {
            widgets::text_in(
                &grid_painter,
                rect.shrink2(Vec2::new(3.0, 0.0)),
                Align2::LEFT_CENTER,
                &key_name(note.key),
                theme::mono_font(theme::TEXT_TINY),
                theme::window_background(),
            );
        }
        // Its velocity stem.
        let x = rect.left().round() + 0.5;
        let top = layout.lane.bottom() - 2.0 - velocity_height(note.velocity, layout.lane);
        let stem = if selected { theme::text_primary() } else { clip.color };
        lane_painter.vline(x, egui::Rangef::new(top, layout.lane.bottom()), Stroke::new(1.0, stem));
        lane_painter.rect_filled(Rect::from_min_size(Pos2::new(x - 2.5, top), Vec2::new(5.0, 2.0)), 0.0, stem);
    }
    if let Some(rect) = selecting {
        grid_painter.rect_stroke(rect, 0.0, theme::hairline(theme::text_secondary()), StrokeKind::Inside);
    }
    painter.hline(layout.lane.x_range(), layout.lane.top() + 0.5, theme::hairline(theme::border_strong()));

    // The key column: keys, every C named.
    let key_painter = painter.with_clip_rect(layout.keys);
    key_painter.rect_filled(layout.keys, 0.0, theme::window_background());
    for key in first_key..=last_key {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.keys.left(), top),
            Pos2::new(layout.keys.right(), top + layout.key_height),
        );
        let fill = if is_black(key) { theme::window_background() } else { theme::control_background() };
        key_painter.rect_filled(row.shrink2(Vec2::new(0.0, 0.5)), 0.0, fill);
        if key % 12 == 0 && layout.key_height >= 8.0 {
            widgets::text_in(
                &key_painter,
                row.shrink2(Vec2::new(3.0, 0.0)),
                Align2::LEFT_CENTER,
                &key_name(key),
                theme::mono_font(theme::TEXT_TINY),
                theme::text_secondary(),
            );
        }
    }
    let corner = Rect::from_min_max(
        Pos2::new(layout.keys.left(), layout.ruler.top()),
        Pos2::new(layout.keys.right(), layout.ruler.bottom()),
    );
    painter.rect_filled(corner, 0.0, theme::panel_background());
    let lane_keys = Rect::from_min_max(
        Pos2::new(layout.keys.left(), layout.lane.top()),
        Pos2::new(layout.keys.right(), layout.lane.bottom()),
    );
    painter.rect_filled(lane_keys, 0.0, theme::panel_background());
    if lane_keys.height() >= 24.0 {
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &painter,
            Rect::from_min_size(lane_keys.min, Vec2::new(KEYS - 3.0, 10.0)),
            Align2::RIGHT_CENTER,
            "127",
            font.clone(),
            theme::text_hint(),
        );
        widgets::text_in(
            &painter,
            Rect::from_min_max(
                Pos2::new(lane_keys.left(), lane_keys.bottom() - 10.0),
                Pos2::new(lane_keys.right() - 3.0, lane_keys.bottom()),
            ),
            Align2::RIGHT_CENTER,
            "1",
            font,
            theme::text_hint(),
        );
    }
    painter.vline(layout.keys.right() - 0.5, layout.keys.y_range(), theme::hairline(theme::border_strong()));
}

/// Names the piano roll's parts for screen readers and tests: the grid, each note in view and
/// its velocity stem.
fn for_readers(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let grid = ui.interact(layout.grid, Id::new("piano-roll-readers"), Sense::hover());
    let count = clip.notes.len();
    grid.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Piano roll, {count} notes")));
    for (i, note) in clip.notes.iter().enumerate() {
        let rect = layout.note_rect(note).intersect(layout.grid);
        if !rect.is_positive() {
            continue;
        }
        let selected = state.selected.contains(&i);
        let label = format!("Note {} at beat {}", key_name(note.key), round_beat(note.time));
        let response = ui.interact(rect, Id::new(("piano-roll-note", i)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Other, true, selected, &label));
        let x = layout.x_of(note.time);
        let stem = Rect::from_min_max(Pos2::new(x - 2.0, layout.lane.top()), Pos2::new(x + 2.0, layout.lane.bottom()))
            .intersect(layout.lane);
        if stem.is_positive() {
            let velocity = suhina_session::midi_velocity(note.velocity);
            let label =
                format!("Velocity of note {} at beat {}: {velocity}", key_name(note.key), round_beat(note.time));
            let response = ui.interact(stem, Id::new(("piano-roll-stem", i)), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, duration: f64, key: u8) -> Note {
        Note { time, duration, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0 }
    }

    #[test]
    fn the_editor_spans_the_loop_or_the_clips_part_of_its_content() {
        assert_eq!(content_end(Some(4.0), 0.0, 48.0, 4.0), 4.0);
        assert_eq!(content_end(None, 2.0, 8.0, 16.0), 10.0);
        assert_eq!(content_end(None, 0.0, f64::NAN, 16.0), 16.0);
        assert_eq!(content_end(None, 0.0, 0.25, 16.0), 1.0);
        assert_eq!(content_end(None, 0.0, 1e300, 16.0), 120_000.0);
    }

    #[test]
    fn keys_are_named_with_60_as_c3() {
        assert_eq!(key_name(60), "C3");
        assert_eq!(key_name(0), "C-2");
        assert_eq!(key_name(127), "G8");
        assert_eq!(key_name(61), "C#3");
        assert!(is_black(61) && !is_black(60));
    }

    #[test]
    fn the_grid_follows_the_chosen_value_and_thins_out_when_lines_crowd() {
        assert_eq!(grid_step(Quantization::Beats(0.25), 4.0), 0.25);
        assert_eq!(grid_step(Quantization::Bars(1), 3.0), 3.0);
        assert_eq!(grid_step(Quantization::None, 4.0), 0.0);
        assert_eq!(NoteGrid::new(0.25, 40.0), NoteGrid { step: 0.25, snaps: true });
        // A sixteenth at 10 points a beat is 2.5 points: doubled twice to 10.
        assert_eq!(NoteGrid::new(0.25, 10.0).step, 1.0);
        assert_eq!(NoteGrid::new(0.0, 40.0), NoteGrid { step: 1.0, snaps: false });
        let grid = NoteGrid::new(0.25, 40.0);
        assert_eq!(grid.place(1.3, false, false), 1.25);
        assert_eq!(grid.place(1.3, false, true), 1.25);
        assert_eq!(grid.place(1.45, false, true), 1.25);
        assert_eq!(grid.place(1.3, true, false), 1.3);
    }

    #[test]
    fn a_move_goes_by_grid_steps_and_stops_at_the_limits() {
        let grid = NoteGrid::new(0.25, 40.0);
        let notes = [note(1.1, 1.0, 60), note(2.0, 1.0, 120)];
        // By whole steps, so an off-grid note stays as far off the grid.
        assert_eq!(move_delta(0.3, 0, &notes, grid, false), (0.25, 0));
        assert_eq!(move_delta(0.3, 0, &notes, grid, true), (0.3, 0));
        // Never before the start, never above key 127: the whole selection stops.
        assert_eq!(move_delta(-5.0, 0, &notes, grid, false), (-1.1, 0));
        assert_eq!(move_delta(0.0, 12, &notes, grid, false), (0.0, 7));
        assert_eq!(move_delta(0.0, -100, &notes, grid, false), (0.0, -60));
    }

    #[test]
    fn a_resize_puts_the_end_on_the_grid_and_keeps_notes_long_enough() {
        let grid = NoteGrid::new(0.25, 40.0);
        let grabbed = note(1.0, 1.0, 60);
        let other = note(3.0, 0.5, 62);
        assert_eq!(resize_by(0.3, &grabbed, &[grabbed, other], grid, false), 0.25);
        assert!((resize_by(0.3, &grabbed, &[grabbed], grid, true) - 0.3).abs() < 1e-12);
        // Dragged onto its start: one step long.
        assert_eq!(resize_by(-1.0, &grabbed, &[grabbed], grid, false), -0.75);
        // The other selected note is shorter: it keeps the shortest length.
        assert!((resize_by(-0.75, &grabbed, &[grabbed, other], grid, false) - (MIN_LENGTH - 0.5)).abs() < 1e-12);
    }
}
