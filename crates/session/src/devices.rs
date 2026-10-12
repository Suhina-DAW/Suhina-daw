//! Commands for devices on tracks (ADR 0004): listing the built-in devices, adding, removing and
//! switching them, and setting their parameters. Each edit is one undo step, and a playing
//! engine hears it at once: a chain that keeps its devices keeps playing (held notes, tails).

use loopwerk_device_api::{DeviceInfo, ParamKind, Role, Unit};
use loopwerk_model::{Device, DeviceKind, DeviceParam, Extensions};
use serde_json::{Value, json};

use crate::commands::{channel_param, number, track_id};
use crate::{CommandError, ParamEdit, Session};

/// Devices one track may hold.
const MAX_DEVICES: usize = 64;

/// The built-in devices and their parameters.
pub(crate) fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(loopwerk_device_registry::DEVICES.iter().map(|info| info_json(info)).collect()))
}

fn info_json(info: &DeviceInfo) -> Value {
    json!({
        "id": info.id,
        "name": info.name,
        "role": match info.role { Role::Effect => "effect", Role::Instrument => "instrument" },
        "params": info.params.iter().map(|p| {
            let mut v = json!({
                "id": p.id,
                "name": p.name,
                "min": p.min,
                "max": p.max,
                "default": p.default,
                "unit": unit_name(p.unit),
            });
            let kind = match p.kind {
                ParamKind::Continuous(_) => "number",
                ParamKind::Toggle => "toggle",
                ParamKind::Integer => "integer",
                ParamKind::Choice(options) => {
                    v["options"] = json!(options);
                    "choice"
                }
            };
            v["kind"] = json!(kind);
            v
        }).collect::<Vec<_>>(),
    })
}

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::None => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Percent => "%",
        Unit::Balance => "balance",
        Unit::Octaves => "octaves",
    }
}

/// The devices on a track, in order, with every parameter's value for built-in ones.
pub(crate) fn track_devices(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let devices = channel_param(session, params)?.devices.clone();
    Ok(Value::Array(
        devices
            .iter()
            .enumerate()
            .map(|(index, d)| {
                let mut value = device_json(index, d);
                if matches!(d.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }) {
                    value["plugin"] = plugin_json(session.plugin_report(track, index));
                }
                value
            })
            .collect(),
    ))
}

fn device_json(index: usize, device: &Device) -> Value {
    let (kind, id) = match &device.kind {
        DeviceKind::Builtin(id) => ("builtin", Some(id.clone())),
        DeviceKind::Clap { id } => ("clap", Some(id.clone())),
        DeviceKind::Vst3 { id } => ("vst3", Some(id.clone())),
        DeviceKind::Vst2 { id } => ("vst2", Some(id.to_string())),
        _ => ("other", None),
    };
    let info = id.as_deref().filter(|_| kind == "builtin").and_then(loopwerk_device_registry::info);
    let params = info.map(|info| {
        info.params
            .iter()
            .map(|p| {
                let value = device.params.iter().find(|v| v.id == p.id).map_or(p.default, |v| p.clamp(v.value));
                json!({ "id": p.id, "value": value })
            })
            .collect::<Vec<_>>()
    });
    json!({
        "index": index,
        "name": device.name,
        "kind": kind,
        "id": id,
        "enabled": device.enabled,
        "known": kind != "builtin" || info.is_some(),
        "params": params,
    })
}

/// The `device` index parameter, checked against the track's devices.
fn device_index(len: usize, params: &Value) -> Result<usize, CommandError> {
    let index = params
        .get("device")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("device must be a device index, from 0, as in track.devices")?;
    if index >= len {
        return Err(format!("the track has no device {index}; it has {len}").into());
    }
    Ok(index)
}

pub(crate) fn add(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = params.get("id").and_then(Value::as_str).ok_or("id must be a built-in device id, from devices.list")?;
    let info = loopwerk_device_registry::info(id)
        .ok_or_else(|| format!("no built-in device {id:?}; devices.list lists them"))?;
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let instrument = info.role == Role::Instrument;
    let has_instrument = channel
        .devices
        .first()
        .and_then(|d| match &d.kind {
            DeviceKind::Builtin(id) => loopwerk_device_registry::info(id),
            _ => None,
        })
        .is_some_and(|i| i.role == Role::Instrument);
    let position = match params.get("position") {
        None => {
            if instrument {
                0
            } else {
                channel.devices.len()
            }
        }
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    if instrument && has_instrument {
        return Err("the track has an instrument already; remove it first".into());
    }
    if instrument && position != 0 {
        return Err("an instrument goes first in a track's chain (position 0)".into());
    }
    if has_instrument && position == 0 {
        return Err("position 0 is the track's instrument; effects go after it".into());
    }
    let device = Device {
        name: info.name.to_string(),
        kind: DeviceKind::Builtin(info.id.to_string()),
        enabled: true,
        state: None,
        params: Vec::new(),
        extensions: Extensions::default(),
    };
    channel.devices.insert(position, device);
    Ok(json!({ "device": position, "id": info.id, "name": info.name }))
}

pub(crate) fn remove(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let removed = channel.devices.remove(index);
    Ok(json!({ "removed": removed.name, "devices": channel.devices.len() }))
}

pub(crate) fn set_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    device.enabled = enabled;
    Ok(json!({ "device": index, "enabled": enabled }))
}

pub(crate) fn set_param(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id, from devices.list")?;
    let value = number(params, "value")?;
    let track = track_id(&session.project, params)?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    let DeviceKind::Builtin(id) = &device.kind else {
        return Err("only built-in devices have parameters here; plug-in parameters come later".into());
    };
    let info =
        loopwerk_device_registry::info(id).ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
    let param_index = info
        .param_index(param)
        .ok_or_else(|| format!("{} has no parameter {param:?}; devices.list lists them", info.name))?;
    let spec = info.params.get(param_index).ok_or("the parameter is missing")?;
    // Stored as the device will use it: limited to its range, whole for toggles and choices.
    let value = spec.clamp(value);
    match device.params.iter_mut().find(|p| p.id == param) {
        Some(stored) => stored.value = value,
        None => device.params.push(DeviceParam::new(param, value)),
    }
    // A playing engine takes this as a message, without a new graph.
    session.param_edit = Some(ParamEdit { track, device: index, param: param_index, value });
    Ok(json!({ "device": index, "param": param, "value": value }))
}

/// The key of the plug-in device the `track` and `device` parameters name.
fn plugin_key(session: &Session, params: &Value) -> Result<u64, CommandError> {
    let track = track_id(&session.project, params)?;
    let index = params
        .get("device")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("device must be a device index, from 0, as in track.devices")?;
    crate::plugin_key_at(&session.project, track, index)
        .ok_or_else(|| format!("device {index} on the track is not a plug-in").into())
}

/// Adds an installed plug-in from the catalog to a track.
pub(crate) fn add_plugin(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("plugin").and_then(Value::as_str).ok_or("plugin must be a plug-in name, id or file")?;
    let env = session.plugins.env().ok_or("no plug-in catalog is open; plugins.scan looks for installed ones")?;
    let plugin = env
        .catalog
        .find(name)
        .or_else(|| env.catalog.plugins().find(|p| p.spec() == name))
        .ok_or_else(|| format!("no plug-in {name:?} in the catalog; plugins.list lists them"))?
        .clone();
    let instrument = crate::is_instrument(&plugin);
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let position = match params.get("position") {
        None if instrument => 0,
        None => channel.devices.len(),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= channel.devices.len())
            .ok_or_else(|| format!("position must be from 0 to {}", channel.devices.len()))?,
    };
    if instrument && position != 0 {
        return Err("an instrument goes first in a track's chain (position 0)".into());
    }
    let device = Device {
        name: plugin.name.clone(),
        kind: crate::rack::device_kind(&plugin),
        enabled: true,
        state: None,
        params: Vec::new(),
        extensions: Extensions::default(),
    };
    channel.devices.insert(position, device);
    Ok(json!({
        "device": position,
        "name": plugin.name,
        "vendor": plugin.vendor,
        "format": plugin.format(),
        "instrument": instrument,
    }))
}

/// A plug-in device's state as JSON.
pub(crate) fn plugin_json(report: Option<crate::PluginReport>) -> Value {
    let Some(report) = report else { return json!({ "status": "stopped" }) };
    json!({
        "status": report.status.name(),
        "reason": report.status.reason(),
        "plugin": report.plugin.as_ref().map(|(name, vendor, how)| json!({ "name": name, "vendor": vendor, "found": how })),
        "instrument": report.instrument,
        "latency": report.latency,
        "editor_open": report.editor_open,
        "state_refused": report.state_refused,
    })
}

pub(crate) fn plugin_status(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    Ok(plugin_json(session.plugins.report(key)))
}

pub(crate) fn open_editor(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    // Over the app's window the editor opens in the background; `device.plugin` says when.
    Ok(match session.plugins.open_editor(key)? {
        Some((width, height)) => json!({ "editor_open": true, "width": width, "height": height }),
        None => json!({ "editor_open": "opening" }),
    })
}

pub(crate) fn close_editor(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    session.plugins.close_editor(key)?;
    Ok(json!({ "editor_open": false }))
}

pub(crate) fn restart(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    session.plugins.restart(key)?;
    Ok(json!({ "status": "loading" }))
}
