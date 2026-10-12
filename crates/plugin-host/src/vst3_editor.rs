//! A VST3 plug-in's editor: the controller's `IPlugView`, attached to a window of the host's
//! own ([`crate::window`]). The plug-in asks for a new size through the frame the host gives
//! it (`IPlugFrame::resizeView`); when the user resizes a resizable window, the view is told
//! (`onSize`).

use vst3::Steinberg::Vst::{IEditController, IEditControllerTrait};
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use crate::window::{self, Size, Window};

/// The frame the view asks to be resized through.
struct Frame;

impl Class for Frame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        // SAFETY: the view asking, and the size it wants, for the call.
        let (Some(view), Some(rect)) = (unsafe { ComRef::from_raw(view) }, unsafe { size.as_mut() }) else {
            return kInvalidArgument;
        };
        window::resize(size_of(rect));
        // SAFETY: a live view, told its new size as the SDK asks after a resize.
        unsafe { view.onSize(rect) };
        kResultOk
    }
}

fn size_of(rect: &ViewRect) -> Size {
    let span = |from: i32, to: i32| u32::try_from(to.saturating_sub(from)).unwrap_or(0).clamp(1, 16_384);
    Size { width: span(rect.left, rect.right), height: span(rect.top, rect.bottom) }
}

/// An open editor.
pub(crate) struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<Frame>,
    window: Window,
    size: Size,
    resizable: bool,
}

/// The platform type a view is attached as on this system.
const PLATFORM: FIDString = if cfg!(windows) { kPlatformTypeHWND } else { kPlatformTypeX11EmbedWindowID };

impl Editor {
    /// Opens `controller`'s editor in a window titled `title`.
    pub(crate) fn open(controller: &ComPtr<IEditController>, title: &str) -> Result<Self, String> {
        // SAFETY: a live controller; the name is a static C string.
        let raw = unsafe { controller.createView(c"editor".as_ptr()) };
        // SAFETY: an owned reference to the view, or null when there is no editor.
        let view = unsafe { ComPtr::from_raw(raw) }.ok_or("the plug-in has no editor")?;
        // SAFETY: a live view; the platform name is a static C string.
        if unsafe { view.isPlatformTypeSupported(PLATFORM) } != kResultOk {
            return Err("the plug-in's editor does not run on this system".into());
        }
        let mut rect = ViewRect { left: 0, top: 0, right: 800, bottom: 600 };
        // SAFETY: a live view filling a rectangle.
        let _ = unsafe { view.getSize(&mut rect) };
        let size = size_of(&rect);
        // SAFETY: a live view.
        let resizable = unsafe { view.canResize() } == kResultTrue;
        let window = Window::open(title, size, resizable)?;
        let frame = ComWrapper::new(Frame);
        let frame_pointer = frame.to_com_ptr::<IPlugFrame>().ok_or("frame")?;
        // SAFETY: a live view and a frame that outlives it (it is dropped after `removed`).
        let _ = unsafe { view.setFrame(frame_pointer.as_ptr()) };
        // SAFETY: a live view and the native window it goes in, which stays open until `close`.
        if unsafe { view.attached(window.handle(), PLATFORM) } != kResultOk {
            // SAFETY: as above, letting go of the frame.
            let _ = unsafe { view.setFrame(std::ptr::null_mut()) };
            return Err("the plug-in's editor did not open".into());
        }
        Ok(Self { view, frame, window, size, resizable })
    }

    pub(crate) fn size(&self) -> Size {
        self.size
    }

    /// Tells the view about a size the user gave the window. Returns whether the user asked
    /// the window to close.
    pub(crate) fn tick(&mut self) -> bool {
        if self.resizable {
            let now = window::client_size();
            if now != self.size && now.width > 0 && now.height > 0 {
                let mut rect = ViewRect { left: 0, top: 0, right: now.width as i32, bottom: now.height as i32 };
                // SAFETY: a live view, asked whether it takes the size and then given it.
                let _ = unsafe { self.view.checkSizeConstraint(&mut rect) };
                // SAFETY: as above.
                let _ = unsafe { self.view.onSize(&mut rect) };
                self.size = now;
            }
        }
        window::take_close_request()
    }

    /// Closes the editor: the view lets go of the window, then the window closes.
    pub(crate) fn close(self) {
        // SAFETY: a live, attached view.
        let _ = unsafe { self.view.removed() };
        // SAFETY: as above.
        let _ = unsafe { self.view.setFrame(std::ptr::null_mut()) };
        let Self { view, frame, window, .. } = self;
        drop(view);
        drop(frame);
        drop(window);
    }
}
