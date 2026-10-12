//! The main window: control bar, the Session or Arrangement view and the status bar around an
//! open session.
//! Every change goes through `Session::execute` (AGENTS.md rule 6); the window only reads the
//! project and the engine's monitor to draw them.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Align2, Id, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use loopwerk_engine::model::TrackId;
use loopwerk_session::Session;
use serde_json::json;

use crate::arrangement_view::{self, ArrangementViewState};
use crate::grid::{Grid, SlotState};
use crate::motion;
use crate::session_view::{self, SessionViewState};
use crate::theme::{self, PAD};
use crate::timeline::Timeline;
use crate::widgets::{self, Action, Drag};

/// Launch quantisation choices, as `launcher.setQuantization` takes them.
const QUANTIZATION: [&str; 14] = [
    "none", "8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32",
];
/// The engine's quantisation before anything is set.
const DEFAULT_QUANTIZATION: &str = "1 bar";

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);

/// The main view; Tab switches between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Session,
    Arrangement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Hint(String),
    Done(String),
    Error(String),
}

/// The window's state.
pub struct App {
    session: Option<Session>,
    shown: View,
    view: SessionViewState,
    arrangement: ArrangementViewState,
    status: Status,
    quantization: &'static str,
    /// Where each column's playing clip started, in beats, to show its progress.
    launches: HashMap<TrackId, (usize, f64)>,
    tempo_drag: Option<f64>,
    open_request: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            session: None,
            shown: View::Session,
            view: SessionViewState::default(),
            arrangement: ArrangementViewState::default(),
            status: Status::Hint("Drop a Live Set (.als) on the window to open it".into()),
            quantization: DEFAULT_QUANTIZATION,
            launches: HashMap::new(),
            tempo_drag: None,
            open_request: None,
        }
    }

    /// Shows `session`, replacing the one open before. `warnings` are what the importer skipped.
    pub fn set_session(&mut self, session: Session, warnings: &[String]) {
        let name = session
            .source()
            .and_then(|p| p.file_name())
            .map_or("the project".into(), |n| n.to_string_lossy().to_string());
        self.status = match warnings.len() {
            0 => Status::Done(format!("Opened {name}")),
            1 => Status::Hint(format!("Opened {name}: {}", warnings.first().map_or("", String::as_str))),
            n => Status::Hint(format!(
                "Opened {name} with {n} warnings; the first: {}",
                warnings.first().map_or("", String::as_str)
            )),
        };
        if session.player().is_none() {
            self.status = Status::Hint(format!("Opened {name} without audio output; launching needs one"));
        }
        self.session = Some(session);
        self.view = SessionViewState::default();
        self.arrangement = ArrangementViewState::default();
        self.launches.clear();
        self.quantization = DEFAULT_QUANTIZATION;
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn view(&self) -> View {
        self.shown
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Turns animations on or off (a setting; on by default).
    pub fn set_animations(ctx: &egui::Context, on: bool) {
        motion::set_enabled(ctx, on);
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// A file the user dropped on the window, for the app to open.
    pub fn take_open_request(&mut self) -> Option<PathBuf> {
        self.open_request.take()
    }

    /// Runs one command and shows its outcome in the status bar.
    pub fn run(&mut self, action: &Action) {
        let Some(session) = self.session.as_mut() else { return };
        match session.execute(action.id, &action.params) {
            Ok(_) => {
                if action.id == "launcher.setQuantization"
                    && let Some(value) = action.params["value"].as_str()
                    && let Some(known) = QUANTIZATION.iter().find(|q| **q == value)
                {
                    self.quantization = known;
                }
                if let Some(done) = describe(action) {
                    self.status = Status::Done(done);
                }
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Draws the window into `ui` and runs what the user asked for.
    pub fn ui(&mut self, ui: &mut Ui) {
        let mut actions = Vec::new();
        self.dropped_files(ui);
        self.shortcuts(ui, &mut actions);

        let grid = self.session.as_ref().map(|s| Grid::build(s.project(), s.player().map(|p| p.monitor())));
        let progress = grid.as_ref().map(|g| self.progress(g)).unwrap_or_default();

        egui::Panel::top("control-bar")
            .exact_size(theme::CONTROL_BAR)
            .frame(egui::Frame::NONE.fill(theme::SURFACE_1))
            .show(ui, |ui| self.control_bar(ui, grid.as_ref(), &mut actions));
        egui::Panel::bottom("status-bar")
            .exact_size(theme::STATUS_BAR)
            .frame(egui::Frame::NONE.fill(theme::SURFACE_1))
            .show(ui, |ui| self.status_bar(ui, grid.as_ref()));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::LINE)).show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            // A 1 px gap shows the line colour between the bars and the view.
            let inner = Rect::from_min_max(area.min + Vec2::new(0.0, 1.0), area.max - Vec2::new(0.0, 1.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| match (&grid, self.shown) {
                (Some(grid), View::Session) => session_view::show(ui, grid, &progress, &mut self.view, &mut actions),
                (Some(_), View::Arrangement) => {
                    if let Some(session) = &self.session {
                        let timeline = Timeline::build(session.project(), session.player().map(|p| p.monitor()));
                        arrangement_view::show(ui, &timeline, &mut self.arrangement, &mut actions);
                    }
                }
                (None, _) => empty(ui),
            });
        });

        for action in &actions {
            self.run(action);
        }
        if grid.is_some_and(|g| {
            g.running || g.columns.iter().any(|c| c.stop_queued || c.slots.iter().any(|s| s.state == SlotState::Queued))
        }) {
            ui.ctx().request_repaint();
        }
    }

    fn dropped_files(&mut self, ui: &Ui) {
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("als")) {
                self.open_request = Some(path);
            } else {
                self.status = Status::Error(format!("{} is not a Live Set (.als)", path.display()));
            }
        }
    }

    fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        if self.session.is_none() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        ui.input_mut(|i| {
            if i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_Y) {
                actions.push(Action::new("edit.redo", json!({})));
            } else if i.consume_shortcut(&UNDO) {
                actions.push(Action::new("edit.undo", json!({})));
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                self.shown = match self.shown {
                    View::Session => View::Arrangement,
                    View::Arrangement => View::Session,
                };
            }
            if i.consume_key(Modifiers::NONE, Key::Space) {
                let running = self.session.as_ref().and_then(|s| s.player()).is_some_and(|p| p.monitor().running());
                actions.push(Action::new(if running { "transport.stop" } else { "transport.play" }, json!({})));
            }
        });
    }

    /// How far each column's playing clip is, from the beat it was first seen playing.
    fn progress(&mut self, grid: &Grid) -> Vec<Option<f32>> {
        grid.columns
            .iter()
            .map(|column| {
                let playing = column.slots.iter().position(|s| s.state == SlotState::Playing);
                let Some(slot) = playing else {
                    self.launches.remove(&column.track);
                    return None;
                };
                let started = match self.launches.get(&column.track) {
                    Some((s, beat)) if *s == slot && *beat <= grid.beat => *beat,
                    _ => {
                        self.launches.insert(column.track, (slot, grid.beat));
                        grid.beat
                    }
                };
                let length = column.slots.get(slot).map_or(0.0, |s| s.length);
                (length > 0.0).then(|| (((grid.beat - started) % length) / length) as f32)
            })
            .collect()
    }

    fn control_bar(&mut self, ui: &mut Ui, grid: Option<&Grid>, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let Some(grid) = grid else {
            widgets::text_in(
                &painter,
                bar.shrink2(Vec2::new(PAD, 0.0)),
                Align2::LEFT_CENTER,
                "Loopwerk",
                theme::ui_font(theme::TEXT),
                theme::TEXT_SECONDARY,
            );
            return;
        };
        let row = |x: f32, width: f32| {
            Rect::from_min_size(Pos2::new(x, bar.center().y - theme::ROW / 2.0), Vec2::new(width, theme::ROW))
        };

        // Left: tempo, time signature, launch quantisation.
        let mut x = bar.left() + PAD;
        let tempo_rect = row(x, 56.0);
        let tempo_shown = self.tempo_drag.unwrap_or(grid.tempo_bpm);
        let response = widgets::value_area(ui, tempo_rect, Id::new("tempo"), "Tempo", format!("{tempo_shown:.2}"));
        match widgets::drag_value(ui, &response, &mut self.tempo_drag, grid.tempo_bpm, 0.1, (20.0, 999.0), 120.0) {
            Drag::Done(bpm) => {
                actions.push(Action::new("transport.setTempo", json!({ "bpm": (bpm * 100.0).round() / 100.0 })))
            }
            Drag::Moving(_) | Drag::None => {}
        }
        painter.rect_filled(
            tempo_rect,
            0.0,
            motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response)),
        );
        let mono = theme::mono_font(theme::TEXT);
        widgets::text_in(
            &painter,
            tempo_rect.shrink2(Vec2::new(PAD, 0.0)),
            Align2::RIGHT_CENTER,
            &format!("{tempo_shown:.2}"),
            mono.clone(),
            theme::TEXT_PRIMARY,
        );
        x = tempo_rect.right() + PAD;
        let session = self.session.as_ref();
        let signature = session.map(|s| s.project().transport.time_signature);
        let signature = signature.map_or_else(String::new, |ts| format!("{} / {}", ts.numerator, ts.denominator));
        let ts_rect = row(x, 40.0);
        widgets::text_in(&painter, ts_rect, Align2::CENTER_CENTER, &signature, mono.clone(), theme::TEXT_SECONDARY);
        x = ts_rect.right() + PAD;
        let quant_rect = row(x, 64.0);
        let mut chosen = self.quantization;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            egui::ComboBox::from_id_salt("quantization")
                .width(quant_rect.width())
                .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::TEXT_VALUE))
                .show_ui(ui, |ui| {
                    for q in QUANTIZATION {
                        ui.selectable_value(&mut chosen, q, egui::RichText::new(q).font(mono.clone()));
                    }
                });
        });
        if chosen != self.quantization {
            actions.push(Action::new("launcher.setQuantization", json!({ "value": chosen })));
        }

        // Centre: play, stop, position.
        let transport_width = 28.0 * 2.0 + 1.0 + PAD + 64.0;
        let mut x = bar.center().x - transport_width / 2.0;
        let play = row(x, 28.0);
        let response = widgets::button_area(ui, play, Id::new("play"), "Play");
        if response.clicked() {
            actions.push(Action::new("transport.play", json!({})));
        }
        let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
        let hover = motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response));
        painter.rect_filled(play, 0.0, motion::mix(hover, theme::SURFACE_3, running));
        let glyph = motion::mix(theme::TEXT_PRIMARY, theme::ACCENT, running);
        let c = play.center();
        painter.add(egui::Shape::convex_polygon(
            vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)],
            glyph,
            egui::Stroke::NONE,
        ));
        x = play.right() + 1.0;
        let stop = row(x, 28.0);
        let response = widgets::button_area(ui, stop, Id::new("stop"), "Stop");
        if response.clicked() {
            actions.push(Action::new("transport.stop", json!({})));
        }
        painter.rect_filled(
            stop,
            0.0,
            motion::mix(theme::SURFACE_2, theme::HOVER_ON_SURFACE_2, widgets::hover(ui, &response)),
        );
        painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, theme::TEXT_PRIMARY);
        x = stop.right() + PAD;
        let position = row(x, 64.0);
        painter.rect_filled(position, 0.0, theme::SURFACE_0);
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            mono,
            theme::TEXT_PRIMARY,
        );

        // Right: the view switch; the current view is underlined.
        let mut right = bar.right() - PAD;
        let mut underline_at = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Session")] {
            let color = if self.shown == view { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("view", name)), format!("{name} view"));
            if response.clicked() {
                self.shown = view;
            }
            let color = motion::mix(color, theme::TEXT_PRIMARY, widgets::hover(ui, &response));
            let pos = Pos2::new(rect.left(), rect.center().y - size.y / 2.0);
            painter.galley(pos, galley, color);
            if self.shown == view {
                underline_at = Some((pos.x, pos.y + size.y + 1.0, size.x));
            }
            right = rect.left() - 12.0;
        }
        // The underline slides to the view that was picked.
        if let Some((x, y, width)) = underline_at {
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::new("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::new("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)), 0.0, theme::TEXT_PRIMARY);
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::LINE));
    }

    fn status_bar(&self, ui: &mut Ui, grid: Option<&Grid>) {
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = match &self.status {
            Status::Hint(t) => (t.as_str(), theme::TEXT_SECONDARY),
            Status::Done(t) => (t.as_str(), theme::TEXT_SECONDARY),
            Status::Error(t) => (t.as_str(), theme::RECORDING),
        };
        let right = grid.map(|g| {
            let tracks = g.columns.iter().filter(|c| c.kind != crate::grid::ColumnKind::Return).count();
            format!("{tracks} tracks · {} scenes", g.scenes.len())
        });
        let right_width = if right.is_some() { 140.0 } else { 0.0 };
        let left =
            Rect::from_min_max(bar.min + Vec2::new(PAD, 0.0), Pos2::new(bar.right() - right_width - PAD, bar.bottom()));
        widgets::text_in(&painter, left, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
        if let Some(right) = right {
            let rect =
                Rect::from_min_max(Pos2::new(bar.right() - right_width, bar.top()), bar.max - Vec2::new(PAD, 0.0));
            widgets::text_in(
                &painter,
                rect,
                Align2::RIGHT_CENTER,
                &right,
                theme::ui_font(theme::TEXT_SMALL),
                theme::TEXT_TERTIARY,
            );
        }
    }
}

/// The window with no project open.
fn empty(ui: &mut Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::SURFACE_0);
    painter.text(
        area.center(),
        Align2::CENTER_CENTER,
        "Drop a Live Set (.als) here to open it",
        theme::ui_font(theme::TEXT),
        theme::TEXT_TERTIARY,
    );
}

/// What the status bar says after a command ran, or nothing for silent ones.
fn describe(action: &Action) -> Option<String> {
    let p = &action.params;
    Some(match action.id {
        "clip.launch" => format!("Launched scene {} on track {}", p["scene"], p["track"]),
        "scene.launch" => format!("Launched scene {}", p["scene"]),
        "track.stop" => format!("Stopping track {}", p["track"]),
        "launcher.stopAll" => "Stopping all clips".into(),
        "launcher.setQuantization" => format!("Launch quantisation {}", p["value"].as_str().unwrap_or_default()),
        "transport.setTempo" => format!("Tempo {}", p["bpm"]),
        "edit.undo" => "Undone".into(),
        "edit.redo" => "Redone".into(),
        _ => return None,
    })
}
