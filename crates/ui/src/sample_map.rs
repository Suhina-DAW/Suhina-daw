//! The sample map view (docs/spec/sample-map.md): points laid out by how they sound, coloured by
//! kind, as a cloud or a grid. The view draws and reports what the user did as [`MapEvent`]s;
//! the browser turns them into its own selection, previews and drags.

use std::collections::HashSet;

use egui::{
    Align2, Color32, CornerRadius, Key, PointerButton, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use suhina_soundmap::grid::Grid;
use suhina_soundmap::kinds::Kind;

use crate::theme;

/// How long the pointer rests on a point before it is previewed, so a fast sweep does not play
/// everything it crosses.
pub const HOVER_PREVIEW_SECONDS: f64 = 0.040;
/// How near (in points of the screen) the pointer must be to a point to be over it.
const HIT_RADIUS: f32 = 7.0;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 64.0;
/// A kind with fewer items than this in view gets no label.
const LABEL_MIN_ITEMS: usize = 8;
/// Above this zoom the kind labels fade out, as point names become readable.
const LABEL_FADE_ZOOM: f32 = 6.0;
const TOP_BAR: f32 = 24.0;
const LEGEND: f32 = 20.0;
const SIMILAR_WIDTH: f32 = 180.0;

/// One point, as the browser gives it to the view.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPoint {
    /// The file name shown in the tooltip and the similar list.
    pub name: String,
    /// More lines for the tooltip: folder, duration, tempo and key.
    pub detail: String,
    /// Position in 0–1 (the cloud layout).
    pub position: [f32; 2],
    pub kind: Kind,
    pub guessed: bool,
    /// Used by a clip of the active project.
    pub in_use: bool,
    pub selected: bool,
    /// Matches the search text, the tags and the legend (dimmed when not).
    pub matches: bool,
    /// An NKS preset, drawn as a ring.
    pub preset: bool,
    pub pinned: bool,
    /// The colour tags it carries.
    pub tags: Vec<String>,
}

/// Cloud or grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Cloud,
    Grid,
}

/// What the user did on the map. Indices are into the points given to [`show`].
#[derive(Debug, Clone, PartialEq)]
pub enum MapEvent {
    /// Preview this item (the pointer rests on it, or it was clicked or reached by a key).
    Preview(usize),
    /// Select these items; `add` keeps the selection and toggles them.
    Select {
        items: Vec<usize>,
        add: bool,
    },
    /// Drag these items into the project.
    StartDrag(Vec<usize>),
    /// Add this item to the selected track (double-click).
    Add(usize),
    /// The user asked for items similar to this one; the browser answers with
    /// [`MapView::show_similar`].
    Similar(usize),
    /// Pin it, or unpin it when it is pinned.
    Pin(usize),
    /// Set (`on`) or clear a colour tag on it.
    Tag {
        item: usize,
        tag: String,
        on: bool,
    },
    ShowInFolder(usize),
    /// The layout switch changed.
    Layout(Layout),
    /// The large-view button.
    ToggleLarge,
}

/// The view's own state, kept by the browser between frames.
#[derive(Debug, Clone)]
pub struct MapView {
    pub layout: Layout,
    pub preview_on_hover: bool,
    /// The map is shown large, in the main area.
    pub large: bool,
    zoom: f32,
    /// The map position at the middle of the view.
    centre: [f32; 2],
    hovered: Option<(usize, f64)>,
    previewed: Option<usize>,
    /// Kinds the legend shows; the others are dimmed. Empty: all.
    pub legend: HashSet<Kind>,
    /// Shift-drag rectangle, from where it started.
    rubber_band: Option<Pos2>,
    similar: Option<(usize, Vec<usize>)>,
    /// Where the points were drawn last frame.
    area: Rect,
}

impl Default for MapView {
    fn default() -> Self {
        MapView {
            layout: Layout::Cloud,
            preview_on_hover: true,
            large: false,
            zoom: 1.0,
            centre: [0.5, 0.5],
            hovered: None,
            previewed: None,
            legend: HashSet::new(),
            rubber_band: None,
            similar: None,
            area: Rect::NOTHING,
        }
    }
}

impl MapView {
    /// Shows the answer to [`MapEvent::Similar`]: rings these items and lists them at the side.
    pub fn show_similar(&mut self, of: usize, items: Vec<usize>) {
        self.similar = Some((of, items));
    }

    pub fn similar(&self) -> Option<&(usize, Vec<usize>)> {
        self.similar.as_ref()
    }

    /// Fits all points in view.
    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.centre = [0.5, 0.5];
    }

    fn transform(&self, rect: Rect) -> Transform {
        Transform { rect, scale: rect.width().min(rect.height()) * 0.92 * self.zoom, centre: self.centre }
    }

    fn zoom_at(&mut self, rect: Rect, pointer: Pos2, factor: f32) {
        let before = self.transform(rect).to_map(pointer);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.transform(rect).to_map(pointer);
        self.centre = [self.centre[0] + before[0] - after[0], self.centre[1] + before[1] - after[1]];
        self.clamp_centre();
    }

    fn pan(&mut self, rect: Rect, delta: Vec2) {
        let scale = self.transform(rect).scale.max(1e-6);
        self.centre = [self.centre[0] - delta.x / scale, self.centre[1] - delta.y / scale];
        self.clamp_centre();
    }

    fn clamp_centre(&mut self) {
        for c in &mut self.centre {
            *c = c.clamp(0.0, 1.0);
        }
    }
}

/// Map coordinates (0–1) to the screen and back.
#[derive(Debug, Clone, Copy)]
struct Transform {
    rect: Rect,
    scale: f32,
    centre: [f32; 2],
}

impl Transform {
    fn to_screen(self, p: [f32; 2]) -> Pos2 {
        self.rect.center() + Vec2::new((p[0] - self.centre[0]) * self.scale, (p[1] - self.centre[1]) * self.scale)
    }

    fn to_map(self, p: Pos2) -> [f32; 2] {
        let d = p - self.rect.center();
        [self.centre[0] + d.x / self.scale.max(1e-6), self.centre[1] + d.y / self.scale.max(1e-6)]
    }
}

/// Where each point is drawn: its cloud position, or the centre of its grid cell.
fn positions(points: &[MapPoint], layout: Layout, grid: Option<&Grid>) -> Vec<[f32; 2]> {
    match (layout, grid) {
        (Layout::Grid, Some(grid)) if grid.cells.len() == points.len() => {
            let side = grid.columns.max(grid.rows).max(1) as f32;
            let offset = [(side - grid.columns as f32) / 2.0, (side - grid.rows as f32) / 2.0];
            grid.cells
                .iter()
                .map(|(c, r)| [(*c as f32 + 0.5 + offset[0]) / side, (*r as f32 + 0.5 + offset[1]) / side])
                .collect()
        }
        _ => points.iter().map(|p| p.position).collect(),
    }
}

/// The point nearest to `pointer` on screen, within `radius`.
fn hit(screen: &[Pos2], pointer: Pos2, radius: f32) -> Option<usize> {
    screen
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.distance_sq(pointer)))
        .filter(|(_, d)| *d <= radius * radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The point nearest to `from` in the direction `direction` (within 60° of it), for the arrow
/// keys.
fn step(positions: &[[f32; 2]], from: usize, direction: [f32; 2]) -> Option<usize> {
    let origin = positions.get(from)?;
    positions
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, p)| {
            let d = [p[0] - origin[0], p[1] - origin[1]];
            let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let along = (d[0] * direction[0] + d[1] * direction[1]) / length.max(1e-9);
            // Within 60° of the direction; nearer and straighter is better.
            (length > 0.0 && along >= 0.5).then_some((i, length / along))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// For each kind with enough items, where its label goes: the item of that kind nearest to the
/// kind's centre.
fn label_spots(points: &[MapPoint], positions: &[[f32; 2]]) -> Vec<(Kind, [f32; 2])> {
    let mut out = Vec::new();
    for kind in Kind::ALL {
        let mine: Vec<[f32; 2]> =
            points.iter().zip(positions).filter(|(p, _)| p.kind == kind && p.matches).map(|(_, q)| *q).collect();
        if mine.len() < LABEL_MIN_ITEMS || kind == Kind::Other && mine.len() < LABEL_MIN_ITEMS * 4 {
            continue;
        }
        let n = mine.len() as f32;
        let centre = [mine.iter().map(|p| p[0]).sum::<f32>() / n, mine.iter().map(|p| p[1]).sum::<f32>() / n];
        if let Some(spot) = mine.iter().min_by(|a, b| {
            let da = (a[0] - centre[0]).powi(2) + (a[1] - centre[1]).powi(2);
            let db = (b[0] - centre[0]).powi(2) + (b[1] - centre[1]).powi(2);
            da.total_cmp(&db)
        }) {
            out.push((kind, *spot));
        }
    }
    out
}

/// A kind's colour: Suhina's muted palette, extended, with neighbouring kinds told apart.
pub fn kind_colour(kind: Kind) -> Color32 {
    let hex: u32 = match kind {
        Kind::Kick => 0xD0_6E_5E,
        Kind::Snare => 0x73_AA_C0,
        Kind::Clap => 0x5E_C0_B0,
        Kind::ClosedHat => 0xC6_AC_5E,
        Kind::OpenHat => 0xD0_8F_6E,
        Kind::Hat => 0xD8_C4_7A,
        Kind::Rim => 0x8C_A8_E0,
        Kind::Tom => 0xB0_74_A4,
        Kind::Crash => 0xE0_A0_C8,
        Kind::Ride => 0xC8_B8_E8,
        Kind::Cymbal => 0xA4_97_CC,
        Kind::Percussion => 0x8F_C0_6E,
        Kind::Bass => 0xA8_4F_4F,
        Kind::Tonal => 0x6E_B0_D0,
        Kind::Vocal => 0xE8_D0_A0,
        Kind::Effect => 0x9C_C4_9C,
        Kind::Other => 0x8A_86_80,
    };
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Draws the map in the space `ui` offers and reports what the user did.
pub fn show(
    ui: &mut Ui,
    view: &mut MapView,
    points: &[MapPoint],
    grid: Option<&Grid>,
    status: &str,
    tags: &[String],
    events: &mut Vec<MapEvent>,
) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let top = Rect::from_min_size(full.min, Vec2::new(full.width(), TOP_BAR));
    let bottom = Rect::from_min_max(Pos2::new(full.min.x, full.max.y - LEGEND), full.max);
    let mut area = Rect::from_min_max(Pos2::new(full.min.x, top.max.y), Pos2::new(full.max.x, bottom.min.y));
    let side = view.similar.is_some().then(|| {
        let side = Rect::from_min_max(Pos2::new(area.max.x - SIMILAR_WIDTH, area.min.y), area.max);
        area.max.x = side.min.x;
        side
    });

    top_bar(ui, top, view, status, events);
    let positions = positions(points, view.layout, grid);
    map_area(ui, area, view, &Content { points, positions: &positions, grid, tags }, events);
    if let Some(side) = side {
        similar_strip(ui, side, view, points, events);
    }
    legend(ui, bottom, view, points);
}

fn top_bar(ui: &mut Ui, rect: Rect, view: &mut MapView, status: &str, events: &mut Vec<MapEvent>) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(theme::PAD, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    for (layout, name) in [(Layout::Cloud, "Cloud"), (Layout::Grid, "Grid")] {
        if child.selectable_label(view.layout == layout, name).clicked() && view.layout != layout {
            view.layout = layout;
            events.push(MapEvent::Layout(layout));
        }
    }
    child.add_space(theme::PAD);
    child.label(egui::RichText::new(status).color(theme::text_secondary()).size(theme::TEXT_SMALL));
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .button(if view.large { "Close" } else { "Large" })
            .on_hover_text("Show the map in the main area")
            .clicked()
        {
            events.push(MapEvent::ToggleLarge);
        }
        if ui.button("Fit").on_hover_text("Fit all points in view").clicked() {
            view.fit();
        }
        if ui.button("−").on_hover_text("Zoom out").clicked() {
            view.zoom = (view.zoom / 1.5).max(MIN_ZOOM);
            view.clamp_centre();
        }
        if ui.button("+").on_hover_text("Zoom in").clicked() {
            view.zoom = (view.zoom * 1.5).min(MAX_ZOOM);
        }
    });
}

/// What the map area draws.
struct Content<'a> {
    points: &'a [MapPoint],
    /// Where each point is, in map coordinates.
    positions: &'a [[f32; 2]],
    grid: Option<&'a Grid>,
    /// The library's colour tags, for the right-click menu.
    tags: &'a [String],
}

fn map_area(ui: &mut Ui, rect: Rect, view: &mut MapView, content: &Content<'_>, events: &mut Vec<MapEvent>) {
    let Content { points, positions, grid, tags } = *content;
    view.area = rect;
    let id = ui.id().with("sample-map");
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, theme::window_background());

    // Zoom and pan.
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let pinch = ui.input(|i| i.zoom_delta());
        let factor = (scroll / 200.0).exp() * pinch;
        if (factor - 1.0).abs() > 1e-4 {
            view.zoom_at(rect, pointer, factor);
        }
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let ctrl = ui.input(|i| i.modifiers.command);
    let transform = view.transform(rect);
    let screen: Vec<Pos2> = positions.iter().map(|p| transform.to_screen(*p)).collect();
    let pointer = response.hover_pos();
    let under = pointer.and_then(|p| hit(&screen, p, HIT_RADIUS.max(cell_size(view, grid, transform) / 2.0)));

    if response.drag_started_by(PointerButton::Primary) {
        let start = response.interact_pointer_pos();
        let on = start.and_then(|p| hit(&screen, p, HIT_RADIUS));
        match on {
            Some(i) => {
                let selected: Vec<usize> =
                    points.iter().enumerate().filter(|(_, p)| p.selected).map(|(j, _)| j).collect();
                let items = if points.get(i).is_some_and(|p| p.selected) { selected } else { vec![i] };
                events.push(MapEvent::StartDrag(items));
            }
            None if shift => view.rubber_band = start,
            None => {}
        }
    }
    if response.dragged_by(PointerButton::Primary) && view.rubber_band.is_none() && under.is_none()
        || response.dragged_by(PointerButton::Middle)
    {
        view.pan(rect, response.drag_delta());
    }
    if let Some(start) = view.rubber_band {
        let end = pointer.or_else(|| response.interact_pointer_pos()).unwrap_or(start);
        let band = Rect::from_two_pos(start, end);
        painter.rect_stroke(band, CornerRadius::ZERO, Stroke::new(1.0, theme::accent()), StrokeKind::Inside);
        if response.drag_stopped() {
            let items: Vec<usize> =
                screen.iter().enumerate().filter(|(_, p)| band.contains(**p)).map(|(i, _)| i).collect();
            events.push(MapEvent::Select { items, add: true });
            view.rubber_band = None;
        }
    }

    // Clicks.
    if response.double_clicked() {
        match under {
            Some(i) => events.push(MapEvent::Add(i)),
            None => view.fit(),
        }
    } else if response.clicked() {
        match under {
            Some(i) => {
                events.push(MapEvent::Select { items: vec![i], add: ctrl });
                events.push(MapEvent::Preview(i));
                view.previewed = Some(i);
            }
            None => view.similar = None,
        }
    }
    if response.secondary_clicked() {
        view.hovered = under.map(|i| (i, 0.0));
    }
    let menu_item = view.hovered.map(|(i, _)| i).or(under);
    response.context_menu(|ui| {
        if let Some(i) = menu_item {
            if ui.button("Show similar").clicked() {
                events.push(MapEvent::Similar(i));
                ui.close();
            }
            let pinned = points.get(i).is_some_and(|p| p.pinned);
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                events.push(MapEvent::Pin(i));
                ui.close();
            }
            if !tags.is_empty() {
                ui.menu_button("Tag", |ui| {
                    for tag in tags {
                        let mut on = points.get(i).is_some_and(|p| p.tags.contains(tag));
                        if ui.checkbox(&mut on, tag.as_str()).clicked() {
                            events.push(MapEvent::Tag { item: i, tag: tag.clone(), on });
                            ui.close();
                        }
                    }
                });
            }
            if ui.button("Show in folder").clicked() {
                events.push(MapEvent::ShowInFolder(i));
                ui.close();
            }
        }
    });

    // Preview on hover, after the pointer rests.
    let now = ui.input(|i| i.time);
    match (under, view.hovered) {
        (Some(i), Some((j, since))) if i == j => {
            if view.preview_on_hover && view.previewed != Some(i) && now - since >= HOVER_PREVIEW_SECONDS {
                events.push(MapEvent::Preview(i));
                view.previewed = Some(i);
            } else if view.previewed != Some(i) {
                ui.ctx().request_repaint();
            }
        }
        (Some(i), _) => {
            view.hovered = Some((i, now));
            ui.ctx().request_repaint();
        }
        (None, _) if !response.context_menu_opened() => {
            view.hovered = None;
            if !view.preview_on_hover {
                view.previewed = None;
            }
        }
        (None, _) => {}
    }

    // Keys: arrows step to the nearest point that way; Escape ends Show similar.
    if response.contains_pointer() || response.has_focus() {
        let keys = [
            (Key::ArrowLeft, [-1.0, 0.0]),
            (Key::ArrowRight, [1.0, 0.0]),
            (Key::ArrowUp, [0.0, -1.0]),
            (Key::ArrowDown, [0.0, 1.0]),
        ];
        for (key, direction) in keys {
            if ui.input(|i| i.key_pressed(key)) {
                let from = points.iter().position(|p| p.selected).or(view.previewed);
                let next = match from {
                    Some(from) => step(positions, from, direction),
                    None => points.iter().position(|p| p.matches),
                };
                if let Some(next) = next {
                    events.push(MapEvent::Select { items: vec![next], add: false });
                    events.push(MapEvent::Preview(next));
                    view.previewed = Some(next);
                }
            }
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            // Escape ends Show similar first, then closes the large map.
            if view.similar.take().is_none() && view.large {
                events.push(MapEvent::ToggleLarge);
            }
        }
    }

    draw_points(&painter, view, points, &screen, grid, transform, under);
    draw_labels(&painter, view, points, positions, transform);

    if let Some(point) = under.and_then(|i| points.get(i)) {
        let kind =
            if point.guessed { format!("{} (guessed)", point.kind.label()) } else { point.kind.label().to_string() };
        response.on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(&point.name).color(theme::text_primary()));
            ui.label(egui::RichText::new(kind).color(theme::text_secondary()));
            if !point.detail.is_empty() {
                ui.label(egui::RichText::new(&point.detail).color(theme::text_secondary()));
            }
        });
    }
}

/// The side of a grid cell on screen (0 in the cloud).
fn cell_size(view: &MapView, grid: Option<&Grid>, transform: Transform) -> f32 {
    match (view.layout, grid) {
        (Layout::Grid, Some(grid)) => transform.scale / grid.columns.max(grid.rows).max(1) as f32,
        _ => 0.0,
    }
}

fn draw_points(
    painter: &egui::Painter,
    view: &MapView,
    points: &[MapPoint],
    screen: &[Pos2],
    grid: Option<&Grid>,
    transform: Transform,
    under: Option<usize>,
) {
    let visible = painter.clip_rect().expand(8.0);
    let cell = cell_size(view, grid, transform);
    let radius = (2.0 * view.zoom.sqrt()).clamp(2.0, 6.0);
    let similar: HashSet<usize> = view.similar.as_ref().map(|(_, s)| s.iter().copied().collect()).unwrap_or_default();
    let dimmed = |i: usize, p: &MapPoint| {
        !p.matches
            || !view.legend.is_empty() && !view.legend.contains(&p.kind)
            || view.similar.is_some() && !similar.contains(&i) && view.similar.as_ref().is_some_and(|(of, _)| *of != i)
    };
    // Dimmed points first, so the others are drawn over them.
    for pass in [true, false] {
        for (i, (point, at)) in points.iter().zip(screen).enumerate() {
            if dimmed(i, point) != pass || !visible.contains(*at) {
                continue;
            }
            let mut colour = kind_colour(point.kind);
            if point.guessed {
                colour = colour.gamma_multiply(0.7);
            }
            if pass {
                colour = colour.gamma_multiply(0.18);
            }
            if cell > 0.0 {
                let side = (cell - 1.0).max(1.0);
                let r = Rect::from_center_size(*at, Vec2::splat(side));
                if point.preset {
                    painter.rect_stroke(
                        r.shrink(1.0),
                        CornerRadius::same(2),
                        Stroke::new(1.5, colour),
                        StrokeKind::Inside,
                    );
                } else {
                    painter.rect_filled(r, CornerRadius::same(2), colour);
                }
            } else if point.preset {
                painter.circle_stroke(*at, radius + 0.5, Stroke::new(1.2, colour));
            } else {
                painter.circle_filled(*at, radius, colour);
            }
            let size = if cell > 0.0 { cell / 2.0 } else { radius };
            if point.in_use {
                painter.circle_stroke(*at, size + 3.0, Stroke::new(1.5, colour));
            }
            if point.selected {
                painter.circle_stroke(*at, size + 1.5, Stroke::new(1.0, theme::text_primary()));
            }
            if similar.contains(&i) {
                painter.circle_stroke(*at, size + 4.5, Stroke::new(1.0, theme::accent()));
            }
            if view.previewed == Some(i) || under == Some(i) {
                painter.circle_stroke(*at, size + 2.5, Stroke::new(1.5, theme::accent()));
            }
        }
    }
}

fn draw_labels(
    painter: &egui::Painter,
    view: &MapView,
    points: &[MapPoint],
    positions: &[[f32; 2]],
    transform: Transform,
) {
    let fade = ((LABEL_FADE_ZOOM - view.zoom) / 2.0).clamp(0.0, 1.0);
    if fade <= 0.0 {
        return;
    }
    for (kind, spot) in label_spots(points, positions) {
        let at = transform.to_screen(spot);
        let colour = kind_colour(kind).gamma_multiply(fade);
        let font = theme::ui_font(theme::TEXT * 1.6);
        // A shadow keeps the label readable over the points.
        painter.text(
            at + Vec2::new(1.0, 1.0),
            Align2::CENTER_CENTER,
            kind.label(),
            font.clone(),
            theme::window_background().gamma_multiply(fade),
        );
        painter.text(at, Align2::CENTER_CENTER, kind.label(), font, colour);
    }
}

fn similar_strip(ui: &mut Ui, rect: Rect, view: &mut MapView, points: &[MapPoint], events: &mut Vec<MapEvent>) {
    let Some((of, items)) = view.similar.clone() else { return };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::panel_background());
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink(theme::PAD)).layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let name = points.get(of).map_or("", |p| p.name.as_str());
    child.label(
        egui::RichText::new(format!("Similar to {name}")).color(theme::text_secondary()).size(theme::TEXT_SMALL),
    );
    egui::ScrollArea::vertical().show(&mut child, |ui| {
        for i in items {
            let Some(point) = points.get(i) else { continue };
            ui.horizontal(|ui| {
                if ui.small_button("▶").on_hover_text(format!("Preview {}", point.name)).clicked() {
                    events.push(MapEvent::Preview(i));
                }
                let row = ui
                    .selectable_label(point.selected, egui::RichText::new(&point.name).color(kind_colour(point.kind)));
                if row.clicked() {
                    events.push(MapEvent::Select { items: vec![i], add: false });
                    events.push(MapEvent::Preview(i));
                }
            });
        }
    });
    if child.button("Close").clicked() {
        view.similar = None;
    }
}

fn legend(ui: &mut Ui, rect: Rect, view: &mut MapView, points: &[MapPoint]) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(theme::PAD, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ctrl = child.input(|i| i.modifiers.command);
    for kind in Kind::ALL {
        let count = points.iter().filter(|p| p.kind == kind).count();
        if count == 0 {
            continue;
        }
        let shown = view.legend.is_empty() || view.legend.contains(&kind);
        let colour = if shown { kind_colour(kind) } else { kind_colour(kind).gamma_multiply(0.3) };
        let (dot, dot_response) = child.allocate_exact_size(Vec2::splat(10.0), Sense::click());
        child.painter().circle_filled(dot.center(), 3.5, colour);
        let text = egui::RichText::new(format!("{} {count}", kind.label())).color(colour).size(theme::TEXT_SMALL);
        let response: Response = child.add(egui::Label::new(text).sense(Sense::click()));
        child.add_space(theme::PAD);
        if response.clicked() || dot_response.clicked() {
            toggle_legend(&mut view.legend, kind, ctrl);
        }
    }
}

/// A click on a kind in the legend: show only it (Ctrl: add or remove it); clicking the only
/// kind shown shows all again.
fn toggle_legend(legend: &mut HashSet<Kind>, kind: Kind, add: bool) {
    if add {
        if !legend.remove(&kind) {
            legend.insert(kind);
        }
    } else if legend.len() == 1 && legend.contains(&kind) {
        legend.clear();
    } else {
        legend.clear();
        legend.insert(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32, kind: Kind) -> MapPoint {
        MapPoint {
            name: format!("{x},{y}"),
            detail: String::new(),
            position: [x, y],
            kind,
            guessed: false,
            in_use: false,
            selected: false,
            matches: true,
            preset: false,
            pinned: false,
            tags: Vec::new(),
        }
    }

    #[test]
    fn transform_round_trips_and_zoom_keeps_the_point_under_the_pointer() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(400.0, 300.0));
        let mut view = MapView::default();
        let t = view.transform(rect);
        let p = [0.3, 0.8];
        let back = t.to_map(t.to_screen(p));
        assert!((back[0] - p[0]).abs() < 1e-5 && (back[1] - p[1]).abs() < 1e-5);
        let pointer = Pos2::new(150.0, 100.0);
        let before = view.transform(rect).to_map(pointer);
        view.zoom_at(rect, pointer, 2.0);
        let after = view.transform(rect).to_map(pointer);
        assert!((before[0] - after[0]).abs() < 1e-4 && (before[1] - after[1]).abs() < 1e-4);
        view.zoom_at(rect, pointer, 1e-6);
        assert_eq!(view.zoom, MIN_ZOOM);
        view.fit();
        assert_eq!((view.zoom, view.centre), (1.0, [0.5, 0.5]));
    }

    #[test]
    fn the_nearest_point_within_reach_is_hit() {
        let screen = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(4.0, 0.0)];
        assert_eq!(hit(&screen, Pos2::new(5.0, 1.0), 7.0), Some(2));
        assert_eq!(hit(&screen, Pos2::new(50.0, 50.0), 7.0), None);
        assert_eq!(hit(&[], Pos2::ZERO, 7.0), None);
    }

    #[test]
    fn arrow_keys_step_to_the_nearest_point_that_way() {
        let positions = [[0.5, 0.5], [0.6, 0.5], [0.9, 0.52], [0.5, 0.2], [0.4, 0.9]];
        assert_eq!(step(&positions, 0, [1.0, 0.0]), Some(1));
        assert_eq!(step(&positions, 1, [1.0, 0.0]), Some(2));
        assert_eq!(step(&positions, 0, [0.0, -1.0]), Some(3));
        assert_eq!(step(&positions, 0, [0.0, 1.0]), Some(4));
        assert_eq!(step(&positions, 2, [1.0, 0.0]), None);
        assert_eq!(step(&positions, 99, [1.0, 0.0]), None);
    }

    #[test]
    fn labels_go_to_kinds_with_enough_items() {
        let mut points: Vec<MapPoint> = (0..10).map(|i| point(0.1 + i as f32 * 0.01, 0.1, Kind::Kick)).collect();
        points.extend((0..3).map(|i| point(0.9, 0.9 + i as f32 * 0.01, Kind::Snare)));
        let positions: Vec<[f32; 2]> = points.iter().map(|p| p.position).collect();
        let spots = label_spots(&points, &positions);
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].0, Kind::Kick);
        assert!((spots[0].1[0] - 0.145).abs() < 0.01);
    }

    #[test]
    fn grid_positions_are_cell_centres() {
        let points = vec![point(0.0, 0.0, Kind::Kick), point(1.0, 1.0, Kind::Kick)];
        let grid = Grid { columns: 2, rows: 1, cells: vec![(0, 0), (1, 0)] };
        // A 2 by 1 grid sits in the middle of the square map.
        assert_eq!(positions(&points, Layout::Grid, Some(&grid)), vec![[0.25, 0.5], [0.75, 0.5]]);
        assert_eq!(positions(&points, Layout::Cloud, Some(&grid)), vec![[0.0, 0.0], [1.0, 1.0]]);
        // A grid that does not match the points falls back to the cloud.
        let wrong = Grid { columns: 1, rows: 1, cells: vec![(0, 0)] };
        assert_eq!(positions(&points, Layout::Grid, Some(&wrong)), vec![[0.0, 0.0], [1.0, 1.0]]);
    }

    #[test]
    fn the_legend_shows_one_kind_or_several() {
        let mut legend = HashSet::new();
        toggle_legend(&mut legend, Kind::Kick, false);
        assert_eq!(legend, HashSet::from([Kind::Kick]));
        toggle_legend(&mut legend, Kind::Snare, true);
        assert_eq!(legend, HashSet::from([Kind::Kick, Kind::Snare]));
        toggle_legend(&mut legend, Kind::Hat, false);
        assert_eq!(legend, HashSet::from([Kind::Hat]));
        toggle_legend(&mut legend, Kind::Hat, false);
        assert!(legend.is_empty());
    }

    #[test]
    fn every_kind_has_its_own_colour() {
        let colours: HashSet<Color32> = Kind::ALL.iter().map(|k| kind_colour(*k)).collect();
        assert_eq!(colours.len(), Kind::ALL.len());
    }

    #[test]
    fn hovering_previews_and_clicking_selects() {
        use egui_kittest::Harness;
        let points: Vec<MapPoint> =
            (0..20).map(|i| point((i % 5) as f32 / 4.0, (i / 5) as f32 / 3.0, Kind::Kick)).collect();
        let state = (MapView::default(), Vec::<MapEvent>::new());
        let mut harness = Harness::builder().with_size(egui::vec2(400.0, 340.0)).build_ui_state(
            |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
                show(ui, view, &points, None, "20 samples", &[], events)
            },
            state,
        );
        harness.run();
        // Point 0 at map (0, 0): find it on screen from the same transform the view uses.
        let view = &harness.state().0;
        let at = view.transform(view.area).to_screen([0.0, 0.0]);
        harness.hover_at(at);
        harness.run();
        std::thread::sleep(std::time::Duration::from_millis(60));
        harness.hover_at(at + Vec2::new(0.5, 0.0));
        harness.run();
        assert!(harness.state().1.contains(&MapEvent::Preview(0)), "{:?}", harness.state().1);
        harness.state_mut().1.clear();
        harness.drag_at(at);
        harness.run();
        harness.drop_at(at);
        harness.run();
        assert!(
            harness.state().1.contains(&MapEvent::Select { items: vec![0], add: false }),
            "{:?}",
            harness.state().1
        );
    }
}

#[cfg(test)]
mod look {
    use super::*;

    /// Renders a map of generated clusters to `target/sample-map.png` to look at (not a test of
    /// behaviour; run with `--ignored`).
    #[test]
    #[ignore]
    fn render_a_map() {
        use egui_kittest::Harness;
        let mut state = 5u32;
        let mut noise = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32 - 0.5
        };
        let centres = [
            (Kind::Kick, [0.2, 0.6]),
            (Kind::Bass, [0.3, 0.8]),
            (Kind::Snare, [0.5, 0.4]),
            (Kind::Clap, [0.6, 0.55]),
            (Kind::ClosedHat, [0.8, 0.3]),
            (Kind::OpenHat, [0.85, 0.5]),
            (Kind::Percussion, [0.5, 0.8]),
            (Kind::Other, [0.4, 0.15]),
        ];
        let mut points = Vec::new();
        for (k, (kind, centre)) in centres.iter().enumerate() {
            for i in 0..150 {
                let mut p = MapPoint {
                    name: format!("{} {i}", kind.label()),
                    detail: String::new(),
                    position: [centre[0] + noise() * 0.22, centre[1] + noise() * 0.22],
                    kind: *kind,
                    guessed: i % 7 == 0,
                    in_use: i == 3 && k % 2 == 0,
                    selected: false,
                    matches: true,
                    preset: i % 25 == 0,
                    pinned: false,
                    tags: Vec::new(),
                };
                p.position = [p.position[0].clamp(0.0, 1.0), p.position[1].clamp(0.0, 1.0)];
                points.push(p);
            }
        }
        let positions: Vec<[f32; 2]> = points.iter().map(|p| p.position).collect();
        let grid = suhina_soundmap::grid::snap(&positions, 1.0);
        for layout in [Layout::Cloud, Layout::Grid] {
            let view = MapView { layout, ..MapView::default() };
            let mut harness = Harness::builder().with_size(egui::vec2(900.0, 640.0)).build_ui_state(
                |ui, view: &mut MapView| {
                    crate::theme::install(ui.ctx());
                    let mut events = Vec::new();
                    show(ui, view, &points, Some(&grid), "1 200 samples", &[], &mut events);
                },
                view,
            );
            harness.run();
            let image = harness.render().unwrap();
            let name = format!("../../target/sample-map-{layout:?}.png").to_lowercase();
            image.save(name).unwrap();
        }
    }
}
