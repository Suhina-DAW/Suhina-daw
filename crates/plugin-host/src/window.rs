//! The window a plug-in's editor lives in: a plain top-level window on the host's main thread,
//! which the plug-in's own view is attached to. One editor is open per host process at most,
//! so the window's state (asked to close, its size) is kept in statics.
//!
//! Windows only for now; elsewhere opening one is an error (X11 embedding comes with Linux
//! support, ADR 0006).

/// The size of a window's inside, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, IDC_ARROW,
        LoadCursorW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
        SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE, WM_SIZE, WNDCLASSW, WS_CAPTION, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_THICKFRAME,
    };

    use super::Size;

    /// The user asked the window to close.
    static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
    /// The window's inside size as the system last reported it, width in the high half.
    static CLIENT_SIZE: AtomicU64 = AtomicU64::new(0);
    /// The open window's style, for turning an inside size into a window size.
    static STYLE: AtomicU32 = AtomicU32::new(0);
    /// The open window, as a number (0 for none).
    static OPEN: AtomicIsize = AtomicIsize::new(0);

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    const CLASS: &str = "LoopwerkPluginEditor";

    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_CLOSE => {
                // The host closes it, after the plug-in has let go of it.
                CLOSE_REQUESTED.store(true, Ordering::Release);
                0
            }
            WM_SIZE => {
                let (width, height) = ((lparam & 0xFFFF) as u64, ((lparam >> 16) & 0xFFFF) as u64);
                CLIENT_SIZE.store(width << 32 | height, Ordering::Release);
                0
            }
            // SAFETY: the system's default handling of a message for this window.
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    /// Registers the window class once; whether it is registered.
    fn register() -> bool {
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        *REGISTERED.get_or_init(|| {
            let class = wide(CLASS);
            // SAFETY: a null name asks for this program's own module.
            let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
            // SAFETY: the system's own arrow cursor.
            let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) };
            let description = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: cursor,
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            // SAFETY: the description and the class name it points to outlive the call; the
            // system copies them.
            unsafe { RegisterClassW(&description) != 0 }
        })
    }

    /// The outer size for an inside of `size` with `style`.
    fn outer(size: Size, style: u32) -> (i32, i32) {
        let mut rect = RECT { left: 0, top: 0, right: size.width as i32, bottom: size.height as i32 };
        // SAFETY: a rectangle to adjust in place.
        unsafe { AdjustWindowRectEx(&mut rect, style, 0, 0) };
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    pub(crate) struct Window {
        hwnd: HWND,
    }

    impl Window {
        /// Opens a window `title` with an inside of `size`; the user may resize it when
        /// `resizable`.
        pub(crate) fn open(title: &str, size: Size, resizable: bool) -> Result<Self, String> {
            if OPEN.load(Ordering::Acquire) != 0 {
                return Err("an editor is already open".into());
            }
            if !register() {
                return Err("could not register the editor's window class".into());
            }
            let mut style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            if resizable {
                style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
            }
            let (width, height) = outer(size, style);
            let (class, title) = (wide(CLASS), wide(title));
            // SAFETY: as in `register`.
            let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
            // SAFETY: a registered class and two strings that outlive the call; no parent, menu
            // or creation data.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    title.as_ptr(),
                    style,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    width,
                    height,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                )
            };
            if hwnd.is_null() {
                return Err("could not open the editor's window".into());
            }
            CLOSE_REQUESTED.store(false, Ordering::Release);
            CLIENT_SIZE.store(u64::from(size.width) << 32 | u64::from(size.height), Ordering::Release);
            STYLE.store(style, Ordering::Release);
            OPEN.store(hwnd as isize, Ordering::Release);
            // SAFETY: the window just made.
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            Ok(Self { hwnd })
        }

        /// The native handle, for the plug-in to attach its view to.
        pub(crate) fn handle(&self) -> *mut c_void {
            self.hwnd
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            OPEN.store(0, Ordering::Release);
            // SAFETY: the window this made, destroyed once, on the thread that made it.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }

    /// Resizes the open window so its inside is `size` (a plug-in asked for it).
    pub(crate) fn resize(size: Size) {
        let hwnd = OPEN.load(Ordering::Acquire) as HWND;
        if hwnd.is_null() {
            return;
        }
        let (width, height) = outer(size, STYLE.load(Ordering::Acquire));
        CLIENT_SIZE.store(u64::from(size.width) << 32 | u64::from(size.height), Ordering::Release);
        // SAFETY: the open window; only its size changes.
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, width, height, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)
        };
    }

    /// Whether the user asked the window to close since the last call.
    pub(crate) fn take_close_request() -> bool {
        CLOSE_REQUESTED.swap(false, Ordering::AcqRel)
    }

    /// The window's inside size as the system last reported it.
    pub(crate) fn client_size() -> Size {
        let both = CLIENT_SIZE.load(Ordering::Acquire);
        Size { width: (both >> 32) as u32, height: both as u32 }
    }

    /// Handles the window messages waiting for this thread's windows (the editor's and the
    /// plug-in's own). Called from the host's main loop.
    pub(crate) fn pump() {
        // SAFETY: a zeroed message is a valid value to be filled.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: the message is filled by the call; null asks for any window of this thread.
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: a message just taken from the queue.
            unsafe { TranslateMessage(&message) };
            // SAFETY: as above.
            unsafe { DispatchMessageW(&message) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::ffi::c_void;

    use super::Size;

    pub(crate) struct Window;

    impl Window {
        pub(crate) fn open(_title: &str, _size: Size, _resizable: bool) -> Result<Self, String> {
            Err("plug-in editors are not supported on this system yet".into())
        }

        pub(crate) fn handle(&self) -> *mut c_void {
            std::ptr::null_mut()
        }
    }

    impl Drop for Window {
        /// Nothing to close here; the editors that drop it explicitly do the same everywhere.
        fn drop(&mut self) {}
    }

    pub(crate) fn resize(_size: Size) {}

    pub(crate) fn take_close_request() -> bool {
        false
    }

    pub(crate) fn client_size() -> Size {
        Size { width: 0, height: 0 }
    }

    pub(crate) fn pump() {}
}

pub(crate) use imp::{Window, client_size, pump, resize, take_close_request};
