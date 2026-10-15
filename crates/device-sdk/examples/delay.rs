//! Stereo delay with an embedded control panel.
mod common;
#[path = "common/delay_panel.rs"]
mod delay_panel;
use common::{finish, parameter, wire};
use suhina_device_sdk::{
    Connection, DeviceGraph, DeviceRole, FilterMode, Node as N, NodeKind as K, ParameterControl as C, Waveform,
};
fn op(g: &mut DeviceGraph, id: &str, kind: K, inputs: &[(&str, &str)]) {
    g.nodes.push(N::new(id, kind));
    for (port, source) in inputs {
        wire(g, source, id, port);
    }
}
fn binary(g: &mut DeviceGraph, id: &str, kind: K, a: &str, b: &str) {
    op(g, id, kind, &[("a", a), ("b", b)]);
}
pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.delay", "Delay", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];
    for (id, lo, hi, value, unit, log) in [
        ("feedback", 0., 95., 50., "%", false),
        ("wet", 0., 100., 50., "%", false),
        ("time_left", 1., 4000., 250., "ms", true),
        ("time_right", 1., 4000., 250., "ms", true),
        ("sync_left", 0., 1., 1., "", false),
        ("sync_right", 0., 1., 1., "", false),
        ("division_left", 1., 16., 3., "", false),
        ("division_right", 1., 16., 3., "", false),
        ("offset_left", -33.3, 33.3, 0., "%", false),
        ("offset_right", -33.3, 33.3, 0., "%", false),
        ("link", 0., 1., 1., "", false),
        ("filter_on", 0., 1., 1., "", false),
        ("cutoff", 50., 18000., 1000., "Hz", true),
        ("width", 0.5, 9., 8., "", false),
        ("ping_pong", 0., 1., 0., "", false),
        ("freeze", 0., 1., 0., "", false),
        ("mode", 0., 2., 0., "", false),
        ("mod_delay", 0., 100., 0., "%", false),
        ("mod_filter", 0., 100., 0., "%", false),
        ("mod_rate", 0.01, 20., 0.5, "Hz", true),
        ("mod_shape", 0., 6., 1., "", false),
        ("mod_rate_mode", 0., 5., 0., "", false),
        ("mod_time", 1., 20000., 2000., "ms", true),
        ("mod_division", 1., 64., 4., "", false),
        ("mod_morph", 0., 100., 0., "%", false),
    ] {
        parameter(&mut g, id, (lo, hi), value, unit, log);
        if let Some(p) = g.parameters.last_mut() {
            p.smoothing_ms = 0.;
            p.control = if matches!(id, "sync_left" | "sync_right" | "link" | "filter_on" | "ping_pong" | "freeze") {
                C::Toggle
            } else if id.starts_with("division")
                || matches!(id, "mode" | "mod_shape" | "mod_division" | "mod_rate_mode")
            {
                C::Integer
            } else if log {
                C::LogKnob
            } else {
                C::Knob
            };
        }
    }
    let mut expanded = suhina_device_sdk::Parameter::new("lfo_expanded", "LFO details", (0., 1.), 1.);
    expanded.control = C::Toggle;
    expanded.smoothing_ms = 0.;
    g.parameters.push(expanded);
    for (id, labels) in [
        ("mod_shape", vec!["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "S&H", "Wander"]),
        ("mod_rate_mode", vec!["Rate", "Time", "Synced", "Triplet", "Dotted", "16th"]),
    ] {
        if let Some(p) = g.parameters.iter_mut().find(|p| p.id == id) {
            p.control = C::Choice;
            p.ui.options = labels.into_iter().map(str::to_string).collect();
        }
    }
    for side in ["left", "right"] {
        let peer = if side == "left" { "right" } else { "left" };
        for kind in ["sync", "division", "offset", "time"] {
            if let Some(p) = g.parameters.iter_mut().find(|p| p.id == format!("{kind}_{side}")) {
                p.ui.link = Some(suhina_device_sdk::ParameterLink {
                    enabled_by: "link".into(),
                    peer: format!("{kind}_{peer}"),
                });
            }
        }
    }
    for (id, value) in [
        ("zero", 0.),
        ("one", 1.),
        ("two", 2.),
        ("four", 4.),
        ("sixty", 60.),
        ("percent", 0.01),
        ("millisecond", 0.001),
    ] {
        g.nodes.push(N::new(id, K::Constant { value }));
    }
    op(&mut g, "tempo", K::Tempo, &[]);
    binary(&mut g, "beat_time", K::Divide, "sixty", "tempo");
    binary(&mut g, "sixteenth", K::Divide, "beat_time", "four");
    for side in ["left", "right"] {
        binary(&mut g, &format!("seconds_{side}"), K::Multiply, &format!("time_{side}"), "millisecond");
        binary(&mut g, &format!("sync_time_{side}"), K::Multiply, "sixteenth", &format!("division_{side}"));
        op(
            &mut g,
            &format!("chosen_{side}"),
            K::Select,
            &[
                ("a", &format!("seconds_{side}")),
                ("b", &format!("sync_time_{side}")),
                ("condition", &format!("sync_{side}")),
            ],
        );
        binary(&mut g, &format!("offset_fraction_{side}"), K::Multiply, &format!("offset_{side}"), "percent");
        binary(
            &mut g,
            &format!("effective_offset_{side}"),
            K::Multiply,
            &format!("offset_fraction_{side}"),
            &format!("sync_{side}"),
        );
        binary(&mut g, &format!("offset_scale_{side}"), K::Add, "one", &format!("effective_offset_{side}"));
        binary(
            &mut g,
            &format!("base_{side}"),
            K::Multiply,
            &format!("chosen_{side}"),
            &format!("offset_scale_{side}"),
        );
    }
    op(&mut g, "right_time", K::Select, &[("a", "base_right"), ("b", "base_left"), ("condition", "link")]);
    op(&mut g, "stereo_time", K::Stereo, &[("left", "base_left"), ("right", "right_time")]);
    op(
        &mut g,
        "mod_frequency",
        K::LfoRate,
        &[
            ("rate", "mod_rate"),
            ("time", "mod_time"),
            ("division", "mod_division"),
            ("mode", "mod_rate_mode"),
            ("tempo", "tempo"),
        ],
    );
    let shapes =
        [Waveform::Sine, Waveform::Triangle, Waveform::SawUp, Waveform::SawDown, Waveform::Square, Waveform::Random];
    for (i, shape) in shapes.into_iter().enumerate() {
        op(&mut g, &format!("wave{i}"), K::Oscillator { shape }, &[("frequency", "mod_frequency")]);
        if i > 0 {
            g.nodes.push(N::new(format!("shape_threshold{i}"), K::Constant { value: (i - 1) as f64 }));
            binary(&mut g, &format!("shape_active{i}"), K::Greater, "mod_shape", &format!("shape_threshold{i}"));
            let prev = if i == 1 { "wave0".into() } else { format!("shape_mix{}", i - 1) };
            op(
                &mut g,
                &format!("shape_mix{i}"),
                K::Select,
                &[("a", &prev), ("b", &format!("wave{i}")), ("condition", &format!("shape_active{i}"))],
            );
        }
    }
    g.nodes.push(N::new("wander_time", K::Constant { value: 0.2 }));
    g.nodes.push(N::new("wander_threshold", K::Constant { value: 5.0 }));
    op(&mut g, "wander", K::Smooth, &[("in", "wave5"), ("time", "wander_time")]);
    binary(&mut g, "wander_active", K::Greater, "mod_shape", "wander_threshold");
    op(&mut g, "selected_wave", K::Select, &[("a", "shape_mix5"), ("b", "wander"), ("condition", "wander_active")]);
    binary(&mut g, "morph_amount", K::Multiply, "mod_morph", "percent");
    op(&mut g, "lfo", K::Mix, &[("a", "selected_wave"), ("b", "wave0"), ("amount", "morph_amount")]);
    binary(&mut g, "delay_amount", K::Multiply, "mod_delay", "percent");
    binary(&mut g, "delay_motion", K::Multiply, "lfo", "delay_amount");
    binary(&mut g, "delay_scale", K::Add, "one", "delay_motion");
    binary(&mut g, "delay_time", K::Multiply, "stereo_time", "delay_scale");
    binary(&mut g, "filter_amount", K::Multiply, "mod_filter", "percent");
    binary(&mut g, "filter_motion", K::Multiply, "lfo", "filter_amount");
    binary(&mut g, "filter_octaves", K::Multiply, "filter_motion", "four");
    binary(&mut g, "filter_scale", K::Power, "two", "filter_octaves");
    binary(&mut g, "filter_center", K::Multiply, "cutoff", "filter_scale");
    binary(&mut g, "half_width", K::Divide, "width", "two");
    binary(&mut g, "band_ratio", K::Power, "two", "half_width");
    binary(&mut g, "low_edge", K::Divide, "filter_center", "band_ratio");
    binary(&mut g, "high_edge", K::Multiply, "filter_center", "band_ratio");
    binary(&mut g, "input_amount", K::Subtract, "one", "freeze");
    op(&mut g, "ping_input", K::Stereo, &[("left", "in"), ("right", "zero")]);
    op(&mut g, "source", K::Select, &[("a", "in"), ("b", "ping_input"), ("condition", "ping_pong")]);
    binary(&mut g, "feed", K::Multiply, "source", "input_amount");
    op(&mut g, "sum", K::Add, &[("a", "feed"), ("b", "feedback_audio")]);
    op(&mut g, "delay", K::Delay { max_seconds: 4. }, &[("in", "sum"), ("time", "delay_time"), ("mode", "mode")]);
    op(&mut g, "highpass", K::Filter { mode: FilterMode::Highpass }, &[("in", "delay"), ("frequency", "low_edge")]);
    op(&mut g, "lowpass", K::Filter { mode: FilterMode::Lowpass }, &[("in", "highpass"), ("frequency", "high_edge")]);
    op(&mut g, "filtered", K::Mix, &[("a", "delay"), ("b", "lowpass"), ("amount", "filter_on")]);
    g.nodes.push(N::new("swapped", K::Stereo));
    g.connections.push(Connection::ports("filtered", "right", "swapped", "left"));
    g.connections.push(Connection::ports("filtered", "left", "swapped", "right"));
    op(&mut g, "return_audio", K::Select, &[("a", "filtered"), ("b", "swapped"), ("condition", "ping_pong")]);
    binary(&mut g, "feedback_fraction", K::Multiply, "feedback", "percent");
    binary(&mut g, "wet_fraction", K::Multiply, "wet", "percent");
    op(&mut g, "feedback_amount", K::Select, &[("a", "feedback_fraction"), ("b", "one"), ("condition", "freeze")]);
    binary(&mut g, "feedback_audio", K::Multiply, "return_audio", "feedback_amount");
    op(&mut g, "mix", K::Mix, &[("a", "in"), ("b", "filtered"), ("amount", "wet_fraction")]);
    wire(&mut g, "mix", "out", "in");
    g.ui = Some(delay_panel::build(&g.parameters));
    for node in &mut g.nodes {
        node.visible = false;
    }
    let mut g = finish(g)?;
    for node in &mut g.nodes {
        node.visible = false;
    }
    Ok(g)
}
#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "delay")
}
