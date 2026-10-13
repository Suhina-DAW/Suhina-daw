//! Context menus the views share. Every entry is a command, the same the CLI and MCP run.

use egui::{Id, Key, Sense, Ui, Vec2};
use loopwerk_engine::model::{Rgb, TrackId};
use serde_json::json;

use crate::grid::TRACK_PALETTE;
use crate::theme;
use crate::widgets::Action;

/// Colours a track can be given from its menu: the brief's track colours and four more.
pub const TRACK_SWATCHES: [Rgb; 8] = [
    TRACK_PALETTE[0],
    TRACK_PALETTE[1],
    TRACK_PALETTE[2],
    TRACK_PALETTE[3],
    Rgb(0x8F, 0xA8, 0x7A),
    Rgb(0xC2, 0x7C, 0x8E),
    Rgb(0x7F, 0x8F, 0xB0),
    Rgb(0xB9, 0xB2, 0xA6),
];

/// What a track's menu needs to know.
pub struct TrackMenu<'a> {
    pub track: TrackId,
    pub name: &'a str,
    pub is_group: bool,
    /// The tracks selected in the view; grouping takes them when this track is one of them.
    pub selection: &'a [TrackId],
}

/// The menu of a track header or column: name, colour, group, ungroup, delete.
pub fn track_menu(ui: &mut Ui, menu: &TrackMenu<'_>, actions: &mut Vec<Action>) {
    let track = menu.track.0;
    ui.set_min_width(180.0);

    // Name: edited in place, applied with Enter.
    let id = Id::new(("rename", track));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| menu.name.to_string());
    let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Name").desired_width(f32::INFINITY));
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        let name = text.trim();
        if !name.is_empty() && name != menu.name {
            actions.push(Action::new("track.rename", json!({ "track": track, "name": name })));
        }
        ui.data_mut(|d| d.remove::<String>(id));
        ui.close();
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, text));

    // Colour.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(3.0);
        for color in TRACK_SWATCHES {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
            let hex = format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Colour {hex}")));
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(color.0, color.1, color.2));
            if response.hovered() {
                ui.painter().rect_stroke(rect, 0.0, theme::hairline(theme::text_primary()), egui::StrokeKind::Inside);
            }
            if response.clicked() {
                actions.push(Action::new("track.setColor", json!({ "track": track, "color": hex })));
                ui.close();
            }
        }
    });
    ui.separator();

    let selected = menu.selection.contains(&menu.track) && menu.selection.len() > 1;
    let group_label = if selected { "Group selected tracks" } else { "Group" };
    if ui.add(egui::Button::new(group_label).shortcut_text("Ctrl+G")).clicked() {
        let tracks: Vec<u32> = if selected { menu.selection.iter().map(|t| t.0).collect() } else { vec![track] };
        actions.push(Action::new("track.group", json!({ "tracks": tracks })));
        ui.close();
    }
    if menu.is_group && ui.button("Ungroup").clicked() {
        actions.push(Action::new("track.ungroup", json!({ "track": track })));
        ui.close();
    }
    ui.separator();
    if ui.button("Delete track").clicked() {
        actions.push(Action::new("track.delete", json!({ "track": track })));
        ui.close();
    }
}

/// The menu of an arrangement clip.
pub fn clip_menu(ui: &mut Ui, track: TrackId, index: usize, actions: &mut Vec<Action>) {
    ui.set_min_width(140.0);
    if ui.add(egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
        actions.push(Action::new("clip.duplicate", json!({ "track": track.0, "index": index })));
        ui.close();
    }
    if ui.add(egui::Button::new("Delete").shortcut_text("Del")).clicked() {
        actions.push(Action::new("clip.delete", json!({ "track": track.0, "index": index })));
        ui.close();
    }
}
