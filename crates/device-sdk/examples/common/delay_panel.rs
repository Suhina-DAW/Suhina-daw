use suhina_device_sdk::{Parameter, ui::*};
const TEXT: [u8; 4] = [194, 194, 190, 255];
const DARK: [u8; 4] = [24, 24, 24, 255];
const BG: [u8; 4] = [54, 54, 54, 255];
const CYAN: [u8; 4] = [3, 195, 213, 255];
const ORANGE: [u8; 4] = [255, 173, 86, 255];
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
fn equal(id: &str, v: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: (v as f32).into() }))
}
fn conditional_text(x: f32, y: f32, label: &str, color: [u8; 4], visible: UiValue) -> Draw {
    let mut d = text(x, y, label, color, true);
    if let Draw::Text { position, .. } = &mut d {
        position[0] = UiValue::from(x) + (UiValue::from(1.) + visible * (-1.).into()) * 4096.0.into();
    }
    d
}
fn button(ui: &mut DeviceUi, id: &str, value: Option<u16>, label: &str, bounds: [f32; 4]) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Button { label: label.into(), off_label: None, indicator: false }),
        id: format!("{id}_{}", value.unwrap_or(99)),
        name: label.into(),
        tooltip: format!("{id}: {label}"),
        bounds,
        visible_when: None,
        gesture: value.map_or(UiGesture::Toggle, UiGesture::Set),
        x: Some(UiBinding::new(id)),
        y: None,
        drawing: vec![],
    });
}
fn when(ui: &mut DeviceUi, id: &str, value: u16) {
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(equal(id, value));
    }
}
fn number(ui: &mut DeviceUi, id: &str, bounds: [f32; 4], decimals: usize, log: bool, color: [u8; 4], center: bool) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Number {
            decimals,
            format: if id == "cutoff" { TextFormat::Frequency } else { TextFormat::Number },
            color,
            center,
            bordered: false,
            prefix: String::new(),
            suffix: String::new(),
        }),
        id: id.into(),
        name: id.replace('_', " "),
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log }),
        drawing: vec![],
    });
}
fn knob(ui: &mut DeviceUi, id: &str, label: &str, [x, y, w, h]: [f32; 4], log: bool) {
    let label_height = if label.is_empty() { 0. } else { 12. };
    ui.controls.push(CustomControl {
        widget: Some(Widget::Knob { label: label.into() }),
        id: id.into(),
        name: if label.is_empty() { id.replace('_', " ") } else { label.into() },
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds: [x, y - label_height, w, h + label_height],
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log }),
        drawing: vec![],
    });
}
pub fn build(parameters: &[Parameter]) -> DeviceUi {
    let mut ui = DeviceUi {
        size: [400., 180.],
        viewport_size: Some([400., 180.]),
        background: BG,
        hidden_parameters: parameters.iter().map(|p| p.id.clone()).collect(),
        ..DeviceUi::default()
    };
    for (side, x) in [("left", 7.), ("right", 77.)] {
        let sync = format!("sync_{side}");
        ui.layers.push(text(x + 23., 0., if side == "left" { "Left" } else { "Right" }, TEXT, true));
        button(&mut ui, &sync, None, "Sync", [x, 13., 46., 12.]);
        when(&mut ui, &sync, 1);
        button(&mut ui, &sync, None, "Time", [x, 13., 46., 12.]);
        if let Some(c) = ui.controls.last_mut() {
            c.id = format!("{sync}_time");
            c.widget = Some(Widget::Button { label: "Sync".into(), off_label: Some("Time".into()), indicator: false });
        }
        when(&mut ui, &sync, 0);
        for (i, value) in [1, 2, 3, 4, 5, 6, 8, 16].into_iter().enumerate() {
            button(
                &mut ui,
                &format!("division_{side}"),
                Some(value),
                &value.to_string(),
                [x + (i % 2) as f32 * 24., 29. + (i / 2) as f32 * 16., 22., 13.],
            );
            when(&mut ui, &sync, 1);
        }
        number(&mut ui, &format!("offset_{side}"), [x, 94., 46., 12.], 1, false, TEXT, true);
        if let Some(c) = ui.controls.last_mut()
            && let Some(Widget::Number { bordered, .. }) = &mut c.widget
        {
            *bordered = true;
        }
        when(&mut ui, &sync, 1);
        knob(&mut ui, &format!("time_{side}"), "", [x, 39., 46., 63.], true);
        when(&mut ui, &sync, 0);
    }
    button(&mut ui, "link", None, "↔", [58., 56., 14., 12.]);
    knob(&mut ui, "feedback", "Feedback", [37., 126., 54., 40.], false);
    button(&mut ui, "freeze", None, "*", [93., 136., 13., 13.]);
    ui.layers.push(text(368., 0., "Smoothing", TEXT, true));
    for (i, label) in ["Repitch", "Fade", "Jump"].into_iter().enumerate() {
        button(&mut ui, "mode", Some(i as u16), label, [340., 13. + i as f32 * 14., 55., 12.]);
    }
    button(&mut ui, "ping_pong", None, "Ping Pong", [340., 94., 55., 12.]);
    knob(&mut ui, "wet", "Dry/Wet", [345., 126., 44., 40.], false);
    let expanded = UiValue::normalized("lfo_expanded", false);
    ui.layers.push(Draw::Rect {
        rect: [
            129.0.into(),
            (UiValue::from(165.) + expanded.clone() * (-30.).into()),
            204.0.into(),
            UiValue::from(15.) + expanded.clone() * 30.0.into(),
        ],
        color: DARK,
        radius: 0.,
    });
    for (state, height, offset) in [(1, 118., 0.), (0, 148., 30.)] {
        let layer_start = ui.layers.len();
        let control_start = ui.controls.len();
        ui.controls.push(CustomControl {
            widget: Some(Widget::FilterBand { min_hz: 50., max_hz: 18000. }),
            id: "filter_graph".into(),
            name: "Filter frequency and width".into(),
            tooltip: "Drag horizontally for frequency, vertically for width".into(),
            bounds: [129., 0., 204., height],
            visible_when: None,
            gesture: UiGesture::Position,
            x: Some(UiBinding { parameter: "cutoff".into(), logarithmic: true }),
            y: Some(UiBinding::new("width")),
            drawing: vec![],
        });
        button(&mut ui, "filter_on", None, "Filter", [129., 120. + offset, 43., 12.]);
        if let Some(c) = ui.controls.last_mut() {
            c.widget = Some(Widget::Button { label: "Filter".into(), off_label: None, indicator: true });
        }
        ui.layers.push(text(182., 120. + offset, "Freq", TEXT, false));
        number(&mut ui, "cutoff", [208., 120. + offset, 53., 12.], 0, true, CYAN, false);
        ui.layers.push(text(266., 120. + offset, "Width", TEXT, false));
        number(&mut ui, "width", [302., 120. + offset, 31., 12.], 2, false, CYAN, false);
        ui.controls.push(CustomControl {
            widget: Some(Widget::Disclosure { label: "LFO".into() }),
            id: "lfo_disclosure".into(),
            name: "LFO details".into(),
            tooltip: "Expand or collapse LFO details".into(),
            bounds: [129., 135. + offset, 43., 12.],
            visible_when: None,
            gesture: UiGesture::Toggle,
            x: Some(UiBinding::new("lfo_expanded")),
            y: None,
            drawing: vec![],
        });
        ui.layers.push(text(178., 135. + offset, "Delay", TEXT, false));
        number(&mut ui, "mod_delay", [207., 135. + offset, 52., 12.], 1, false, ORANGE, false);
        ui.layers.push(text(266., 135. + offset, "Filter", TEXT, false));
        number(&mut ui, "mod_filter", [298., 135. + offset, 35., 12.], 1, false, ORANGE, false);
        let visible = equal("lfo_expanded", state);
        let hidden = (UiValue::from(1.) + visible.clone() * (-1.).into()) * 4096.0.into();
        for layer in &mut ui.layers[layer_start..] {
            if let Draw::Text { position, .. } = layer {
                position[0] = position[0].clone() + hidden.clone();
            }
        }
        for control in &mut ui.controls[control_start..] {
            control.visible_when = Some(visible.clone());
            if state == 0 {
                control.id.push_str("_collapsed");
            }
        }
    }
    let details_layer_start = ui.layers.len();
    let details_control_start = ui.controls.len();
    ui.layers.push(conditional_text(196., 149., "Time", TEXT, equal("mod_rate_mode", 1)));
    ui.layers.push(conditional_text(
        196.,
        149.,
        "Rate",
        TEXT,
        UiValue::from(1.) + equal("mod_rate_mode", 1) * (-1.).into(),
    ));
    number(&mut ui, "mod_rate", [209., 149., 54., 12.], 2, true, ORANGE, false);
    when(&mut ui, "mod_rate_mode", 0);
    number(&mut ui, "mod_time", [209., 149., 54., 12.], 0, true, ORANGE, false);
    when(&mut ui, "mod_rate_mode", 1);
    number(&mut ui, "mod_division", [209., 149., 54., 12.], 0, false, ORANGE, false);
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(equal("mod_rate_mode", 2) + equal("mod_rate_mode", 3) + equal("mod_rate_mode", 4));
        if let Some(Widget::Number { prefix, .. }) = &mut c.widget {
            *prefix = "1/".into();
        }
    }
    number(&mut ui, "mod_division", [209., 149., 54., 12.], 0, false, ORANGE, false);
    if let Some(c) = ui.controls.last_mut() {
        c.id = "mod_division_16th".into();
        if let Some(Widget::Number { suffix, .. }) = &mut c.widget {
            *suffix = "16th".into();
        }
    }
    when(&mut ui, "mod_rate_mode", 5);
    ui.layers.push(text(266., 149., "Wave", TEXT, false));
    ui.controls.push(CustomControl {
        widget: Some(Widget::WaveformMenu),
        id: "mod_shape".into(),
        name: "LFO waveform".into(),
        tooltip: "Choose the LFO waveform".into(),
        bounds: [299., 149., 34., 12.],
        visible_when: None,
        gesture: UiGesture::Menu,
        x: Some(UiBinding::new("mod_shape")),
        y: None,
        drawing: vec![],
    });
    button(&mut ui, "mod_rate_mode", None, "Rate", [182., 163., 67., 12.]);
    if let Some(c) = ui.controls.last_mut() {
        c.name = "LFO rate mode".into();
        c.tooltip = "Choose free rate, time or a tempo-synchronized division".into();
        c.gesture = UiGesture::Menu;
        c.widget = Some(Widget::Menu);
    }
    ui.layers.push(text(266., 163., "Morph", TEXT, false));
    number(&mut ui, "mod_morph", [302., 163., 31., 12.], 1, false, ORANGE, false);
    let hidden = (UiValue::from(1.) + expanded.clone() * (-1.).into()) * 4096.0.into();
    for layer in &mut ui.layers[details_layer_start..] {
        if let Draw::Text { position, .. } = layer {
            position[0] = position[0].clone() + hidden.clone();
        }
    }
    for control in &mut ui.controls[details_control_start..] {
        control.visible_when = Some(
            control.visible_when.take().map_or_else(|| expanded.clone(), |condition| condition * expanded.clone()),
        );
    }
    ui
}
