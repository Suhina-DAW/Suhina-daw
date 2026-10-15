use super::*;
const TEXT: [u8; 4] = [236, 232, 225, 255];
const DARK: [u8; 4] = [37, 36, 33, 255];
const RIM: [u8; 4] = [56, 55, 51, 255];
const ACCENT: [u8; 4] = [166, 163, 255, 255];
const INK: [u8; 4] = [29, 28, 26, 255];
fn text(x: f32, y: f32, label: &str, color: [u8; 4], center: bool) -> Draw {
    Draw::Text {
        position: [x.into(), y.into()],
        text: label.into(),
        size: 10.,
        color,
        parameter: None,
        decimals: 0,
        align: if center { TextAlign::Center } else { TextAlign::Left },
        format: TextFormat::Number,
    }
}
fn rect(x: f32, y: f32, w: UiValue, h: f32, color: [u8; 4]) -> Draw {
    Draw::Rect { rect: [x.into(), y.into(), w, h.into()], color, radius: 0. }
}
fn equal(id: &str, v: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: (v as f32).into() }))
}
fn readout(id: &str, x: f32, y: f32, decimals: usize, color: [u8; 4], center: bool) -> Draw {
    Draw::Text {
        position: [x.into(), y.into()],
        text: String::new(),
        size: 10.,
        color,
        parameter: Some(id.into()),
        decimals,
        align: if center { TextAlign::Center } else { TextAlign::Left },
        format: TextFormat::Number,
    }
}
fn conditional_text(x: f32, y: f32, label: &str, color: [u8; 4], visible: UiValue) -> Draw {
    let mut d = text(x, y, label, color, true);
    if let Draw::Text { position, .. } = &mut d {
        position[0] = UiValue::from(x) + (UiValue::from(1.) + visible * (-1.).into()) * 4096.0.into();
    }
    d
}
fn button(ui: &mut DeviceUi, id: &str, value: Option<u16>, label: &str, b: [f32; 4]) {
    let [_, _, w, h] = b;
    let active = value.map_or_else(|| UiValue::normalized(id, false), |v| equal(id, v));
    ui.controls.push(CustomControl {
        id: format!("{id}_{}", value.unwrap_or(99)),
        name: label.into(),
        tooltip: format!("{id}: {label}"),
        bounds: b,
        widget: None,
        visible_when: None,
        gesture: value.map_or(UiGesture::Toggle, UiGesture::Set),
        x: Some(UiBinding::new(id)),
        y: None,
        drawing: vec![
            rect(0., 0., w.into(), h, [46, 45, 42, 255]),
            rect(0., 0., active.clone() * w.into(), h, ACCENT),
            conditional_text(w / 2., 0., label, TEXT, UiValue::from(1.) + active.clone() * (-1.).into()),
            conditional_text(w / 2., 0., label, INK, active),
        ],
    });
}
fn number(ui: &mut DeviceUi, id: &str, b: [f32; 4], decimals: usize, log: bool, color: [u8; 4], center: bool) {
    ui.controls.push(CustomControl {
        id: id.into(),
        name: id.replace('_', " "),
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds: b,
        widget: None,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log }),
        drawing: vec![readout(id, if center { b[2] / 2. } else { 0. }, 0., decimals, color, center)],
    });
}
fn knob(ui: &mut DeviceUi, id: &str, label: &str, b: [f32; 4], log: bool) {
    let [x, y, w, h] = b;
    let label_height = if label.is_empty() { 0. } else { 12. };
    let angle = UiValue::from(-2.35) + UiValue::normalized(id, log) * 4.7.into();
    let cx = w / 2.;
    let cy = (h - 12.) / 2.;
    let center_y = cy + label_height;
    let radius = cy.clamp(0., 9.);
    ui.controls.push(CustomControl {
        id: id.into(),
        name: if label.is_empty() { id.replace('_', " ") } else { label.into() },
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds: [x, y - label_height, w, h + label_height],
        widget: None,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log }),
        drawing: vec![
            text(cx, 0., label, TEXT, true),
            Draw::Circle { center: [cx.into(), center_y.into()], radius: radius.into(), color: RIM },
            Draw::Circle {
                center: [cx.into(), center_y.into()],
                radius: (radius - 1.).max(0.).into(),
                color: [46, 45, 42, 255],
            },
            Draw::Line {
                from: [cx.into(), center_y.into()],
                to: [
                    UiValue::from(cx) + angle.clone().cos() * (radius - 2.).max(0.).into(),
                    UiValue::from(center_y) + angle.sin() * (radius - 2.).max(0.).into(),
                ],
                color: ACCENT,
                width: 1.5,
            },
            readout(id, cx, h - 12. + label_height, 0, TEXT, true),
        ],
    });
}

/// Reusable functional UI nodes. Each instance uses its control's bindings and bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Widget {
    Button {
        label: String,
        #[serde(default)]
        off_label: Option<String>,
        #[serde(default)]
        indicator: bool,
    },
    Number {
        decimals: usize,
        format: TextFormat,
        color: [u8; 4],
        center: bool,
        #[serde(default)]
        bordered: bool,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
    },
    Knob {
        label: String,
    },
    Disclosure {
        label: String,
    },
    Menu,
    WaveformMenu,
    FilterBand {
        min_hz: f32,
        max_hz: f32,
    },
}
impl CustomControl {
    /// Legacy custom drawings and typed widgets share the host's interaction path.
    pub fn drawings(&self) -> std::borrow::Cow<'_, [Draw]> {
        match &self.widget {
            Some(widget) => std::borrow::Cow::Owned(widget.drawings(self)),
            None => std::borrow::Cow::Borrowed(&self.drawing),
        }
    }

    /// Resolve built-in widget colours against the host's active theme.
    pub fn drawings_with_palette(&self, palette: &WidgetPalette) -> std::borrow::Cow<'_, [Draw]> {
        if self.widget.is_none() {
            return std::borrow::Cow::Borrowed(&self.drawing);
        }
        let mut drawings = self.drawings().into_owned();
        for draw in &mut drawings {
            resolve_widget_draw(draw, palette);
        }
        std::borrow::Cow::Owned(drawings)
    }
}

fn resolve_widget_draw(draw: &mut Draw, palette: &WidgetPalette) {
    let resolve = |color: &mut [u8; 4]| {
        *color = match *color {
            [236, 232, 225, 255] => palette.text,
            [37, 36, 33, 255] => palette.panel,
            [46, 45, 42, 255] => palette.control,
            [56, 55, 51, 255] => palette.border,
            [92, 92, 88, 255] | [112, 112, 112, 255] => palette.border,
            [30, 30, 30, 255] => palette.control,
            [166, 163, 255, 255] => palette.accent,
            [29, 28, 26, 255] => palette.accent_text,
            color => color,
        };
    };
    match draw {
        Draw::Rect { color, .. } | Draw::Circle { color, .. } | Draw::Text { color, .. } => resolve(color),
        Draw::Line { color, .. } | Draw::Arc { color, .. } => resolve(color),
        Draw::Path { color, fill, .. } => {
            resolve(color);
            if let Some(fill) = fill {
                resolve(fill);
            }
        }
        Draw::Gradient { colors, .. } => colors.iter_mut().for_each(resolve),
        Draw::Image { tint, .. } => resolve(tint),
    }
}
impl Widget {
    fn drawings(&self, control: &CustomControl) -> Vec<Draw> {
        let mut ui = DeviceUi::default();
        let [_, _, w, height] = control.bounds;
        let Some(binding) = control.x.as_ref().or(control.y.as_ref()) else {
            return vec![];
        };
        let id = binding.parameter.as_str();
        match self {
            Self::Button { label, off_label, indicator } => {
                let value = if let UiGesture::Set(v) = control.gesture { Some(v) } else { None };
                button(&mut ui, id, value, label, [0., 0., w, height]);
                if let Some(c) = ui.controls.last_mut() {
                    if *indicator {
                        c.drawing = vec![
                            rect(0., 1., 8.0.into(), 8., [92, 92, 88, 255]),
                            rect(0., 1., UiValue::normalized(id, false) * 8.0.into(), 8., ACCENT),
                            text(11., 0., label, TEXT, false),
                        ];
                    } else if let Some(off) = off_label {
                        let active = UiValue::normalized(id, false);
                        c.drawing = vec![
                            rect(0., 0., w.into(), height, ACCENT),
                            rect(0., 0., active.clone() * w.into(), height, ACCENT),
                            conditional_text(w / 2., 0., off, INK, UiValue::from(1.) + active.clone() * (-1.).into()),
                            conditional_text(w / 2., 0., label, INK, active),
                        ];
                    }
                }
            }
            Self::Number { decimals, format, color, center, bordered, prefix, suffix } => {
                number(&mut ui, id, [0., 0., w, height], *decimals, binding.logarithmic, *color, *center);
                if let Some(c) = ui.controls.last_mut() {
                    if let Some(Draw::Text { format: f, text, size, .. }) = c.drawing.first_mut() {
                        *f = *format;
                        *text = prefix.clone();
                        if *bordered {
                            *size = 9.;
                        }
                    }
                    if *bordered {
                        c.drawing.insert(0, rect(0., 0., w.into(), height, [112, 112, 112, 255]));
                        c.drawing.insert(1, rect(1., 1., (w - 2.).into(), height - 2., [30, 30, 30, 255]));
                    }
                    if !suffix.is_empty() {
                        c.drawing.push(text(20., 0., suffix, *color, false));
                    }
                }
            }
            Self::Knob { label } => {
                let label_height = if label.is_empty() { 0. } else { 12. };
                knob(&mut ui, id, label, [0., label_height, w, height - label_height], binding.logarithmic);
            }
            Self::Disclosure { label } => {
                let mut disclosure = vec![text(9., 0., label, TEXT, false)];
                for (value, points) in [(1, [[1., 3.], [6., 3.], [3.5, 8.]]), (0, [[1., 2.], [1., 9.], [6., 5.5]])] {
                    let hidden = (UiValue::from(1.) + equal(id, value) * (-1.).into()) * 4096.0.into();
                    disclosure.push(Draw::Path {
                        points: points
                            .into_iter()
                            .map(|[x, y]| [UiValue::from(x) + hidden.clone(), y.into()])
                            .collect(),
                        color: TEXT,
                        width: 1.,
                        closed: true,
                        fill: Some(TEXT),
                    });
                }

                return disclosure;
            }
            Self::WaveformMenu => {
                let mut wave = vec![];
                for shape in 0..7 {
                    let shift = (UiValue::from(1.) + equal(id, shape) * (-1.).into()) * 4096.0.into();
                    let mut pts = Vec::new();
                    for i in 0..17 {
                        let t = i as f32 / 16.;
                        let y = match shape {
                            0 => (t * std::f32::consts::TAU).sin(),
                            1 => 1. - 4. * (t - 0.5).abs(),
                            2 => t * 2. - 1.,
                            3 => 1. - t * 2.,
                            4 => {
                                if t < 0.5 {
                                    1.
                                } else {
                                    -1.
                                }
                            }
                            5 => ((t * 4.).floor() * 5.).sin(),
                            _ => (t * 8.).sin() * 0.6 + (t * 3.).cos() * 0.4,
                        };
                        pts.push([UiValue::from(2. + t * 15.) + shift.clone(), (6. - y * 3.).into()]);
                    }
                    wave.push(Draw::Path { points: pts, color: ACCENT, width: 1., closed: false, fill: None });
                }
                wave.push(Draw::Path {
                    points: vec![[23.0.into(), 5.0.into()], [26.0.into(), 8.0.into()], [29.0.into(), 5.0.into()]],
                    color: ACCENT,
                    width: 1.,
                    closed: false,
                    fill: None,
                });

                return wave;
            }
            Self::Menu => {
                let mut label = readout(id, 0., 0., 0, ACCENT, false);
                if let Draw::Text { format, .. } = &mut label {
                    *format = TextFormat::Choice;
                }
                return vec![
                    label,
                    Draw::Path {
                        points: vec![
                            [(w - 6.).into(), 4.0.into()],
                            [(w - 3.).into(), 7.0.into()],
                            [w.into(), 4.0.into()],
                        ],
                        color: ACCENT,
                        width: 1.,
                        closed: false,
                        fill: None,
                    },
                ];
            }
            Self::FilterBand { min_hz, max_hz } => {
                let (Some(x), Some(y)) = (&control.x, &control.y) else {
                    return vec![];
                };
                let mut drawing = vec![rect(0., 0., w.into(), height, DARK)];
                for i in 1..28 {
                    let x = i as f32 * w / 28.;
                    drawing.push(Draw::Line {
                        from: [x.into(), 0.0.into()],
                        to: [x.into(), height.into()],
                        color: [56, 55, 51, 255],
                        width: 0.5,
                    });
                }
                for i in 1..6 {
                    let y = i as f32 * height / 6.;
                    drawing.push(Draw::Line {
                        from: [0.0.into(), y.into()],
                        to: [w.into(), y.into()],
                        color: [56, 55, 51, 255],
                        width: 0.5,
                    });
                }
                let cx = UiValue::normalized(&x.parameter, true) * w.into();
                let center_log = UiValue::parameter(&x.parameter, false).ln();
                let half_width = UiValue::parameter(&y.parameter, false) * (std::f32::consts::LN_2 * 0.5).into();
                let low_log = center_log.clone() + half_width.clone() * (-1.).into();
                let high_log_inverse = center_log * (-1.).into() + half_width * (-1.).into();
                let mut points = Vec::new();
                for i in 0..129 {
                    let t = i as f32 / 128.;
                    let log_f = min_hz.ln() + t * (max_hz / min_hz).ln();
                    let highpass_loss =
                        (UiValue::from(1.) + ((low_log.clone() + (-log_f).into()) * 4.0.into()).exp()).ln();
                    let lowpass_loss =
                        (UiValue::from(1.) + ((high_log_inverse.clone() + log_f.into()) * 4.0.into()).exp()).ln();
                    let y = UiValue::from(height * 30. / 118.)
                        + (highpass_loss + lowpass_loss)
                            * ((height * 88. / 118.) / (1.8 * std::f32::consts::LN_10)).into();
                    points.push([(t * w).into(), y]);
                }
                drawing.push(Draw::Path { points, color: ACCENT, width: 1.2, closed: false, fill: None });
                let cy = UiValue::from(height) + UiValue::normalized(&y.parameter, false) * (-height).into();
                drawing.push(Draw::Circle { center: [cx.clone(), cy.clone()], radius: 4.6.into(), color: ACCENT });
                drawing.push(Draw::Circle { center: [cx, cy], radius: 3.0.into(), color: DARK });

                return drawing;
            }
        }
        ui.controls.pop().map_or_else(Vec::new, |c| c.drawing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widgets_work_with_unrelated_parameter_names() {
        let mut amount = Parameter::new("amount", "Amount", (1., 64.), 4.);
        amount.control = crate::ParameterControl::Choice;
        amount.ui.options = (1..=64).map(|i| i.to_string()).collect();
        let spread = Parameter::new("spread", "Spread", (0.5, 9.), 2.);
        let widgets = [
            (Widget::Button { label: "Enable".into(), off_label: None, indicator: false }, UiGesture::Toggle),
            (
                Widget::Number {
                    decimals: 2,
                    format: TextFormat::Number,
                    color: ACCENT,
                    center: false,
                    bordered: false,
                    prefix: String::new(),
                    suffix: String::new(),
                },
                UiGesture::Drag,
            ),
            (Widget::Knob { label: "Amount".into() }, UiGesture::Drag),
            (Widget::Disclosure { label: "Details".into() }, UiGesture::Toggle),
            (Widget::Menu, UiGesture::Menu),
            (Widget::WaveformMenu, UiGesture::Menu),
            (Widget::FilterBand { min_hz: 1., max_hz: 64. }, UiGesture::Position),
        ];
        for (widget, gesture) in widgets {
            let mut bound_amount = amount.clone();
            if matches!(widget, Widget::WaveformMenu) {
                bound_amount.min = 0.;
                bound_amount.max = 6.;
                bound_amount.ui.options.truncate(7);
            }
            let position = gesture == UiGesture::Position;
            let drag = gesture == UiGesture::Drag;
            let control = CustomControl {
                widget: Some(widget),
                id: "reused".into(),
                name: "Reused".into(),
                tooltip: String::new(),
                bounds: [0., 0., 100., 100.],
                visible_when: None,
                gesture,
                x: (!drag).then(|| UiBinding { parameter: "amount".into(), logarithmic: position }),
                y: if drag {
                    Some(UiBinding::new("amount"))
                } else if position {
                    Some(UiBinding::new("spread"))
                } else {
                    None
                },
                drawing: vec![],
            };
            assert!(!control.drawings().is_empty());
            let ui = DeviceUi { controls: vec![control], ..DeviceUi::default() };
            ui.validate(&[bound_amount, spread.clone()]).unwrap();
        }
    }

    #[test]
    fn widget_drawings_resolve_the_host_palette() {
        let control = CustomControl {
            widget: Some(Widget::Button { label: "Enable".into(), off_label: None, indicator: false }),
            id: "enable".into(),
            name: "Enable".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 12.0],
            visible_when: None,
            gesture: UiGesture::Toggle,
            x: Some(UiBinding::new("enabled")),
            y: None,
            drawing: Vec::new(),
        };
        let palette = WidgetPalette {
            text: [1, 2, 3, 255],
            panel: [4, 5, 6, 255],
            control: [7, 8, 9, 255],
            border: [10, 11, 12, 255],
            accent: [13, 14, 15, 255],
            accent_text: [16, 17, 18, 255],
        };
        let draws = control.drawings_with_palette(&palette);
        assert!(matches!(&draws[0], Draw::Rect { color, .. } if *color == palette.control));
        assert!(matches!(&draws[1], Draw::Rect { color, .. } if *color == palette.accent));
        assert!(draws.iter().any(|draw| matches!(draw, Draw::Text { color, .. } if *color == palette.text)));

        let knob = CustomControl {
            widget: Some(Widget::Knob { label: "Amount".into() }),
            id: "amount".into(),
            name: "Amount".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 40.0],
            visible_when: None,
            gesture: UiGesture::Drag,
            x: None,
            y: Some(UiBinding::new("amount")),
            drawing: Vec::new(),
        };
        let draws = knob.drawings_with_palette(&palette);
        assert!(draws.iter().any(|draw| matches!(draw, Draw::Circle { color, .. } if *color == palette.border)));
        assert!(draws.iter().any(|draw| matches!(draw, Draw::Line { color, .. } if *color == palette.accent)));
        assert!(!draws.iter().any(|draw| matches!(draw, Draw::Arc { .. })));

        let authored = CustomControl {
            widget: None,
            id: "authored".into(),
            name: "Authored".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 12.0],
            visible_when: None,
            gesture: UiGesture::Toggle,
            x: Some(UiBinding::new("enabled")),
            y: None,
            drawing: vec![Draw::Rect {
                rect: [0.0.into(), 0.0.into(), 40.0.into(), 12.0.into()],
                color: [166, 163, 255, 255],
                radius: 0.0,
            }],
        };
        assert!(
            matches!(&authored.drawings_with_palette(&palette)[0], Draw::Rect { color, .. } if *color == [166, 163, 255, 255])
        );

        let number = CustomControl {
            widget: Some(Widget::Number {
                decimals: 0,
                format: TextFormat::Number,
                color: TEXT,
                center: true,
                bordered: true,
                prefix: String::new(),
                suffix: String::new(),
            }),
            id: "value".into(),
            name: "Value".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 12.0],
            visible_when: None,
            gesture: UiGesture::Drag,
            x: None,
            y: Some(UiBinding::new("value")),
            drawing: Vec::new(),
        };
        let draws = number.drawings_with_palette(&palette);
        assert!(matches!(&draws[0], Draw::Rect { color, .. } if *color == palette.border));
        assert!(matches!(&draws[1], Draw::Rect { color, .. } if *color == palette.control));
    }
}
