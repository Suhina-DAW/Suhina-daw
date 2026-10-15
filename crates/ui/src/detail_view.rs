//! The detail area: the selected track's device chain, and beside it the selected clip's
//! properties (`docs/spec/ui-session-view.md` › Detail area). The chain is laid out in a row under the view
//! or stacked in a column beside it (a setting). It shows the project as it is; changing devices
//! comes with the device commands. A plug-in device shows where its plug-in stands (loading,
//! running, missing, crashed) and a button that opens its editor or restarts it, through the
//! `device.*` commands (ADR 0006). A plug-in's editor opened inside the panel (the setting)
//! takes the room under that line, and where it lands in the window is reported each frame
//! ([`EditorPlace`]) for the host process to put it there.

use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{DeviceKind, LaneData, Project, TrackId};
use suhina_session::{Placement, PluginReport, PluginStatus};

use crate::grid::gain_to_db;
use crate::piano_roll::{PianoRollState, QUANTIZE_GRIDS};
use crate::theme::{self, PAD, ROW};
use crate::widgets::{self, Action};

/// Width of one device in a row, and of the column the stacked chain takes.
pub const DEVICE_WIDTH: f32 = 220.0;
pub const STACK_WIDTH: f32 = 260.0;
const PARAM_ROW: f32 = 16.0;
/// Width of the clip's properties beside the chain, and of a property's label.
const CLIP_WIDTH: f32 = 196.0;
const LABEL_WIDTH: f32 = 52.0;
const ON_SWITCH: f32 = 9.0;
/// A plug-in device's button (open its editor, restart it).
const BUTTON_HEIGHT: f32 = 18.0;
const BUTTON_WIDTH: f32 = 96.0;
/// Where an embedded editor starts under a plug-in device's top: its header, state line and
/// button.
const EDITOR_TOP: f32 = ROW + PAD + PARAM_ROW + PAD / 2.0 + BUTTON_HEIGHT + PAD / 2.0;

/// Where the panel shows a plug-in's embedded editor this frame, for its host to put it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPlace {
    pub track: TrackId,
    pub device: usize,
    pub placement: Placement,
}

/// The height a device row needs to show an embedded editor `height` pixels tall under a
/// plug-in's line and button, at `pixels_per_point`.
pub fn row_height_with_editor(height: u32, pixels_per_point: f32) -> f32 {
    EDITOR_TOP + height as f32 / pixels_per_point.max(0.01) + PAD
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceBox {
    /// Its index among the track's devices, as the device commands take it.
    pub index: usize,
    pub name: String,
    /// What it is: a built-in effect or instrument, a plug-in format, or one Suhina cannot run.
    pub kind: String,
    pub enabled: bool,
    /// Parameter ids and values as text.
    pub params: Vec<(String, String)>,
    /// For a plug-in: where it stands, while the project plays.
    pub plugin: Option<PluginBox>,
}

/// Where a plug-in device's plug-in stands, as its panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginBox {
    pub state: PluginState,
    /// One line about it: the plug-in that runs, or why it does not.
    pub line: String,
    pub editor_open: bool,
    /// Its editor is open inside the panel: the size of it, in pixels.
    pub embedded: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// The project is not playing, so its plug-ins are not running.
    Stopped,
    Loading,
    Running,
    Missing,
    Crashed,
}

impl PluginBox {
    fn from_report(report: Option<PluginReport>) -> Self {
        let Some(report) = report else {
            return Self {
                state: PluginState::Stopped,
                line: "Not running".into(),
                editor_open: false,
                embedded: None,
            };
        };
        let name = report.plugin.as_ref().map(|(name, vendor, _)| format!("{name} by {vendor}"));
        let (state, line) = match &report.status {
            PluginStatus::Loading => (PluginState::Loading, format!("Loading {}", name.unwrap_or_default())),
            PluginStatus::Running => {
                let mut line = match (&report.state_refused, report.latency) {
                    (Some(_), _) => "Running with its own settings".to_string(),
                    (None, 0) => name.unwrap_or_else(|| "Running".into()),
                    (None, latency) => format!("{} · {latency} frames", name.unwrap_or_else(|| "Running".into())),
                };
                // Blocks it did not answer in time: the chain went on without it for those.
                if report.late_blocks > 0 && report.blocks > 0 {
                    let percent = (u64::from(report.late_blocks) * 100 / u64::from(report.blocks)).max(1);
                    line.push_str(&format!(" · {percent}% of blocks late"));
                }
                (PluginState::Running, line)
            }
            PluginStatus::Missing => (PluginState::Missing, "Not installed; kept with its settings".into()),
            PluginStatus::Crashed(why) => (PluginState::Crashed, format!("Crashed: {why}")),
            PluginStatus::Failed(why) => (PluginState::Crashed, format!("Could not start: {why}")),
        };
        Self { state, line, editor_open: report.editor_open, embedded: report.editor_embedded }
    }

    /// The size the embedded editor takes in the panel, in points.
    fn editor_size(&self, pixels_per_point: f32) -> Option<Vec2> {
        let (width, height) = self.embedded?;
        let scale = pixels_per_point.max(0.01);
        Some(Vec2::new(width as f32 / scale, height as f32 / scale))
    }

    /// What its button does: its label and the command it runs.
    fn button(&self) -> Option<(&'static str, &'static str)> {
        match self.state {
            PluginState::Running if self.editor_open => Some(("Close editor", "device.closeEditor")),
            PluginState::Running => Some(("Open editor", "device.openEditor")),
            PluginState::Crashed => Some(("Restart", "device.restart")),
            PluginState::Stopped | PluginState::Loading | PluginState::Missing => None,
        }
    }
}

/// A selected clip: on the arrangement (its track and index there), or in a launcher slot (its
/// track and scene, from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRef {
    Arrangement(TrackId, usize),
    Slot(TrackId, usize),
}

impl ClipRef {
    pub fn track(self) -> TrackId {
        match self {
            Self::Arrangement(track, _) | Self::Slot(track, _) => track,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub track: String,
    /// Where it is: bar.beat where an arrangement clip starts, or the number of a slot's scene
    /// (`scene` tells which); and its length in bars and beats.
    pub start: String,
    pub scene: bool,
    pub length: String,
    pub looping: bool,
    pub audio: bool,
    pub gain_db: Option<f64>,
    /// Fade-in and fade-out in beats (`clip.setFades`).
    pub fades: (f64, f64),
    pub notes: usize,
}

/// A track's device chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub track: TrackId,
    pub name: String,
    pub devices: Vec<DeviceBox>,
}

/// What the detail area shows: a selected clip comes with its track's chain, since that chain
/// is what the clip plays through.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Devices(Chain),
    Clip(ClipInfo, Chain),
    Nothing,
}

/// The detail for a selected clip (it wins, with its own track's chain) or track.
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<ClipRef>) -> Detail {
    build_with(project, track, clip, &mut |_, _| None)
}

/// [`build`], with each plug-in device's state from `plugins` (track, device index).
pub fn build_with(
    project: &Project,
    track: Option<TrackId>,
    clip: Option<ClipRef>,
    plugins: &mut dyn FnMut(TrackId, usize) -> Option<PluginReport>,
) -> Detail {
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    let track_name =
        |id: TrackId| project.tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    let found = clip.and_then(|clip| match clip {
        ClipRef::Arrangement(id, index) => project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(id))
            .and_then(|l| l.clips.get(index))
            .map(|placed| (id, placed, None)),
        ClipRef::Slot(id, scene) => project
            .launcher
            .columns
            .iter()
            .find(|c| c.track == id)
            .and_then(|c| c.slots.get(scene))
            .and_then(|s| s.clip.as_ref())
            .map(|placed| (id, placed, Some(scene))),
    });
    if let Some((id, placed, scene)) = found {
        let content = project.content(placed.content);
        let audio = content.and_then(|c| {
            c.lanes.iter().find_map(|l| match &l.data {
                LaneData::Audio(region) => Some(region.gain),
                LaneData::Notes(_) => None,
            })
        });
        let notes = content.map_or(0, |c| {
            c.lanes
                .iter()
                .map(|l| match &l.data {
                    LaneData::Notes(notes) => notes.len(),
                    LaneData::Audio(_) => 0,
                })
                .sum()
        });
        let bars_beats = |beats: f64, from_one: u64| {
            let bar = (beats / bar_beats).floor() as u64 + from_one;
            let beat = (beats % bar_beats).floor() as u64 + from_one;
            format!("{bar}.{beat}")
        };
        return Detail::Clip(
            ClipInfo {
                name: content.map(|c| c.name.clone()).unwrap_or_default(),
                track: track_name(id),
                start: scene.map_or_else(|| bars_beats(placed.start, 1), |s| (s + 1).to_string()),
                scene: scene.is_some(),
                length: bars_beats(placed.length, 0),
                looping: placed.loop_range.is_some(),
                audio: audio.is_some(),
                gain_db: audio.map(gain_to_db),
                fades: (placed.fade_in, placed.fade_out),
                notes,
            },
            chain(project, id, plugins),
        );
    }
    track.map_or(Detail::Nothing, |id| Detail::Devices(chain(project, id, plugins)))
}

/// A track's devices as the detail area shows them.
fn chain(project: &Project, id: TrackId, plugins: &mut dyn FnMut(TrackId, usize) -> Option<PluginReport>) -> Chain {
    let track_name = project.tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    let channel = project
        .tracks
        .iter()
        .find(|t| t.id == id)
        .and_then(|t| t.channel)
        .and_then(|c| project.channels.iter().find(|ch| ch.id == c));
    let devices = channel
        .map(|c| {
            c.devices
                .iter()
                .enumerate()
                .map(|(index, d)| DeviceBox {
                    index,
                    name: d.name.clone(),
                    kind: match &d.kind {
                        DeviceKind::Builtin(_) => "Suhina".into(),
                        DeviceKind::Clap { .. } => "CLAP".into(),
                        DeviceKind::Vst3 { .. } => "VST3".into(),
                        DeviceKind::Vst2 { .. } => "VST2".into(),
                        DeviceKind::Foreign { format, .. } => format!("Not supported ({format})"),
                    },
                    enabled: d.enabled,
                    params: d.params.iter().map(|p| (p.id.clone(), format_value(p.value))).collect(),
                    plugin: matches!(
                        d.kind,
                        DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }
                    )
                    .then(|| PluginBox::from_report(plugins(id, index))),
                })
                .collect()
        })
        .unwrap_or_default();
    Chain { track: id, name: track_name, devices }
}

fn format_value(value: f64) -> String {
    if !value.is_finite() {
        return "-".into();
    }
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The detail laid out in a row: devices side by side, or the clip's properties. A plug-in's
/// button adds its command to `actions`; where its embedded editor lands goes to `places`.
pub fn show_row(ui: &mut Ui, detail: &Detail, actions: &mut Vec<Action>, places: &mut Vec<EditorPlace>) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip to see it here"),
        Detail::Devices(chain) => chain_row(ui, &painter, area, chain, actions, places),
        Detail::Clip(info, chain) => {
            let column = clip_properties(&painter, area, info, false);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max);
            if rest.width() > 0.0 {
                chain_row(ui, &painter, rest, chain, actions, places);
            }
        }
    }
}

/// A chain's devices side by side in `area`; one with an embedded editor is as wide as it.
fn chain_row(
    ui: &Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let scale = ui.ctx().pixels_per_point();
    let mut x = area.left();
    for device in &chain.devices {
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale));
        let width = editor.map_or(DEVICE_WIDTH, |e| DEVICE_WIDTH.max(e.x + PAD * 2.0));
        let rect = Rect::from_min_max(Pos2::new(x, area.top()), Pos2::new(x + width, area.bottom()));
        x = rect.right() + 1.0;
        if rect.left() > area.right() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(&painter.with_clip_rect(area), rect, device);
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            places.push(editor_place(ui, area, rect, editor, chain.track, device));
        }
    }
}

/// The detail stacked in a column: devices one under another.
pub fn show_stack(ui: &mut Ui, detail: &Detail, actions: &mut Vec<Action>, places: &mut Vec<EditorPlace>) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip"),
        Detail::Devices(chain) => chain_stack(ui, &painter, area, chain, actions, places),
        Detail::Clip(info, chain) => {
            let column = clip_properties(&painter, area, info, true);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max);
            if rest.height() > 0.0 {
                chain_stack(ui, &painter, rest, chain, actions, places);
            }
        }
    }
}

/// The clip's properties column for a clip whose editor fills the rest of the detail area (the
/// piano roll): on the left in a row, on top when `stacked`. Returns the rest. When the quantise
/// block is open it takes the column's top and the properties move down under it, so the block
/// is part of the column and nothing floats over the note grid.
pub fn show_clip_column(
    ui: &mut Ui,
    info: &ClipInfo,
    stacked: bool,
    piano: &mut PianoRollState,
    actions: &mut Vec<Action>,
) -> Rect {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    let top = if piano.quantize_open {
        let block = Rect::from_min_size(area.min, Vec2::new(CLIP_WIDTH.min(area.width()), quantise_height()));
        quantise_block(ui, &painter, block, piano, actions);
        block.height()
    } else {
        0.0
    };
    let below = Rect::from_min_max(Pos2::new(area.left(), area.top() + top), area.max);
    let column = clip_properties(&painter, below, info, stacked);
    clip_for_readers(ui, column, info);
    if stacked {
        Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max)
    } else {
        Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max)
    }
}

/// The room the quantise block needs: its title, the grid, the amount, the ends box and the two
/// buttons.
fn quantise_height() -> f32 {
    ROW + PAD + PARAM_ROW * 3.0 + BUTTON_HEIGHT + PAD
}

/// The quantise controls at the top of the clip column: title, Grid, Amount, "Move ends too",
/// **Quantise notes** and **Close** (› Quantise in `docs/spec/ui-clip-editor.md`). The settings
/// are kept for Ctrl+U; **Quantise notes** adds `note.quantize` to `actions`.
fn quantise_block(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    piano: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    if rect.width() < 40.0 || rect.height() < 20.0 {
        return;
    }
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let title = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    widgets::text_in(
        painter,
        title.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        "Quantise",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let mut settings = piano.quantize;
    let mut apply = false;
    let body =
        Rect::from_min_max(Pos2::new(rect.left() + PAD, title.bottom()), Pos2::new(rect.right() - PAD, rect.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        let small = theme::ui_font(theme::TEXT_SMALL);
        // Each field takes a row of its own in the 196 px column: beside another, its value would
        // fall off the column's edge. The block still leaves room for the properties under it.
        let mono = theme::mono_font(theme::TEXT_SMALL);
        let mut chosen = settings.grid;
        let shown = chosen;
        let menu = egui::ComboBox::from_id_salt("piano-roll-quantise-grid")
            .width(64.0)
            .selected_text(egui::RichText::new(shown).font(mono.clone()))
            .show_ui(ui, |ui| {
                for grid in QUANTIZE_GRIDS {
                    ui.selectable_value(&mut chosen, grid, egui::RichText::new(grid).font(mono.clone()));
                }
            });
        menu.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Quantise grid {shown}"))
        });
        if chosen != settings.grid {
            settings.grid = chosen;
        }
        let slider = ui.add_sized(
            Vec2::new(ui.available_width().min(150.0), 16.0),
            egui::Slider::new(&mut settings.amount, 0.0..=100.0).suffix(" %").integer().max_decimals(0),
        );
        slider.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("Amount {} %", settings.amount))
        });
        ui.checkbox(&mut settings.ends, egui::RichText::new("Move ends too").font(small.clone()));
        ui.horizontal(|ui| {
            let quantise = ui.button(egui::RichText::new("Quantise notes").font(small.clone()));
            quantise.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Quantise notes"));
            apply = quantise.clicked();
            let close = ui.button(egui::RichText::new("Close").font(small.clone()));
            close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close quantise"));
            if close.clicked() {
                piano.quantize_open = false;
            }
        });
    });
    piano.quantize = settings;
    if apply && let Some(action) = piano.quantize_selected() {
        actions.push(action);
    }
    // Escape closes the block, as it closes the panel it replaces (Escape also clears the note
    // selection in the piano roll; both happen, as both are what the key means there).
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        piano.quantize_open = false;
    }
}

/// A chain's devices one under another in `area`; one with an embedded editor is as tall as
/// it needs (the editor is cut off at the column's right edge when it is wider).
fn chain_stack(
    ui: &Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let scale = ui.ctx().pixels_per_point();
    let mut y = area.top();
    for device in &chain.devices {
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale));
        let rows =
            if device.plugin.is_some() { 2.0 + BUTTON_HEIGHT / PARAM_ROW } else { device.params.len().max(1) as f32 };
        let height = match editor {
            Some(editor) => EDITOR_TOP + editor.y + PAD,
            None => ROW + PAD * 2.0 + rows * PARAM_ROW,
        };
        let rect = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.right(), y + height));
        y = rect.bottom() + 1.0;
        if rect.top() > area.bottom() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(&painter.with_clip_rect(area), rect, device);
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            places.push(editor_place(ui, area, rect, editor, chain.track, device));
        }
    }
}

/// Where a plug-in's embedded editor of `editor` points goes under its line and button in the
/// device's `rect`, as the host takes it: in the window's own pixels, with the part of the
/// panel's `area` (and the device's box) that shows it; what falls outside is cut off.
fn editor_place(ui: &Ui, area: Rect, rect: Rect, editor: Vec2, track: TrackId, device: &DeviceBox) -> EditorPlace {
    let scale = ui.ctx().pixels_per_point();
    let origin = Pos2::new(rect.left() + PAD, rect.top() + EDITOR_TOP);
    let shown = Rect::from_min_size(origin, editor).intersect(rect).intersect(area);
    let pixel = |value: f32| (value * scale).round() as i32;
    let span = |from: f32, to: f32| u32::try_from(pixel(to).saturating_sub(pixel(from))).unwrap_or(0);
    let placement = if shown.is_negative() || shown.width() <= 0.0 || shown.height() <= 0.0 {
        Placement::HIDDEN
    } else {
        Placement {
            x: pixel(origin.x),
            y: pixel(origin.y),
            clip_x: pixel(shown.left()),
            clip_y: pixel(shown.top()),
            clip_width: span(shown.left(), shown.right()),
            clip_height: span(shown.top(), shown.bottom()),
        }
    };
    EditorPlace { track, device: device.index, placement }
}

/// A plug-in device's button under its state line: opens or closes its editor while it runs,
/// restarts it after a crash. A click adds the command to `actions`.
fn plugin_button(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    actions: &mut Vec<Action>,
) {
    let Some((label, command)) = device.plugin.as_ref().and_then(PluginBox::button) else { return };
    let top = rect.top() + ROW + PAD + PARAM_ROW + PAD / 2.0;
    let button = Rect::from_min_size(Pos2::new(rect.left() + PAD, top), Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));
    if button.bottom() > rect.bottom() || !painter.clip_rect().intersects(button) {
        return;
    }
    let response = widgets::button_area(
        ui,
        button,
        egui::Id::new(("plugin-button", track.0, device.index)),
        format!("{label} {}", device.name),
    );
    let hover = widgets::hover(ui, &response);
    let fill = crate::motion::mix(theme::control_background(), theme::control_hover(), hover);
    painter.rect_filled(button, 0.0, fill);
    painter.rect_stroke(button, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    widgets::text_in(
        painter,
        button,
        Align2::CENTER_CENTER,
        label,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    if response.clicked() {
        actions.push(Action::new(command, json!({ "track": track.0, "device": device.index })));
    }
}

/// Names the clip's properties for screen readers and tests: "Clip Walk, scene 2".
fn clip_for_readers(ui: &Ui, rect: Rect, info: &ClipInfo) {
    let response = ui.interact(rect, egui::Id::new("clip-properties"), Sense::hover());
    let label = if info.scene {
        format!("Clip {}, scene {}", info.name, info.start)
    } else {
        format!("Clip {}, start {}", info.name, info.start)
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

/// Names a device's panel for screen readers and tests.
fn name_for_readers(ui: &Ui, rect: Rect, device: &DeviceBox) {
    let response = ui.interact(rect, egui::Id::new(("device", rect.min.x as i32, rect.min.y as i32)), Sense::hover());
    let label = format!("Device {}", device.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_hint());
}

/// One device: a header with its on switch and name, then its parameters, one per row.
fn device_panel(painter: &egui::Painter, rect: Rect, device: &DeviceBox) {
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(header, 0.0, theme::control_background());
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::text_primary());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::text_hint()), egui::StrokeKind::Inside);
    }
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_hint() };
    let name = Rect::from_min_max(
        Pos2::new(switch.right() + PAD, header.top()),
        Pos2::new(header.right() - PAD, header.bottom()),
    );
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &device.name, theme::ui_font(theme::TEXT), name_color);
    widgets::text_in(
        painter,
        name,
        Align2::RIGHT_CENTER,
        &device.kind,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_hint(),
    );

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + PAD, header.bottom() + PAD),
        Pos2::new(rect.right() - PAD, rect.bottom() - PAD),
    );
    if let Some(plugin) = &device.plugin {
        let color = match plugin.state {
            PluginState::Crashed => theme::recording(),
            PluginState::Running => theme::text_secondary(),
            PluginState::Stopped | PluginState::Loading | PluginState::Missing => theme::text_hint(),
        };
        widgets::text_in(
            painter,
            Rect::from_min_size(body.min, Vec2::new(body.width(), PARAM_ROW)),
            Align2::LEFT_CENTER,
            &plugin.line,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );
        return;
    }
    if device.params.is_empty() {
        let text = if device.kind.starts_with("Not supported") {
            "Kept so the chain keeps its shape"
        } else {
            "No parameters"
        };
        widgets::text_in(
            painter,
            Rect::from_min_size(body.min, Vec2::new(body.width(), PARAM_ROW)),
            Align2::LEFT_CENTER,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        return;
    }
    for (index, (id, value)) in device.params.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(body.left(), body.top() + index as f32 * PARAM_ROW),
            Vec2::new(body.width(), PARAM_ROW),
        );
        if row.bottom() > body.bottom() + 0.5 {
            break;
        }
        widgets::text_in(
            painter,
            row,
            Align2::LEFT_CENTER,
            id,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        widgets::text_in(
            painter,
            row,
            Align2::RIGHT_CENTER,
            value,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
}

/// The selected clip: its name and where it is, how long, and what it plays. In a row it takes
/// a column of [`CLIP_WIDTH`] at the left of `area`, stacked the rows it needs at the top;
/// returns the space it took.
fn clip_properties(painter: &egui::Painter, area: Rect, info: &ClipInfo, stacked: bool) -> Rect {
    let mut rows = vec![
        ("Track", info.track.clone()),
        (if info.scene { "Scene" } else { "Start" }, info.start.clone()),
        ("Length", info.length.clone()),
        ("Loop", if info.looping { "On".into() } else { "Off".into() }),
    ];
    if info.audio {
        rows.push(("Gain", info.gain_db.map_or_else(|| "-".into(), |db| format!("{} dB", widgets::format_db(db)))));
        let beats = |b: f64| if b > 0.0 { format!("{} beats", (b * 100.0).round() / 100.0) } else { "-".into() };
        rows.push(("Fade in", beats(info.fades.0)));
        rows.push(("Fade out", beats(info.fades.1)));
    } else {
        rows.push(("Notes", info.notes.to_string()));
    }
    let column = if stacked {
        let height = ROW + PAD * 2.0 + rows.len() as f32 * PARAM_ROW;
        Rect::from_min_size(area.min, Vec2::new(area.width(), height))
    } else {
        Rect::from_min_max(area.min, Pos2::new(area.left() + CLIP_WIDTH, area.bottom()))
    }
    .intersect(area);
    painter.rect_filled(column, 0.0, theme::panel_background());
    let title = Rect::from_min_size(column.min, Vec2::new(column.width(), ROW));
    widgets::text_in(
        painter,
        title.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &info.name,
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let mut y = title.bottom() + PAD;
    for (label, value) in rows {
        let row = Rect::from_min_max(Pos2::new(column.left() + PAD, y), Pos2::new(column.right() - PAD, y + PARAM_ROW));
        y += PARAM_ROW;
        // The value keeps clear of the label, so a long track name is shortened, not drawn over it.
        let value_rect = Rect::from_min_max(Pos2::new(row.left() + LABEL_WIDTH, row.top()), row.max);
        widgets::text_in(
            painter,
            row,
            Align2::LEFT_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        widgets::text_in(
            painter,
            value_rect,
            Align2::RIGHT_CENTER,
            &value,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_project;
    use suhina_engine::model::{Device, DeviceParam, Extensions};

    #[test]
    fn a_selected_clip_shows_its_properties() {
        let detail = build(&demo_project(), Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), 1)));
        let Detail::Clip(info, chain) = detail else { panic!("expected the clip") };
        assert_eq!(chain.track, TrackId(1));
        assert_eq!((info.name.as_str(), info.start.as_str(), info.length.as_str()), ("Beat B", "17.1", "12.0"));
        assert!(info.looping && !info.audio && !info.scene);
        assert_eq!(info.notes, 4);
    }

    #[test]
    fn a_selected_slot_clip_shows_its_scene_loop_and_fades() {
        let mut project = demo_project();
        let column = project.launcher.columns.iter_mut().find(|c| c.track == TrackId(2)).expect("Bass has slots");
        let clip = column.slots[1].clip.as_mut().expect("Bass has a clip in scene 2");
        clip.fade_in = 1.5;
        clip.fade_out = 0.25;
        let content = clip.content;
        let name = project.content(content).map(|c| c.name.clone()).unwrap_or_default();
        let Detail::Clip(info, chain) = build(&project, None, Some(ClipRef::Slot(TrackId(2), 1))) else {
            panic!("expected the slot's clip")
        };
        assert_eq!(chain.track, TrackId(2));
        assert_eq!((info.name, info.start.as_str(), info.scene), (name, "2", true));
        assert_eq!(info.fades, (1.5, 0.25));
        // An empty slot, a scene past the last, or a track without slots shows the track, or nothing.
        let Detail::Devices(chain) = build(&project, Some(TrackId(3)), Some(ClipRef::Slot(TrackId(3), 0))) else {
            panic!("an empty slot shows the track")
        };
        assert_eq!(chain.track, TrackId(3));
        assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(2), 99))), Detail::Nothing);
        assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(10), 0))), Detail::Nothing);
    }

    #[test]
    fn a_selected_track_shows_its_devices() {
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            name: "Utility".into(),
            kind: DeviceKind::Builtin("utility".into()),
            enabled: false,
            state: None,
            params: vec![DeviceParam::new("gain", -3.5)],
            automation: Vec::new(),
            extensions: Extensions::default(),
        });
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        assert_eq!(chain.devices.len(), 1);
        assert_eq!(chain.devices[0].params, [("gain".to_string(), "-3.5".to_string())]);
        assert!(!chain.devices[0].enabled);
        // A clip on the track shows the same chain beside its properties.
        let Detail::Clip(_, with_clip) = build(&project, None, Some(ClipRef::Arrangement(TrackId(1), 0))) else {
            panic!()
        };
        assert_eq!(with_clip, chain);
        assert_eq!(build(&project, None, None), Detail::Nothing);
    }

    fn report(status: PluginStatus, editor_open: bool) -> PluginReport {
        PluginReport {
            status,
            plugin: Some(("Verb".into(), "Vendor".into(), "same")),
            instrument: false,
            latency: 0,
            editor_open,
            editor_embedded: None,
            blocks: 0,
            late_blocks: 0,
            longest_us: 0,
            no_time: 0,
            state_refused: None,
        }
    }

    fn with_plugin() -> Project {
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            name: "Verb".into(),
            kind: DeviceKind::Vst3 { id: "ABC".into() },
            enabled: true,
            state: Some(vec![1, 2]),
            params: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        });
        project
    }

    #[test]
    fn a_plug_in_shows_where_it_stands_and_what_its_button_does() {
        let project = with_plugin();
        let shown = |status: Option<PluginStatus>, open: bool| {
            let Detail::Devices(chain) =
                build_with(&project, Some(TrackId(1)), None, &mut |_, _| status.clone().map(|s| report(s, open)))
            else {
                panic!()
            };
            chain.devices[0].plugin.clone().unwrap()
        };
        let running = shown(Some(PluginStatus::Running), false);
        assert_eq!((running.state, running.line.as_str()), (PluginState::Running, "Verb by Vendor"));
        assert_eq!(running.button(), Some(("Open editor", "device.openEditor")));
        assert_eq!(shown(Some(PluginStatus::Running), true).button(), Some(("Close editor", "device.closeEditor")));
        let crashed = shown(Some(PluginStatus::Crashed("its host process exited".into())), false);
        assert_eq!(crashed.button(), Some(("Restart", "device.restart")));
        assert!(crashed.line.contains("exited"));
        let missing = shown(Some(PluginStatus::Missing), false);
        assert_eq!((missing.state, missing.button()), (PluginState::Missing, None));
        assert_eq!(shown(Some(PluginStatus::Loading), false).button(), None);
        // Not playing: nothing runs, nothing to press.
        let stopped = shown(None, false);
        assert_eq!((stopped.state, stopped.button()), (PluginState::Stopped, None));
        // A built-in effect or instrument has no plug-in part.
        let Detail::Devices(chain) = build(&demo_project(), Some(TrackId(1)), None) else { panic!() };
        assert!(chain.devices.iter().all(|d| d.plugin.is_none()));
    }

    #[test]
    fn an_embedded_editor_takes_its_room_and_says_where_it_lands() {
        use egui_kittest::Harness;
        let project = with_plugin();
        let embedded = PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) };
        let Detail::Devices(chain) = build_with(&project, Some(TrackId(1)), None, &mut |_, _| Some(embedded.clone()))
        else {
            panic!()
        };
        assert_eq!(chain.devices[0].plugin.as_ref().unwrap().embedded, Some((300, 200)));
        let detail = Detail::Devices(chain);
        for stacked in [false, true] {
            let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    let mut actions = Vec::new();
                    if stacked {
                        show_stack(ui, &detail, &mut actions, places);
                    } else {
                        show_row(ui, &detail, &mut actions, places);
                    }
                },
                Vec::new(),
            );
            harness.run();
            let places = harness.state();
            assert_eq!(places.len(), 1, "stacked {stacked}");
            let place = places[0];
            assert_eq!((place.track, place.device), (TrackId(1), 0));
            // Under the plug-in's line and button (past the harness's own margin), the whole
            // editor showing (at 1 pixel a point).
            let p = place.placement;
            let margin = p.y - EDITOR_TOP as i32;
            assert!(margin >= 0, "stacked {stacked}: {p:?}");
            assert_eq!((p.x, p.clip_x, p.clip_y), (PAD as i32 + margin, p.x, p.y), "stacked {stacked}");
            assert_eq!((p.clip_width, p.clip_height), (300, 200), "stacked {stacked}");
        }
        // A panel too short for it shows what fits; one with no room hides it.
        let margin = {
            let mut probe = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    show_row(ui, &detail, &mut Vec::new(), places);
                },
                Vec::new(),
            );
            probe.run();
            probe.state()[0].placement.y as f32 - EDITOR_TOP
        };
        let mut short =
            Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP + 50.0 + margin * 2.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    show_row(ui, &detail, &mut Vec::new(), places);
                },
                Vec::new(),
            );
        short.run();
        assert_eq!((short.state()[0].placement.clip_width, short.state()[0].placement.clip_height), (300, 50));
        let mut none = Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP - 1.0 + margin * 2.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &mut Vec::new(), places);
            },
            Vec::new(),
        );
        none.run();
        assert_eq!(none.state()[0].placement, Placement::HIDDEN);
    }

    #[test]
    fn the_editor_button_asks_for_the_device_command() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let project = with_plugin();
        let Detail::Devices(chain) =
            build_with(&project, Some(TrackId(1)), None, &mut |_, _| Some(report(PluginStatus::Running, false)))
        else {
            panic!()
        };
        let detail = Detail::Devices(chain);
        for stacked in [false, true] {
            let mut harness = Harness::builder().with_size(egui::vec2(600.0, 200.0)).build_ui_state(
                |ui, actions: &mut Vec<Action>| {
                    let mut places = Vec::new();
                    if stacked {
                        show_stack(ui, &detail, actions, &mut places);
                    } else {
                        show_row(ui, &detail, actions, &mut places);
                    }
                    assert!(places.is_empty());
                },
                Vec::new(),
            );
            harness.get_by_label("Open editor Verb").click();
            harness.run();
            let actions = harness.state();
            assert_eq!(actions.len(), 1, "stacked {stacked}");
            assert_eq!(actions[0].id, "device.openEditor");
            assert_eq!(actions[0].params, json!({ "track": 1, "device": 0 }));
        }
    }
}
