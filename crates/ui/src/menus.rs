//! Context menus the views share. Every entry is a command, the same the CLI and MCP run.

use egui::{Id, Key, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{AudioInput, InputChannels, MidiInput, MidiPort, Monitor, Rgb, TrackId};

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
    /// For a track that plays notes: what it hears while armed, and the MIDI ports there are.
    pub midi: Option<(&'a MidiInput, &'a [String])>,
    /// For a track that plays audio: its audio input, and the channels the input device has
    /// (0 while none is open).
    pub audio: Option<(&'a AudioInput, u16)>,
}

/// How a track's audio input reads: "No input", "In 3" or "In 1/2", with its monitoring.
pub fn audio_input_text(input: &AudioInput) -> String {
    let channels = match input.channels {
        InputChannels::None => return "No input".to_string(),
        InputChannels::Mono(c) => format!("In {c}"),
        InputChannels::Stereo(c) => format!("In {c}/{}", c.saturating_add(1)),
    };
    let monitor = match input.monitor {
        Monitor::Auto => "Auto",
        Monitor::In => "Monitor in",
        Monitor::Off => "Monitor off",
    };
    format!("{channels} · {monitor}")
}

/// The choices of a track's audio input (ADR 0017): none, each channel of the input device,
/// each neighbouring pair; then its monitoring. A pick runs `track.setAudioInput`.
pub fn audio_input_choices(ui: &mut Ui, track: TrackId, input: &AudioInput, channels: u16, actions: &mut Vec<Action>) {
    let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
    let mut pick = |ui: &mut Ui, label: &str, on: bool, params: serde_json::Value| {
        if ui.selectable_label(on, text(label)).clicked() && !on {
            let mut params = params;
            params["track"] = json!(track.0);
            actions.push(Action::new("track.setAudioInput", params));
            ui.close();
        }
    };
    pick(ui, "No input", input.channels == InputChannels::None, json!({ "input": "none" }));
    if channels == 0 {
        ui.label(text("No input device: choose one in Settings › Audio"));
    }
    // The device's channels, and the one chosen if the device has fewer now; without a device,
    // the first eight, so a project can be set up before the interface is there.
    let channels = if channels == 0 { 8 } else { channels };
    let chosen = match input.channels {
        InputChannels::None => 0,
        InputChannels::Mono(c) => c,
        InputChannels::Stereo(c) => c.saturating_add(1),
    };
    let shown = channels.max(chosen);
    egui::Grid::new(("audio-channels", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for c in 1..=shown {
            pick(ui, &format!("In {c}"), input.channels == InputChannels::Mono(c), json!({ "input": c.to_string() }));
            if c % 4 == 0 {
                ui.end_row();
            }
        }
    });
    egui::Grid::new(("audio-pairs", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for c in (1..shown).step_by(2) {
            let pair = format!("{c}/{}", c + 1);
            pick(ui, &format!("In {pair}"), input.channels == InputChannels::Stereo(c), json!({ "input": pair }));
            if c % 8 == 7 {
                ui.end_row();
            }
        }
    });
    ui.separator();
    for (label, monitor, value) in [
        ("Monitor auto (while armed)", Monitor::Auto, "auto"),
        ("Monitor in", Monitor::In, "in"),
        ("Monitor off", Monitor::Off, "off"),
    ] {
        pick(ui, label, input.monitor == monitor, json!({ "monitor": value }));
    }
}

/// How a track's MIDI input reads: "All ports", the port, or "No input", with the channel.
pub fn midi_input_text(input: &MidiInput) -> String {
    let port = match &input.port {
        MidiPort::All => "All ports",
        MidiPort::Named(name) => name.as_str(),
        MidiPort::Nothing => "No input",
    };
    match input.channel {
        Some(channel) if input.port != MidiPort::Nothing => format!("{port} · Ch {channel}"),
        _ => port.to_string(),
    }
}

/// The choices of a track's MIDI input (ADR 0017): every port, each port there is (and the one
/// chosen, if it is not there now), none; then every channel or one. A pick runs
/// `track.setMidiInput`.
pub fn midi_input_choices(ui: &mut Ui, track: TrackId, input: &MidiInput, ports: &[String], actions: &mut Vec<Action>) {
    let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
    let mut pick = |ui: &mut Ui, label: &str, on: bool, params: serde_json::Value| {
        if ui.selectable_label(on, text(label)).clicked() && !on {
            let mut params = params;
            params["track"] = json!(track.0);
            actions.push(Action::new("track.setMidiInput", params));
            ui.close();
        }
    };
    pick(ui, "All ports", input.port == MidiPort::All, json!({ "port": "all" }));
    let chosen = match &input.port {
        MidiPort::Named(name) if !ports.contains(name) => Some(name),
        _ => None,
    };
    for name in ports.iter().chain(chosen) {
        let label = if Some(name) == chosen { format!("{name} (not connected)") } else { name.clone() };
        pick(ui, &label, input.port == MidiPort::Named(name.clone()), json!({ "port": name }));
    }
    pick(ui, "No input", input.port == MidiPort::Nothing, json!({ "port": "none" }));
    ui.separator();
    pick(ui, "All channels", input.channel.is_none(), json!({ "channel": 0 }));
    egui::Grid::new(("midi-channels", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for channel in 1..=16u8 {
            pick(ui, &format!("Ch {channel}"), input.channel == Some(channel), json!({ "channel": channel }));
            if channel % 4 == 0 {
                ui.end_row();
            }
        }
    });
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
    if let Some((input, channels)) = menu.audio {
        ui.menu_button(format!("Audio input: {}", audio_input_text(input)), |ui| {
            audio_input_choices(ui, menu.track, input, channels, actions);
        });
    }
    if let Some((input, ports)) = menu.midi {
        ui.menu_button(format!("MIDI input: {}", midi_input_text(input)), |ui| {
            midi_input_choices(ui, menu.track, input, ports, actions);
        });
    }
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

/// The menu of an arrangement clip; `stack` is true when the stacking setting for the clip's
/// kind is on, so its copy leaves the clips it lands on as they are.
pub fn clip_menu(ui: &mut Ui, track: TrackId, index: usize, stack: bool, actions: &mut Vec<Action>) {
    ui.set_min_width(140.0);
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
