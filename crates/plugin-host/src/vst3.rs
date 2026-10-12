//! Running a VST3 plug-in, through the `vst3` bindings to the VST 3 SDK (MIT since 3.8).
//!
//! The component and its controller live on the host's main thread, the audio processor's
//! `process` calls on the audio thread, as the SDK asks. Audio is stereo in and out on the main
//! buses; other buses (side-chains, extra outputs) get silent buffers of their own, so plug-ins
//! that expect every bus to have one get it. The state handed to the engine is the component's
//! (the controller is brought in step with it on load).
//!
//! A plug-in is named `vst3:<bundle or file>` (the first audio class in it) or
//! `vst3:<bundle or file>#<class id>`, the class id as 32 hex digits; `test:vst3` is a gain
//! built into the host for tests ([`crate::test_vst3`]).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use loopwerk_ipc::Served;
use loopwerk_plugins::ParamInfo;
use loopwerk_plugins::scan::PluginInfo;

use crate::vst3_events::{BlockEvents, MidiMapping};
use crate::vst3_params::{BlockChanges, Pending};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// How long the audio thread waits for a block before checking whether it should stop.
const POLL: Duration = Duration::from_millis(100);
/// The category of the classes that process audio.
const AUDIO_CLASS: &str = "Audio Module Class";
/// The category of the class that lists which older plug-ins the module replaces.
const COMPATIBILITY_CLASS: &str = "Plugin Compatibility Class";
/// Bytes of compatibility list read, at most.
const MAX_COMPATIBILITY: usize = 1024 * 1024;

/// The byte order of a class id's canonical text form. On Windows the SDK lays ids out as COM
/// GUIDs, whose first three fields (4, 2 and 2 bytes) are stored little-endian, and its text
/// form (as in `moduleinfo.json`) writes those fields as numbers; elsewhere the 16 bytes are
/// written in the order they are stored.
const TEXT_ORDER: [usize; 16] = if cfg!(windows) {
    [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15]
} else {
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
};

/// A class id as the 32 hex digits the SDK, `moduleinfo.json` and the model use.
pub(crate) fn cid_hex(cid: &TUID) -> String {
    TEXT_ORDER.iter().filter_map(|&i| cid.get(i)).map(|b| format!("{:02X}", *b as u8)).collect()
}

fn cid_from_hex(hex: &str) -> Option<TUID> {
    let (pairs, rest) = hex.as_bytes().as_chunks::<2>();
    if pairs.len() != 16 || !rest.is_empty() {
        return None;
    }
    let mut cid: TUID = [0; 16];
    for (&index, pair) in TEXT_ORDER.iter().zip(pairs) {
        let text = std::str::from_utf8(pair).ok()?;
        *cid.get_mut(index)? = u8::from_str_radix(text, 16).ok()? as _;
    }
    Some(cid)
}

/// Text from a fixed-size UTF-16 field (`String128`), up to its first zero.
fn utf16(field: &[TChar]) -> String {
    let units: Vec<u16> = field.iter().take_while(|c| **c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

/// Text from a fixed-size C string field.
fn text(field: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = field.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn ok(result: tresult) -> bool {
    result == kResultOk
}

/// A loaded plug-in file (or the built-in test plug-in) and its factory.
struct Module {
    factory: Option<ComPtr<IPluginFactory>>,
    library: Option<libloading::Library>,
}

/// The file inside a VST3 bundle that holds the code for this system.
fn binary(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    let name = path.file_name().ok_or("not a VST3 bundle")?;
    let stem = path.file_stem().ok_or("not a VST3 bundle")?;
    let contents = path.join("Contents");
    let inner = if cfg!(windows) {
        contents.join("x86_64-win").join(name)
    } else if cfg!(target_os = "linux") {
        contents.join("x86_64-linux").join(stem).with_extension("so")
    } else {
        return Err("VST3 bundles on macOS are not supported yet".into());
    };
    if inner.is_file() { Ok(inner) } else { Err(format!("no {} in the bundle", inner.display())) }
}

impl Module {
    fn open(path: &Path) -> Result<Self, String> {
        let file = binary(path)?;
        // SAFETY: loading the library runs its initialisation code, which Rust cannot check;
        // that is what this process is for (ADR 0006). The library stays loaded while `self`
        // holds it, which outlives every object the factory makes.
        let library =
            unsafe { libloading::Library::new(&file) }.map_err(|e| format!("could not load the plug-in file: {e}"))?;
        #[cfg(windows)]
        {
            // SAFETY: `InitDll` is the optional entry the VST3 module API defines on Windows,
            // taking nothing and returning a bool.
            let init = unsafe { library.get::<extern "system" fn() -> bool>(b"InitDll\0") };
            if let Ok(init) = init
                && !init()
            {
                return Err("the plug-in's InitDll failed".into());
            }
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: `ModuleEntry` is the entry the VST3 module API defines on Linux; it takes
            // the library handle, which hosts may pass as null when they have none of their own.
            let entry = unsafe { library.get::<extern "system" fn(*mut c_void) -> bool>(b"ModuleEntry\0") };
            if let Ok(entry) = entry
                && !entry(std::ptr::null_mut())
            {
                return Err("the plug-in's ModuleEntry failed".into());
            }
        }
        // SAFETY: `GetPluginFactory` is the module's required entry: no arguments, returning an
        // owned reference to its factory.
        let get = unsafe { library.get::<extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0") }
            .map_err(|_| "the file has no GetPluginFactory; not a VST3 plug-in")?;
        // SAFETY: the pointer is the factory reference the module handed over (or null).
        let factory = unsafe { ComPtr::from_raw(get()) }.ok_or("the plug-in has no factory")?;
        Ok(Self { factory: Some(factory), library: Some(library) })
    }

    fn test() -> Self {
        Self { factory: crate::test_vst3::factory(), library: None }
    }

    fn factory(&self) -> Result<&ComPtr<IPluginFactory>, String> {
        self.factory.as_ref().ok_or_else(|| "the plug-in has no factory".into())
    }

    /// The audio classes: class id, name, and (from the newer class info) vendor, version and
    /// sub-categories.
    fn audio_classes(&self) -> Result<Vec<(TUID, PluginInfo)>, String> {
        let factory = self.factory()?;
        let mut factory_info: PFactoryInfo = blank();
        // SAFETY: the factory is a live COM object; the info is a plain struct it fills.
        let _ = unsafe { factory.getFactoryInfo(&mut factory_info) };
        let factory2 = factory.cast::<IPluginFactory2>();
        // SAFETY: as above.
        let count = unsafe { factory.countClasses() };
        let mut classes = Vec::new();
        for index in 0..count.clamp(0, 4096) {
            let mut info: PClassInfo = blank();
            // SAFETY: as above; the index is below the count the factory gave.
            if !ok(unsafe { factory.getClassInfo(index, &mut info) }) || text(&info.category) != AUDIO_CLASS {
                continue;
            }
            let mut plugin = PluginInfo {
                path: PathBuf::new(),
                id: cid_hex(&info.cid),
                name: text(&info.name),
                vendor: text(&factory_info.vendor),
                version: String::new(),
                features: Vec::new(),
                replaces: Vec::new(),
            };
            if let Some(factory2) = &factory2 {
                let mut info2: PClassInfo2 = blank();
                // SAFETY: as above, through the newer interface the factory said it has.
                if ok(unsafe { factory2.getClassInfo2(index, &mut info2) }) {
                    if !text(&info2.vendor).is_empty() {
                        plugin.vendor = text(&info2.vendor);
                    }
                    plugin.version = text(&info2.version);
                    plugin.features =
                        text(&info2.subCategories).split('|').filter(|s| !s.is_empty()).map(str::to_string).collect();
                }
            }
            classes.push((info.cid, plugin));
        }
        Ok(classes)
    }

    /// The module's compatibility list, from its `Plugin Compatibility Class`
    /// (`IPluginCompatibility`), as the JSON text it writes; `None` when it has none.
    fn compatibility(&self) -> Option<String> {
        let factory = self.factory().ok()?;
        // SAFETY: the factory is a live COM object.
        let count = unsafe { factory.countClasses() };
        let cid = (0..count.clamp(0, 4096)).find_map(|index| {
            let mut info: PClassInfo = blank();
            // SAFETY: as above; the index is below the count the factory gave.
            let found = ok(unsafe { factory.getClassInfo(index, &mut info) });
            (found && text(&info.category) == COMPATIBILITY_CLASS).then_some(info.cid)
        })?;
        let compatibility = self.create::<IPluginCompatibility>(&cid)?;
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>()?;
        // SAFETY: a live object writing to a live stream.
        if !ok(unsafe { compatibility.getCompatibilityJSON(raw.as_ptr()) }) {
            return None;
        }
        let mut bytes = stream.bytes();
        bytes.truncate(MAX_COMPATIBILITY);
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Creates an object of class `cid` and asks it for interface `I`.
    fn create<I: Interface>(&self, cid: &TUID) -> Option<ComPtr<I>> {
        let factory = self.factory().ok()?;
        let mut object: *mut c_void = std::ptr::null_mut();
        // SAFETY: both ids are 16-byte ids that outlive the call; the factory writes an owned
        // reference to `object` or leaves it null.
        let result = unsafe { factory.createInstance(cid.as_ptr(), I::IID.as_ptr().cast(), &mut object) };
        if !ok(result) {
            return None;
        }
        // SAFETY: on success `object` is an owned reference to the interface asked for.
        unsafe { ComPtr::from_raw(object.cast::<I>()) }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // Every object from the factory is gone by now; release the factory, then let the
        // module clean up, then unload it.
        self.factory = None;
        if let Some(library) = &self.library {
            #[cfg(windows)]
            {
                // SAFETY: `ExitDll` is the optional counterpart of `InitDll`.
                if let Ok(exit) = unsafe { library.get::<extern "system" fn() -> bool>(b"ExitDll\0") } {
                    exit();
                }
            }
            #[cfg(target_os = "linux")]
            {
                // SAFETY: `ModuleExit` is the counterpart of `ModuleEntry`.
                if let Ok(exit) = unsafe { library.get::<extern "system" fn() -> bool>(b"ModuleExit\0") } {
                    exit();
                }
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            let _ = library;
        }
        self.library = None;
    }
}

/// A zeroed C struct for the plug-in to fill.
fn blank<T: Copy>() -> T {
    // SAFETY: only used for the SDK's plain C structs (ids, fixed-size strings, numbers), for
    // which all zero bytes is a valid value.
    unsafe { std::mem::zeroed() }
}

/// The module and class a plug-in name points at.
fn open(spec: &str) -> Result<(Module, Option<TUID>), String> {
    if spec == "test:vst3" {
        return Ok((Module::test(), None));
    }
    let file = spec.strip_prefix("vst3:").ok_or_else(|| format!("not a VST3 plug-in: {spec:?}"))?;
    let (path, cid) = match file.rsplit_once('#') {
        Some((path, hex)) => (path, Some(cid_from_hex(hex).ok_or_else(|| format!("not a class id: {hex:?}"))?)),
        None => (file, None),
    };
    Ok((Module::open(Path::new(path))?, cid))
}

/// The plug-ins the file `spec` names holds, without making any (for scanning).
pub(crate) fn describe(spec: &str) -> Result<Vec<PluginInfo>, String> {
    let (module, _) = open(spec)?;
    let mut plugins: Vec<PluginInfo> = module.audio_classes()?.into_iter().map(|(_, info)| info).collect();
    if let Some(list) = module.compatibility() {
        loopwerk_plugins::scan::apply_compatibility_text(&mut plugins, &list);
    }
    Ok(plugins)
}

// ---- What the host gives the plug-in -------------------------------------------------------

struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        // SAFETY: the plug-in passes a pointer to a String128 of its own to fill.
        let Some(name) = (unsafe { name.as_mut() }) else { return kInvalidArgument };
        name.fill(0);
        for (slot, unit) in name.iter_mut().zip("Loopwerk".encode_utf16()) {
            *slot = unit as TChar;
        }
        kResultOk
    }

    unsafe fn createInstance(&self, _cid: *mut TUID, _iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        // SAFETY: the plug-in passes a pointer to its own pointer to fill (or null).
        if let Some(obj) = unsafe { obj.as_mut() } {
            *obj = std::ptr::null_mut();
        }
        kResultFalse
    }
}

struct ComponentHandler;

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

/// A byte stream in memory, for plug-in state.
pub(crate) struct MemoryStream {
    data: Mutex<(Vec<u8>, usize)>,
}

impl MemoryStream {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self { data: Mutex::new((bytes, 0)) }
    }

    pub(crate) fn rewind(&self) {
        if let Ok(mut data) = self.data.lock() {
            data.1 = 0;
        }
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.data.lock().map(|d| d.0.clone()).unwrap_or_default()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, bytes: int32, read: *mut int32) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let (bytes_now, at) = (&data.0, data.1);
        let count = usize::try_from(bytes).unwrap_or(0).min(bytes_now.len().saturating_sub(at));
        if count > 0 && !buffer.is_null() {
            let from = bytes_now.as_ptr().wrapping_add(at);
            // SAFETY: the plug-in's buffer holds at least `bytes` bytes, and `count` is no
            // more than that or than what is left of ours; they are separate allocations.
            unsafe { std::ptr::copy_nonoverlapping(from, buffer.cast::<u8>(), count) };
        }
        data.1 = at + count;
        // SAFETY: the plug-in passes a pointer to a count of its own to fill (or null).
        if let Some(read) = unsafe { read.as_mut() } {
            *read = count as int32;
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, bytes: int32, written: *mut int32) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let count = usize::try_from(bytes).unwrap_or(0);
        if count > 0 && !buffer.is_null() {
            // SAFETY: the plug-in's buffer holds `bytes` bytes for the length of the call.
            let incoming = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), count) };
            let at = data.1;
            let end = at + count;
            if data.0.len() < end {
                data.0.resize(end, 0);
            }
            if let Some(target) = data.0.get_mut(at..end) {
                target.copy_from_slice(incoming);
            }
            data.1 = end;
        }
        // SAFETY: as in `read`.
        if let Some(written) = unsafe { written.as_mut() } {
            *written = count as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut data) = self.data.lock() else { return kResultFalse };
        let base = match mode as IBStream_::IStreamSeekMode {
            IBStream_::IStreamSeekMode_::kIBSeekSet => 0,
            IBStream_::IStreamSeekMode_::kIBSeekCur => data.1 as int64,
            IBStream_::IStreamSeekMode_::kIBSeekEnd => data.0.len() as int64,
            _ => return kInvalidArgument,
        };
        let Some(target) = base.checked_add(pos).filter(|t| *t >= 0) else { return kInvalidArgument };
        data.1 = usize::try_from(target).unwrap_or(usize::MAX).min(data.0.len());
        // SAFETY: as in `read`.
        if let Some(result) = unsafe { result.as_mut() } {
            *result = data.1 as int64;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(data) = self.data.lock() else { return kResultFalse };
        // SAFETY: as in `read`.
        match unsafe { pos.as_mut() } {
            Some(pos) => {
                *pos = data.1 as int64;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

// ---- A running plug-in --------------------------------------------------------------------

/// The audio processor, handed to the audio thread.
struct AudioSide(ComPtr<IAudioProcessor>);

// SAFETY: the VST3 threading model has the audio processor's `setProcessing` and `process`
// called from the audio thread while the component is used from the main thread; this pointer
// is only used for those two calls, on one thread at a time.
unsafe impl Send for AudioSide {}

/// A loaded, active VST3 plug-in and its audio thread.
pub(crate) struct Vst3Plugin {
    audio: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    latency: u32,
    connections: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    /// Parameter values for the processor's next block.
    pending: Pending,
    /// The plug-in's name, for its editor's window.
    name: String,
    editor: Option<crate::vst3_editor::Editor>,
    component: ComPtr<IComponent>,
    _handler: ComWrapper<ComponentHandler>,
    _host: ComWrapper<HostApplication>,
    module: Module,
}

/// Channels in a speaker arrangement (one bit each).
fn channel_count(arrangement: SpeakerArrangement) -> usize {
    arrangement.count_ones() as usize
}

impl Vst3Plugin {
    /// Loads the plug-in `spec` names, activates it for the link's block size at `sample_rate`
    /// and starts serving blocks from `served` on an audio thread.
    pub(crate) fn load(spec: &str, served: Served, sample_rate: f64) -> Result<Self, String> {
        let (module, wanted) = open(spec)?;
        let classes = module.audio_classes()?;
        let (cid, name) = match wanted {
            Some(wanted) => classes.iter().find(|(cid, _)| *cid == wanted),
            None => classes.first(),
        }
        .map(|(cid, info)| (*cid, info.name.clone()))
        .ok_or_else(|| format!("no audio plug-in in {spec:?}"))?;

        let component = module.create::<IComponent>(&cid).ok_or("could not make the plug-in's component")?;
        let host = ComWrapper::new(HostApplication);
        let handler = ComWrapper::new(ComponentHandler);
        let context = host.to_com_ptr::<FUnknown>().ok_or("host object")?;
        // SAFETY: a live component, given the host's context object for the call.
        if !ok(unsafe { component.initialize(context.as_ptr()) }) {
            return Err("the plug-in's component did not initialise".into());
        }

        // The controller: the component itself, or a class of its own.
        let (controller, separate_controller) = match component.cast::<IEditController>() {
            Some(controller) => (Some(controller), false),
            None => {
                let mut controller_cid: TUID = [0; 16];
                // SAFETY: a live component; the id is a 16-byte buffer to fill.
                let has = ok(unsafe { component.getControllerClassId(&mut controller_cid) });
                let controller = if has { module.create::<IEditController>(&controller_cid) } else { None };
                // SAFETY: a live controller, given the host's context object for the call.
                let controller = controller.filter(|c| ok(unsafe { c.initialize(context.as_ptr()) }));
                (controller, true)
            }
        };
        let mut plugin = Self {
            audio: None,
            stop: Arc::new(AtomicBool::new(false)),
            latency: 0,
            connections: None,
            controller,
            separate_controller,
            pending: Pending::default(),
            name,
            editor: None,
            component,
            _handler: handler,
            _host: host,
            module,
        };
        plugin.connect();
        plugin.sync_controller();

        let processor = plugin.component.cast::<IAudioProcessor>().ok_or("the plug-in does not process audio")?;
        let max_frames = served.max_frames();
        let buses = plugin.set_up_buses(&processor);
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: max_frames as int32,
            sampleRate: sample_rate,
        };
        // SAFETY: a live processor; the setup struct lives for the call.
        if !ok(unsafe { processor.setupProcessing(&mut setup) }) {
            return Err("the plug-in refused the sample rate or block size".into());
        }
        // SAFETY: a live, set-up component.
        if !ok(unsafe { plugin.component.setActive(1) }) {
            return Err("the plug-in did not activate".into());
        }
        // SAFETY: a live, active processor.
        plugin.latency = unsafe { processor.getLatencySamples() };

        let stop = plugin.stop.clone();
        let pending = plugin.pending.clone();
        let mapping = MidiMapping::of(plugin.controller.as_ref());
        let audio = AudioSide(processor);
        let thread = thread::Builder::new()
            .name("plug-in audio".into())
            .spawn(move || serve(audio, served, buses, sample_rate, (pending, mapping), &stop))
            .map_err(|e| e.to_string())?;
        plugin.audio = Some(thread);
        Ok(plugin)
    }

    /// Connects component and controller, when they are separate objects that talk.
    fn connect(&mut self) {
        if !self.separate_controller {
            return;
        }
        let Some(controller) = &self.controller else { return };
        let (Some(a), Some(b)) = (self.component.cast::<IConnectionPoint>(), controller.cast::<IConnectionPoint>())
        else {
            return;
        };
        // SAFETY: two live connection points, each given the other.
        let _ = unsafe { a.connect(b.as_ptr()) };
        // SAFETY: as above, the other way.
        let _ = unsafe { b.connect(a.as_ptr()) };
        self.connections = Some((a, b));
    }

    /// Hands the component's state to the controller, so both agree.
    fn sync_controller(&self) {
        let Some(controller) = &self.controller else { return };
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let Some(raw) = stream.to_com_ptr::<IBStream>() else { return };
        // SAFETY: a live component writing to a live stream.
        if ok(unsafe { self.component.getState(raw.as_ptr()) }) {
            stream.rewind();
            // SAFETY: a live controller reading the same stream.
            let _ = unsafe { controller.setComponentState(raw.as_ptr()) };
        }
    }

    /// Asks for stereo on the main buses, activates them, and returns each bus's channel count
    /// (inputs, outputs).
    fn set_up_buses(&self, processor: &ComPtr<IAudioProcessor>) -> (Vec<usize>, Vec<usize>) {
        let audio = MediaTypes_::kAudio as MediaType;
        let count = |direction: BusDirections| -> i32 {
            // SAFETY: a live component.
            unsafe { self.component.getBusCount(audio, direction as BusDirection) }.clamp(0, 64)
        };
        let (inputs, outputs) = (count(BusDirections_::kInput), count(BusDirections_::kOutput));
        let arrangement = |direction: BusDirections, index: i32| -> SpeakerArrangement {
            let mut arrangement: SpeakerArrangement = 0;
            // SAFETY: a live processor writing one value.
            let _ = unsafe { processor.getBusArrangement(direction as BusDirection, index, &mut arrangement) };
            arrangement
        };
        let mut ins: Vec<SpeakerArrangement> = (0..inputs).map(|i| arrangement(BusDirections_::kInput, i)).collect();
        let mut outs: Vec<SpeakerArrangement> = (0..outputs).map(|i| arrangement(BusDirections_::kOutput, i)).collect();
        if let Some(main) = ins.first_mut() {
            *main = SpeakerArr::kStereo;
        }
        if let Some(main) = outs.first_mut() {
            *main = SpeakerArr::kStereo;
        }
        // SAFETY: a live processor; both arrays hold as many entries as the counts given.
        let _ = unsafe { processor.setBusArrangements(ins.as_mut_ptr(), inputs, outs.as_mut_ptr(), outputs) };
        let in_channels: Vec<usize> =
            (0..inputs).map(|i| channel_count(arrangement(BusDirections_::kInput, i))).collect();
        let out_channels: Vec<usize> =
            (0..outputs).map(|i| channel_count(arrangement(BusDirections_::kOutput, i))).collect();
        for (direction, buses) in [(BusDirections_::kInput, inputs), (BusDirections_::kOutput, outputs)] {
            if buses > 0 {
                // SAFETY: a live component; bus 0 exists.
                let _ = unsafe { self.component.activateBus(audio, direction as BusDirection, 0, 1) };
            }
        }
        // An instrument takes notes on its first event input.
        let event = MediaTypes_::kEvent as MediaType;
        let input = BusDirections_::kInput as BusDirection;
        // SAFETY: a live component.
        if unsafe { self.component.getBusCount(event, input) } > 0 {
            // SAFETY: as above; event bus 0 exists.
            let _ = unsafe { self.component.activateBus(event, input, 0, 1) };
        }
        (in_channels, out_channels)
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    pub(crate) fn state(&mut self) -> Result<Vec<u8>, String> {
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>().ok_or("stream")?;
        // SAFETY: a live component writing to a live stream.
        if !ok(unsafe { self.component.getState(raw.as_ptr()) }) {
            return Err("the plug-in did not give its state".into());
        }
        Ok(stream.bytes())
    }

    /// The controller's own state, when it is a separate object that gives one.
    pub(crate) fn controller_state(&mut self) -> Option<Vec<u8>> {
        let controller = self.controller.as_ref()?;
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>()?;
        // SAFETY: a live controller writing to a live stream.
        ok(unsafe { controller.getState(raw.as_ptr()) }).then(|| stream.bytes())
    }

    /// Hands `data` to the component and mirrors it to the controller, then gives the
    /// controller its own `controller` state, as a project saved it. A VST3 plug-in that stands
    /// in for a VST2 one reads the VST2 bank here (ADR 0006, decision 8); a bank of programs
    /// is also set as parameters, since not every plug-in reads one.
    pub(crate) fn set_state(&mut self, data: &[u8], controller_state: Option<&[u8]>) -> Result<(), String> {
        let stream = ComWrapper::new(MemoryStream::new(data.to_vec()));
        let raw = stream.to_com_ptr::<IBStream>().ok_or("stream")?;
        // SAFETY: a live component reading from a live stream.
        let taken = ok(unsafe { self.component.setState(raw.as_ptr()) });
        let parameters = loopwerk_plugins::vst2::program_values(data).map_or(0, |values| self.set_parameters(&values));
        if !taken && parameters == 0 {
            return Err("the plug-in did not take the state".into());
        }
        if let Some(controller) = &self.controller {
            if taken {
                stream.rewind();
                // SAFETY: a live controller reading the same stream.
                let _ = unsafe { controller.setComponentState(raw.as_ptr()) };
            }
            if let Some(own) = controller_state.filter(|s| !s.is_empty()) {
                let own = ComWrapper::new(MemoryStream::new(own.to_vec()));
                let own_raw = own.to_com_ptr::<IBStream>().ok_or("stream")?;
                // SAFETY: a live controller reading a live stream. A controller that refuses
                // its own state still has the component's, so this is not an error.
                let _ = unsafe { controller.setState(own_raw.as_ptr()) };
            }
        }
        Ok(())
    }

    /// Opens the plug-in's editor (or returns the size of the one open).
    pub(crate) fn open_editor(&mut self) -> Result<crate::window::Size, String> {
        if let Some(editor) = &self.editor {
            return Ok(editor.size());
        }
        let controller = self.controller.as_ref().ok_or("the plug-in has no editor")?;
        let editor = crate::vst3_editor::Editor::open(controller, &self.name)?;
        let size = editor.size();
        self.editor = Some(editor);
        Ok(size)
    }

    pub(crate) fn close_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            editor.close();
        }
    }

    /// Keeps the editor in step with its window; returns whether the user closed it.
    pub(crate) fn editor_tick(&mut self) -> bool {
        let closed = self.editor.as_mut().is_some_and(crate::vst3_editor::Editor::tick);
        if closed {
            self.close_editor();
        }
        closed
    }

    /// The controller's parameters, values normalized (0 to 1).
    pub(crate) fn params(&self) -> Vec<ParamInfo> {
        let Some(controller) = &self.controller else { return Vec::new() };
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        (0..count)
            .filter_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                ok(unsafe { controller.getParameterInfo(index, &mut info) }).then(|| self.param(controller, &info))
            })
            .collect()
    }

    /// Parameter `info` with its current value and text.
    fn param(&self, controller: &ComPtr<IEditController>, info: &ParameterInfo) -> ParamInfo {
        use ParameterInfo_::ParameterFlags_ as Flags;
        // SAFETY: a live controller, for a parameter it listed.
        let value = unsafe { controller.getParamNormalized(info.id) };
        let mut shown: String128 = [0; 128];
        // SAFETY: as above, writing at most 128 UTF-16 units.
        let text = if ok(unsafe { controller.getParamStringByValue(info.id, value, &mut shown) }) {
            utf16(&shown)
        } else {
            String::new()
        };
        let flag = |f: ParameterInfo_::ParameterFlags| info.flags & f as int32 != 0;
        ParamInfo {
            id: info.id,
            name: utf16(&info.title),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default: info.defaultNormalizedValue,
            value,
            text,
            steps: u32::try_from(info.stepCount).unwrap_or(0),
            automatable: flag(Flags::kCanAutomate),
            read_only: flag(Flags::kIsReadOnly),
            hidden: flag(Flags::kIsHidden),
        }
    }

    /// Sets parameter `id` to `value` (normalized; kept within 0 to 1): on the controller now,
    /// on the processor with its next block.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) -> Result<ParamInfo, String> {
        let controller = self.controller.as_ref().ok_or("the plug-in has no parameters")?;
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        let info = (0..count)
            .find_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                (ok(unsafe { controller.getParameterInfo(index, &mut info) }) && info.id == id).then_some(info)
            })
            .ok_or_else(|| format!("no parameter {id}"))?;
        let value = value.clamp(0.0, 1.0);
        // SAFETY: a live controller, for a parameter it listed.
        let _ = unsafe { controller.setParamNormalized(id, value) };
        if let Ok(mut pending) = self.pending.lock() {
            pending.push((id, value));
        }
        Ok(self.param(controller, &info))
    }

    /// Sets parameter `i` to `values[i]` (normalized), the numbering a VST3 version keeps from
    /// its VST2 one: on the controller now, on the processor with its next block. Only when the
    /// controller has every id from 0 to the last value's, so a plug-in that numbers its
    /// parameters another way is left alone. Returns how many were set.
    fn set_parameters(&mut self, values: &[f32]) -> usize {
        let Some(controller) = &self.controller else { return 0 };
        // SAFETY: a live controller.
        let count = unsafe { controller.getParameterCount() }.clamp(0, 1 << 16);
        let ids: std::collections::HashSet<ParamID> = (0..count)
            .filter_map(|index| {
                let mut info: ParameterInfo = blank();
                // SAFETY: as above; the index is below the count it gave.
                ok(unsafe { controller.getParameterInfo(index, &mut info) }).then_some(info.id)
            })
            .collect();
        let changes: Option<Vec<(ParamID, ParamValue)>> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let id = ParamID::try_from(index).ok().filter(|id| ids.contains(id))?;
                Some((id, f64::from(*value).clamp(0.0, 1.0)))
            })
            .collect();
        let Some(changes) = changes else { return 0 };
        for (id, value) in &changes {
            // SAFETY: as above, for a parameter it listed.
            let _ = unsafe { controller.setParamNormalized(*id, *value) };
        }
        let set = changes.len();
        if let Ok(mut pending) = self.pending.lock() {
            pending.extend(changes);
        }
        set
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        self.close_editor();
        self.stop.store(true, Ordering::Release);
        if let Some(audio) = self.audio.take() {
            let _ = audio.join();
        }
        // SAFETY: a live component; processing has stopped.
        let _ = unsafe { self.component.setActive(0) };
        if let Some((a, b)) = self.connections.take() {
            // SAFETY: the two connection points connected in `connect`.
            let _ = unsafe { a.disconnect(b.as_ptr()) };
            // SAFETY: as above, the other way.
            let _ = unsafe { b.disconnect(a.as_ptr()) };
        }
        if self.separate_controller
            && let Some(controller) = self.controller.take()
        {
            // SAFETY: a live controller, terminated once before it is released.
            let _ = unsafe { controller.terminate() };
        }
        self.controller = None;
        // SAFETY: a live component, terminated once; it is released when `self` is dropped,
        // before the module (declared last) unloads the code.
        let _ = unsafe { self.component.terminate() };
        let _ = &self.module;
    }
}

/// The audio thread: serves blocks from the link through the processor until `stop` is set.
fn serve(
    audio: AudioSide,
    mut served: Served,
    buses: (Vec<usize>, Vec<usize>),
    sample_rate: f64,
    (pending, mapping): (Pending, MidiMapping),
    stop: &AtomicBool,
) {
    let processor = audio.0;
    let mut changes = BlockChanges::new(pending);
    let notes = BlockEvents::new();
    let max_frames = served.max_frames() as usize;
    let link_channels = served.channels() as usize;
    let (in_channels, out_channels) = buses;
    // One buffer per channel of every bus; the pointers below point into these and stay valid,
    // because the buffers are never resized.
    let mut in_buffers: Vec<Vec<Vec<f32>>> = in_channels.iter().map(|c| vec![vec![0.0; max_frames]; *c]).collect();
    let mut out_buffers: Vec<Vec<Vec<f32>>> = out_channels.iter().map(|c| vec![vec![0.0; max_frames]; *c]).collect();
    let mut in_pointers: Vec<Vec<*mut f32>> =
        in_buffers.iter_mut().map(|bus| bus.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
    let mut out_pointers: Vec<Vec<*mut f32>> =
        out_buffers.iter_mut().map(|bus| bus.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
    let bus_buffers = |pointers: &mut Vec<Vec<*mut f32>>| -> Vec<AudioBusBuffers> {
        pointers
            .iter_mut()
            .map(|channels| AudioBusBuffers {
                numChannels: channels.len() as int32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 { channelBuffers32: channels.as_mut_ptr() },
            })
            .collect()
    };
    let mut inputs = bus_buffers(&mut in_pointers);
    let mut outputs = bus_buffers(&mut out_pointers);
    let mut context: ProcessContext = blank();
    context.sampleRate = sample_rate;
    context.tempo = 120.0;
    context.timeSigNumerator = 4;
    context.timeSigDenominator = 4;

    // SAFETY: a live, active processor, on the audio thread.
    let _ = unsafe { processor.setProcessing(1) };
    while !stop.load(Ordering::Acquire) {
        served.serve_with_events(POLL, |input, events, output| {
            let frames = (input.len() / link_channels.max(1)).min(max_frames);
            for bus in in_buffers.iter_mut() {
                for channel in bus.iter_mut() {
                    channel.fill(0.0);
                }
            }
            if let Some(main) = in_buffers.first_mut() {
                for (index, channel) in main.iter_mut().enumerate() {
                    let source = index.min(link_channels.saturating_sub(1));
                    for (frame, sample) in channel.iter_mut().take(frames).enumerate() {
                        *sample = input.get(frame * link_channels + source).copied().unwrap_or(0.0);
                    }
                }
            }
            let mut data = ProcessData {
                processMode: ProcessModes_::kRealtime as int32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
                numSamples: frames as int32,
                numInputs: inputs.len() as int32,
                numOutputs: outputs.len() as int32,
                inputs: inputs.as_mut_ptr(),
                outputs: outputs.as_mut_ptr(),
                inputParameterChanges: changes.take(events.iter().filter_map(|e| mapping.change(e))),
                outputParameterChanges: std::ptr::null_mut(),
                inputEvents: notes.fill(events),
                outputEvents: std::ptr::null_mut(),
                processContext: &mut context,
            };
            // SAFETY: a live, processing processor on the audio thread; every bus has a buffer
            // per channel of at least `frames` samples, and all of it outlives the call.
            let result = unsafe { processor.process(&mut data) };
            changes.done();
            context.projectTimeSamples += frames as i64;
            context.continousTimeSamples += frames as i64;
            if !ok(result) {
                return;
            }
            if let Some(main) = out_buffers.first() {
                for channel in 0..link_channels {
                    let Some(source) = main.get(channel.min(main.len().saturating_sub(1))) else { break };
                    for (frame, sample) in source.iter().take(frames).enumerate() {
                        if let Some(out) = output.get_mut(frame * link_channels + channel) {
                            *out = *sample;
                        }
                    }
                }
            }
        });
    }
    // SAFETY: as above.
    let _ = unsafe { processor.setProcessing(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_ids_round_trip_as_hex() {
        let cid: TUID = [0x12, -1, 0, 0x7f, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, -128];
        let hex = cid_hex(&cid);
        let expected = if cfg!(windows) { "7F00FF12020104030506070809" } else { "12FF007F01020304050607080900" };
        assert!(hex.starts_with(&expected[..16]), "{hex}");
        assert_eq!(cid_from_hex(&hex), Some(cid));
        assert_eq!(cid_from_hex("12"), None);
        assert_eq!(cid_from_hex(&"zz".repeat(16)), None);
    }

    #[test]
    fn the_memory_stream_reads_writes_and_seeks() {
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let raw = stream.to_com_ptr::<IBStream>().unwrap_or_else(|| panic!("stream"));
        let mut bytes = *b"hello";
        let mut count = 0;
        // SAFETY: a live stream and buffers that outlive the calls.
        unsafe {
            assert_eq!(raw.write(bytes.as_mut_ptr().cast(), 5, &mut count), kResultOk);
        }
        assert_eq!((stream.bytes(), count), (b"hello".to_vec(), 5));
        let mut at = 0;
        // SAFETY: as above.
        unsafe {
            assert_eq!(raw.seek(1, IBStream_::IStreamSeekMode_::kIBSeekSet as int32, &mut at), kResultOk);
        }
        let mut out = [0u8; 8];
        // SAFETY: as above.
        unsafe {
            assert_eq!(raw.read(out.as_mut_ptr().cast(), 8, &mut count), kResultOk);
        }
        assert_eq!((&out[..4], count), (&b"ello"[..], 4));
        // SAFETY: as above.
        let before_start = unsafe { raw.seek(-10, IBStream_::IStreamSeekMode_::kIBSeekCur as int32, &mut at) };
        assert_eq!(before_start, kInvalidArgument);
        // SAFETY: as above.
        assert_eq!(unsafe { raw.tell(&mut at) }, kResultOk);
        assert_eq!(at, 5);
    }

    #[test]
    fn bundle_layout() {
        let missing = binary(Path::new("no-such.vst3"));
        assert!(missing.is_err());
    }
}
