//! The audio output a project plays on: which host (WASAPI, ASIO, ALSA…), device, sample rate
//! and buffer size, and the `audio.*` commands that list, choose and report it. The session
//! only describes the choice; the output layer (`suhina-output`) enumerates devices and opens
//! streams through [`AudioSystem`], which the app or the MCP server hands to [`Projects`]. Like
//! the `plugins.*` commands they belong to Suhina, not to one project.

use std::sync::Arc;

use serde_json::{Value, json};
use suhina_engine::AudioPool;
use suhina_model::Project;

use crate::commands::{CommandKind, CommandSpec, ParamType, optional};
use crate::{CommandError, Player, Projects};

/// Smallest and largest buffer size, in frames, a choice may ask for; the engine renders in
/// blocks of the chosen size (`docs/architecture.md` › Threads).
pub const MIN_BUFFER_FRAMES: u32 = 64;
pub const MAX_BUFFER_FRAMES: u32 = 4096;
/// Highest sample rate a choice may ask for.
pub const MAX_SAMPLE_RATE: u32 = 384_000;

/// The output the user chose. `None` means the host's or device's own default, so an empty
/// choice plays on the system's default output as it is set up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputChoice {
    /// A host's name as [`AudioSystem::hosts`] lists it ("WASAPI", "ASIO").
    pub host: Option<String>,
    /// An output device's name on that host.
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    /// Frames per buffer; the engine renders in blocks of this size.
    pub buffer_frames: Option<u32>,
    /// WASAPI only: take the device for Suhina alone (exclusive mode) when it allows it.
    /// Other programs are not heard on it meanwhile, so it is off unless asked for.
    pub exclusive: bool,
}

impl OutputChoice {
    pub fn to_json(&self) -> Value {
        json!({
            "host": self.host,
            "device": self.device,
            "sample_rate": self.sample_rate,
            "buffer_frames": self.buffer_frames,
            "exclusive": self.exclusive,
        })
    }

    /// This choice with the parts `params` gives changed (`audio.setOutput`): an empty text or
    /// 0 goes back to the default, and a new host forgets the device chosen on the old one.
    pub fn with_params(&self, params: &Value) -> Result<Self, CommandError> {
        let mut next = self.clone();
        let text = |key: &str| -> Result<Option<Option<String>>, CommandError> {
            match params.get(key) {
                None => Ok(None),
                Some(Value::String(s)) if s.trim().is_empty() => Ok(Some(None)),
                Some(Value::String(s)) => Ok(Some(Some(s.trim().to_string()))),
                Some(_) => Err(format!("{key} must be a text").into()),
            }
        };
        let number = |key: &str, max: u32| -> Result<Option<Option<u32>>, CommandError> {
            match params.get(key) {
                None => Ok(None),
                Some(v) => match v.as_u64() {
                    Some(0) => Ok(Some(None)),
                    Some(n) if n <= u64::from(max) => Ok(Some(u32::try_from(n).ok())),
                    _ => Err(format!("{key} must be a whole number from 0 (the default) to {max}").into()),
                },
            }
        };
        if let Some(host) = text("host")? {
            if host != next.host {
                next.device = None;
            }
            next.host = host;
        }
        if let Some(device) = text("device")? {
            next.device = device;
        }
        if let Some(rate) = number("sample_rate", MAX_SAMPLE_RATE)? {
            next.sample_rate = rate;
        }
        if let Some(frames) = number("buffer_frames", MAX_BUFFER_FRAMES)? {
            if frames.is_some_and(|f| f < MIN_BUFFER_FRAMES) {
                return Err(
                    format!("buffer_frames must be from {MIN_BUFFER_FRAMES} to {MAX_BUFFER_FRAMES}, or 0").into()
                );
            }
            next.buffer_frames = frames;
        }
        if let Some(exclusive) = params.get("exclusive") {
            next.exclusive = exclusive.as_bool().ok_or("exclusive must be true or false")?;
        }
        Ok(next)
    }
}

/// An audio host on this system and its output devices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostInfo {
    pub name: String,
    /// The one used when no host is chosen.
    pub default: bool,
    pub devices: Vec<DeviceInfo>,
    /// Why the host's devices could not be listed, when they could not.
    pub error: Option<String>,
}

/// An output device and what it can be opened with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    /// The host's default output device.
    pub default: bool,
    pub channels: u16,
    pub sample_rates: Vec<u32>,
    pub default_sample_rate: Option<u32>,
    /// The buffer sizes it can be asked for, smallest first (the driver's allowed values,
    /// within [`MIN_BUFFER_FRAMES`] … [`MAX_BUFFER_FRAMES`]).
    pub buffer_sizes: Vec<u32>,
    /// The driver's own range, when it reports one.
    pub buffer_range: Option<(u32, u32)>,
    /// Whether it can be opened in exclusive mode (WASAPI).
    pub exclusive: bool,
}

/// What an open output runs with now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputInfo {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    /// Frames the device asks for per callback, as last measured (0 before the first).
    pub buffer_frames: u32,
    /// Frames the engine renders per block.
    pub engine_block: u32,
    pub exclusive: bool,
    /// Measured time from the engine rendering a frame to the device playing it, in ms.
    pub latency_ms: f64,
    /// Device buffers the engine had not filled in time.
    pub underruns: u64,
    /// The device reported an error (unplugged, taken by another program).
    pub failed: bool,
}

impl OutputInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "host": self.host,
            "device": self.device,
            "sample_rate": self.sample_rate,
            "buffer_frames": self.buffer_frames,
            "engine_block": self.engine_block,
            "exclusive": self.exclusive,
            "latency_ms": (self.latency_ms * 100.0).round() / 100.0,
            "underruns": self.underruns,
            "failed": self.failed,
        })
    }

    /// One line for the window: device, host, rate and buffer.
    pub fn describe(&self) -> String {
        let mode = if self.exclusive && self.host == "WASAPI" { ", exclusive" } else { "" };
        format!(
            "{} ({}{mode}, {} Hz, {} frames)",
            self.device,
            self.host,
            self.sample_rate,
            if self.buffer_frames > 0 { self.buffer_frames } else { self.engine_block }
        )
    }
}

/// A playing output's live numbers, read from the main thread.
pub trait LiveOutput: Send + Sync {
    fn info(&self) -> OutputInfo;
}

/// A player on the output a choice names.
pub struct Opened {
    pub player: Player,
    /// What the user should hear about: the chosen device was not there and the default
    /// plays instead, or a part of the choice could not be had.
    pub notice: Option<String>,
    /// The previous player's plug-ins run on in the new one; when false the session starts
    /// them again (a new sample rate, or the old output did not give them back).
    pub plugins_kept: bool,
}

/// The audio devices of the system, and opening a project on one of them; made by the output
/// layer, which is the only code that opens an audio device.
pub trait AudioSystem: Send + Sync {
    /// The hosts and their output devices.
    fn hosts(&self) -> Vec<HostInfo>;

    /// Plays `project` with its transport stopped on the output `choice` names, or on the
    /// default one, with a notice, when that is not there. `previous` is the player it
    /// replaces: it is closed first (a device may only be open once), and its running plug-ins
    /// carry over when the sample rate stays the same.
    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String>;
}

static AUDIO_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "audio.devices",
        label: "List the audio hosts (WASAPI, ASIO, ALSA…) and each one's output devices with their sample rates and buffer sizes, and the output chosen",
        params: &[],
        kind: CommandKind::Audio,
        run: devices,
    },
    CommandSpec {
        id: "audio.setOutput",
        label: "Choose the audio output: host, device, sample rate, buffer size (the engine renders in blocks of it) and WASAPI exclusive mode; parts left out stay, an empty text or 0 goes back to the default. Reopens the active project's output, keeping the transport where it is",
        params: &[
            optional("host", ParamType::Text, "a host from audio.devices, \"\" for the default"),
            optional("device", ParamType::Text, "an output device of that host, \"\" for its default"),
            optional("sample_rate", ParamType::Integer, "Hz, one the device lists; 0 for its default"),
            optional("buffer_frames", ParamType::Integer, "frames per buffer, 64 to 4096; 0 for the default"),
            optional("exclusive", ParamType::Bool, "WASAPI: open the device for Suhina alone (default false)"),
        ],
        kind: CommandKind::Audio,
        run: set_output,
    },
    CommandSpec {
        id: "audio.info",
        label: "Say what the active project plays on now: host, device, sample rate, buffer frames, the engine's block, measured latency and underruns",
        params: &[],
        kind: CommandKind::Audio,
        run: info,
    },
];

/// The commands for the audio output.
pub fn audio_commands() -> &'static [CommandSpec<Projects>] {
    AUDIO_COMMANDS
}

fn system(projects: &Projects) -> Result<Arc<dyn AudioSystem>, CommandError> {
    projects.audio_system().ok_or_else(|| {
        CommandError("no audio output here; the desktop app has one, and suhina-cli mcp with --audio".into())
    })
}

pub(crate) fn hosts_json(hosts: &[HostInfo]) -> Value {
    Value::Array(
        hosts
            .iter()
            .map(|h| {
                json!({
                    "name": h.name,
                    "default": h.default,
                    "error": h.error,
                    "devices": h.devices.iter().map(|d| json!({
                        "name": d.name,
                        "default": d.default,
                        "channels": d.channels,
                        "sample_rates": d.sample_rates,
                        "default_sample_rate": d.default_sample_rate,
                        "buffer_sizes": d.buffer_sizes,
                        "buffer_range": d.buffer_range.map(|(min, max)| json!({ "min": min, "max": max })),
                        "exclusive": d.exclusive,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn devices(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let hosts = system(projects)?.hosts();
    Ok(json!({ "hosts": hosts_json(&hosts), "chosen": projects.output_choice().to_json() }))
}

fn set_output(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let choice = projects.output_choice().with_params(params)?;
    projects.set_output_choice(choice);
    let notice = projects.reopen_output()?;
    let mut result = info(projects, &Value::Null)?;
    result["notice"] = json!(notice);
    Ok(result)
}

fn info(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let chosen = projects.output_choice().to_json();
    let playing = projects.session_mut(None).ok().and_then(|s| s.player()).and_then(Player::output_info);
    Ok(match playing {
        Some(info) => {
            let mut value = info.to_json();
            value["playing"] = json!(true);
            value["chosen"] = chosen;
            value
        }
        None => json!({ "playing": false, "chosen": chosen }),
    })
}

/// Where the transport was, to put it back on a new output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Transport {
    running: bool,
    beat: f64,
    marker: f64,
    metronome: bool,
}

impl Transport {
    pub(crate) fn of(player: &Player) -> Self {
        let monitor = player.monitor();
        Self {
            running: monitor.running(),
            beat: monitor.beat(),
            marker: monitor.start_beat(),
            metronome: monitor.metronome(),
        }
    }

    /// Sends `player` the commands that bring it to this state: the marker where it was, or,
    /// while playing, the playhead (which then is where the next start carries on from, as
    /// after "stop here"), and the metronome.
    pub(crate) fn restore(self, player: &mut Player) -> Result<(), CommandError> {
        use suhina_engine::LaunchCommand;
        let beat = if self.running { self.beat } else { self.marker };
        if beat.is_finite() && beat > 0.0 {
            player.send(LaunchCommand::SetPosition { beat })?;
        }
        if self.metronome {
            player.send(LaunchCommand::SetMetronome { on: true, gain: None })?;
        }
        if self.running {
            player.send(LaunchCommand::Play)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_changes_only_what_is_given() {
        let start = OutputChoice {
            host: Some("WASAPI".into()),
            device: Some("Speakers".into()),
            sample_rate: Some(48_000),
            buffer_frames: Some(256),
            exclusive: false,
        };
        let same = start.with_params(&json!({})).unwrap();
        assert_eq!(same, start);

        let buffer = start.with_params(&json!({ "buffer_frames": 512 })).unwrap();
        assert_eq!(buffer.buffer_frames, Some(512));
        assert_eq!(buffer.device.as_deref(), Some("Speakers"));

        // A new host forgets the device; an empty text and 0 go back to the default.
        let asio = start.with_params(&json!({ "host": "ASIO" })).unwrap();
        assert_eq!((asio.host.as_deref(), asio.device), (Some("ASIO"), None));
        let same_host = start.with_params(&json!({ "host": "WASAPI" })).unwrap();
        assert_eq!(same_host.device.as_deref(), Some("Speakers"));
        let reset = start.with_params(&json!({ "device": "", "sample_rate": 0, "buffer_frames": 0 })).unwrap();
        assert_eq!((reset.device, reset.sample_rate, reset.buffer_frames), (None, None, None));
        let both = start.with_params(&json!({ "host": "ASIO", "device": "Interface", "exclusive": true })).unwrap();
        assert_eq!((both.device.as_deref(), both.exclusive), (Some("Interface"), true));
    }

    #[test]
    fn a_choice_refuses_nonsense() {
        let start = OutputChoice::default();
        for bad in [
            json!({ "buffer_frames": 32 }),
            json!({ "buffer_frames": 8192 }),
            json!({ "buffer_frames": -1 }),
            json!({ "buffer_frames": 1.5 }),
            json!({ "sample_rate": 10_000_000 }),
            json!({ "host": 5 }),
            json!({ "exclusive": "yes" }),
        ] {
            assert!(start.with_params(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn output_info_describes_itself() {
        let info = OutputInfo {
            host: "WASAPI".into(),
            device: "Speakers".into(),
            sample_rate: 44_100,
            buffer_frames: 0,
            engine_block: 512,
            exclusive: true,
            latency_ms: 23.456,
            ..OutputInfo::default()
        };
        assert_eq!(info.describe(), "Speakers (WASAPI, exclusive, 44100 Hz, 512 frames)");
        assert_eq!(info.to_json()["latency_ms"], json!(23.46));
    }
}
