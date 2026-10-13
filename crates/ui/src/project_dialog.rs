//! The project dialog (File › New or open, Ctrl+T): a new blank project, a new one from a
//! template, or a project to open, typed as a path or picked from the recent ones. And the save
//! prompt, which asks for a name and a folder when a project is saved for the first time, saved
//! under another name, or saved as a template. Both are drawn over the window; Escape closes them.

use std::path::{Path, PathBuf};

use egui::{Align2, Id, Key, Pos2, Rect, Sense, Ui, Vec2};

use crate::motion;
use crate::theme::{self, PAD, ROW};
use crate::widgets;

const WIDTH: f32 = 620.0;
const HEIGHT: f32 = 380.0;
const SAVE_WIDTH: f32 = 420.0;
const LINE: f32 = 20.0;
const BUTTON: Vec2 = Vec2::new(96.0, 18.0);

/// What the project dialog keeps between frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogState {
    /// The path typed into the open field.
    pub path: String,
    /// The open field takes the keys when the dialog appears (File › Open, Ctrl+O).
    pub focus_path: bool,
}

/// What the dialog lists: the recent projects, the templates and where they are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    pub recent: Vec<PathBuf>,
    pub templates: Vec<PathBuf>,
    pub templates_folder: String,
}

/// What the user chose in the project dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Open(PathBuf),
    NewBlank,
    NewFromTemplate(PathBuf),
    Close,
}

/// Draws the project dialog over `area`, above everything else.
pub fn show(ui: &Ui, area: Rect, state: &mut DialogState, lists: &Lists) -> Option<Choice> {
    egui::Area::new(Id::new("project-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw(ui, area, state, lists)
        })
        .inner
}

fn draw(ui: &mut Ui, area: Rect, state: &mut DialogState, lists: &Lists) -> Option<Choice> {
    let mut choice = None;
    let panel = panel(ui, area, Vec2::new(WIDTH, HEIGHT), "New or open a project", &mut choice);
    let painter = ui.painter_at(area);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let half = (panel.width() - 3.0 * PAD) / 2.0;
    let left = Rect::from_min_size(Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD), Vec2::new(half, 0.0));
    let right = Rect::from_min_size(Pos2::new(left.right() + PAD, left.top()), Vec2::new(half, 0.0));
    let bottom = panel.bottom() - PAD;

    // New: blank, or from a template.
    heading(&painter, left, "New");
    let mut y = left.top() + LINE;
    let blank = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(half, LINE));
    if row(ui, &painter, blank, "New blank project", "4 tracks, 2 returns, 8 scenes", "New blank project").clicked() {
        choice = Some(Choice::NewBlank);
    }
    y = blank.bottom() + PAD;
    heading(&painter, Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(half, 0.0)), "From a template");
    y += LINE;
    let note = Rect::from_min_max(Pos2::new(left.left(), bottom - 28.0), Pos2::new(left.right(), bottom));
    let list = Rect::from_min_max(Pos2::new(left.left(), y), Pos2::new(left.right(), note.top() - 4.0));
    if lists.templates.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(half, LINE));
        let text = "No templates yet: File › Save as template";
        widgets::text_in(&painter, empty, Align2::LEFT_CENTER, text, small.clone(), theme::text_hint());
    } else {
        scrolled(ui, list, "templates", |ui| {
            for path in &lists.templates {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                let painter = ui.painter().clone();
                let label = format!("Template {}", stem(path));
                if row(ui, &painter, r, &stem(path), &kind(path), &label).clicked() {
                    choice = Some(Choice::NewFromTemplate(path.clone()));
                }
            }
        });
    }
    let folder = format!("Templates in {}; the folder is set in Settings › General", lists.templates_folder);
    let lines = widgets::fitted(&painter, &folder, small.clone(), theme::text_hint(), note.width());
    painter.galley(Pos2::new(note.left(), note.center().y - lines.size().y / 2.0), lines, theme::text_hint());

    // Open: a path, or a recent project.
    heading(&painter, right, "Open");
    let y = right.top() + LINE;
    let open_button = Rect::from_min_size(Pos2::new(right.right() - 56.0, y + 1.0), Vec2::new(56.0, BUTTON.y));
    let field = Rect::from_min_max(Pos2::new(right.left(), y), Pos2::new(open_button.left() - 4.0, y + 20.0));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut state.path).hint_text("Path to a .suhina or .als file").font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Project path"));
    if std::mem::take(&mut state.focus_path) {
        edit.request_focus();
    }
    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if button(ui, &painter, open_button, "Open", "Open the project at the path") || entered {
        let path = state.path.trim().trim_matches('"');
        if !path.is_empty() {
            choice = Some(Choice::Open(PathBuf::from(path)));
        }
    }
    let recent_top = field.bottom() + PAD;
    heading(&painter, Rect::from_min_size(Pos2::new(right.left(), recent_top), Vec2::new(half, 0.0)), "Recent");
    let list = Rect::from_min_max(Pos2::new(right.left(), recent_top + LINE), Pos2::new(right.right(), bottom));
    if lists.recent.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(half, LINE));
        widgets::text_in(&painter, empty, Align2::LEFT_CENTER, "Nothing opened yet", small, theme::text_hint());
    } else {
        scrolled(ui, list, "recent", |ui| {
            for path in &lists.recent {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                let painter = ui.painter().clone();
                let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                let label = format!("Recent {}", stem(path));
                if row(ui, &painter, r, &stem(path), &folder, &label).clicked() {
                    choice = Some(Choice::Open(path.clone()));
                }
            }
        });
    }
    if choice.is_none() && ui.input(|i| i.key_pressed(Key::Escape)) {
        choice = Some(Choice::Close);
    }
    choice
}

/// What the save prompt keeps between frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePrompt {
    /// A template is a copy in the templates folder; the project stays with its own file.
    pub template: bool,
    pub name: String,
    pub folder: String,
    /// The name field takes the keys when the prompt appears.
    pub focus: bool,
}

/// What the user did with the save prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveChoice {
    Save(PathBuf),
    Cancel,
}

/// Draws the save prompt over `area`.
pub fn show_save(ui: &Ui, area: Rect, prompt: &mut SavePrompt) -> Option<SaveChoice> {
    egui::Area::new(Id::new("save-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_save(ui, area, prompt)
        })
        .inner
}

fn draw_save(ui: &mut Ui, area: Rect, prompt: &mut SavePrompt) -> Option<SaveChoice> {
    let mut closed: Option<SaveChoice> = None;
    let title = if prompt.template { "Save as template" } else { "Save project" };
    let panel = panel(ui, area, Vec2::new(SAVE_WIDTH, ROW + 4.0 * LINE + 2.0 * PAD), title, &mut closed);
    let painter = ui.painter_at(area);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let label_width = 52.0;
    let mut y = panel.top() + ROW + PAD;
    let mut field = |ui: &mut Ui, label: &str, value: &mut String, focus: bool| {
        let r = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
        widgets::text_in(&painter, r, Align2::LEFT_CENTER, label, small.clone(), theme::text_secondary());
        let input = Rect::from_min_max(Pos2::new(r.left() + label_width, r.top()), r.max);
        let edit = ui.put(input, egui::TextEdit::singleline(value).font(small.clone()));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
        if focus {
            edit.request_focus();
        }
        y += LINE + 4.0;
        edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
    };
    let focus = std::mem::take(&mut prompt.focus);
    let mut entered = field(ui, "Name", &mut prompt.name, focus);
    entered |= field(ui, "Folder", &mut prompt.folder, false);
    let name = prompt.name.trim();
    let path = (!name.is_empty() && !name.contains(['/', '\\', ':']))
        .then(|| Path::new(prompt.folder.trim()).join(format!("{name}.{}", suhina_session::PROJECT_EXTENSION)));
    let note = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
    let text = match &path {
        Some(path) if path.exists() => format!("{} is there already and will be replaced", path.display()),
        Some(path) => path.display().to_string(),
        None => "A name without / \\ or :".to_string(),
    };
    let color = if path.as_ref().is_some_and(|p| p.exists()) { theme::recording() } else { theme::text_hint() };
    widgets::text_in(&painter, note, Align2::LEFT_CENTER, &text, small, color);
    let save = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    let cancel = Rect::from_min_size(Pos2::new(save.left() - 4.0 - BUTTON.x, save.top()), BUTTON);
    let saved = button(ui, &painter, save, "Save", "Save");
    if button(ui, &painter, cancel, "Cancel", "Cancel saving") || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(SaveChoice::Cancel);
    }
    if closed.is_some() {
        return Some(SaveChoice::Cancel);
    }
    match path {
        Some(path) if saved || entered => Some(SaveChoice::Save(path)),
        _ => None,
    }
}

/// A panel in the middle of `area` with a title row and a Close button, the area behind dimmed
/// and its clicks swallowed. Returns the panel; `closed` is set when Close is clicked.
fn panel<T: CloseChoice>(ui: &Ui, area: Rect, size: Vec2, title: &str, closed: &mut Option<T>) -> Rect {
    let behind = ui.interact(area, Id::new(("dialog-behind", title)), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(0.6));
    let size = Vec2::new(size.x.min(area.width() - 2.0 * PAD), size.y.min(area.height() - 2.0 * PAD));
    let rect = Rect::from_center_size(area.center(), size);
    painter.rect_filled(rect, 0.0, theme::panel_background());
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(bar, 0.0, theme::control_background());
    let font = theme::ui_font(theme::TEXT);
    widgets::text_in(
        &painter,
        bar.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        title,
        font,
        theme::text_primary(),
    );
    let close = Rect::from_min_size(Pos2::new(bar.right() - 52.0, bar.top()), Vec2::new(52.0, ROW));
    let response = widgets::button_area(ui, close, Id::new(("dialog-close", title)), format!("Close {title}"));
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(close, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(&painter, close, Align2::CENTER_CENTER, "Close", small, theme::text_primary());
    if response.clicked() {
        *closed = Some(T::close());
    }
    rect
}

/// The choice that closes a dialog.
trait CloseChoice {
    fn close() -> Self;
}

impl CloseChoice for Choice {
    fn close() -> Self {
        Choice::Close
    }
}

impl CloseChoice for SaveChoice {
    fn close() -> Self {
        SaveChoice::Cancel
    }
}

fn heading(painter: &egui::Painter, at: Rect, text: &str) {
    let r = Rect::from_min_size(at.min, Vec2::new(at.width(), LINE));
    widgets::text_in(painter, r, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
    painter.hline(r.x_range(), r.bottom() - 2.5, theme::hairline(theme::border()));
}

/// A row to click: a name, and a note on the right.
fn row(ui: &Ui, painter: &egui::Painter, rect: Rect, name: &str, note: &str, label: &str) -> egui::Response {
    let response = widgets::button_area(ui, rect, Id::new(("dialog-row", label)), label);
    let fill = motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, fill);
    let inner = rect.shrink2(Vec2::new(PAD, 0.0));
    let font = theme::ui_font(theme::TEXT);
    let name_width = widgets::text_width(ui, name, font.clone()).min(inner.width() * 0.6);
    widgets::text_in(painter, inner, Align2::LEFT_CENTER, name, font, theme::text_primary());
    let rest = Rect::from_min_max(Pos2::new(inner.left() + name_width + 8.0, inner.top()), inner.max);
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(painter, rest, Align2::RIGHT_CENTER, note, small, theme::text_hint());
    response
}

fn button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str) -> bool {
    let response = widgets::button_area(ui, rect, Id::new(("dialog-button", label)), label);
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, small, theme::text_primary());
    response.clicked()
}

fn scrolled(ui: &mut Ui, rect: Rect, salt: &str, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
    });
}

fn stem(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// What a template file is, by its extension.
fn kind(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("als") => "Live Set".into(),
        _ => "Suhina project".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn dialog(lists: Lists) -> Harness<'static, (DialogState, Option<Choice>)> {
        Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
            move |ui, (state, chosen): &mut (DialogState, Option<Choice>)| {
                let area = ui.max_rect();
                if let Some(choice) = show(ui, area, state, &lists) {
                    *chosen = Some(choice);
                }
            },
            (DialogState::default(), None),
        )
    }

    #[test]
    fn the_dialog_makes_new_projects_and_opens_recent_ones() {
        let lists = Lists {
            recent: vec![PathBuf::from("songs/Night.suhina")],
            // `Groove.locr` was saved before October 2026; it is listed like the others.
            templates: vec![
                PathBuf::from("templates/Beat.suhina"),
                PathBuf::from("templates/Groove.locr"),
                PathBuf::from("templates/Old.als"),
            ],
            templates_folder: "templates".into(),
        };
        let mut harness = dialog(lists);
        harness.run();
        harness.get_by_label("New blank project").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewBlank));
        harness.get_by_label("Template Beat").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewFromTemplate("templates/Beat.suhina".into())));
        harness.get_by_label("Template Groove").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewFromTemplate("templates/Groove.locr".into())));
        harness.get_by_label("Recent Night").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Open("songs/Night.suhina".into())));
    }

    #[test]
    fn a_typed_path_is_opened_and_escape_closes() {
        let mut harness = dialog(Lists::default());
        harness.run();
        harness.get_by_label("Project path").click();
        harness.run();
        harness.get_by_label("Project path").type_text("\"C:/songs/a.als\"");
        harness.run();
        harness.get_by_label("Open the project at the path").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Open("C:/songs/a.als".into())));
        harness.state_mut().1 = None;
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Close));
    }

    #[test]
    fn the_save_prompt_makes_the_path_from_name_and_folder() {
        let prompt = SavePrompt { template: false, name: String::new(), folder: "songs".into(), focus: true };
        let mut harness = Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
            |ui, (prompt, chosen): &mut (SavePrompt, Option<SaveChoice>)| {
                let area = ui.max_rect();
                if let Some(choice) = show_save(ui, area, prompt) {
                    *chosen = Some(choice);
                }
            },
            (prompt, None),
        );
        harness.run();
        // Without a name there is nothing to save.
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state().1, None);
        harness.get_by_label("Name").click();
        harness.run();
        harness.get_by_label("Name").type_text("Night drive");
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state().1, Some(SaveChoice::Save(Path::new("songs").join("Night drive.suhina"))));
        harness.get_by_label("Cancel saving").click();
        harness.run();
        assert_eq!(harness.state().1, Some(SaveChoice::Cancel));
    }
}
