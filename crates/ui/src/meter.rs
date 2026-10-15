//! Level meters (`docs/spec/level-meters.md`, sizes in `docs/design/components.md` › Meter):
//! the engine's peak and RMS levels of a track or the main output as two bars, one per side,
//! on the volume control's dB scale, with the highest peak of the last moments held as a line
//! and a clip light that a click turns off (`mixer.resetClip`).

use egui::{Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_engine::{Level, MeterMonitor};

use crate::theme;
use crate::widgets::Action;

/// The scale: the volume control's range, in dB, bottom to top.
pub const RANGE: (f64, f64) = (-70.0, 6.0);
/// Above this the bar turns `meter.hot`: the top 12 % of the scale.
const HOT_DB: f64 = -3.0;
/// How long the highest peak stays shown before it follows the level down, in seconds.
const HOLD_SECONDS: f64 = 1.5;
/// A bar's width, the gap between the two, and the clip light's height with its gap.
pub const BAR: f32 = 4.0;
const GAP: f32 = 2.0;
const CLIP: f32 = 3.0;
/// The width of a meter standing up, both bars and the gap.
pub const WIDTH: f32 = BAR * 2.0 + GAP;

/// What the meters show this frame, read from the playing engine.
#[derive(Debug, Clone, Default)]
pub struct Meters {
    pub main: Level,
    pub tracks: Vec<(TrackId, Level)>,
}

impl Meters {
    /// The levels `monitor` holds now; nothing without an engine.
    pub fn read(monitor: Option<&MeterMonitor>) -> Self {
        monitor.map_or_else(Self::default, |m| Self { main: m.main(), tracks: m.tracks() })
    }

    pub fn track(&self, track: TrackId) -> Level {
        self.tracks.iter().find(|(t, _)| *t == track).map(|(_, l)| *l).unwrap_or_default()
    }

    /// Whether any meter shows something, so the window keeps drawing while they move.
    pub fn moving(&self) -> bool {
        let lit = |l: &Level| l.peak.iter().any(|&p| p > 0.0) || l.clipped;
        lit(&self.main) || self.tracks.iter().any(|(_, l)| lit(l))
    }
}

/// Whose meter it is: the clip light's click resets this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Main,
    Track(TrackId),
}

/// Where a level falls on the scale, 0 … 1.
pub fn fraction(level: f32) -> f32 {
    let db = 20.0 * f64::from(level).log10();
    if !db.is_finite() {
        return 0.0;
    }
    ((db - RANGE.0) / (RANGE.1 - RANGE.0)).clamp(0.0, 1.0) as f32
}

/// The held peak: the highest of the last `HOLD_SECONDS`, per side.
#[derive(Debug, Clone, Copy, Default)]
struct Hold {
    peak: [f32; 2],
    since: [f64; 2],
}

fn held(ui: &Ui, id: Id, level: &Level) -> [f32; 2] {
    let now = ui.input(|i| i.time);
    ui.ctx().data_mut(|d| {
        let hold = d.get_temp_mut_or_default::<Hold>(id);
        for side in 0..2 {
            let (Some(peak), Some(since), Some(&now_peak)) =
                (hold.peak.get_mut(side), hold.since.get_mut(side), level.peak.get(side))
            else {
                continue;
            };
            if now_peak >= *peak || now - *since > HOLD_SECONDS {
                *peak = now_peak;
                *since = now;
            }
        }
        hold.peak
    })
}

/// Text for screen readers and tests: the louder side's peak, and the clip light.
fn describe(name: &str, level: &Level) -> String {
    let peak = level.peak[0].max(level.peak[1]);
    let db = 20.0 * f64::from(peak).log10();
    let value = if db.is_finite() && db > RANGE.0 { format!("{db:.1} dB") } else { "silent".into() };
    let clipped = if level.clipped { ", clipped" } else { "" };
    format!("{name} level: {value}{clipped}")
}

/// Draws a meter of `level` in `rect`: standing up (bars bottom to top, the clip light on top)
/// when `rect` is taller than wide, lying down (left to right, the light on the right)
/// otherwise. A click on it turns the clip light off.
pub fn show(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    source: Source,
    level: &Level,
    actions: &mut Vec<Action>,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 || !rect.intersects(painter.clip_rect()) {
        return;
    }
    let id = Id::new(("meter", source));
    let response = ui.interact(rect.intersect(painter.clip_rect()), id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, describe(name, level)));
    if response.clicked() && level.clipped {
        actions.push(match source {
            Source::Main => Action::new("mixer.resetClip", json!({ "main": true })),
            Source::Track(track) => Action::new("mixer.resetClip", json!({ "track": track.0 })),
        });
    }
    let hold = held(ui, id, level);
    let upright = rect.height() > rect.width();
    let (light, bars) = if upright {
        let light = Rect::from_min_size(rect.min, Vec2::new(rect.width(), CLIP));
        (light, Rect::from_min_max(Pos2::new(rect.left(), light.bottom() + 1.0), rect.max))
    } else {
        let light = Rect::from_min_max(Pos2::new(rect.right() - CLIP, rect.top()), rect.max);
        (light, Rect::from_min_max(rect.min, Pos2::new(light.left() - 1.0, rect.bottom())))
    };
    painter.rect_filled(light, 0.0, if level.clipped { theme::recording() } else { theme::window_background() });
    let hot_from = ((HOT_DB - RANGE.0) / (RANGE.1 - RANGE.0)) as f32;
    for side in 0..2 {
        let offset = side as f32 * (BAR + GAP);
        // The bar's track, and a part of it from 0 to `to` (0 … 1).
        let bar = if upright {
            Rect::from_min_size(Pos2::new(bars.left() + offset, bars.top()), Vec2::new(BAR, bars.height()))
        } else {
            Rect::from_min_size(Pos2::new(bars.left(), bars.top() + offset), Vec2::new(bars.width(), BAR))
        };
        let part = |from: f32, to: f32| {
            if upright {
                Rect::from_x_y_ranges(
                    bar.x_range(),
                    egui::Rangef::new(bar.bottom() - bar.height() * to, bar.bottom() - bar.height() * from),
                )
            } else {
                Rect::from_x_y_ranges(
                    egui::Rangef::new(bar.left() + bar.width() * from, bar.left() + bar.width() * to),
                    bar.y_range(),
                )
            }
        };
        painter.rect_filled(bar, 0.0, theme::window_background());
        let peak = fraction(level.peak.get(side).copied().unwrap_or(0.0));
        let rms = fraction(level.rms.get(side).copied().unwrap_or(0.0));
        // The peak dimmer, the RMS level in the full colour over it.
        for (to, alpha) in [(peak, 0.45), (rms, 1.0)] {
            if to <= 0.0 {
                continue;
            }
            painter.rect_filled(part(0.0, to.min(hot_from)), 0.0, theme::meter_normal().gamma_multiply(alpha));
            if to > hot_from {
                painter.rect_filled(part(hot_from, to), 0.0, theme::meter_hot().gamma_multiply(alpha));
            }
        }
        let hold = fraction(hold.get(side).copied().unwrap_or(0.0));
        if hold > 0.0 {
            let color = if hold > hot_from { theme::meter_hot() } else { theme::meter_normal() };
            let line = if upright {
                let y = (bar.bottom() - bar.height() * hold).max(bar.top());
                Rect::from_x_y_ranges(bar.x_range(), egui::Rangef::new(y, y + 1.0))
            } else {
                let x = (bar.left() + bar.width() * hold).min(bar.right());
                Rect::from_x_y_ranges(egui::Rangef::new(x - 1.0, x), bar.y_range())
            };
            painter.rect_filled(line, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_the_volume_controls() {
        assert_eq!(fraction(0.0), 0.0);
        assert_eq!(fraction(f32::NAN), 0.0);
        assert!((fraction(1.0) - 70.0 / 76.0).abs() < 1e-6);
        assert_eq!(fraction(4.0), 1.0);
        // -3 dB is where the top 12 % begin.
        let hot = fraction(10f32.powf(-3.0 / 20.0));
        assert!((hot - 0.88).abs() < 0.005, "{hot}");
    }

    #[test]
    fn the_description_names_the_louder_side_and_the_clip() {
        let level = Level { peak: [0.25, 0.5], rms: [0.1, 0.1], clipped: false };
        assert_eq!(describe("Drums", &level), "Drums level: -6.0 dB");
        assert_eq!(describe("Main", &Level { clipped: true, ..Level::default() }), "Main level: silent, clipped");
    }
}
