//! A VST3 plug-in built into the host for tests (`test:vst3`): a gain (0.5 at first) with its
//! state (the gain as 4 bytes, little-endian `f32`), a reported latency of [`LATENCY`] frames
//! and a controller of its own. Its factory is handed to the host directly, so the tests run
//! the host's whole VST3 path except opening a file. A compatibility class says it replaces
//! the VST2 plug-in [`REPLACES`] (`Gain`, named "Gain"), and the component reads that plug-in's
//! saved state as a standard VST2 bank too: a chunk bank whose chunk is the gain. The controller
//! keeps whatever state it is given, and has one parameter, the gain (id 0), which the
//! processor follows when a block brings a change. It has an event input too: a note-on adds
//! its velocity to the first output channel at its frame, and its controller maps MIDI
//! controller 7 to the gain (`IMidiMapping`). Modelled on the `vst3` crate's gain
//! example (MIT/Apache-2.0).

use std::ffi::{c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, uid};

/// Latency the test plug-in reports, in frames.
pub(crate) const LATENCY: u32 = 32;

const PROCESSOR_CID: TUID = uid(0x4C6F6F70, 0x7765726B, 0x54657374, 0x47616931);
const CONTROLLER_CID: TUID = uid(0x4C6F6F70, 0x7765726B, 0x54657374, 0x43746C31);
/// The id of its one parameter, the gain.
const GAIN_PARAMETER: u32 = 0;
const COMPATIBILITY_CID: TUID = uid(0x4C6F6F70, 0x7765726B, 0x54657374, 0x436D7031);
/// The class id of the VST2 plug-in the test plug-in says it replaces: `Gain` named "Gain".
pub(crate) const REPLACES: &str = "5653544761696E6761696E0000000000";

fn copy_text(text: &str, field: &mut [c_char]) {
    field.fill(0);
    let room = field.len().saturating_sub(1);
    for (slot, byte) in field.iter_mut().take(room).zip(text.bytes()) {
        *slot = byte as c_char;
    }
}

struct Processor {
    gain: Arc<AtomicU32>,
}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor);
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        // SAFETY: the host passes a 16-byte id to fill.
        match unsafe { class_id.as_mut() } {
            Some(class_id) => {
                *class_id = CONTROLLER_CID;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        if media == MediaTypes_::kEvent as MediaType {
            return i32::from(dir == BusDirections_::kInput as BusDirection);
        }
        i32::from(media == MediaTypes_::kAudio as MediaType)
    }

    unsafe fn getBusInfo(&self, _media: MediaType, _dir: BusDirection, _index: i32, _bus: *mut BusInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn getRoutingInfo(&self, _in_info: *mut RoutingInfo, _out_info: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(&self, _media: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else { return kInvalidArgument };
        let bytes = read_all(&stream);
        // Its own state is the gain; a VST2 bank holds it as the chunk (after a 160-byte header).
        let gain = match bytes.as_slice() {
            [a, b, c, d] => f32::from_le_bytes([*a, *b, *c, *d]),
            bank if bank.starts_with(b"CcnK") && bank.get(8..12) == Some(b"FBCh") => match bank.get(160..) {
                Some([a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
                _ => return kResultFalse,
            },
            _ => return kResultFalse,
        };
        if !gain.is_finite() {
            return kResultFalse;
        }
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: as in `setState`.
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else { return kInvalidArgument };
        let mut bytes = f32::from_bits(self.gain.load(Ordering::Relaxed)).to_le_bytes();
        let mut written = 0;
        // SAFETY: a live stream and a 4-byte buffer.
        unsafe { stream.write(bytes.as_mut_ptr().cast(), 4, &mut written) }
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        _num_ins: i32,
        _outputs: *mut SpeakerArrangement,
        _num_outs: i32,
    ) -> tresult {
        kResultTrue
    }

    unsafe fn getBusArrangement(&self, _dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        // SAFETY: the host passes one value to fill.
        match (index, unsafe { arr.as_mut() }) {
            (0, Some(arr)) => {
                *arr = SpeakerArr::kStereo;
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        // The constant's type differs between systems (i32 on Windows, u32 elsewhere).
        if i64::from(size) == i64::from(SymbolicSampleSizes_::kSample32) { kResultOk } else { kNotImplemented }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        LATENCY
    }

    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        // SAFETY: the host passes its process data for the call.
        let Some(data) = (unsafe { data.as_ref() }) else { return kInvalidArgument };
        if data.numInputs < 1 || data.numOutputs < 1 {
            return kResultOk;
        }
        // SAFETY: the host's parameter changes, or null, for the call.
        if let Some(changes) = unsafe { ComRef::from_raw(data.inputParameterChanges) } {
            // SAFETY: a live list for the call.
            for index in 0..unsafe { changes.getParameterCount() } {
                // SAFETY: as above; the index is below its count.
                let raw = unsafe { changes.getParameterData(index) };
                // SAFETY: a queue the list holds for the call, or null.
                let Some(queue) = (unsafe { ComRef::from_raw(raw) }) else { continue };
                // SAFETY: a live queue.
                let (id, points) = (unsafe { queue.getParameterId() }, unsafe { queue.getPointCount() });
                let (mut offset, mut value) = (0, 0.0);
                // SAFETY: a live queue and two values to fill.
                let read = unsafe { queue.getPoint(points - 1, &mut offset, &mut value) };
                if id == GAIN_PARAMETER && read == kResultOk {
                    self.gain.store((value as f32).to_bits(), Ordering::Relaxed);
                }
            }
        }
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        let frames = usize::try_from(data.numSamples).unwrap_or(0);
        // SAFETY: the host gives at least one input bus.
        let input = unsafe { &*data.inputs };
        // SAFETY: and at least one output bus.
        let output = unsafe { &*data.outputs };
        // SAFETY: the host fills 32-bit buffers (it set up 32-bit processing).
        let sources = unsafe { input.__field0.channelBuffers32 };
        // SAFETY: as above.
        let targets = unsafe { output.__field0.channelBuffers32 };
        for channel in 0..input.numChannels.min(output.numChannels).max(0) as usize {
            // SAFETY: each bus has `numChannels` channel pointers.
            let source = unsafe { *sources.wrapping_add(channel) };
            // SAFETY: as above.
            let target = unsafe { *targets.wrapping_add(channel) };
            // SAFETY: each channel pointer has `numSamples` samples.
            let source = unsafe { std::slice::from_raw_parts(source, frames) };
            // SAFETY: as above, for the output, which does not overlap the input.
            let target = unsafe { std::slice::from_raw_parts_mut(target, frames) };
            for (o, i) in target.iter_mut().zip(source) {
                *o = i * gain;
            }
            if channel == 0 {
                self.add_notes(data, target);
            }
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

/// Everything left in `stream`, up to 64 KiB.
fn read_all(stream: &ComRef<'_, IBStream>) -> Vec<u8> {
    let mut bytes = vec![0u8; 64 * 1024];
    let mut read = 0;
    // SAFETY: a live stream and a buffer of the length given.
    let result = unsafe { stream.read(bytes.as_mut_ptr().cast(), bytes.len() as i32, &mut read) };
    bytes.truncate(if result == kResultOk { usize::try_from(read).unwrap_or(0) } else { 0 });
    bytes
}

#[derive(Default)]
struct Controller {
    state: std::sync::Mutex<Vec<u8>>,
    /// What it shows for the gain parameter, as `f64` bits.
    gain: std::sync::atomic::AtomicU64,
}

impl Class for Controller {
    type Interfaces = (IEditController, IMidiMapping);
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else { return kInvalidArgument };
        // The component's own state is the gain; show it.
        if let [a, b, c, d] = read_all(&stream)[..] {
            let gain = f64::from(f32::from_le_bytes([a, b, c, d]));
            self.gain.store(gain.to_bits(), Ordering::Relaxed);
        }
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else { return kInvalidArgument };
        if let Ok(mut own) = self.state.lock() {
            *own = read_all(&stream);
        }
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else { return kInvalidArgument };
        let Ok(own) = self.state.lock() else { return kResultFalse };
        let mut written = 0;
        // SAFETY: a live stream and the bytes, which outlive the call.
        unsafe { stream.write(own.as_ptr() as *mut c_void, own.len() as i32, &mut written) }
    }

    unsafe fn getParameterCount(&self) -> i32 {
        1
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        // SAFETY: the host passes a struct to fill.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        if index != 0 {
            return kInvalidArgument;
        }
        info.id = GAIN_PARAMETER;
        info.defaultNormalizedValue = 0.5;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as int32;
        for (slot, unit) in info.title.iter_mut().zip("Gain".encode_utf16().chain(std::iter::once(0))) {
            *slot = unit as TChar;
        }
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, _id: u32, _value: f64, _string: *mut String128) -> tresult {
        kInvalidArgument
    }

    unsafe fn getParamValueByString(&self, _id: u32, _string: *mut TChar, _value: *mut f64) -> tresult {
        kInvalidArgument
    }

    unsafe fn normalizedParamToPlain(&self, _id: u32, value: f64) -> f64 {
        value
    }

    unsafe fn plainParamToNormalized(&self, _id: u32, value: f64) -> f64 {
        value
    }

    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        if id == GAIN_PARAMETER { f64::from_bits(self.gain.load(Ordering::Relaxed)) } else { 0.0 }
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        if id != GAIN_PARAMETER {
            return kInvalidArgument;
        }
        self.gain.store(value.to_bits(), Ordering::Relaxed);
        kResultOk
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}

struct Compatibility;

impl Class for Compatibility {
    type Interfaces = (IPluginCompatibility,);
}

impl IPluginCompatibilityTrait for Compatibility {
    unsafe fn getCompatibilityJSON(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream.
        let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else { return kInvalidArgument };
        // The text form is the one on this system ([`crate::vst3::cid_hex`]); trailing commas
        // as some plug-ins write them.
        let text =
            format!(r#"[ {{ "New": "{}", "Old": [ "{REPLACES}", ], }}, ]"#, crate::vst3::cid_hex(&PROCESSOR_CID));
        let mut written = 0;
        // SAFETY: the bytes outlive the call; the length fits an i32.
        unsafe { stream.write(text.as_ptr() as *mut c_void, text.len() as i32, &mut written) }
    }
}

impl Processor {
    /// Adds each note-on's velocity at its frame.
    fn add_notes(&self, data: &ProcessData, target: &mut [f32]) {
        // SAFETY: the host's events, or null, for the call.
        let Some(events) = (unsafe { ComRef::from_raw(data.inputEvents) }) else { return };
        // SAFETY: a live list for the call.
        for index in 0..unsafe { events.getEventCount() } {
            let mut event: Event = blank_event();
            // SAFETY: as above; one event to fill.
            if unsafe { events.getEvent(index, &mut event) } != kResultOk
                || event.r#type != Event_::EventTypes_::kNoteOnEvent as u16
            {
                continue;
            }
            // SAFETY: a note-on event, so the union holds a note-on.
            let velocity = unsafe { event.__field0.noteOn.velocity };
            if let Some(sample) = usize::try_from(event.sampleOffset).ok().and_then(|f| target.get_mut(f)) {
                *sample += velocity;
            }
        }
    }
}

fn blank_event() -> Event {
    Event {
        busIndex: 0,
        sampleOffset: 0,
        ppqPosition: 0.0,
        flags: 0,
        r#type: 0,
        __field0: Event__type0 {
            noteOn: NoteOnEvent { channel: 0, pitch: 0, tuning: 0.0, velocity: 0.0, length: 0, noteId: -1 },
        },
    }
}

impl IMidiMappingTrait for Controller {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus: i32,
        channel: i16,
        number: CtrlNumber,
        id: *mut ParamID,
    ) -> tresult {
        if bus != 0 || channel != 0 || number != 7 || id.is_null() {
            return kResultFalse;
        }
        // SAFETY: the host passes an id to fill.
        unsafe { *id = GAIN_PARAMETER };
        kResultOk
    }
}

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        // SAFETY: the host passes a struct to fill.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        copy_text("Loopwerk", &mut info.vendor);
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        3
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        // SAFETY: the host passes a struct to fill.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        let (cid, category) = match index {
            0 => (PROCESSOR_CID, "Audio Module Class"),
            1 => (CONTROLLER_CID, "Component Controller Class"),
            2 => (COMPATIBILITY_CID, "Plugin Compatibility Class"),
            _ => return kInvalidArgument,
        };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_text(category, &mut info.category);
        copy_text("Loopwerk Test Gain", &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        // SAFETY: the host passes 16-byte ids.
        let cid = unsafe { *(cid as *const TUID) };
        let object = match cid {
            PROCESSOR_CID => {
                ComWrapper::new(Processor { gain: Arc::new(AtomicU32::new(0.5f32.to_bits())) }).to_com_ptr::<FUnknown>()
            }
            CONTROLLER_CID => ComWrapper::new(Controller::default()).to_com_ptr::<FUnknown>(),
            COMPATIBILITY_CID => ComWrapper::new(Compatibility).to_com_ptr::<FUnknown>(),
            _ => None,
        };
        let Some(object) = object else { return kInvalidArgument };
        let ptr = object.as_ptr();
        // SAFETY: `ptr` is a live COM object, so its virtual table pointer is valid.
        let vtbl = unsafe { (*ptr).vtbl };
        // SAFETY: as above.
        let query = unsafe { (*vtbl).queryInterface };
        // SAFETY: queryInterface on the object it belongs to; it adds the reference handed out.
        unsafe { query(ptr, iid as *mut TUID, obj) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        // SAFETY: the host passes a struct to fill.
        let Some(info) = (unsafe { info.as_mut() }) else { return kInvalidArgument };
        if index != 0 {
            return kInvalidArgument;
        }
        info.cid = PROCESSOR_CID;
        copy_text("Audio Module Class", &mut info.category);
        copy_text("Loopwerk Test Gain", &mut info.name);
        copy_text("Fx|Tools", &mut info.subCategories);
        copy_text("Loopwerk", &mut info.vendor);
        copy_text("1.0", &mut info.version);
        kResultOk
    }
}

/// The factory, as a module's `GetPluginFactory` would give it.
pub(crate) fn factory() -> Option<ComPtr<IPluginFactory>> {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>()
}
