//! SDK drawing programs are evaluated only on the UI thread.
use crate::{detail_view::GraphParameter, widgets::Action};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use serde_json::json;
use std::{cell::RefCell, sync::Arc};
use suhina_device_sdk::{DeviceGraph, ui::*};
use suhina_engine::model::TrackId;

pub(crate) struct Parsed {
    pub graph: DeviceGraph,
    pub canvas: Option<Arc<DeviceUi>>,
}
thread_local! {static GRAPHS:RefCell<Vec<(Vec<u8>,Arc<Parsed>)>>=const{RefCell::new(Vec::new())};}
pub(crate) fn graph(bytes: &[u8]) -> Option<Arc<Parsed>> {
    if let Some(found) =
        GRAPHS.with(|cache| cache.borrow().iter().find(|(key, _)| key == bytes).map(|(_, v)| v.clone()))
    {
        return Some(found);
    }
    let mut graph = DeviceGraph::from_sd(std::str::from_utf8(bytes).ok()?).ok()?;
    let canvas = graph.ui.take().map(Arc::new);
    let parsed = Arc::new(Parsed { graph, canvas });
    GRAPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push((bytes.to_vec(), parsed.clone()));
        while cache.len() > 64 || cache.iter().map(|(b, _)| b.len()).sum::<usize>() > 16 * 1024 * 1024 {
            cache.remove(0);
        }
    });
    Some(parsed)
}
#[derive(Clone)]
struct Loaded {
    source: Arc<DeviceUi>,
    images: Vec<Result<egui::TextureHandle, String>>,
}
fn images(ctx: &egui::Context, source: &Arc<DeviceUi>) -> Arc<Loaded> {
    let key = egui::Id::new("sd-image-cache");
    let mut cache = ctx.data_mut(|d| d.get_temp::<Vec<Arc<Loaded>>>(key)).unwrap_or_default();
    if let Some(found) = cache.iter().find(|v| Arc::ptr_eq(&v.source, source)) {
        return found.clone();
    }
    let images = source
        .assets
        .iter()
        .map(|a| {
            a.decode_rgba()
                .map(|(pixels, size)| {
                    ctx.load_texture(
                        &a.id,
                        egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
                        egui::TextureOptions {
                            wrap_mode: egui::TextureWrapMode::Repeat,
                            ..egui::TextureOptions::LINEAR
                        },
                    )
                })
                .map_err(|e| e.to_string())
        })
        .collect();
    let loaded = Arc::new(Loaded { source: source.clone(), images });
    cache.push(loaded.clone());
    if cache.len() > 8 {
        cache.remove(0);
    }
    ctx.data_mut(|d| d.insert_temp(key, cache));
    loaded
}
fn color(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
fn normalized(p: &GraphParameter, log: bool) -> f64 {
    if log && p.min > 0.0 {
        ((p.value.max(p.min).ln() - p.min.ln()) / (p.max.ln() - p.min.ln())).clamp(0.0, 1.0)
    } else if p.max > p.min {
        ((p.value - p.min) / (p.max - p.min)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn paint(painter: &egui::Painter, drawings: &[Draw], origin: Pos2, parameters: &[GraphParameter], loaded: &Loaded) {
    let lookup = |id: &str, norm: bool, log: bool| {
        parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
    };
    let value = |v: &UiValue| v.evaluate(&lookup);
    let point = |p: &[UiValue; 2]| origin + Vec2::new(value(&p[0]), value(&p[1]));
    let rectangle = |r: &[UiValue; 4]| {
        Rect::from_min_size(
            origin + Vec2::new(value(&r[0]), value(&r[1])),
            Vec2::new(value(&r[2]).clamp(0.0, 8192.0), value(&r[3]).clamp(0.0, 8192.0)),
        )
    };
    for draw in drawings {
        match draw {
            Draw::Rect { rect, color: c, radius } => {
                painter.rect_filled(rectangle(rect), *radius, color(*c));
            }
            Draw::Circle { center, radius, color: c } => {
                painter.circle_filled(point(center), value(radius).clamp(0.0, 8192.0), color(*c));
            }
            Draw::Line { from, to, color: c, width } => {
                painter.line_segment([point(from), point(to)], Stroke::new(*width, color(*c)));
            }
            Draw::Path { points, color: c, width, closed, fill } => {
                let points = points.iter().map(point).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    painter.add(egui::Shape::convex_polygon(points, color(*fill), Stroke::new(*width, color(*c))));
                } else if *closed {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(*width, color(*c))));
                } else {
                    painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
                }
            }
            Draw::Arc { center, radius, start, end, color: c, width } => {
                let center = point(center);
                let radius = value(radius).clamp(0.0, 8192.0);
                let start = value(start);
                let end = value(end);
                let points = (0..=64)
                    .map(|i| {
                        let a = start + (end - start) * i as f32 / 64.0;
                        center + Vec2::new(a.cos(), a.sin()) * radius
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
            }
            Draw::Text { position, text, color: c, size, parameter, decimals } => {
                let label = parameter
                    .as_ref()
                    .and_then(|id| parameters.iter().find(|p| p.id == *id))
                    .map_or_else(|| text.clone(), |p| format!("{text}{:.*} {}", *decimals, p.value, p.unit));
                painter.text(
                    point(position),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(*size),
                    color(*c),
                );
            }
            Draw::Gradient { rect, colors } => {
                let r = rectangle(rect);
                let mut mesh = egui::Mesh::default();
                for (pos, c) in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().zip(colors)
                {
                    mesh.colored_vertex(pos, color(*c));
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(egui::Shape::mesh(mesh));
            }
            Draw::Image { asset, rect, uv, tint, tile_size, fit } => {
                if let Some(i) = loaded.source.assets.iter().position(|a| a.id == *asset) {
                    match loaded.images.get(i) {
                        Some(Ok(texture)) => {
                            let mut r = rectangle(rect);
                            let mut uv = Rect::from_min_max(Pos2::new(uv[0], uv[1]), Pos2::new(uv[2], uv[3]));
                            let size = texture.size_vec2() * uv.size();
                            if let Some([w, h]) = tile_size {
                                uv.max = uv.min + Vec2::new(r.width() / w, r.height() / h);
                            } else if size.x > 0.0 && size.y > 0.0 && r.width() > 0.0 && r.height() > 0.0 {
                                match fit {
                                    ImageFit::Contain => {
                                        let scale = (r.width() / size.x).min(r.height() / size.y);
                                        r = Rect::from_center_size(r.center(), size * scale);
                                    }
                                    ImageFit::Cover => {
                                        let scale = (r.width() / size.x).max(r.height() / size.y);
                                        uv = Rect::from_center_size(uv.center(), uv.size() * r.size() / (size * scale));
                                    }
                                    ImageFit::Stretch => {}
                                }
                            }
                            painter.image(texture.id(), r, uv, color(*tint));
                        }
                        Some(Err(error)) => {
                            painter.text(
                                rectangle(rect).min,
                                egui::Align2::LEFT_TOP,
                                error,
                                egui::FontId::proportional(12.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        None => {}
                    }
                }
            }
        }
    }
}
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    canvas: &Arc<DeviceUi>,
    parameters: &[GraphParameter],
    track: TrackId,
    device: usize,
    actions: &mut Vec<Action>,
) {
    let loaded = images(ui.ctx(), canvas);
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, color(canvas.background));
    paint(&painter, &canvas.layers, rect.min, parameters, &loaded);
    for control in &canvas.controls {
        let [x, y, w, h] = control.bounds;
        let bounds = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
        paint(
            &painter.with_clip_rect(bounds.intersect(painter.clip_rect())),
            &control.drawing,
            bounds.min,
            parameters,
            &loaded,
        );
        let response = ui
            .interact(bounds, egui::Id::new(("sd-custom", track.0, device, &control.id)), egui::Sense::click_and_drag())
            .on_hover_text(&control.tooltip);
        if response.clicked() {
            response.request_focus();
        }
        let focused = response.has_focus();
        let (dx, dy, button, fine) = ui.input(|i| {
            (
                i32::from(focused && i.key_pressed(egui::Key::ArrowRight))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowLeft)),
                i32::from(focused && i.key_pressed(egui::Key::ArrowUp))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowDown)),
                focused && (i.key_down(egui::Key::Space) || i.key_down(egui::Key::Enter)),
                if i.modifiers.shift { 0.1 } else { 1.0 },
            )
        });
        let held_id = response.id.with("held");
        let held = response.is_pointer_button_down_on() || button;
        let was_held = ui.ctx().data_mut(|d| {
            let previous = d.get_temp::<bool>(held_id).unwrap_or(false);
            d.insert_temp(held_id, held);
            previous
        });
        let mut control_actions = Vec::new();
        let mut descriptions = Vec::new();
        for (axis, binding) in [control.x.as_ref(), control.y.as_ref()].into_iter().enumerate() {
            let Some(binding) = binding else { continue };
            let Some(p) = parameters.iter().find(|p| p.id == binding.parameter) else { continue };
            descriptions.push(format!("{}: {} {}", p.name, p.value, p.unit));
            let old = normalized(p, binding.logarithmic);
            let mut next = old;
            match control.gesture {
                UiGesture::Position if response.dragged() || response.clicked() => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        next = if axis == 0 {
                            (pos.x - bounds.left()) as f64 / w as f64
                        } else {
                            (bounds.bottom() - pos.y) as f64 / h as f64
                        };
                    }
                }
                UiGesture::Drag if response.dragged() => {
                    let delta = response.drag_delta();
                    next += if axis == 0 { delta.x as f64 / w as f64 } else { -delta.y as f64 / h as f64 } * fine;
                }
                UiGesture::Toggle if response.clicked() => next = if old >= 0.5 { 0.0 } else { 1.0 },
                UiGesture::Momentary => next = f64::from(response.is_pointer_button_down_on() || button),
                _ => {}
            }
            if !matches!(control.gesture, UiGesture::Toggle | UiGesture::Momentary) {
                next += (if axis == 0 { dx } else { dy }) as f64 * 0.01 * fine;
            }
            let mut value = if binding.logarithmic {
                (p.min.ln() + next.clamp(0.0, 1.0) * (p.max.ln() - p.min.ln())).exp()
            } else {
                p.min + next.clamp(0.0, 1.0) * (p.max - p.min)
            };
            let discrete = matches!(
                p.control,
                suhina_device_sdk::ParameterControl::Integer
                    | suhina_device_sdk::ParameterControl::Toggle
                    | suhina_device_sdk::ParameterControl::Button
                    | suhina_device_sdk::ParameterControl::Choice
                    | suhina_device_sdk::ParameterControl::Radio
            );
            let key_delta = if axis == 0 { dx } else { dy };
            if key_delta != 0
                && !matches!(control.gesture, UiGesture::Toggle | UiGesture::Momentary)
                && (discrete || p.ui.step > 0.0)
            {
                let step = if discrete { p.ui.step.max(1.0) } else { p.ui.step * fine };
                value = (p.value + f64::from(key_delta) * step).clamp(p.min, p.max);
            }
            if response.double_clicked() && !matches!(control.gesture, UiGesture::Toggle | UiGesture::Momentary) {
                value = p.default;
            }
            // Reuse the SDK's discrete/range rules through the shared command.
            let changed = next != old || response.double_clicked();
            let interacting = response.clicked()
                || response.dragged()
                || dx != 0
                || dy != 0
                || (control.gesture == UiGesture::Momentary && (held || was_held));
            if (interacting && changed) || response.drag_stopped() || (was_held && !held) {
                control_actions.push(Action::new(
                    "device.setParam",
                    json!({
                        "track": track.0, "device": device, "param": p.id, "value": value,
                        "continuous": response.dragged() || response.is_pointer_button_down_on() || button,
                        "gesture": control.id
                    }),
                ));
            }
        }
        // Both axes are one undo gesture, including a single click or a keyboard step.
        let count = control_actions.len();
        for (index, mut action) in control_actions.into_iter().enumerate() {
            if index + 1 < count {
                action.params["continuous"] = json!(true);
            }
            actions.push(action);
        }
        response.widget_info(|| {
            let kind = if matches!(control.gesture, UiGesture::Toggle | UiGesture::Momentary) {
                egui::WidgetType::Button
            } else {
                egui::WidgetType::DragValue
            };
            let mut info = egui::WidgetInfo::labeled(kind, true, &control.name);
            info.current_text_value = Some(descriptions.join(", "));
            info
        });
        if focused {
            painter.rect_stroke(
                bounds,
                2.0,
                Stroke::new(1.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameter(id: &str, logarithmic: bool) -> GraphParameter {
        GraphParameter {
            default: 0.5,
            ui: Default::default(),
            control: Default::default(),
            unit: String::new(),
            id: id.into(),
            name: id.into(),
            min: if logarithmic { 20.0 } else { 0.0 },
            max: if logarithmic { 20000.0 } else { 1.0 },
            value: if logarithmic { 1000.0 } else { 0.5 },
        }
    }
    #[test]
    fn idle_log_control_does_not_emit_edits_and_xy_click_is_one_gesture() {
        let ctx = egui::Context::default();
        let canvas = Arc::new(DeviceUi {
            size: [160.0, 160.0],
            controls: vec![CustomControl {
                id: "xy".into(),
                name: "XY".into(),
                tooltip: String::new(),
                bounds: [0.0, 0.0, 100.0, 100.0],
                gesture: UiGesture::Position,
                x: Some(UiBinding { parameter: "x".into(), logarithmic: true }),
                y: Some(UiBinding::new("y")),
                drawing: vec![],
            }],
            ..Default::default()
        });
        let params = [parameter("x", true), parameter("y", false)];
        let mut actions = Vec::new();
        let mut frame = |events| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::splat(160.0)),
                    &canvas,
                    &params,
                    TrackId(1),
                    0,
                    &mut actions,
                )
            });
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(Pos2::new(75.0, 25.0)),
            egui::Event::PointerButton {
                pos: Pos2::new(75.0, 25.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        frame(vec![egui::Event::PointerButton {
            pos: Pos2::new(75.0, 25.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].params["continuous"], true);
        assert_eq!(actions[1].params["continuous"], false);
        assert_eq!(actions[0].params["gesture"], "xy");
        assert!((actions[0].params["value"].as_f64().unwrap() - 3556.5588).abs() < 0.1);
        assert_eq!(actions[1].params["value"], 0.75);
    }
}
