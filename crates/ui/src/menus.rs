//! Context menus the views share. Every entry is a command, the same the CLI and MCP run.

use egui::{Id, Key, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{Rgb, TimeSignature, TrackId};

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
    /// The groups the track can move into, with their names.
    pub into: &'a [(TrackId, String)],
    /// Takes the track out of its group, one level up; `None` when it is in none.
    pub out: Option<Action>,
}

/// The menu of a track header or column: name, colour, group, ungroup, move into or out of a
/// group, delete.
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
    if !menu.into.is_empty() {
        ui.menu_button("Move into group", |ui| {
            for (group, name) in menu.into {
                if ui.button(name).clicked() {
                    actions.push(Action::new("track.setGroup", json!({ "track": track, "group": group.0 })));
                    ui.close();
                }
            }
        });
    }
    if let Some(out) = &menu.out
        && ui.button("Move out of group").clicked()
    {
        actions.push(out.clone());
        ui.close();
    }
    ui.separator();
    if ui.button("Delete track").clicked() {
        actions.push(Action::new("track.delete", json!({ "track": track })));
        ui.close();
    }
}

/// The menu of an arrangement clip; `stack` is true when the stacking setting for the clip's
/// kind is on, so its copy leaves the clips it lands on as they are.
pub fn clip_menu(ui: &mut Ui, track: TrackId, index: usize, stack: bool, actions: &mut Vec<Action>) {
    ui.set_min_width(140.0);
    if ui.add(egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
        actions.push(Action::new("clip.copy", json!({ "track": track.0, "index": index })));
        ui.close();
    }
    if ui.add(egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
        let mut params = json!({ "track": track.0, "index": index });
        if stack {
            params["stack"] = json!(true);
        }
        actions.push(Action::new("clip.duplicate", params));
        ui.close();
    }
    if ui.add(egui::Button::new("Delete").shortcut_text("Del")).clicked() {
        actions.push(Action::new("clip.delete", json!({ "track": track.0, "index": index })));
        ui.close();
    }
}

/// The editing part of an arrangement clip's menu: split it at `at`, the beat right-clicked on
/// (when it is inside the clip), and consolidate it into a new clip.
pub fn clip_edit_menu(
    ui: &mut Ui,
    track: TrackId,
    index: usize,
    span: (f64, f64),
    at: Option<f64>,
    actions: &mut Vec<Action>,
) {
    let (start, length) = span;
    ui.separator();
    let inside = at.filter(|beat| *beat > start && *beat < start + length);
    if ui.add_enabled(inside.is_some(), egui::Button::new("Split here")).clicked()
        && let Some(beat) = inside
    {
        actions.push(Action::new("clip.split", json!({ "track": track.0, "index": index, "at": beat })));
        ui.close();
    }
    if ui.add(egui::Button::new("Consolidate").shortcut_text("Ctrl+J")).clicked() {
        actions
            .push(Action::new("clip.consolidate", json!({ "track": track.0, "start": start, "end": start + length })));
        ui.close();
    }
}

/// What a scene's menu needs to know.
pub struct SceneMenu<'a> {
    /// Counted from 1, as commands take it.
    pub number: usize,
    pub name: &'a str,
    pub tempo_bpm: Option<f64>,
    pub time_signature: Option<TimeSignature>,
    /// The scene is the only one, so it cannot be deleted.
    pub only: bool,
}

/// The menu of a scene's launch button: name, tempo, time signature, insert, duplicate, delete.
pub fn scene_menu(ui: &mut Ui, menu: &SceneMenu<'_>, actions: &mut Vec<Action>) {
    let scene = menu.number;
    ui.set_min_width(180.0);
    let tempo = menu.tempo_bpm.map(|bpm| format!("{bpm}")).unwrap_or_default();
    let signature = menu.time_signature.map(|ts| format!("{}/{}", ts.numerator, ts.denominator)).unwrap_or_default();
    let fields = [
        ("name", menu.name.to_string(), "Name"),
        ("tempo", tempo, "Tempo (bpm), empty for none"),
        ("signature", signature, "Time signature, such as 3/4"),
    ];
    for (key, current, hint) in fields {
        let id = Id::new(("scene-field", key, scene));
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(f32::INFINITY));
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if text.trim() != current {
                actions.extend(scene_field_action(key, scene, &text));
            }
            ui.data_mut(|d| d.remove::<String>(id));
            ui.close();
            return;
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    ui.separator();
    if ui.button("Insert scene below").clicked() {
        actions.push(Action::new("scene.insert", json!({ "scene": scene + 1 })));
        ui.close();
    }
    if ui.button("Duplicate scene").clicked() {
        actions.push(Action::new("scene.duplicate", json!({ "scene": scene })));
        ui.close();
    }
    if ui.add_enabled(!menu.only, egui::Button::new("Delete scene")).clicked() {
        actions.push(Action::new("scene.delete", json!({ "scene": scene })));
        ui.close();
    }
}

/// The command a scene menu field sends when its text is applied: a rename, a tempo or a time
/// signature, cleared by empty text. Text that is not a number or a time signature is sent as
/// it is, so the command says what is wrong with it.
pub fn scene_field_action(field: &str, scene: usize, text: &str) -> Option<Action> {
    let text = text.trim();
    match field {
        "name" => Some(Action::new("scene.rename", json!({ "scene": scene, "name": text }))),
        "tempo" if text.is_empty() => Some(Action::new("scene.setTempo", json!({ "scene": scene }))),
        "tempo" => {
            let bpm = text.parse::<f64>().map_or_else(|_| json!(text), |bpm| json!(bpm));
            Some(Action::new("scene.setTempo", json!({ "scene": scene, "bpm": bpm })))
        }
        "signature" if text.is_empty() => Some(Action::new("scene.setTimeSignature", json!({ "scene": scene }))),
        "signature" => {
            let (numerator, denominator) = text.split_once('/').unwrap_or((text, ""));
            let part = |p: &str| p.trim().parse::<u64>().map_or_else(|_| json!(p.trim()), |n| json!(n));
            Some(Action::new(
                "scene.setTimeSignature",
                json!({ "scene": scene, "numerator": part(numerator), "denominator": part(denominator) }),
            ))
        }
        _ => None,
    }
}

/// A clip slot's right-click menu: copy its clip, or paste the copied clip into it (`scene`
/// from 1). Copy is greyed out on an empty slot, Paste while nothing has been copied.
pub fn slot_menu(
    ui: &mut Ui,
    track: TrackId,
    scene: usize,
    has_clip: bool,
    can_paste: bool,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(140.0);
    if ui.add_enabled(has_clip, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
        actions.push(Action::new("clip.copy", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
    if ui.add_enabled(can_paste, egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
        actions.push(Action::new("clip.paste", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_fields_become_commands() {
        let action = |field, text| scene_field_action(field, 2, text).map(|a| (a.id, a.params));
        assert_eq!(action("name", " Drop "), Some(("scene.rename", json!({ "scene": 2, "name": "Drop" }))));
        assert_eq!(action("tempo", "128"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": 128.0 }))));
        assert_eq!(action("tempo", ""), Some(("scene.setTempo", json!({ "scene": 2 }))));
        assert_eq!(
            action("signature", "7/8"),
            Some(("scene.setTimeSignature", json!({ "scene": 2, "numerator": 7, "denominator": 8 })))
        );
        assert_eq!(action("signature", " "), Some(("scene.setTimeSignature", json!({ "scene": 2 }))));
        // Not a number: sent as text, so the command's error says what is wrong.
        assert_eq!(action("tempo", "fast"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": "fast" }))));
        assert_eq!(action("colour", "red"), None);
    }
}
