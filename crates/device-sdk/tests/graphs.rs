use suhina_device_sdk::{
    Connection as C, DeviceGraph, DeviceRole, NODE_CATALOG, Node as N, NodeKind as K, Parameter, ParameterControl,
    ProcessContext, Waveform,
};
#[path = "common/mod.rs"]
mod examples;

fn graph(kind: K) -> DeviceGraph {
    if matches!(kind, K::MidiTranspose { .. }) {
        let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::MidiEffect);
        g.nodes = vec![N::new("in", K::MidiInput), N::new("node", kind), N::new("out", K::MidiOutput)];
        g.connections = vec![C::new("in", "node"), C::new("node", "out")];
        return g;
    }
    if matches!(kind, K::MidiKey | K::MidiVelocity | K::MidiGate) {
        let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::Instrument);
        g.nodes = vec![N::new("node", kind), N::audio_output("out")];
        g.connections = vec![C::new("node", "out")];
        return g;
    }
    let mut g = DeviceGraph::new("org.test.graph", "Graph", DeviceRole::Effect);
    g.parameters.push(Parameter::new("amount", "Amount", (0.0, 2.0), 0.5));
    g.nodes = vec![N::audio_input("in"), N::new("node", kind), N::audio_output("out")];
    let ports = g.nodes[1].kind.info().inputs;
    for port in ports.iter().filter(|port| port.default.is_none()) {
        g.connections.push(C::ports("in", "out", "node", port.name));
    }
    if ports.iter().all(|port| port.default.is_some()) {
        g.nodes.push(N::new("sum", K::Add));
        g.connections.extend([
            C::ports("in", "out", "sum", "a"),
            C::ports("node", "out", "sum", "b"),
            C::new("sum", "out"),
        ]);
    } else {
        g.connections.push(C::new("node", "out"));
    }
    g
}

#[test]
fn every_catalog_node_round_trips_resets_and_ignores_host_block_size() {
    let mut kinds = NODE_CATALOG
        .iter()
        .filter(|info| !matches!(info.kind, "audio_input" | "audio_output" | "midi_input" | "midi_output"))
        .map(|info| serde_json::from_str::<K>(info.example).unwrap())
        .collect::<Vec<_>>();
    kinds.extend([
        K::Envelope { rms: true },
        K::Oscillator { shape: Waveform::Random },
        K::Filter { mode: suhina_device_sdk::FilterMode::Highpass },
        K::Filter { mode: suhina_device_sdk::FilterMode::Bandpass },
        K::Filter { mode: suhina_device_sdk::FilterMode::Peak },
        K::Filter { mode: suhina_device_sdk::FilterMode::LowShelf },
        K::Filter { mode: suhina_device_sdk::FilterMode::HighShelf },
    ]);
    for kind in kinds {
        let g = graph(kind.clone());
        let restored = DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap();
        assert_eq!(g, restored);
        if g.role == DeviceRole::MidiEffect {
            assert!(g.compile_midi().is_ok());
            continue;
        }
        if g.role == DeviceRole::Instrument {
            assert!(g.compile_instrument_at(48_000.0).is_ok());
            continue;
        }
        for sr in [8000.0, 44100.0, 48000.0, 96000.0, 384000.0] {
            let mut effect = g.compile_at(sr).unwrap();
            let input = (0..2048).flat_map(|n| [(n as f32 * 0.03).sin(), -(n as f32 * 0.05).cos()]).collect::<Vec<_>>();
            let mut expected = input.clone();
            effect.process(&mut expected);
            assert!(expected.iter().all(|v| v.is_finite()), "{kind:?}");
            for frames in [1, 7, 31, 512] {
                effect.reset();
                let mut output = input.clone();
                for block in output.chunks_mut(frames * 2) {
                    effect.process(block);
                }
                assert_eq!(output, expected, "{kind:?}: {sr}, {frames}");
            }
            effect.set_parameter(0, f64::NAN);
            effect.set_parameter(99, f64::INFINITY);
            let mut hostile = [f32::NAN, f32::INFINITY, f32::MAX, -f32::MAX, 1.0];
            effect.process(&mut hostile);
            assert!(hostile.iter().all(|v| v.is_finite()), "{kind:?}");
            assert_eq!(hostile[4], 0.0);
        }
    }
}

fn feedback() -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.echo", "Echo", DeviceRole::Effect);
    g.parameters.push(Parameter::new("feedback", "Feedback", (0.0, 0.9), 0.5));
    g.nodes = vec![
        N::audio_input("in"),
        N::new("sum", K::Add),
        N::new("delay", K::SampleDelay),
        N::gain("feedback", "feedback"),
        N::audio_output("out"),
    ];
    g.connections = vec![
        C::ports("in", "out", "sum", "a"),
        C::ports("feedback", "out", "sum", "b"),
        C::new("sum", "delay"),
        C::new("delay", "feedback"),
        C::new("sum", "out"),
    ];
    g
}

#[test]
fn feedback_impulse_matches_golden_and_node_storage_order_is_irrelevant() {
    let mut g = feedback();
    let bytes = include_bytes!("golden/feedback.f32");
    let expected = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect::<Vec<_>>();
    for _ in 0..g.nodes.len() {
        let mut effect = g.compile().unwrap();
        let mut output = vec![0.0; expected.len()];
        output[0] = 1.0;
        output[1] = -1.0;
        effect.process(&mut output);
        assert_eq!(output, expected);
        g.nodes.rotate_left(1);
    }
}

#[test]
fn fractional_delay_has_the_expected_two_impulse_taps() {
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.nodes.push(N::new("time", K::Constant { value: 1.5 / 48000.0 }));
    g.connections.push(C::ports("time", "out", "node", "time"));
    let mut out = [1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [0.0, 0.0, 0.5, -0.5, 0.5, -0.5, 0.0, 0.0]);
}

#[test]
fn branching_channel_ports_and_parameter_changes_work() {
    let mut g = graph(K::Stereo);
    g.connections =
        vec![C::ports("in", "right", "node", "left"), C::ports("in", "left", "node", "right"), C::new("node", "out")];
    let mut out = [1.0, 2.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [2.0, 1.0]);
    let mut g = graph(K::Multiply);
    g.nodes.push(N::new("amount", K::Parameter { parameter: "amount".into() }));
    g.connections.retain(|e| e.to_port != "b");
    g.connections.push(C::ports("amount", "out", "node", "b"));
    let mut effect = g.compile().unwrap();
    let mut out = [1.0, -1.0];
    effect.process(&mut out);
    assert_eq!(out, [0.5, -0.5]);
    effect.set_parameter(0, 1.5);
    effect.process(&mut vec![0.0; 1920]);
    let mut out = [1.0, -1.0];
    effect.process(&mut out);
    assert_eq!(out, [1.5, -1.5]);
}

#[test]
fn invalid_ports_cycles_orphans_and_memory_requests_have_actionable_errors() {
    let mut g = feedback();
    g.nodes[2].kind = K::Negate;
    assert!(g.validate().unwrap_err().to_string().contains("cycle"));
    let mut g = feedback();
    g.connections[0].to_port = "typo".into();
    assert!(g.validate().unwrap_err().to_string().contains("sum"));
    let mut g = feedback();
    g.connections.push(g.connections[0].clone());
    assert!(g.validate().unwrap_err().to_string().contains("multiple connections"));
    let mut g = feedback();
    g.nodes.push(N::new("orphan", K::Constant { value: 1.0 }));
    assert!(g.validate().unwrap_err().to_string().contains("orphan"));
    let mut g = graph(K::Delay { max_seconds: 60.0 });
    assert!(g.validate().unwrap_err().to_string().contains("history"));
    g.nodes[1].kind = K::Delay { max_seconds: f64::INFINITY };
    assert!(g.validate().is_err());
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.connections.push(C::ports("out", "out", "node", "time"));
    assert!(g.validate().is_err());
    let mut g = graph(K::Delay { max_seconds: 0.01 });
    g.connections.push(C::ports("node", "out", "node", "time"));
    assert!(g.validate().unwrap_err().to_string().contains("cycle"));
}

#[test]
fn version_one_gain_example_remains_readable_and_new_nodes_require_version_two() {
    let g = DeviceGraph::from_sd(include_str!("fixtures/gain-v1.sd")).unwrap();
    let mut out = [1.0, -1.0];
    g.compile().unwrap().process(&mut out);
    assert_eq!(out, [1.0, -1.0]);
    let text = graph(K::Tanh).to_sd().unwrap().replace("\"version\": 2", "\"version\": 1");
    assert!(DeviceGraph::from_sd(&text).is_err());
    assert!(g.compile_at(f64::NAN).is_err());
    assert!(g.compile_at(1e9).is_err());
}

#[test]
fn discrete_controls_are_validated_and_clamped_consistently() {
    let mut g = graph(K::Gain { parameter: "amount".into() });
    let p = &mut g.parameters[0];
    p.control = ParameterControl::Integer;
    p.default = 1.0;
    assert_eq!(p.clamp(1.7), 2.0);
    assert!(g.validate().is_ok());
    g.parameters[0].default = 0.5;
    assert!(g.validate().is_err());
    g.parameters[0].control = ParameterControl::LogSlider;
    assert!(g.validate().is_err());
    g.parameters[0].control = ParameterControl::Toggle;
    g.parameters[0].max = 1.0;
    g.parameters[0].default = 0.0;
    assert!(g.validate().is_ok());
    assert_eq!(g.parameters[0].clamp(0.7), 1.0);
}

#[test]
fn synced_lfo_follows_transport_position() {
    let mut g = graph(K::SyncedLfo { shape: Waveform::Sine });
    let mut effect = g.compile().unwrap();
    let context = ProcessContext { tempo: 120.0, beat: 0.25, playing: false };
    let mut out = [0.0; 4];
    effect.process_context(&context, &mut out);
    assert_eq!(out, [1.0; 4]);
    g.nodes[1].kind = K::Beat;
    let mut out = [0.0; 4];
    g.compile_at(8000.0).unwrap().process_context(&ProcessContext { beat: 1.0, playing: true, ..context }, &mut out);
    assert_eq!(out[0], 1.0);
    assert!((out[2] - 1.00025).abs() < 1e-6);
}

#[test]
fn six_authoring_examples_save_restore_and_have_the_expected_sound() {
    for name in ["stereo-utility", "auto-pan", "filter", "distortion", "feedback-delay", "compressor"] {
        let g = examples::build(name).unwrap();
        assert_eq!(DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap(), g);
        let mut effect = g.compile().unwrap();
        let mut out = vec![0.5; 48000];
        effect.process(&mut out);
        assert!(out.iter().all(|v| v.is_finite()));
        match name {
            "stereo-utility" => assert_eq!(out, vec![0.5; 48000]),
            "auto-pan" => {
                assert!((out[24000] - 0.0).abs() < 1e-5);
                assert!((out[24001] - 0.5).abs() < 1e-5);
            }
            "filter" => assert!((out[47998] - 0.5).abs() < 1e-4),
            "distortion" => assert!((out[47998] - (0.25 + 0.5 * 1.0f32.tanh())).abs() < 1e-6),
            "feedback-delay" => {
                assert_eq!(out[0], 0.25);
                assert_eq!(out[24000], 0.5);
            }
            "compressor" => assert!(out[47998] > 0.1 && out[47998] < 0.2, "{}", out[47998]),
            _ => {}
        }
    }
}

#[test]
fn every_node_has_documented_ports_and_a_valid_catalog_example() {
    let docs = include_str!("../../../docs/spec/sdk-nodes.md").replace("\r\n", "\n");
    let table =
        docs.split("<!-- sdk-nodes:begin -->\n").nth(1).unwrap().split("<!-- sdk-nodes:end -->").next().unwrap();
    assert_eq!(table, suhina_device_sdk::node_reference_markdown());
    for info in NODE_CATALOG {
        let kind: K = serde_json::from_str(info.example).unwrap();
        assert_eq!(kind.info().kind, info.kind);
        let row = docs.lines().find(|line| line.starts_with(&format!("| `{}` /", info.kind))).unwrap();
        assert!(row.contains(info.description), "stale description for {}", info.kind);
        for port in info.inputs {
            assert!(row.contains(&format!("`{}`", port.name)), "{}", info.kind);
        }
    }
}

#[test]
fn parameter_changes_are_ramped_without_steps_and_independent_of_block_size() {
    let g = graph(K::Gain { parameter: "amount".into() });
    for rate in [8000.0, 48000.0, 96000.0] {
        let mut effect = g.compile_at(rate).unwrap();
        effect.process(&mut [1.0; 2]);
        effect.set_parameter(0, 1.5);
        let frames = (rate * 0.02) as usize;
        let mut expected = vec![1.0; frames * 2];
        effect.process(&mut expected);
        assert!(expected[0] > 0.5 && expected[0] < 0.52);
        assert_eq!(*expected.last().unwrap(), 1.5);
        assert!(expected.windows(2).all(|w| (w[1] - w[0]).abs() < 0.01));
        let mut effect = g.compile_at(rate).unwrap();
        effect.process(&mut [1.0; 2]);
        effect.set_parameter(0, 1.5);
        let mut output = vec![1.0; frames * 2];
        for block in output.chunks_mut(14) {
            effect.process(block);
        }
        assert_eq!(output, expected);
    }
}

#[test]
fn customized_controls_round_trip_and_reject_invalid_choice_metadata() {
    use suhina_device_sdk::ParameterControl::*;
    for control in [Slider, LogSlider, Integer, Toggle, Knob, LogKnob, Number, VerticalSlider, Button, Choice, Radio] {
        let mut g = graph(K::Gain { parameter: "amount".into() });
        let p = &mut g.parameters[0];
        p.control = control;
        p.default = 1.0;
        if matches!(control, LogSlider | LogKnob) {
            p.min = 0.01;
        }
        if matches!(control, Toggle | Button) {
            p.max = 1.0;
        }
        if matches!(control, Choice | Radio) {
            p.ui.options = vec!["Off".into(), "Unity".into(), "Boost".into()];
        }
        p.ui.tooltip = "Output gain".into();
        p.ui.group = "Output".into();
        p.ui.decimals = 2;
        let text = g.to_sd().unwrap();
        assert_eq!(DeviceGraph::from_sd(&text).unwrap(), g);
        g.parameters[0].ui.decimals = 100;
        assert!(g.validate().is_err());
    }
}

#[test]
fn visual_ui_settings_round_trip_and_hostile_geometry_is_rejected() {
    use suhina_device_sdk::{ControlShape, LabelPosition};
    let mut g = graph(K::Gain { parameter: "amount".into() });
    let ui = &mut g.parameters[0].ui;
    ui.size = Some([240.0, 120.0]);
    ui.position = Some([300.0, 20.0]);
    ui.accent = Some([20, 190, 130]);
    ui.background = Some([12, 20, 25]);
    ui.text_color = Some([240, 240, 240]);
    ui.font_size = 18.0;
    ui.monospace = true;
    ui.shape = ControlShape::Square;
    ui.label_position = LabelPosition::Top;
    ui.show_value = false;
    assert_eq!(DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap(), g);
    for position in [[f32::NAN, 0.0], [-1.0, 0.0], [9000.0, 0.0]] {
        let mut invalid = g.clone();
        invalid.parameters[0].ui.position = Some(position);
        assert!(invalid.validate().is_err());
    }
    for size in [[0.0, 100.0], [240.0, 24.0], [f32::INFINITY, 100.0], [240.0, 9000.0]] {
        let mut invalid = g.clone();
        invalid.parameters[0].ui.size = Some(size);
        assert!(invalid.validate().is_err());
    }
    g.parameters[0].ui.font_size = 100.0;
    assert!(g.validate().is_err());
}
