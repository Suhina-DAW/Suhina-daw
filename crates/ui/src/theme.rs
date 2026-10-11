//! Design tokens from `docs/design/brief.md` › Tokens and the sizes in
//! `docs/design/components.md`. Views use these names, never raw colours or sizes.

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const SURFACE_0: Color32 = rgb(0x1D1C1A);
pub const SURFACE_1: Color32 = rgb(0x252421);
pub const SURFACE_2: Color32 = rgb(0x2E2D2A);
pub const SURFACE_3: Color32 = rgb(0x45433F);
/// The brighter `surface.3`: playing clip, playing scene, volume fill.
pub const SURFACE_3_BRIGHT: Color32 = rgb(0x4A4843);
pub const LINE: Color32 = rgb(0x383733);
pub const LINE_STRONG: Color32 = rgb(0x45433F);
pub const TEXT_PRIMARY: Color32 = rgb(0xECE8E1);
pub const TEXT_SECONDARY: Color32 = rgb(0x9A958C);
pub const TEXT_TERTIARY: Color32 = rgb(0x66625B);
/// Clip names and values on fields, between primary and secondary.
pub const TEXT_VALUE: Color32 = rgb(0xC9C4BB);
pub const ACCENT: Color32 = rgb(0xA6A3FF);
pub const RECORDING: Color32 = rgb(0xE5484D);
pub const METER_OK: Color32 = rgb(0x8BC48A);
pub const METER_HOT: Color32 = rgb(0xE3C86A);

/// Hover: one step lighter than the surface underneath (components › Interaction states).
pub const HOVER_ON_SURFACE_2: Color32 = rgb(0x34332F);
pub const HOVER_ON_SURFACE_1: Color32 = rgb(0x2A2926);
/// The mark of an empty slot.
pub const EMPTY_MARK: Color32 = rgb(0x4A4743);
/// A muted track's fills.
pub const MUTED_FILL: Color32 = rgb(0x34322F);

/// Grid rows, headers, fields.
pub const ROW: f32 = 20.0;
pub const MENU_BAR: f32 = 20.0;
pub const CONTROL_BAR: f32 = 28.0;
pub const STATUS_BAR: f32 = 20.0;
pub const TRACK_COLUMN: f32 = 116.0;
pub const RETURN_COLUMN: f32 = 84.0;
pub const SCENE_COLUMN: f32 = 96.0;
/// The mixer section under the Session grid.
pub const MIXER: f32 = 72.0;
pub const KNOB: f32 = 22.0;
pub const TOGGLE: f32 = 18.0;
/// Padding inside cells and strips.
pub const PAD: f32 = 6.0;

/// Text sizes: UI text, small values and labels, rulers and key labels.
pub const TEXT: f32 = 11.0;
pub const TEXT_SMALL: f32 = 10.0;
pub const TEXT_TINY: f32 = 9.0;

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Every number, time and position is monospaced.
pub fn mono_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn hairline(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// Sets egui's visuals and text styles to the tokens, for the few stock widgets the views use.
pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        style.text_styles = [
            (TextStyle::Heading, ui_font(TEXT)),
            (TextStyle::Body, ui_font(TEXT)),
            (TextStyle::Button, ui_font(TEXT)),
            (TextStyle::Small, ui_font(TEXT_SMALL)),
            (TextStyle::Monospace, mono_font(TEXT_SMALL)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size.y = ROW;
        let visuals = &mut style.visuals;
        visuals.panel_fill = SURFACE_1;
        visuals.window_fill = SURFACE_1;
        visuals.extreme_bg_color = SURFACE_0;
        visuals.faint_bg_color = SURFACE_1;
        visuals.window_stroke = hairline(LINE);
        visuals.window_corner_radius = egui::CornerRadius::ZERO;
        visuals.menu_corner_radius = egui::CornerRadius::ZERO;
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.popup_shadow = egui::epaint::Shadow::NONE;
        visuals.selection.bg_fill = SURFACE_3;
        visuals.selection.stroke = hairline(ACCENT);
        visuals.override_text_color = None;
        for (widget, fill) in [
            (&mut visuals.widgets.noninteractive, SURFACE_1),
            (&mut visuals.widgets.inactive, SURFACE_2),
            (&mut visuals.widgets.hovered, HOVER_ON_SURFACE_2),
            (&mut visuals.widgets.active, SURFACE_3),
            (&mut visuals.widgets.open, SURFACE_3),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.corner_radius = egui::CornerRadius::ZERO;
            widget.bg_stroke = Stroke::NONE;
            widget.expansion = 0.0;
        }
        visuals.widgets.noninteractive.bg_stroke = hairline(LINE);
        visuals.widgets.noninteractive.fg_stroke = hairline(TEXT_SECONDARY);
        visuals.widgets.inactive.fg_stroke = hairline(TEXT_VALUE);
        visuals.widgets.hovered.fg_stroke = hairline(TEXT_PRIMARY);
        visuals.widgets.active.fg_stroke = hairline(TEXT_PRIMARY);
        visuals.widgets.open.fg_stroke = hairline(TEXT_PRIMARY);
    });
}
