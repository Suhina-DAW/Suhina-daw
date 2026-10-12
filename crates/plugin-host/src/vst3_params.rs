//! Parameter changes for a VST3 processor. A processor learns new parameter values only through
//! `process`: each block carries an `IParameterChanges` list with a queue of values per
//! parameter. The control thread puts `(id, value)` pairs in a [`Pending`] list; the audio
//! thread moves them into a list made before processing starts, so it does not allocate.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};

/// Most parameter changes one block carries; more wait for the next block.
pub(crate) const MAX_CHANGES: usize = 4096;

/// Values waiting for the next block: parameter id and normalized value.
pub(crate) type Pending = Arc<Mutex<Vec<(ParamID, ParamValue)>>>;

/// One parameter's change in a block: a single point at the block's start.
struct Queue {
    id: Cell<ParamID>,
    value: Cell<ParamValue>,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        1
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 || offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: both pointers are non-null and point at values the plug-in passed to fill.
        unsafe { *offset = 0 };
        // SAFETY: as above.
        unsafe { *value = self.value.get() };
        kResultOk
    }

    unsafe fn addPoint(&self, _offset: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kResultFalse
    }
}

/// The changes of one block.
struct Changes {
    queues: Vec<ComWrapper<Queue>>,
    pointers: Vec<ComPtr<IParamValueQueue>>,
    count: Cell<usize>,
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.count.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        match usize::try_from(index).ok().filter(|i| *i < self.count.get()).and_then(|i| self.pointers.get(i)) {
            // The list keeps its reference; the plug-in borrows it for the block.
            Some(queue) => queue.as_ptr(),
            None => std::ptr::null_mut(),
        }
    }

    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

/// The audio thread's side: the list it hands to `process`.
pub(crate) struct BlockChanges {
    changes: ComWrapper<Changes>,
    pointer: Option<ComPtr<IParameterChanges>>,
    pending: Pending,
}

impl BlockChanges {
    /// Made before processing starts; allocates the list for [`MAX_CHANGES`] parameters.
    pub(crate) fn new(pending: Pending) -> Self {
        let queues: Vec<ComWrapper<Queue>> =
            (0..MAX_CHANGES).map(|_| ComWrapper::new(Queue { id: Cell::new(0), value: Cell::new(0.0) })).collect();
        let pointers = queues.iter().filter_map(|q| q.to_com_ptr::<IParamValueQueue>()).collect();
        let changes = ComWrapper::new(Changes { queues, pointers, count: Cell::new(0) });
        let pointer = changes.to_com_ptr::<IParameterChanges>();
        Self { changes, pointer, pending }
    }

    /// Takes what is pending (if the control thread is not using the list right now) and
    /// returns the list for this block, or null when nothing changed. Real-time safe: no
    /// allocation, and it never waits for the lock.
    pub(crate) fn take(&mut self) -> *mut IParameterChanges {
        let Ok(mut pending) = self.pending.try_lock() else { return std::ptr::null_mut() };
        if pending.is_empty() {
            return std::ptr::null_mut();
        }
        let room = pending.len().min(self.changes.pointers.len());
        for ((id, value), queue) in pending.drain(..room).zip(self.changes.queues.iter()) {
            queue.id.set(id);
            queue.value.set(value);
        }
        self.changes.count.set(room);
        self.pointer.as_ref().map_or(std::ptr::null_mut(), ComPtr::as_ptr)
    }

    /// Empties the list after the block.
    pub(crate) fn done(&self) {
        self.changes.count.set(0);
    }
}
