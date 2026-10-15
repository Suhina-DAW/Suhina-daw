//! Context menus the views share. Every entry is a command, the same the CLI and MCP run.

use egui::{Id, Key, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{Rgb, TrackId};

use crate::grid::to_rgb;
use crate::theme;
use crate::widgets::Action;

/// Colours a track can be given from its menu: the theme's eight track colours
/// (`track.1` to `track.8`; the first four are the default track colours).
pub fn track_swatches() -> [Rgb; 8] {
    [
        theme::track_color_1(),
        theme::track_color_2(),
        theme::track_color_3(),
        theme::track_color_4(),
        theme::track_color_5(),
        theme::track_color_6(),
        theme::track_color_7(),
        theme::track_color_8(),
    ]
    .map(to_rgb)
}

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
        for color in track_swatches() {
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

/// The menu of arrangement clips; `stack` is true when the stacking setting for the clips' kind
/// is on, so their copies leave the clips they land on as they are. `clips` is the selection the
/// click fell in, or the one clip it fell on; `enabled` is whether the clicked clip plays now.
pub fn clip_menu(ui: &mut Ui, clips: &[(TrackId, usize)], stack: bool, enabled: bool, actions: &mut Vec<Action>) {
    if clips.is_empty() {
        return;
    }
    let list: Vec<serde_json::Value> =
        clips.iter().map(|(track, index)| json!({ "track": track.0, "index": index })).collect();
    ui.set_min_width(140.0);
    if ui.add(egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
        let mut params = json!({ "clips": list.clone() });
        if stack {
            params["stack"] = json!(true);
        }
        actions.push(Action::new("clip.duplicate", params));
        ui.close();
    }
    let mute = if enabled { "Mute" } else { "Unmute" };
    if ui.add(egui::Button::new(mute).shortcut_text("Ctrl+M")).clicked() {
        actions.push(Action::new("clip.setEnabled", json!({ "clips": list.clone(), "enabled": !enabled })));
        ui.close();
    }
    if ui.add(egui::Button::new("Delete").shortcut_text("Del")).clicked() {
        actions.push(Action::new("clip.delete", json!({ "clips": list })));
        ui.close();
    }
}
