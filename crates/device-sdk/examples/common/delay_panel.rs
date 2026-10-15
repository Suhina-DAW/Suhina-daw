use suhina_device_sdk::{Parameter, ui::*};
const ACCENT: [u8; 4] = [166, 163, 255, 255];
fn equal(id: &str, v: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: (v as f32).into() }))
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
        hidden_parameters: parameters.iter().map(|p| p.id.clone()).collect(),
        ..DeviceUi::default()
    };
    for (side, x) in [("left", 7.), ("right", 77.)] {
        let sync = format!("sync_{side}");
        let label = if side == "left" { "L Sync" } else { "R Sync" };
        button(&mut ui, &sync, None, label, [x, 13., 46., 12.]);
        when(&mut ui, &sync, 1);
        button(&mut ui, &sync, None, label, [x, 13., 46., 12.]);
        if let Some(c) = ui.controls.last_mut() {
            c.id = format!("{sync}_time");
            c.widget = Some(Widget::Button {
                label: label.into(),
                off_label: Some(if side == "left" { "L Time" } else { "R Time" }.into()),
                indicator: false,
            });
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
        number(&mut ui, &format!("offset_{side}"), [x, 94., 46., 12.], 1, false, [236, 232, 225, 255], true);
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
    for (i, label) in ["Repitch", "Fade", "Jump"].into_iter().enumerate() {
        button(&mut ui, "mode", Some(i as u16), label, [340., 13. + i as f32 * 14., 55., 12.]);
    }
    button(&mut ui, "ping_pong", None, "Ping Pong", [340., 94., 55., 12.]);
    knob(&mut ui, "wet", "Dry/Wet", [345., 126., 44., 40.], false);
    let expanded = UiValue::normalized("lfo_expanded", false);
    for (state, height, offset) in [(1, 118., 0.), (0, 148., 30.)] {
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
        number(&mut ui, "cutoff", [208., 120. + offset, 53., 12.], 0, true, ACCENT, false);
        number(&mut ui, "width", [302., 120. + offset, 31., 12.], 2, false, ACCENT, false);
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
        number(&mut ui, "mod_delay", [207., 135. + offset, 52., 12.], 1, false, ACCENT, false);
        number(&mut ui, "mod_filter", [298., 135. + offset, 35., 12.], 1, false, ACCENT, false);
        let visible = equal("lfo_expanded", state);
        for control in &mut ui.controls[control_start..] {
            control.visible_when = Some(visible.clone());
            if state == 0 {
                control.id.push_str("_collapsed");
            }
        }
    }
    let details_control_start = ui.controls.len();
    number(&mut ui, "mod_rate", [209., 149., 54., 12.], 2, true, ACCENT, false);
    when(&mut ui, "mod_rate_mode", 0);
    number(&mut ui, "mod_time", [209., 149., 54., 12.], 0, true, ACCENT, false);
    when(&mut ui, "mod_rate_mode", 1);
    number(&mut ui, "mod_division", [209., 149., 54., 12.], 0, false, ACCENT, false);
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(equal("mod_rate_mode", 2) + equal("mod_rate_mode", 3) + equal("mod_rate_mode", 4));
        if let Some(Widget::Number { prefix, .. }) = &mut c.widget {
            *prefix = "1/".into();
        }
    }
    number(&mut ui, "mod_division", [209., 149., 54., 12.], 0, false, ACCENT, false);
    if let Some(c) = ui.controls.last_mut() {
        c.id = "mod_division_16th".into();
        if let Some(Widget::Number { suffix, .. }) = &mut c.widget {
            *suffix = "16th".into();
        }
    }
    when(&mut ui, "mod_rate_mode", 5);
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
    number(&mut ui, "mod_morph", [302., 163., 31., 12.], 1, false, ACCENT, false);
    for control in &mut ui.controls[details_control_start..] {
        control.visible_when = Some(
            control.visible_when.take().map_or_else(|| expanded.clone(), |condition| condition * expanded.clone()),
        );
    }
    ui
}
