//! The detail area: the selected track's device chain, and beside it the selected clip's
//! properties (`docs/spec/ui-session-view.md` › Detail area). The chain is laid out in a row under the view
//! or stacked in a column beside it (a setting). It shows the project as it is; changing devices
//! comes with the device commands.

use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use loopwerk_engine::model::{DeviceKind, LaneData, Project, TrackId};

use crate::grid::gain_to_db;
use crate::theme::{self, PAD, ROW};
use crate::widgets;

/// Width of one device in a row, and of the column the stacked chain takes.
pub const DEVICE_WIDTH: f32 = 220.0;
pub const STACK_WIDTH: f32 = 260.0;
const PARAM_ROW: f32 = 16.0;
/// Width of the clip's properties beside the chain, and of a property's label.
const CLIP_WIDTH: f32 = 196.0;
const LABEL_WIDTH: f32 = 52.0;
const ON_SWITCH: f32 = 9.0;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceBox {
    pub name: String,
    /// What it is: a built-in device, a plug-in format, or one Loopwerk cannot run.
    pub kind: String,
    pub enabled: bool,
    /// Parameter ids and values as text.
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub track: String,
    /// bar.beat where it starts, and its length in bars and beats.
    pub start: String,
    pub length: String,
    pub looping: bool,
    pub audio: bool,
    pub gain_db: Option<f64>,
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
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<(TrackId, usize)>) -> Detail {
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    let track_name =
        |id: TrackId| project.tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    if let Some((id, index)) = clip
        && let Some(placed) =
            project.arrangement.lanes.iter().find(|l| l.track == Some(id)).and_then(|l| l.clips.get(index))
    {
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
                start: bars_beats(placed.start, 1),
                length: bars_beats(placed.length, 0),
                looping: placed.loop_range.is_some(),
                audio: audio.is_some(),
                gain_db: audio.map(gain_to_db),
                notes,
            },
            chain(project, id),
        );
    }
    track.map_or(Detail::Nothing, |id| Detail::Devices(chain(project, id)))
}

/// A track's devices as the detail area shows them.
fn chain(project: &Project, id: TrackId) -> Chain {
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
                .map(|d| DeviceBox {
                    name: d.name.clone(),
                    kind: match &d.kind {
                        DeviceKind::Builtin(_) => "Loopwerk".into(),
                        DeviceKind::Clap { .. } => "CLAP".into(),
                        DeviceKind::Vst3 { .. } => "VST3".into(),
                        DeviceKind::Vst2 { .. } => "VST2".into(),
                        DeviceKind::Foreign { format, .. } => format!("Not supported ({format})"),
                    },
                    enabled: d.enabled,
                    params: d.params.iter().map(|p| (p.id.clone(), format_value(p.value))).collect(),
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

/// The detail laid out in a row: devices side by side, or the clip's properties.
pub fn show_row(ui: &mut Ui, detail: &Detail) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::surface_0());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip to see it here"),
        Detail::Devices(chain) => chain_row(ui, &painter, area, chain),
        Detail::Clip(info, chain) => {
            let column = clip_properties(&painter, area, info, false);
            let rest = Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max);
            if rest.width() > 0.0 {
                chain_row(ui, &painter, rest, chain);
            }
        }
    }
}

/// A chain's devices side by side in `area`.
fn chain_row(ui: &Ui, painter: &egui::Painter, area: Rect, chain: &Chain) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let mut x = area.left();
    for device in &chain.devices {
        let rect = Rect::from_min_max(Pos2::new(x, area.top()), Pos2::new(x + DEVICE_WIDTH, area.bottom()));
        x = rect.right() + 1.0;
        if rect.left() > area.right() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(&painter.with_clip_rect(area), rect, device);
    }
}

/// The detail stacked in a column: devices one under another.
pub fn show_stack(ui: &mut Ui, detail: &Detail) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::surface_0());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip"),
        Detail::Devices(chain) => chain_stack(ui, &painter, area, chain),
        Detail::Clip(info, chain) => {
            let column = clip_properties(&painter, area, info, true);
            let rest = Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max);
            if rest.height() > 0.0 {
                chain_stack(ui, &painter, rest, chain);
            }
        }
    }
}

/// A chain's devices one under another in `area`.
fn chain_stack(ui: &Ui, painter: &egui::Painter, area: Rect, chain: &Chain) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let mut y = area.top();
    for device in &chain.devices {
        let height = ROW + PAD * 2.0 + device.params.len().max(1) as f32 * PARAM_ROW;
        let rect = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.right(), y + height));
        y = rect.bottom() + 1.0;
        if rect.top() > area.bottom() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(&painter.with_clip_rect(area), rect, device);
    }
}

/// Names a device's panel for screen readers and tests.
fn name_for_readers(ui: &Ui, rect: Rect, device: &DeviceBox) {
    let response = ui.interact(rect, egui::Id::new(("device", rect.min.x as i32, rect.min.y as i32)), Sense::hover());
    let label = format!("Device {}", device.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_tertiary());
}

/// One device: a header with its on switch and name, then its parameters, one per row.
fn device_panel(painter: &egui::Painter, rect: Rect, device: &DeviceBox) {
    painter.rect_filled(rect, 0.0, theme::surface_1());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(header, 0.0, theme::surface_2());
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::text_primary());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::text_tertiary()), egui::StrokeKind::Inside);
    }
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_tertiary() };
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
        theme::text_tertiary(),
    );

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + PAD, header.bottom() + PAD),
        Pos2::new(rect.right() - PAD, rect.bottom() - PAD),
    );
    if device.params.is_empty() {
        let text = if device.kind.starts_with("Not supported") {
            "Kept so the chain keeps its shape"
        } else {
            "Its own window"
        };
        widgets::text_in(
            painter,
            Rect::from_min_size(body.min, Vec2::new(body.width(), PARAM_ROW)),
            Align2::LEFT_CENTER,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_tertiary(),
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
        ("Start", info.start.clone()),
        ("Length", info.length.clone()),
        ("Loop", if info.looping { "On".into() } else { "Off".into() }),
    ];
    if info.audio {
        rows.push(("Gain", info.gain_db.map_or_else(|| "-".into(), |db| format!("{} dB", widgets::format_db(db)))));
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
    painter.rect_filled(column, 0.0, theme::surface_1());
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
    use loopwerk_engine::model::{Device, DeviceParam, Extensions};

    #[test]
    fn a_selected_clip_shows_its_properties() {
        let detail = build(&demo_project(), Some(TrackId(1)), Some((TrackId(1), 1)));
        let Detail::Clip(info, chain) = detail else { panic!("expected the clip") };
        assert_eq!(chain.track, TrackId(1));
        assert_eq!((info.name.as_str(), info.start.as_str(), info.length.as_str()), ("Beat B", "17.1", "12.0"));
        assert!(info.looping && !info.audio);
        assert_eq!(info.notes, 4);
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
            extensions: Extensions::default(),
        });
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        assert_eq!(chain.devices.len(), 1);
        assert_eq!(chain.devices[0].params, [("gain".to_string(), "-3.5".to_string())]);
        assert!(!chain.devices[0].enabled);
        // A clip on the track shows the same chain beside its properties.
        let Detail::Clip(_, with_clip) = build(&project, None, Some((TrackId(1), 0))) else { panic!() };
        assert_eq!(with_clip, chain);
        assert_eq!(build(&project, None, None), Detail::Nothing);
    }
}
