//! The plug-ins a project runs (ADR 0006): for each plug-in device, the installed plug-in found
//! for it in the catalog, started in a host process with the state the project saved, off the
//! audio thread, and handed to the playing engine; its health watched, and a restart that gets
//! its last state back.
//!
//! The engine only sees each plug-in's audio half ([`Hosted`], a [`PluginProcessor`]); the
//! control half ([`Plugin`]) stays here, on the session's thread. A plug-in that is not
//! installed is *missing*: the device stays in the project with its state untouched, and the
//! engine passes the sound through it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use loopwerk_engine::{
    DeviceEvent, MAX_BLOCK_FRAMES, PluginProcessor, PluginSlotInfo, Plugins, model::DeviceKind, plugin_devices,
};
use loopwerk_model::{Device, Project};
use loopwerk_plugins::library::{Hosting, group_for};
use loopwerk_plugins::scan::{Catalog, PluginInfo};
use loopwerk_plugins::{Event, EventKind, Health, Launch, MAX_EVENTS, Plugin, PluginAudio, PluginError, SavedState};

use crate::{CommandError, Player};

/// How long starting a plug-in (the host process, loading it, its state) may take.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the session looks whether its plug-ins still run.
const HEALTH_EVERY: Duration = Duration::from_millis(250);
/// How often the state of each running plug-in is read, so a restart after a crash gets back
/// what the user set in its editor (ADR 0006, decision 5).
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// MIDI controller numbers an all-notes-off is sent as.
const ALL_NOTES_OFF: u8 = 123;
const SUSTAIN: u8 = 64;

/// The host process group a plug-in runs in (`None`: a process of its own).
pub type ProcessGroup = Box<dyn Fn(&PluginInfo) -> Option<String> + Send + Sync>;

/// What the session needs to start plug-ins: the catalog of installed ones, the host
/// executable, and which plug-ins share a host process (ADR 0006, decision 2).
pub struct PluginEnv {
    pub catalog: Catalog,
    pub host: PathBuf,
    /// The process group of a plug-in (`None`: a process of its own).
    pub group: ProcessGroup,
}

impl PluginEnv {
    /// `catalog` with plug-ins run by `host`, every instance of one plug-in file in one process
    /// (the default).
    pub fn new(catalog: Catalog, host: PathBuf) -> Self {
        Self { catalog, host, group: Box::new(|plugin| group_for(Hosting::default(), plugin)) }
    }

    /// What `library` gives: its catalog, its host, and how each plug-in is hosted (its
    /// `plugins.setHosting` choice, else one process per plug-in file).
    pub fn from_library(library: &mut loopwerk_plugins::library::Library) -> Self {
        let host = library.host().to_path_buf();
        let catalog = library.catalog().clone();
        let chosen: HashMap<String, Hosting> = catalog
            .plugins()
            .map(PluginInfo::spec)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|spec| {
                let hosting = library.hosting(&spec);
                (spec, hosting)
            })
            .filter(|(_, hosting)| *hosting != Hosting::default())
            .collect();
        let group: ProcessGroup =
            Box::new(move |plugin| group_for(chosen.get(&plugin.spec()).copied().unwrap_or_default(), plugin));
        Self { catalog, host, group }
    }
}

/// Where a plug-in device stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Its host process is starting it.
    Loading,
    Running,
    /// No installed plug-in stands for it; it keeps its state and passes the sound through.
    Missing,
    /// Its host process exited or stopped answering; the engine plays on without it until it
    /// is restarted.
    Crashed(String),
    /// It could not be started.
    Failed(String),
}

impl PluginStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Running => "running",
            Self::Missing => "missing",
            Self::Crashed(_) => "crashed",
            Self::Failed(_) => "failed",
        }
    }

    /// Why it crashed or failed.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Crashed(why) | Self::Failed(why) => Some(why),
            _ => None,
        }
    }
}

/// One plug-in device's running plug-in, as the session sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReport {
    pub status: PluginStatus,
    /// The installed plug-in that stands for it: name, vendor, how it was found.
    pub plugin: Option<(String, String, &'static str)>,
    pub instrument: bool,
    /// Frames of delay it reported.
    pub latency: u32,
    pub editor_open: bool,
    /// Why it runs with its own defaults, when it did not take the saved state.
    pub state_refused: Option<String>,
}

/// How far a project's plug-ins have got with starting, for a dialog while they load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginProgress {
    /// Plug-ins that are installed and should run.
    pub total: usize,
    pub ready: usize,
    /// The names of those still starting.
    pub loading: Vec<String>,
    /// Plug-in devices with no installed plug-in.
    pub missing: usize,
    /// The names of those that could not start or crashed.
    pub failed: Vec<String>,
}

/// The audio half of a hosted plug-in, as the engine runs it: real-time safe.
pub struct Hosted {
    audio: PluginAudio,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<Event>,
}

impl Hosted {
    pub fn new(audio: PluginAudio) -> Self {
        Self {
            audio,
            input: vec![0.0; MAX_BLOCK_FRAMES * 2],
            output: vec![0.0; MAX_BLOCK_FRAMES * 2],
            events: Vec::with_capacity(MAX_EVENTS),
        }
    }
}

/// `event` at `frame` as the plug-in host takes it, into `out` (within its capacity).
fn push_event(out: &mut Vec<Event>, frame: u32, event: DeviceEvent) {
    let mut push = |kind: EventKind| {
        if out.len() < out.capacity().min(MAX_EVENTS) {
            out.push(Event { frame, note: -1, kind });
        }
    };
    match event {
        DeviceEvent::On { key, velocity } if key < 128 => {
            push(EventKind::NoteOn { channel: 0, key, velocity: velocity.clamp(0.0, 1.0) });
        }
        DeviceEvent::Off { key, velocity } if key < 128 => {
            push(EventKind::NoteOff { channel: 0, key, velocity: velocity.clamp(0.0, 1.0) });
        }
        DeviceEvent::Controller { number, value } if number < 128 => {
            push(EventKind::Controller { channel: 0, number, value: value.clamp(0.0, 1.0) });
        }
        DeviceEvent::PitchBend(value) => push(EventKind::PitchBend { channel: 0, value: value.clamp(-1.0, 1.0) }),
        DeviceEvent::AllNotesOff => {
            push(EventKind::Controller { channel: 0, number: SUSTAIN, value: 0.0 });
            push(EventKind::Controller { channel: 0, number: ALL_NOTES_OFF, value: 0.0 });
        }
        DeviceEvent::On { .. }
        | DeviceEvent::Off { .. }
        | DeviceEvent::Controller { .. }
        | DeviceEvent::Pressure(_) => {}
    }
}

impl PluginProcessor for Hosted {
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, DeviceEvent)], timeout: Duration) -> bool {
        let samples = buffer.len();
        let (Some(input), Some(output)) = (self.input.get_mut(..samples), self.output.get_mut(..samples)) else {
            return false;
        };
        input.copy_from_slice(buffer);
        self.events.clear();
        for &(frame, event) in events {
            push_event(&mut self.events, u32::try_from(frame).unwrap_or(u32::MAX), event);
        }
        if self.audio.process_with_events(input, &self.events, output, timeout) == loopwerk_plugins::Outcome::Done {
            buffer.copy_from_slice(output);
            true
        } else {
            false
        }
    }
}

/// The installed plug-in for `device` and whether it is an instrument; `None` when nothing
/// installed stands for it (or it is not a plug-in).
fn find<'a>(catalog: &'a Catalog, device: &Device) -> Option<(loopwerk_plugins::matching::Match<'a>, bool)> {
    let (found, _) = crate::plugins::device_plugin(catalog, device)?;
    let instrument = is_instrument(found.plugin);
    Some((found, instrument))
}

/// Whether the plug-in plays notes, from the features its scan found (CLAP `instrument`, VST3
/// subcategories such as `Instrument|Synth`).
pub fn is_instrument(plugin: &PluginInfo) -> bool {
    plugin.features.iter().any(|f| f.to_ascii_lowercase().contains("instrument"))
}

/// The state the project saved for `device`, with a VST3 controller's own state.
fn saved_state(device: &Device) -> Option<SavedState> {
    let controller = device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone());
    device.state.clone().map(|data| SavedState { data, controller })
}

/// A plug-in started off the session's thread.
struct Started {
    key: u64,
    /// Which start of the device this answers; an older one is dropped.
    generation: u64,
    result: Result<(Plugin, PluginAudio, Option<String>), String>,
}

struct Instance {
    status: PluginStatus,
    plugin: Option<(String, String, &'static str)>,
    info: PluginSlotInfo,
    control: Option<Plugin>,
    generation: u64,
    state_refused: Option<String>,
}

/// The plug-ins one project runs, while it plays.
pub struct Rack {
    env: Option<std::sync::Arc<PluginEnv>>,
    sample_rate: u32,
    instances: HashMap<u64, Instance>,
    /// The last state of plug-ins that were stopped (a device removed, the project no longer
    /// playing), so they come back as they were.
    kept: HashMap<u64, SavedState>,
    sender: Sender<Started>,
    receiver: Receiver<Started>,
    next_generation: u64,
    checked: Instant,
    saved: Instant,
}

impl Default for Rack {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            env: None,
            sample_rate: 0,
            instances: HashMap::new(),
            kept: HashMap::new(),
            sender,
            receiver,
            next_generation: 0,
            checked: Instant::now(),
            saved: Instant::now(),
        }
    }
}

impl Rack {
    /// What plug-ins are started with. A new one (after a scan, or another hosting choice)
    /// makes the plug-ins that were not found, or could not start, be looked for again.
    pub fn set_env(&mut self, env: std::sync::Arc<PluginEnv>) {
        if self.env.as_ref().is_some_and(|old| std::sync::Arc::ptr_eq(old, &env)) {
            return;
        }
        self.instances.retain(|_, i| !matches!(i.status, PluginStatus::Missing | PluginStatus::Failed(_)));
        self.env = Some(env);
    }

    pub fn env(&self) -> Option<&PluginEnv> {
        self.env.as_deref()
    }

    /// Stops every plug-in, keeping each one's last state for when it starts again.
    pub fn stop_all(&mut self) {
        for (key, instance) in self.instances.drain() {
            if let Some(mut control) = instance.control
                && control.health() == Health::Running
                && let Ok(state) = control.save_state()
            {
                self.kept.insert(key, state);
            }
        }
    }

    /// What is known about the plug-in device `key`.
    pub fn report(&mut self, key: u64) -> Option<PluginReport> {
        let instance = self.instances.get_mut(&key)?;
        let editor_open = instance.control.as_mut().is_some_and(Plugin::editor_open);
        Some(PluginReport {
            status: instance.status.clone(),
            plugin: instance.plugin.clone(),
            instrument: instance.info.instrument,
            latency: instance.info.latency,
            editor_open,
            state_refused: instance.state_refused.clone(),
        })
    }

    /// How far the plug-ins have got with starting.
    pub fn progress(&self) -> PluginProgress {
        let mut progress = PluginProgress::default();
        let name = |i: &Instance| i.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
        for instance in self.instances.values() {
            match &instance.status {
                PluginStatus::Missing => progress.missing += 1,
                PluginStatus::Running => progress.ready += 1,
                PluginStatus::Loading => progress.loading.push(name(instance)),
                PluginStatus::Crashed(_) | PluginStatus::Failed(_) => progress.failed.push(name(instance)),
            }
        }
        progress.total = self.instances.len() - progress.missing;
        progress.loading.sort();
        progress.failed.sort();
        progress
    }

    /// The slots the engine's graphs are built with.
    fn slot_info(&self) -> HashMap<u64, PluginSlotInfo> {
        self.instances
            .iter()
            .filter(|(_, i)| i.status != PluginStatus::Missing)
            .map(|(key, i)| (*key, i.info))
            .collect()
    }

    /// Brings the running plug-ins in line with `project`: starts the ones new devices need
    /// (in the background), stops the ones whose devices are gone, takes in the ones that have
    /// started and looks after their health. Returns what the user should hear about.
    pub fn sync(&mut self, project: &Project, player: Option<&mut Player>) -> Vec<String> {
        let mut notices = Vec::new();
        let Some(player) = player else {
            self.stop_all();
            return notices;
        };
        let Some(env) = self.env.clone() else { return notices };
        let sample_rate = player.sample_rate();
        if sample_rate != self.sample_rate {
            self.stop_all();
            self.sample_rate = sample_rate;
        }
        let devices = plugin_devices(project);
        let mut changed = false;
        // Devices that are gone.
        let gone: Vec<u64> = self.instances.keys().copied().filter(|k| !devices.iter().any(|d| d.key == *k)).collect();
        for key in gone {
            if let Some(mut instance) = self.instances.remove(&key) {
                if let Some(control) = instance.control.as_mut()
                    && control.health() == Health::Running
                    && let Ok(state) = control.save_state()
                {
                    self.kept.insert(key, state);
                }
                changed = true;
            }
        }
        // New ones.
        let mut missing = Vec::new();
        for found in &devices {
            if self.instances.contains_key(&found.key) {
                continue;
            }
            let Some(device) =
                project.channels.iter().find(|c| c.id == found.channel).and_then(|c| c.devices.get(found.position))
            else {
                continue;
            };
            changed = true;
            let Some((matched, instrument)) = find(&env.catalog, device) else {
                missing.push(device.name.clone());
                self.instances.insert(
                    found.key,
                    Instance {
                        status: PluginStatus::Missing,
                        plugin: None,
                        info: PluginSlotInfo::default(),
                        control: None,
                        generation: 0,
                        state_refused: None,
                    },
                );
                continue;
            };
            let state = self.kept.remove(&found.key).or_else(|| saved_state(device));
            let launch = Launch {
                host: env.host.clone(),
                plugin: matched.plugin.spec(),
                channels: 2,
                max_frames: MAX_BLOCK_FRAMES as u32,
                sample_rate: f64::from(sample_rate),
                timeout: START_TIMEOUT,
                group: (env.group)(matched.plugin),
            };
            self.next_generation += 1;
            let generation = self.next_generation;
            self.instances.insert(
                found.key,
                Instance {
                    status: PluginStatus::Loading,
                    plugin: Some((matched.plugin.name.clone(), matched.plugin.vendor.clone(), matched.how.name())),
                    info: PluginSlotInfo { instrument, latency: 0 },
                    control: None,
                    generation,
                    state_refused: None,
                },
            );
            spawn_start(self.sender.clone(), found.key, generation, launch, state);
        }
        if !missing.is_empty() {
            notices.push(missing_notice(&env, &missing));
        }
        changed |= self.receive(player, &mut notices);
        self.watch(&mut notices);
        if changed {
            player.set_plugin_info(self.slot_info());
            if let Err(e) = player.update(project) {
                notices.push(format!("plug-ins: {}", e.0));
            }
        }
        notices
    }

    /// Takes in plug-ins that have started: hands each one's audio half to the engine. Returns
    /// whether what the graphs need to know changed (a latency).
    fn receive(&mut self, player: &mut Player, notices: &mut Vec<String>) -> bool {
        let mut changed = false;
        while let Ok(started) = self.receiver.try_recv() {
            let Some(instance) = self.instances.get_mut(&started.key) else { continue };
            if instance.generation != started.generation {
                continue;
            }
            let name = instance.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
            match started.result {
                Ok((control, audio, refused)) => {
                    if let Some(why) = &refused {
                        notices.push(format!("{name} did not take its saved settings ({why}); it runs with its own"));
                    }
                    let latency = control.latency();
                    if latency != instance.info.latency {
                        instance.info.latency = latency;
                        changed = true;
                    }
                    instance.state_refused = refused;
                    match player.send_plugin(started.key, Box::new(Hosted::new(audio))) {
                        Ok(()) => instance.status = PluginStatus::Running,
                        Err(e) => instance.status = PluginStatus::Failed(e.0),
                    }
                    instance.control = Some(control);
                }
                Err(why) => {
                    notices.push(format!("{name} could not start: {why}"));
                    instance.status = PluginStatus::Failed(why);
                }
            }
        }
        changed
    }

    /// Notices plug-ins that crashed or hung, and now and then reads each one's state.
    fn watch(&mut self, notices: &mut Vec<String>) {
        if self.checked.elapsed() < HEALTH_EVERY {
            return;
        }
        self.checked = Instant::now();
        let save = self.saved.elapsed() >= SAVE_EVERY;
        if save {
            self.saved = Instant::now();
        }
        for instance in self.instances.values_mut() {
            let Some(control) = instance.control.as_mut() else { continue };
            if instance.status != PluginStatus::Running {
                continue;
            }
            let name = instance.plugin.as_ref().map_or("A plug-in", |p| p.0.as_str());
            match control.health() {
                Health::Running => {
                    if save {
                        let _kept_for_a_restart = control.save_state();
                    }
                }
                Health::Crashed => {
                    notices
                        .push(format!("{name} crashed; the project plays on without it (restart it from its device)"));
                    instance.status = PluginStatus::Crashed("its host process exited".into());
                }
                Health::Hung => {
                    notices.push(format!(
                        "{name} stopped answering; the project plays on without it (restart it from its device)"
                    ));
                    instance.status = PluginStatus::Crashed("it stopped answering".into());
                }
            }
        }
    }

    /// Restarts the plug-in device `key` in a fresh host process with its last state, in the
    /// background; it plays again once it has started.
    pub fn restart(&mut self, key: u64) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device has no plug-in running")?;
        if instance.status == PluginStatus::Missing {
            return Err("the plug-in is not installed; there is nothing to restart".into());
        }
        let Some(control) = instance.control.take() else {
            return Err("the plug-in is still starting".into());
        };
        self.next_generation += 1;
        instance.generation = self.next_generation;
        instance.status = PluginStatus::Loading;
        let (sender, generation) = (self.sender.clone(), self.next_generation);
        let spawned = std::thread::Builder::new().name("loopwerk-plugin-restart".into()).spawn(move || {
            let mut control = control;
            let result = control.restart().map(|audio| (control, audio, None)).map_err(|e| e.to_string());
            let _gone = sender.send(Started { key, generation, result });
        });
        spawned.map(|_| ()).map_err(|e| CommandError(format!("could not restart the plug-in: {e}")))
    }

    /// Opens the editor of the plug-in device `key`; returns the size of its inside.
    pub fn open_editor(&mut self, key: u64) -> Result<(u32, u32), CommandError> {
        self.running(key)?.open_editor().map_err(|e| CommandError(e.to_string()))
    }

    pub fn close_editor(&mut self, key: u64) -> Result<(), CommandError> {
        self.running(key)?.close_editor().map_err(|e| CommandError(e.to_string()))
    }

    fn running(&mut self, key: u64) -> Result<&mut Plugin, CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        match &instance.status {
            PluginStatus::Running => instance.control.as_mut().ok_or_else(|| "the plug-in is still starting".into()),
            PluginStatus::Loading => Err("the plug-in is still starting".into()),
            PluginStatus::Missing => Err("the plug-in is not installed".into()),
            PluginStatus::Crashed(why) => Err(format!("the plug-in crashed ({why}); restart it first").into()),
            PluginStatus::Failed(why) => Err(format!("the plug-in could not start: {why}").into()),
        }
    }

    /// Waits until no plug-in is starting any more, taking each in as it comes (tests, and
    /// callers that need every plug-in before going on), at most `timeout`.
    pub fn wait_started(&mut self, project: &Project, player: &mut Player, timeout: Duration) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        let mut notices = self.sync(project, Some(player));
        while self.instances.values().any(|i| i.status == PluginStatus::Loading) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            notices.extend(self.sync(project, Some(player)));
        }
        notices
    }
}

/// What the status bar says about plug-ins that were not found: why, when the catalog is empty
/// (no scan yet, or the host to scan with is missing), else which.
fn missing_notice(env: &PluginEnv, missing: &[String]) -> String {
    if env.catalog.plugins().next().is_none() {
        if !env.host.exists() {
            return format!(
                "{} plug-ins cannot be looked for: the plug-in host {} is missing",
                missing.len(),
                env.host.display()
            );
        }
        return format!(
            "{} plug-ins cannot be found yet: no plug-in scan has run (Settings › Plug-ins › Scan)",
            missing.len()
        );
    }
    format!(
        "{} plug-in{} not installed, kept with {} settings: {}",
        missing.len(),
        if missing.len() == 1 { " is" } else { "s are" },
        if missing.len() == 1 { "its" } else { "their" },
        missing.join(", ")
    )
}

/// Starts a plug-in on a thread of its own and sends the result.
fn spawn_start(sender: Sender<Started>, key: u64, generation: u64, launch: Launch, state: Option<SavedState>) {
    let fallback = sender.clone();
    let spawned = std::thread::Builder::new().name("loopwerk-plugin-start".into()).spawn(move || {
        let result = start(launch, state).map_err(|e| e.to_string());
        let _gone = sender.send(Started { key, generation, result });
    });
    if let Err(e) = spawned {
        let _gone = fallback.send(Started { key, generation, result: Err(format!("no thread to start it on: {e}")) });
    }
}

/// Starts the plug-in `launch` names and gives it `state`. A plug-in that refuses the state
/// still runs, with its own defaults; the reason comes back.
fn start(launch: Launch, state: Option<SavedState>) -> Result<(Plugin, PluginAudio, Option<String>), PluginError> {
    let (mut plugin, audio) = Plugin::start(launch)?;
    let mut refused = None;
    if let Some(state) = state {
        match plugin.load_state(state) {
            Ok(()) => {}
            Err(PluginError::Host(why)) => refused = Some(why),
            Err(other) => return Err(other),
        }
    }
    Ok((plugin, audio, refused))
}

/// A project's plug-ins started for an offline render (ADR 0006: offline renders run the same
/// plug-ins through the same engine, without a deadline). Keep it until the render is done:
/// dropping it ends the host processes.
pub struct OfflinePlugins {
    /// For the renderer.
    pub plugins: Plugins,
    controls: Vec<Plugin>,
    /// What the user should hear about: missing plug-ins, ones that could not start or did not
    /// take their state.
    pub notices: Vec<String>,
}

impl OfflinePlugins {
    /// No plug-ins: a render without a catalog passes plug-in devices through.
    pub fn none() -> Self {
        Self { plugins: Plugins::new(), controls: Vec::new(), notices: Vec::new() }
    }

    /// Plug-ins running for this render.
    pub fn running(&self) -> usize {
        self.controls.len()
    }
}

/// Starts every plug-in device of `project` that is installed, at `sample_rate`, and waits for
/// each.
pub fn start_offline(project: &Project, env: &PluginEnv, sample_rate: u32) -> OfflinePlugins {
    let mut offline = OfflinePlugins { plugins: Plugins::new(), controls: Vec::new(), notices: Vec::new() };
    let mut missing = Vec::new();
    for found in plugin_devices(project) {
        let Some(device) =
            project.channels.iter().find(|c| c.id == found.channel).and_then(|c| c.devices.get(found.position))
        else {
            continue;
        };
        let Some((matched, instrument)) = find(&env.catalog, device) else {
            missing.push(device.name.clone());
            continue;
        };
        let launch = Launch {
            host: env.host.clone(),
            plugin: matched.plugin.spec(),
            channels: 2,
            max_frames: MAX_BLOCK_FRAMES as u32,
            sample_rate: f64::from(sample_rate),
            timeout: START_TIMEOUT,
            group: (env.group)(matched.plugin),
        };
        match start(launch, saved_state(device)) {
            Ok((control, audio, refused)) => {
                if let Some(why) = refused {
                    offline.notices.push(format!("{} did not take its saved settings ({why})", device.name));
                }
                let info = PluginSlotInfo { instrument, latency: control.latency() };
                offline.plugins.insert(found.key, info, Box::new(Hosted::new(audio)));
                offline.controls.push(control);
            }
            Err(e) => offline.notices.push(format!("{} could not start: {e}", device.name)),
        }
    }
    if !missing.is_empty() {
        offline.notices.push(format!("not installed, passed through: {}", missing.join(", ")));
    }
    offline
}

/// The device kind a project stores for the installed plug-in `plugin`.
pub fn device_kind(plugin: &PluginInfo) -> DeviceKind {
    match plugin.format() {
        "vst3" => DeviceKind::Vst3 { id: plugin.id.clone() },
        _ => DeviceKind::Clap { id: plugin.id.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_each_state() {
        let mut rack = Rack::default();
        let instance = |status: PluginStatus, name: &str| Instance {
            status,
            plugin: Some((name.to_string(), "V".into(), "same")),
            info: PluginSlotInfo::default(),
            control: None,
            generation: 0,
            state_refused: None,
        };
        rack.instances.insert(1, instance(PluginStatus::Running, "A"));
        rack.instances.insert(2, instance(PluginStatus::Loading, "C"));
        rack.instances.insert(3, instance(PluginStatus::Loading, "B"));
        rack.instances.insert(4, instance(PluginStatus::Missing, "D"));
        rack.instances.insert(5, instance(PluginStatus::Failed("no".into()), "E"));
        assert_eq!(
            rack.progress(),
            PluginProgress {
                total: 4,
                ready: 1,
                loading: vec!["B".into(), "C".into()],
                missing: 1,
                failed: vec!["E".into()]
            }
        );
        // A new catalog looks for the missing and failed ones again.
        rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
        rack.set_env(std::sync::Arc::new(PluginEnv::new(Catalog::new(), PathBuf::from("host"))));
        assert_eq!(rack.progress().missing, 0);
        assert!(rack.progress().failed.is_empty());
        assert_eq!(rack.progress().total, 3);
    }

    #[test]
    fn notes_become_the_hosts_events() {
        let mut out = Vec::with_capacity(8);
        push_event(&mut out, 3, DeviceEvent::On { key: 60, velocity: 2.0 });
        push_event(&mut out, 4, DeviceEvent::Off { key: 200, velocity: 0.5 });
        push_event(&mut out, 5, DeviceEvent::PitchBend(-3.0));
        push_event(&mut out, 6, DeviceEvent::Pressure(0.5));
        push_event(&mut out, 7, DeviceEvent::AllNotesOff);
        assert_eq!(
            out,
            [
                Event { frame: 3, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } },
                Event { frame: 5, note: -1, kind: EventKind::PitchBend { channel: 0, value: -1.0 } },
                Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 64, value: 0.0 } },
                Event { frame: 7, note: -1, kind: EventKind::Controller { channel: 0, number: 123, value: 0.0 } },
            ]
        );
        // Never more than the room made for them.
        let mut full = Vec::with_capacity(1);
        push_event(&mut full, 0, DeviceEvent::AllNotesOff);
        assert_eq!(full.len(), 1);
    }

    #[test]
    fn instruments_are_told_by_their_features() {
        let plugin = |features: &[&str]| PluginInfo {
            path: PathBuf::from("x.vst3"),
            id: "x".into(),
            name: "X".into(),
            vendor: "V".into(),
            version: "1".into(),
            features: features.iter().map(|f| f.to_string()).collect(),
            replaces: Vec::new(),
        };
        assert!(is_instrument(&plugin(&["Instrument|Synth"])));
        assert!(is_instrument(&plugin(&["instrument", "stereo"])));
        assert!(!is_instrument(&plugin(&["Fx|Reverb"])));
        assert!(!is_instrument(&plugin(&[])));
        assert_eq!(device_kind(&plugin(&[])), DeviceKind::Vst3 { id: "x".into() });
    }
}
