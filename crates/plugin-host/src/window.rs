//! The windows plug-ins' editors live in: plain top-level windows on the host's main thread,
//! which the plug-ins' own views are attached to. Several may be open at once, one per
//! instance whose editor is open; what happened to each (asked to close, its size) is kept by
//! its id in a list on the main thread.
//!
//! Windows (Win32) and Linux (X11, through `x11rb`); elsewhere opening one is an error for now
//! (ADR 0006).
//!
//! An editor's window is owned by the engine's window when the engine names it
//! ([`set_owner`]): it floats above Loopwerk, has no taskbar entry of its own and is minimised
//! with it (Win32 owner window; X11 `WM_TRANSIENT_FOR`).

use std::cell::Cell;

thread_local! {
    static OWNER: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The engine's window that the next editor window opened on this thread belongs to (a Win32
/// `HWND`, an X11 window id), or `None` for a window of its own.
pub(crate) fn set_owner(owner: Option<u64>) {
    OWNER.with(|o| o.set(owner));
}

/// The engine's window that the next editor window opened on this thread belongs to, as
/// [`set_owner`](self::set_owner) set it: read only where a window can be opened (Windows, X11).
#[cfg(any(windows, target_os = "linux"))]
fn owner() -> Option<u64> {
    OWNER.with(Cell::get)
}

/// The size of a window's inside, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

/// Names one open window, for the code that has no hold of the [`Window`] itself (a plug-in's
/// frame asking for a new size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowId(usize);

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, IDC_ARROW,
        IsWindow, LoadCursorW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW,
        PostThreadMessageW, QS_ALLINPUT, RegisterClassW, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
        SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE, WM_SIZE, WNDCLASSW, WS_CAPTION, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_THICKFRAME,
    };

    use super::{Size, WindowId};

    /// Wakes the main thread from [`wait`] when a control message has come, from another thread.
    #[derive(Clone, Copy)]
    pub(crate) struct Waker(u32);

    impl Waker {
        /// A waker for the calling thread (the host's main thread).
        pub(crate) fn current() -> Self {
            // SAFETY: no arguments; the id of the calling thread.
            Self(unsafe { GetCurrentThreadId() })
        }

        pub(crate) fn wake(self) {
            // SAFETY: a message without pointers to a thread id; if that thread is gone the call
            // fails, which is fine.
            unsafe { PostThreadMessageW(self.0, WM_APP, 0, 0) };
        }
    }

    /// Waits until a window message is waiting for this thread (input, painting, a plug-in's
    /// timer), the waker wakes it, or `timeout` has passed, so an editor is served at once
    /// rather than between ticks.
    pub(crate) fn wait(timeout: Duration) {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: no handles to wait on, only this thread's message queue.
        unsafe { MsgWaitForMultipleObjectsEx(0, std::ptr::null(), millis, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
    }

    /// What happened to one open window.
    struct State {
        id: WindowId,
        /// The user asked the window to close.
        close_requested: bool,
        /// The window's inside size as the system last reported it.
        size: Size,
        /// The window's style, for turning an inside size into a window size.
        style: u32,
    }

    thread_local! {
        // The editors' windows live on the host's main thread, like the plug-ins' views.
        static OPEN: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
    }

    /// Runs `f` on the state of window `id`, if it is open. The window procedure may be called
    /// from inside a system call made here; such a nested call finds the list busy and does
    /// nothing rather than borrowing it twice.
    fn with_state<R>(id: WindowId, f: impl FnOnce(&mut State) -> R) -> Option<R> {
        OPEN.with(|open| {
            let mut open = open.try_borrow_mut().ok()?;
            open.iter_mut().find(|s| s.id == id).map(f)
        })
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    const CLASS: &str = "LoopwerkPluginEditor";

    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let id = WindowId(hwnd as usize);
        match message {
            WM_CLOSE => {
                // The host closes it, after the plug-in has let go of it.
                with_state(id, |s| s.close_requested = true);
                0
            }
            WM_SIZE => {
                let (width, height) = ((lparam & 0xFFFF) as u32, ((lparam >> 16) & 0xFFFF) as u32);
                with_state(id, |s| s.size = Size { width, height });
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
            // The engine's window, if it still is one: the editor's is owned by it (not a child
            // of it), so it floats above Loopwerk and goes with it.
            let owner = super::owner()
                .and_then(|o| usize::try_from(o).ok())
                .map(|o| o as HWND)
                // SAFETY: only asks whether the handle names a window; any value is allowed.
                .filter(|&o| unsafe { IsWindow(o) } != 0)
                .unwrap_or(std::ptr::null_mut());
            // SAFETY: a registered class and two strings that outlive the call; an owner window
            // just checked (or none); no menu or creation data.
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
                    owner,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                )
            };
            if hwnd.is_null() {
                return Err("could not open the editor's window".into());
            }
            let state = State { id: WindowId(hwnd as usize), close_requested: false, size, style };
            let kept = OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok());
            let window = Self { hwnd };
            if !kept {
                return Err("could not keep the editor's window".into());
            }
            // SAFETY: the window just made.
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            Ok(window)
        }

        /// The native handle, for the plug-in to attach its view to.
        pub(crate) fn handle(&self) -> *mut c_void {
            self.hwnd
        }

        pub(crate) fn id(&self) -> WindowId {
            WindowId(self.hwnd as usize)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            let id = self.id();
            OPEN.with(|open| {
                if let Ok(mut open) = open.try_borrow_mut() {
                    open.retain(|s| s.id != id);
                }
            });
            // SAFETY: the window this made, destroyed once, on the thread that made it.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }

    /// Resizes window `id` so its inside is `size` (a plug-in asked for it).
    pub(crate) fn resize(id: WindowId, size: Size) {
        let Some(style) = with_state(id, |s| {
            s.size = size;
            s.style
        }) else {
            return;
        };
        let (width, height) = outer(size, style);
        // SAFETY: an open window (it is in the list); only its size changes.
        unsafe {
            SetWindowPos(
                id.0 as HWND,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    /// Whether the user asked window `id` to close since the last call.
    pub(crate) fn take_close_request(id: WindowId) -> bool {
        with_state(id, |s| std::mem::take(&mut s.close_requested)).unwrap_or(false)
    }

    /// Window `id`'s inside size as the system last reported it.
    pub(crate) fn client_size(id: WindowId) -> Size {
        with_state(id, |s| s.size).unwrap_or(Size { width: 0, height: 0 })
    }

    /// Handles the window messages waiting for this thread's windows (the editors' and the
    /// plug-ins' own). Called from the host's main loop.
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

#[cfg(target_os = "linux")]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;

    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    use super::{Size, WindowId};

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            WM_PROTOCOLS,
            WM_DELETE_WINDOW,
            _NET_WM_NAME,
            UTF8_STRING,
        }
    }

    /// An open window: its own connection to the X server, the window and what happened to it.
    struct Open {
        connection: RustConnection,
        window: u32,
        atoms: Atoms,
        close_requested: bool,
        size: Size,
    }

    thread_local! {
        // The editors' windows live on the host's main thread, like the plug-ins' views.
        static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
    }

    /// Runs `f` on open window `id`, if it is open (and the list is not busy).
    fn with_open<R>(id: WindowId, f: impl FnOnce(&mut Open) -> R) -> Option<R> {
        OPEN.with(|open| {
            let mut open = open.try_borrow_mut().ok()?;
            open.iter_mut().find(|o| o.window as usize == id.0).map(f)
        })
    }

    fn failed(what: &str) -> impl Fn(String) -> String + '_ {
        move |e| format!("could not {what} the editor's window: {e}")
    }

    pub(crate) struct Window {
        window: u32,
    }

    impl Window {
        /// Opens a window `title` with an inside of `size` on the X server `DISPLAY` names; the
        /// user may resize it when `resizable` (otherwise its smallest and largest size are
        /// that size).
        pub(crate) fn open(title: &str, size: Size, resizable: bool) -> Result<Self, String> {
            let connect = failed("open");
            let (connection, screen) = x11rb::connect(None).map_err(|e| connect(e.to_string()))?;
            let root = connection.setup().roots.get(screen).ok_or("no X screen")?;
            let (root_window, depth, background) = (root.root, root.root_depth, root.black_pixel);
            let atoms = Atoms::new(&connection).map_err(|e| connect(e.to_string()))?;
            let atoms = atoms.reply().map_err(|e| connect(e.to_string()))?;
            let window = connection.generate_id().map_err(|e| connect(e.to_string()))?;
            let width = u16::try_from(size.width).unwrap_or(u16::MAX);
            let height = u16::try_from(size.height).unwrap_or(u16::MAX);
            let aux = CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY).background_pixel(background);
            let make = || -> Result<(), String> {
                let error = |e: x11rb::errors::ConnectionError| connect(e.to_string());
                connection
                    .create_window(
                        depth,
                        window,
                        root_window,
                        0,
                        0,
                        width,
                        height,
                        0,
                        WindowClass::INPUT_OUTPUT,
                        0,
                        &aux,
                    )
                    .map_err(error)?;
                connection
                    .change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes())
                    .map_err(error)?;
                connection
                    .change_property8(
                        PropMode::REPLACE,
                        window,
                        atoms._NET_WM_NAME,
                        atoms.UTF8_STRING,
                        title.as_bytes(),
                    )
                    .map_err(error)?;
                connection
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        atoms.WM_PROTOCOLS,
                        AtomEnum::ATOM,
                        &[atoms.WM_DELETE_WINDOW],
                    )
                    .map_err(error)?;
                connection
                    .change_property8(
                        PropMode::REPLACE,
                        window,
                        AtomEnum::WM_CLASS,
                        AtomEnum::STRING,
                        b"loopwerk-plugin-editor\0Loopwerk\0",
                    )
                    .map_err(error)?;
                if let Some(owner) = super::owner().and_then(|o| u32::try_from(o).ok()) {
                    // Floats above the engine's window and goes with it.
                    connection
                        .change_property32(
                            PropMode::REPLACE,
                            window,
                            AtomEnum::WM_TRANSIENT_FOR,
                            AtomEnum::WINDOW,
                            &[owner],
                        )
                        .map_err(error)?;
                }
                if !resizable {
                    // WM_NORMAL_HINTS: program-specified minimum and maximum size, both this one.
                    let (w, h) = (u32::from(width), u32::from(height));
                    let hints = [0x10 | 0x20, 0, 0, 0, 0, w, h, w, h, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                    connection
                        .change_property32(
                            PropMode::REPLACE,
                            window,
                            AtomEnum::WM_NORMAL_HINTS,
                            AtomEnum::WM_SIZE_HINTS,
                            &hints,
                        )
                        .map_err(error)?;
                }
                connection.map_window(window).map_err(error)?;
                connection.flush().map_err(error)?;
                Ok(())
            };
            make()?;
            let fresh = Open { connection, window, atoms, close_requested: false, size };
            let kept = OPEN.with(|open| match open.try_borrow_mut() {
                Ok(mut open) => {
                    open.push(fresh);
                    None
                }
                Err(_) => Some(fresh),
            });
            if let Some(lost) = kept {
                let _ = lost.connection.destroy_window(lost.window);
                let _ = lost.connection.flush();
                return Err("could not keep the editor's window".into());
            }
            Ok(Self { window })
        }

        /// The window's id, as the pointer-sized handle plug-ins take on X11.
        pub(crate) fn handle(&self) -> *mut c_void {
            self.window as usize as *mut c_void
        }

        pub(crate) fn id(&self) -> WindowId {
            WindowId(self.window as usize)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            let closed = OPEN.with(|open| {
                let mut open = open.try_borrow_mut().ok()?;
                let at = open.iter().position(|o| o.window == self.window)?;
                Some(open.swap_remove(at))
            });
            if let Some(open) = closed {
                let _ = open.connection.destroy_window(open.window);
                let _ = open.connection.flush();
            }
        }
    }

    /// Resizes window `id` so its inside is `size` (a plug-in asked for it).
    pub(crate) fn resize(id: WindowId, size: Size) {
        with_open(id, |open| {
            let aux = ConfigureWindowAux::new().width(size.width).height(size.height);
            let _ = open.connection.configure_window(open.window, &aux);
            let _ = open.connection.flush();
            open.size = size;
        });
    }

    /// Whether the user asked window `id` to close since the last call.
    pub(crate) fn take_close_request(id: WindowId) -> bool {
        with_open(id, |open| std::mem::take(&mut open.close_requested)).unwrap_or(false)
    }

    /// Window `id`'s inside size as the X server last reported it.
    pub(crate) fn client_size(id: WindowId) -> Size {
        with_open(id, |open| open.size).unwrap_or(Size { width: 0, height: 0 })
    }

    /// Whether an editor's window is open, so the main loop serves it often.
    pub(crate) fn any_open() -> bool {
        OPEN.with(|open| open.try_borrow().map_or(true, |open| !open.is_empty()))
    }

    /// Takes in the events waiting for every open window: a close request, a new size. (The
    /// plug-ins' own windows are on their own connections and served through their run loops.)
    pub(crate) fn pump() {
        OPEN.with(|open| {
            let Ok(mut open) = open.try_borrow_mut() else { return };
            for open in open.iter_mut() {
                while let Ok(Some(event)) = open.connection.poll_for_event() {
                    match event {
                        Event::ClientMessage(message) => {
                            let [first, ..] = message.data.as_data32();
                            if message.window == open.window && first == open.atoms.WM_DELETE_WINDOW {
                                open.close_requested = true;
                            }
                        }
                        Event::ConfigureNotify(configured) if configured.window == open.window => {
                            open.size =
                                Size { width: u32::from(configured.width), height: u32::from(configured.height) };
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use std::ffi::c_void;

    use super::{Size, WindowId};

    pub(crate) struct Window;

    impl Window {
        pub(crate) fn open(_title: &str, _size: Size, _resizable: bool) -> Result<Self, String> {
            Err("plug-in editors are not supported on this system yet".into())
        }

        pub(crate) fn handle(&self) -> *mut c_void {
            std::ptr::null_mut()
        }

        pub(crate) fn id(&self) -> WindowId {
            WindowId(0)
        }
    }

    impl Drop for Window {
        /// Nothing to close here; the editors that drop it explicitly do the same everywhere.
        fn drop(&mut self) {}
    }

    pub(crate) fn resize(_id: WindowId, _size: Size) {}

    pub(crate) fn take_close_request(_id: WindowId) -> bool {
        false
    }

    pub(crate) fn client_size(_id: WindowId) -> Size {
        Size { width: 0, height: 0 }
    }

    pub(crate) fn pump() {}

    pub(crate) fn any_open() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub(crate) use imp::any_open;
#[cfg(windows)]
pub(crate) use imp::{Waker, wait};
pub(crate) use imp::{Window, client_size, pump, resize, take_close_request};
