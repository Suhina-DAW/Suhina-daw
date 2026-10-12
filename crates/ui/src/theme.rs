//! Design tokens from `docs/design/brief.md` › Tokens and the sizes in
//! `docs/design/components.md`. Views use these names, never raw colours or sizes.
//!
//! Colours come from the theme in use (a [`Palette`]), so the user can change them: a theme is a
//! JSON file of token names and `#RRGGBB` values (see [`Theme::from_json`]). Sizes are fixed.

use std::cell::Cell;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use serde_json::{Map, Value};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Declares the colour tokens once: the [`Palette`] field, its name in a theme file, the default
/// (the built-in dark theme), what it is used for, and a function returning it from the theme in
/// use.
macro_rules! palette {
    ($($field:ident, $key:literal, $hex:literal, $use:literal;)*) => {
        /// Every colour token of a theme.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $(#[doc = $use] pub $field: Color32,)*
        }

        impl Palette {
            /// The built-in dark theme: the values in the design brief.
            pub const DARK: Self = Self { $($field: rgb($hex),)* };

            /// Each token's name in a theme file, what it is used for, and its colour.
            pub fn tokens(&self) -> Vec<(&'static str, &'static str, Color32)> {
                vec![$(($key, $use, self.$field),)*]
            }

            /// The colour of the token named `key`, to change.
            pub fn token_mut(&mut self, key: &str) -> Option<&mut Color32> {
                match key {
                    $($key => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }

        $(#[doc = $use] pub fn $field() -> Color32 { PALETTE.with(|p| p.get().$field) })*
    };
}

palette! {
    surface_0, "surface.0", 0x1D1C1A, "Grid background, empty slots, lanes";
    surface_1, "surface.1", 0x252421, "Panels, headers, beat lines";
    surface_2, "surface.2", 0x2E2D2A, "Fields, stopped clips, buttons";
    surface_3, "surface.3", 0x45433F, "Selected, pressed";
    surface_3_bright, "surface.3.bright", 0x4A4843, "Playing clip, playing scene, volume fill";
    hover_on_surface_1, "hover.surface.1", 0x2A2926, "Hover over surface.1";
    hover_on_surface_2, "hover.surface.2", 0x34332F, "Hover over surface.2";
    line, "line", 0x383733, "Separators between regions and rows";
    line_strong, "line.strong", 0x45433F, "Session columns, arrangement tracks";
    text_primary, "text.primary", 0xECE8E1, "Names and values";
    text_secondary, "text.secondary", 0x9A958C, "Labels and notes";
    text_tertiary, "text.tertiary", 0x66625B, "Inactive, hints, section titles";
    text_value, "text.value", 0xC9C4BB, "Clip names, values on fields, the chosen option";
    text_playing, "text.playing", 0xFFFFFF, "The name of a playing clip";
    accent, "accent", 0xA6A3FF, "Playhead, queued clips, keys being set";
    recording, "state.recording", 0xE5484D, "Recording, errors";
    meter_ok, "meter.ok", 0x8BC48A, "Meter below the top";
    meter_hot, "meter.hot", 0xE3C86A, "Meter near the top";
    empty_mark, "slot.empty", 0x4A4743, "The mark of an empty slot";
    muted_fill, "muted.fill", 0x34322F, "A muted track's fills";
    pan_background, "pan.background", 0x232220, "Behind a track header's pan field";
    pan_centre, "pan.centre", 0x55524C, "The centre line of a pan field";
}

impl Default for Palette {
    fn default() -> Self {
        Self::DARK
    }
}

thread_local! {
    /// The colours in use. The UI draws on one thread; tests each get their own, so a test that
    /// changes the theme does not change another's.
    static PALETTE: Cell<Palette> = const { Cell::new(Palette::DARK) };
}

/// The colours in use.
pub fn palette() -> Palette {
    PALETTE.with(Cell::get)
}

/// Draws with `palette` from the next frame on: the tokens and egui's own visuals.
pub fn set_palette(ctx: &egui::Context, palette: Palette) {
    PALETTE.with(|p| p.set(palette));
    ctx.all_styles_mut(|style| visuals(&mut style.visuals, &palette));
    ctx.request_repaint();
}

/// The id of the built-in theme, in the settings file.
pub const BUILT_IN: &str = "loopwerk-dark";

/// A named set of colours, kept in a theme file:
///
/// ```json
/// { "name": "Night", "colors": { "surface.0": "#1D1C1A", "accent": "#A6A3FF" } }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub colors: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self { name: "Loopwerk Dark".into(), colors: Palette::DARK }
    }
}

impl Theme {
    /// Reads a theme file. Tokens it leaves out keep the built-in colour, so a file only needs
    /// the colours it changes; unknown tokens and values that are not `#RRGGBB` are skipped with
    /// a warning.
    pub fn from_json(text: &str) -> (Self, Vec<String>) {
        let mut theme = Self::default();
        let mut warnings = Vec::new();
        let root = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(root)) => root,
            Ok(_) => return (theme, vec!["a theme file is a JSON object".into()]),
            Err(e) => return (theme, vec![format!("the theme is not JSON: {e}")]),
        };
        match root.get("name") {
            Some(Value::String(name)) if !name.trim().is_empty() => theme.name = name.trim().to_string(),
            Some(_) => warnings.push("the theme's name is not text".into()),
            None => {}
        }
        match root.get("colors") {
            Some(Value::Object(colors)) => {
                for (key, value) in colors {
                    let parsed = value.as_str().and_then(parse_hex);
                    match (theme.colors.token_mut(key), parsed) {
                        (Some(slot), Some(color)) => *slot = color,
                        (None, _) => warnings.push(format!("{key} is not a colour token")),
                        (Some(_), None) => warnings.push(format!("{key}: {value} is not a #RRGGBB colour")),
                    }
                }
            }
            Some(_) => warnings.push("the theme's colors are not an object".into()),
            None => {}
        }
        for key in root.keys().filter(|k| !matches!(k.as_str(), "name" | "colors")) {
            warnings.push(format!("{key} is not part of a theme"));
        }
        (theme, warnings)
    }

    /// The theme as a file, every token written out so it can be edited.
    pub fn to_json(&self) -> String {
        let colors: Map<String, Value> = self
            .colors
            .tokens()
            .into_iter()
            .map(|(key, _, color)| (key.to_string(), Value::String(hex(color))))
            .collect();
        let mut root = Map::new();
        root.insert("name".into(), Value::String(self.name.clone()));
        root.insert("colors".into(), Value::Object(colors));
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }
}

/// `#RRGGBB`, as theme files write a colour.
pub fn hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// Reads `#RRGGBB` (the `#` may be left out).
pub fn parse_hex(text: &str) -> Option<Color32> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

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

/// The UI typeface: Encode Sans Semi Expanded (SIL Open Font License 1.1,
/// `fonts/encode-sans/OFL.txt`).
const UI_FONT: &[u8] = include_bytes!("../fonts/encode-sans/EncodeSansSemiExpanded-Regular.ttf");
/// Numbers, times and positions: IBM Plex Mono (SIL Open Font License 1.1,
/// `fonts/ibm-plex-mono/OFL.txt`).
const MONO_FONT: &[u8] = include_bytes!("../fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");

/// egui's fonts with ours first: Encode Sans for UI text, IBM Plex Mono for numbers. egui's own
/// fonts stay behind them for characters ours lack.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("encode-sans".into(), std::sync::Arc::new(egui::FontData::from_static(UI_FONT)));
    fonts.font_data.insert("ibm-plex-mono".into(), std::sync::Arc::new(egui::FontData::from_static(MONO_FONT)));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "encode-sans".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "ibm-plex-mono".into());
    fonts
}

/// Sets the fonts, and egui's visuals and text styles to the tokens, for the few stock widgets
/// the views use.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    let palette = palette();
    ctx.all_styles_mut(|style| {
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
        visuals(&mut style.visuals, &palette);
    });
}

/// egui's visuals in the colours of `p`.
fn visuals(visuals: &mut egui::Visuals, p: &Palette) {
    *visuals = egui::Visuals::dark();
    visuals.panel_fill = p.surface_1;
    visuals.window_fill = p.surface_1;
    visuals.extreme_bg_color = p.surface_0;
    visuals.faint_bg_color = p.surface_1;
    visuals.window_stroke = hairline(p.line);
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.selection.bg_fill = p.surface_3;
    visuals.selection.stroke = hairline(p.accent);
    visuals.override_text_color = None;
    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, p.surface_1),
        (&mut visuals.widgets.inactive, p.surface_2),
        (&mut visuals.widgets.hovered, p.hover_on_surface_2),
        (&mut visuals.widgets.active, p.surface_3),
        (&mut visuals.widgets.open, p.surface_3),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.corner_radius = egui::CornerRadius::ZERO;
        widget.bg_stroke = Stroke::NONE;
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = hairline(p.line);
    visuals.widgets.noninteractive.fg_stroke = hairline(p.text_secondary);
    visuals.widgets.inactive.fg_stroke = hairline(p.text_value);
    visuals.widgets.hovered.fg_stroke = hairline(p.text_primary);
    visuals.widgets.active.fg_stroke = hairline(p.text_primary);
    visuals.widgets.open.fg_stroke = hairline(p.text_primary);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_survives_a_round_trip() {
        let mut theme = Theme { name: "Night".into(), colors: Palette::DARK };
        theme.colors.accent = rgb(0x12AB9F);
        theme.colors.surface_0 = rgb(0x000000);
        assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    }

    #[test]
    fn a_theme_file_needs_only_the_colours_it_changes() {
        let (theme, warnings) = Theme::from_json(r##"{ "name": "Blue", "colors": { "accent": "#3366ff" } }"##);
        assert!(warnings.is_empty());
        assert_eq!(theme.name, "Blue");
        assert_eq!(theme.colors.accent, rgb(0x3366FF));
        assert_eq!(theme.colors.surface_0, Palette::DARK.surface_0);
    }

    #[test]
    fn bad_values_keep_the_built_in_colours() {
        let text =
            r##"{ "name": 3, "colors": { "accent": "blue", "surface.9": "#000000", "line": "#12345" }, "font": 1 }"##;
        let (theme, warnings) = Theme::from_json(text);
        assert_eq!(theme, Theme::default());
        assert_eq!(warnings.len(), 5, "{warnings:?}");
        assert_eq!(Theme::from_json("[1, 2]").1.len(), 1);
        assert_eq!(Theme::from_json("{ not json").0, Theme::default());
    }

    #[test]
    fn colours_are_written_and_read_as_hex() {
        assert_eq!(hex(rgb(0x1D1C1A)), "#1D1C1A");
        assert_eq!(parse_hex("#a6a3ff"), Some(rgb(0xA6A3FF)));
        assert_eq!(parse_hex("A6A3FF"), Some(rgb(0xA6A3FF)));
        assert_eq!(parse_hex("#A6A3F"), None);
        assert_eq!(parse_hex("#GGGGGG"), None);
        assert_eq!(parse_hex("#+1+2+3"), None);
    }

    #[test]
    fn every_token_has_its_own_name() {
        let tokens = Palette::DARK.tokens();
        let mut keys: Vec<_> = tokens.iter().map(|(key, _, _)| *key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), tokens.len());
    }

    #[test]
    fn the_palette_in_use_changes_with_the_theme() {
        let ctx = egui::Context::default();
        let mut night = Palette::DARK;
        night.accent = rgb(0x00FF00);
        set_palette(&ctx, night);
        assert_eq!(accent(), rgb(0x00FF00));
        set_palette(&ctx, Palette::DARK);
        assert_eq!(accent(), Palette::DARK.accent);
    }
}
