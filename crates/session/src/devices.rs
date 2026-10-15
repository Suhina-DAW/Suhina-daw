//! Commands for devices on tracks (ADR 0004): listing the built-in effects and instruments, adding, removing and
//! switching them, and setting their parameters. Each edit is one undo step, and a playing
//! engine hears it at once: a chain that keeps its devices keeps playing (held notes, tails).

use serde_json::{Value, json};
use suhina_device_api::{DeviceInfo, ParamKind, Role, Unit};
use suhina_engine::param_id;
use suhina_model::{AutomationLane, AutomationPoint, Device, DeviceKind, DeviceParam, Extensions};

use crate::commands::{channel_param, number, track_id};
use crate::{CommandError, ParamEdit, Session};

/// Devices one track may hold.
const MAX_DEVICES: usize = 64;

/// The built-in effects and instruments with their parameters.
pub(crate) fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(suhina_device_registry::DEVICES.iter().map(|info| info_json(info)).collect()))
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
    let info = id.as_deref().filter(|_| kind == "builtin").and_then(suhina_device_registry::info);
    let params = info.map(|info| {
        info.params
            .iter()
            .map(|p| {
                let value = device.params.iter().find(|v| v.id == p.id).map_or(p.default, |v| p.clamp(v.value));
                json!({ "id": p.id, "value": value })
            })
            .collect::<Vec<_>>()
    });
    // A plug-in's: the values set on it, by parameter id (the rest are as its state says).
    let params = params.or_else(|| {
        is_plugin(&device.kind).then(|| {
            device
                .params
                .iter()
                .filter_map(|p| Some(json!({ "id": param_id(&p.id)?, "value": p.value })))
                .collect::<Vec<_>>()
        })
    });
    let mut value = json!({
        "index": index,
        "name": device.name,
        "kind": kind,
        "id": id,
        "enabled": device.enabled,
        "known": kind != "builtin" || info.is_some(),
        "params": params,
    });
    if !device.automation.is_empty() {
        value["automation"] = device
            .automation
            .iter()
            .map(|lane| {
                let points: Vec<[f64; 2]> = lane.points.iter().map(|p| [p.beat, p.value]).collect();
                json!({ "param": param_id(&lane.param), "points": points })
            })
            .collect();
    }
    value
}

fn is_plugin(kind: &DeviceKind) -> bool {
    matches!(kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. })
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
    let preset = match params.get("preset") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let path = v.as_str().ok_or("preset must be a preset file's path")?;
            let voice = match params.get("voice") {
                None | Some(Value::Null) => 1,
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n >= 1)
                    .ok_or("voice must be a whole number from 1")?,
            };
            Some(read_preset(std::path::Path::new(path), voice)?)
        }
    };
    let id = match (params.get("id").and_then(Value::as_str), &preset) {
        (Some(id), Some(p)) if id != p.device => {
            return Err(format!("the preset is for {:?}, not {id:?}", p.device).into());
        }
        (Some(id), _) => id,
        (None, Some(p)) => p.device,
        (None, None) => return Err("id must be the id of a built-in effect or instrument, from devices.list".into()),
    };
    let info = suhina_device_registry::info(id)
        .ok_or_else(|| format!("no built-in effect or instrument {id:?}; devices.list lists them"))?;
    if info.role == Role::Instrument {
        let track = track_id(&session.project, params)?;
        if session.project.tracks.iter().find(|t| t.id == track).is_some_and(|t| !t.content.notes) {
            return Err("an instrument goes on a MIDI track".into());
        }
    }
    let channel = channel_param(session, params)?;
    if channel.devices.len() >= MAX_DEVICES {
        return Err(format!("a track holds at most {MAX_DEVICES} devices").into());
    }
    let instrument = info.role == Role::Instrument;
    let has_instrument = channel
        .devices
        .first()
        .and_then(|d| match &d.kind {
            DeviceKind::Builtin(id) => suhina_device_registry::info(id),
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
        params: preset
            .as_ref()
            .map_or_else(Vec::new, |p| p.params.iter().map(|(id, value)| DeviceParam::new(*id, *value)).collect()),
        extensions: Extensions::default(),
        automation: Vec::new(),
    };
    channel.devices.insert(position, device);
    Ok(json!({ "device": position, "id": info.id, "name": info.name, "preset": preset.map(|p| p.name) }))
}

/// Reads voice `voice` of the preset file at `path`.
fn read_preset(path: &std::path::Path, voice: usize) -> Result<suhina_device_registry::Preset, CommandError> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > suhina_device_registry::MAX_PRESET_BYTES as u64 {
        return Err(format!("{}: a preset file is at most 64 KiB", path.display()).into());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    suhina_device_registry::preset(&data, voice).map_err(|e| format!("{}: {e}", path.display()).into())
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
    if channel.devices.get(index).is_some_and(|d| is_plugin(&d.kind)) {
        return set_plugin_param(session, params, track, index, param, value);
    }
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    let DeviceKind::Builtin(id) = &device.kind else {
        return Err("this device has no parameters Suhina can set (it is from another program)".into());
    };
    let info = suhina_device_registry::info(id).ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
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

/// Sets parameter `param` (its id) of the plug-in device at `index` on `track`: kept in the
/// project, and heard from the next block of a playing engine.
fn set_plugin_param(
    session: &mut Session,
    params: &Value,
    track: suhina_model::TrackId,
    index: usize,
    param: &str,
    value: f64,
) -> Result<Value, CommandError> {
    let id = plugin_param_id(param)?;
    let value = plugin_value(session, track, index, id, value)?;
    let channel = channel_param(session, params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    match device.params.iter_mut().find(|p| param_id(&p.id) == Some(id)) {
        Some(stored) => {
            stored.id = id.to_string();
            stored.value = value;
        }
        None => device.params.push(DeviceParam::new(id.to_string(), value)),
    }
    // A plug-in's parameter goes to the engine by its id.
    let param = usize::try_from(id).map_err(|_| "the parameter id is too large")?;
    session.param_edit = Some(ParamEdit { track, device: index, param, value });
    Ok(json!({ "device": index, "param": id, "value": value }))
}

/// Most points one automation lane holds.
const MAX_POINTS: usize = 65_536;

/// Sets or removes the automation lane of one parameter of a plug-in device.
pub(crate) fn set_automation(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    const POINTS: &str = "points must be a list of numbers, a beat then a value for each point: [0, 0.2, 4, 0.8]";
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id")?;
    let numbers = params.get("points").and_then(Value::as_array).ok_or(POINTS)?;
    if numbers.len() % 2 != 0 {
        return Err(POINTS.into());
    }
    if numbers.len() / 2 > MAX_POINTS {
        return Err(format!("a lane holds at most {MAX_POINTS} points").into());
    }
    let track = track_id(&session.project, params)?;
    let channel = channel_param(session, params)?;
    let index = device_index(channel.devices.len(), params)?;
    if !channel.devices.get(index).is_some_and(|d| is_plugin(&d.kind)) {
        return Err("only plug-in devices can be automated so far; built-in effects and instruments follow".into());
    }
    let id = plugin_param_id(param)?;
    let mut points = Vec::with_capacity(numbers.len() / 2);
    for pair in numbers.chunks_exact(2) {
        let number = |v: &Value| v.as_f64().filter(|n| n.is_finite());
        let (Some(beat), Some(value)) = (pair.first().and_then(number), pair.get(1).and_then(number)) else {
            return Err(POINTS.into());
        };
        if !(0.0..=suhina_engine::MAX_START_BEAT).contains(&beat) {
            return Err(format!("a point's beat must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
        }
        let value = plugin_value(session, track, index, id, value)?;
        points.push(AutomationPoint { beat, value });
    }
    // In beat order; points given at one beat keep their order (a jump).
    points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let count = points.len();
    let channel = channel_param(session, params)?;
    let device = channel.devices.get_mut(index).ok_or("the device is missing")?;
    device.automation.retain(|lane| param_id(&lane.param) != Some(id));
    if !points.is_empty() {
        device.automation.push(AutomationLane { param: id.to_string(), points });
    }
    Ok(json!({ "device": index, "param": id, "points": count }))
}

/// The parameters of a running plug-in device, as its plug-in lists them.
pub(crate) fn plugin_params(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let key = plugin_key(session, params)?;
    let list = session.plugins.params(key)?;
    Ok(Value::Array(
        list.iter()
            .filter(|p| !p.hidden)
            .map(|p| {
                json!({
                    "id": p.id, "name": p.name, "group": p.group, "min": p.min, "max": p.max,
                    "default": p.default, "value": p.value, "text": p.text, "steps": p.steps,
                    "automatable": p.automatable, "read_only": p.read_only,
                })
            })
            .collect(),
    ))
}

fn plugin_param_id(param: &str) -> Result<u32, CommandError> {
    param_id(param).ok_or_else(|| {
        format!("{param:?} is not a plug-in parameter id (a whole number); device.pluginParams lists them").into()
    })
}

/// `value` for parameter `id` of the plug-in device at `index` on `track`, limited to its range
/// when its parameters have been listed (`device.pluginParams`); as given otherwise.
fn plugin_value(
    session: &Session,
    track: suhina_model::TrackId,
    index: usize,
    id: u32,
    value: f64,
) -> Result<f64, CommandError> {
    let key = crate::plugin_key_at(&session.project, track, index);
    match key.and_then(|key| session.plugins.param_range(key, id)) {
        Some(Some((min, max))) => Ok(value.clamp(min, max)),
        Some(None) => Err(format!("the plug-in has no parameter {id}; device.pluginParams lists them").into()),
        None => Ok(value),
    }
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
    if instrument {
        let track = track_id(&session.project, params)?;
        if session.project.tracks.iter().find(|t| t.id == track).is_some_and(|t| !t.content.notes) {
            return Err("an instrument goes on a MIDI track".into());
        }
    }
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
        automation: Vec::new(),
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
        "blocks": report.blocks,
        "late_blocks": report.late_blocks,
        "longest_us": report.longest_us,
        "no_time": report.no_time,
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
