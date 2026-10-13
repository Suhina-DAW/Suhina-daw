//! Live's own devices that map onto Loopwerk's built-in devices (ADR 0004). Each mapping follows
//! `docs/spec/als-format.md` › Devices: the element Live stores, the unit of every value, and
//! the Loopwerk parameter it becomes. Parameter ids are the devices' stable ids
//! (`devices/<name>` in this repository).

use loopwerk_model::DeviceParam;
use roxmltree::Node;

use crate::{boolean, number, path};

/// Gain at or below this (in dB) is the Loopwerk utility's silence.
const SILENCE_DB: f64 = -70.0;

/// If `node` is a Live device with a Loopwerk counterpart: the Loopwerk device id, a default
/// name, and the parameter values. `warn` gets notes about values that could not be read.
pub(crate) fn builtin(
    node: Node,
    warn: &mut dyn FnMut(String),
) -> Option<(&'static str, &'static str, Vec<DeviceParam>)> {
    match node.tag_name().name() {
        "StereoGain" => Some(("utility", "Utility", utility(node, warn))),
        _ => None,
    }
}

/// An automatable value: `<name>/Manual@Value`.
fn manual(node: Node, name: &str) -> Option<f64> {
    number(node, &[name, "Manual"])
}

fn manual_flag(node: Node, name: &str) -> Option<bool> {
    boolean(node, &[name, "Manual"])
}

/// A plain (not automatable) flag: `<name>@Value`.
fn plain_flag(node: Node, name: &str) -> Option<bool> {
    path(node, &[name]).and_then(|n| n.attribute("Value")).and_then(|v| match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn toggle(on: bool) -> f64 {
    if on { 1.0 } else { 0.0 }
}

/// Live's Utility (`StereoGain`) as Loopwerk's `utility`.
fn utility(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut params = Vec::new();
    let mut set = |id: &str, value: Option<f64>| {
        if let Some(value) = value {
            params.push(DeviceParam::new(id, value));
        }
    };

    // Gain is stored as a linear factor; ours is in decibels, with silence at the bottom.
    set("gain", manual(node, "Gain").map(|g| if g > 0.0 { (20.0 * g.log10()).max(SILENCE_DB) } else { SILENCE_DB }));
    set("balance", manual(node, "Balance"));

    // Mid/Side is a mode of Live's Width knob, with its own value 0 (only mid) .. 2 (only
    // side). Ours is a separate control from -1 to 1, next to a width that stays at 100 %.
    if plain_flag(node, "MidSideBalanceOn") == Some(true) {
        set("width", Some(1.0));
        set("midSide", manual(node, "MidSideBalance").map(|v| v - 1.0));
    } else {
        set("width", manual(node, "StereoWidth"));
        set("midSide", Some(0.0));
    }

    set("mono", manual_flag(node, "Mono").map(toggle));
    set("bassMono", manual_flag(node, "BassMono").map(toggle));
    set("bassMonoFrequency", manual(node, "BassMonoFrequency"));
    set("bassMonoListen", plain_flag(node, "BassMonoAudition").map(toggle));
    set("invertLeft", manual_flag(node, "PhaseInvertL").map(toggle));
    set("invertRight", manual_flag(node, "PhaseInvertR").map(toggle));

    // Live: 0 Left, 1 Stereo, 2 Right, 3 Swap. Ours: 0 Stereo, 1 Left, 2 Right, 3 Swap.
    let channel = manual(node, "ChannelMode").map(|mode| match mode as i64 {
        0 => 1.0,
        1 => 0.0,
        2 => 2.0,
        3 => 3.0,
        other => {
            warn(format!("Utility channel mode {other} is not understood; using stereo"));
            0.0
        }
    });
    set("channel", channel);
    set("dcFilter", manual_flag(node, "DcFilter").map(toggle));
    set("mute", manual_flag(node, "Mute").map(toggle));
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utility_from(inner: &str) -> (Vec<DeviceParam>, Vec<String>) {
        let xml = format!("<StereoGain>{inner}</StereoGain>");
        let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{e}"));
        let mut warnings = Vec::new();
        let (id, _, params) =
            builtin(doc.root_element(), &mut |w| warnings.push(w)).unwrap_or_else(|| panic!("not mapped"));
        assert_eq!(id, "utility");
        (params, warnings)
    }

    fn param(params: &[DeviceParam], id: &str) -> f64 {
        params.iter().find(|p| p.id == id).map(|p| p.value).unwrap_or_else(|| panic!("{id} missing: {params:?}"))
    }

    fn manual_xml(name: &str, value: &str) -> String {
        format!(r#"<{name}><Manual Value="{value}"/><AutomationTarget Id="1"/></{name}>"#)
    }

    #[test]
    fn gain_becomes_decibels() {
        for (stored, db) in
            [("1", 0.0), ("0.2511886358", -12.0), ("1.99526238", 6.0), ("56.2341309", 35.0), ("0", -70.0)]
        {
            let (params, _) = utility_from(&manual_xml("Gain", stored));
            assert!((param(&params, "gain") - db).abs() < 1e-3, "{stored}: {}", param(&params, "gain"));
        }
    }

    #[test]
    fn width_or_mid_side_mode() {
        let (params, _) = utility_from(&(manual_xml("StereoWidth", "4") + &manual_xml("MidSideBalance", "0.5")));
        assert_eq!((param(&params, "width"), param(&params, "midSide")), (4.0, 0.0));
        let on = r#"<MidSideBalanceOn Value="true"/>"#;
        for (stored, ours) in [("0", -1.0), ("0.5", -0.5), ("1", 0.0), ("1.5", 0.5), ("2", 1.0)] {
            let (params, _) =
                utility_from(&(manual_xml("StereoWidth", "4") + &manual_xml("MidSideBalance", stored) + on));
            assert_eq!((param(&params, "width"), param(&params, "midSide")), (1.0, ours), "{stored}");
        }
    }

    #[test]
    fn channel_modes_are_reordered() {
        for (stored, ours) in [("0", 1.0), ("1", 0.0), ("2", 2.0), ("3", 3.0)] {
            let (params, warnings) = utility_from(&manual_xml("ChannelMode", stored));
            assert_eq!(param(&params, "channel"), ours);
            assert!(warnings.is_empty());
        }
        let (params, warnings) = utility_from(&manual_xml("ChannelMode", "7"));
        assert_eq!(param(&params, "channel"), 0.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn switches_and_frequency() {
        let xml = [
            manual_xml("Mono", "true"),
            manual_xml("BassMono", "true"),
            manual_xml("BassMonoFrequency", "500"),
            manual_xml("PhaseInvertL", "true"),
            manual_xml("PhaseInvertR", "false"),
            manual_xml("DcFilter", "true"),
            manual_xml("Mute", "false"),
            manual_xml("Balance", "-1"),
            r#"<BassMonoAudition Value="true"/>"#.to_string(),
        ]
        .concat();
        let (params, _) = utility_from(&xml);
        let expected = [
            ("mono", 1.0),
            ("bassMono", 1.0),
            ("bassMonoFrequency", 500.0),
            ("invertLeft", 1.0),
            ("invertRight", 0.0),
            ("dcFilter", 1.0),
            ("mute", 0.0),
            ("balance", -1.0),
            ("bassMonoListen", 1.0),
        ];
        for (id, value) in expected {
            assert_eq!(param(&params, id), value, "{id}");
        }
    }

    #[test]
    fn missing_and_malformed_values_are_left_out() {
        let (params, _) = utility_from(&(manual_xml("Gain", "lots") + &manual_xml("Mono", "maybe")));
        assert!(params.iter().all(|p| p.id != "gain" && p.id != "mono"), "{params:?}");
        assert!(params.iter().all(|p| p.value.is_finite()));
    }

    #[test]
    fn other_devices_are_not_mapped() {
        let doc = roxmltree::Document::parse("<Eq8/>").unwrap_or_else(|e| panic!("{e}"));
        assert!(builtin(doc.root_element(), &mut |_| {}).is_none());
    }
}
