//! The windows plug-ins' editors live in: plain top-level windows on the host's main thread,
//! which the plug-ins' own views are attached to. Several may be open at once, one per
//! instance whose editor is open; what happened to each (asked to close, its size) is kept by
//! its id in a list on the main thread.
//!
//! Windows (Win32) and Linux (X11, through `x11rb`); elsewhere opening one is an error for now
//! (ADR 0006).
//!
//! An editor's window is owned by the engine's window when the engine names it
//! ([`set_owner`]): it floats above Suhina, has no taskbar entry of its own and is minimised
//! with it (Win32 owner window; X11 `WM_TRANSIENT_FOR`).
//!
//! An embedded editor ([`Window::open_embedded`]) is instead a child inside the engine's
//! window, where its device panel shows it: two windows, a *clip* the size of the panel's
//! visible area (what falls outside it is cut off, so a scrolled editor does not paint over the
//! rest of Suhina) and inside it the *view* the plug-in draws in, at the editor's full size.
//! The engine says where both go ([`place`]); until it does, the editor is hidden.

use std::cell::Cell;

pub(crate) use suhina_plugins::Placement;

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
        AdjustWindowRectEx, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_TOP,
        IDC_ARROW, IsWindow, LoadCursorW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
        PeekMessageW, PostThreadMessageW, QS_ALLINPUT, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
        WM_SIZE, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_OVERLAPPED, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
    };

    use super::{Placement, Size, WindowId};

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
        /// An embedded editor's clip window, the one inside the engine's window; the window
        /// itself (the view) is inside it.
        clip: Option<HWND>,
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

    const CLASS: &str = "SuhinaPluginEditor";

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
            // of it), so it floats above Suhina and goes with it.
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
            let state = State { id: WindowId(hwnd as usize), close_requested: false, size, style, clip: None };
            let kept = OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok());
            let window = Self { hwnd };
            if !kept {
                return Err("could not keep the editor's window".into());
            }
            // SAFETY: the window just made.
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            Ok(window)
        }

        /// Opens an editor's window of inside `size` as a child inside the engine's window
        /// `parent` (a Win32 `HWND`, which may belong to another process), hidden until
        /// [`place`] puts it somewhere in it.
        pub(crate) fn open_embedded(parent: u64, size: Size) -> Result<Self, String> {
            if !register() {
                return Err("could not register the editor's window class".into());
            }
            let parent = usize::try_from(parent).ok().map(|p| p as HWND).ok_or("no window to embed the editor in")?;
            // SAFETY: only asks whether the handle names a window; any value is allowed.
            if unsafe { IsWindow(parent) } == 0 {
                return Err("the window to embed the editor in is gone".into());
            }
            let (class, title) = (wide(CLASS), wide(""));
            // SAFETY: as in `register`.
            let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
            // The clip: a hidden, empty child of the engine's window, which `place` sizes to
            // the panel's visible area and shows.
            let clip_style = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
            // SAFETY: a registered class and two strings that outlive the call; a parent
            // window just checked; no menu or creation data.
            let clip = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    title.as_ptr(),
                    clip_style,
                    0,
                    0,
                    0,
                    0,
                    parent,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                )
            };
            if clip.is_null() {
                return Err("could not open the editor's window in the engine's".into());
            }
            // The view: the editor's full size, inside the clip.
            let style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
            let (width, height) = outer(size, style);
            // SAFETY: as above, inside the clip just made.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    title.as_ptr(),
                    style,
                    0,
                    0,
                    width,
                    height,
                    clip,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                )
            };
            if hwnd.is_null() {
                // SAFETY: the clip this made, on this thread.
                unsafe { DestroyWindow(clip) };
                return Err("could not open the editor's window in the engine's".into());
            }
            let state = State { id: WindowId(hwnd as usize), close_requested: false, size, style, clip: Some(clip) };
            let kept = OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok());
            if !kept {
                // SAFETY: both windows this made, on this thread; the view first, as a child of
                // the clip.
                unsafe { DestroyWindow(hwnd) };
                // SAFETY: as above.
                unsafe { DestroyWindow(clip) };
                return Err("could not keep the editor's window".into());
            }
            Ok(Self { hwnd })
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
            let clip = OPEN.with(|open| {
                let mut open = open.try_borrow_mut().ok()?;
                let at = open.iter().position(|s| s.id == id)?;
                open.swap_remove(at).clip
            });
            // SAFETY: the window this made, destroyed once, on the thread that made it.
            unsafe { DestroyWindow(self.hwnd) };
            if let Some(clip) = clip {
                // SAFETY: the clip this made for an embedded window, destroyed once, on the
                // thread that made it, after the view inside it.
                unsafe { DestroyWindow(clip) };
            }
        }
    }

    /// Puts embedded window `id` at `placement` in the engine's window: the clip at the
    /// visible area, the view inside it so that the editor's origin lands at `(x, y)` of the
    /// engine's window. An empty visible area hides it. Nothing happens to a window that is
    /// not embedded.
    pub(crate) fn place(id: WindowId, placement: Placement) {
        let Some(Some(clip)) = with_state(id, |s| s.clip) else { return };
        let Placement { x, y, clip_x, clip_y, clip_width, clip_height } = placement;
        if clip_width == 0 || clip_height == 0 {
            // SAFETY: an open window (it is in the list).
            unsafe { ShowWindow(clip, SW_HIDE) };
            return;
        }
        let span = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
        // SAFETY: the view, an open window; only its place inside the clip changes.
        unsafe {
            SetWindowPos(
                id.0 as HWND,
                std::ptr::null_mut(),
                x.saturating_sub(clip_x),
                y.saturating_sub(clip_y),
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        // SAFETY: the clip, an open window, put over the engine's other children where the
        // panel shows the editor.
        unsafe {
            SetWindowPos(
                clip,
                HWND_TOP,
                clip_x,
                clip_y,
                span(clip_width),
                span(clip_height),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
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

    use super::{Placement, Size, WindowId};

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
        /// An embedded editor's clip window, the one inside the engine's window; the window
        /// itself (the view) is inside it.
        clip: Option<u32>,
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
                        b"suhina-plugin-editor\0Suhina\0",
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
            eprintln!("TRACE host opened window={window} delete={}", atoms.WM_DELETE_WINDOW);
            let fresh = Open { connection, window, atoms, close_requested: false, size, clip: None };
            Self::keep(fresh)
        }

        /// Opens an editor's window of inside `size` as a child inside the engine's window
        /// `parent` (an X11 window id, which may belong to another client), hidden until
        /// [`place`] puts it somewhere in it.
        pub(crate) fn open_embedded(parent: u64, size: Size) -> Result<Self, String> {
            let connect = failed("open");
            let parent = u32::try_from(parent).map_err(|_| "no window to embed the editor in")?;
            let (connection, screen) = x11rb::connect(None).map_err(|e| connect(e.to_string()))?;
            let root = connection.setup().roots.get(screen).ok_or("no X screen")?;
            let (depth, background) = (root.root_depth, root.black_pixel);
            let atoms = Atoms::new(&connection).map_err(|e| connect(e.to_string()))?;
            let atoms = atoms.reply().map_err(|e| connect(e.to_string()))?;
            let clip = connection.generate_id().map_err(|e| connect(e.to_string()))?;
            let window = connection.generate_id().map_err(|e| connect(e.to_string()))?;
            let width = u16::try_from(size.width).unwrap_or(u16::MAX);
            let height = u16::try_from(size.height).unwrap_or(u16::MAX);
            let make = || -> Result<(), String> {
                let error = |e: x11rb::errors::ConnectionError| connect(e.to_string());
                // The clip: unmapped (hidden) until `place` sizes it to the panel's visible
                // area; 1 × 1 for now, as X refuses an empty window. The server is asked
                // whether it took it (`check`): a parent that is no window is an error here
                // rather than an event later.
                let plain = CreateWindowAux::new();
                connection
                    .create_window(depth, clip, parent, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &plain)
                    .map_err(error)?
                    .check()
                    .map_err(|e| format!("no window to embed the editor in: {e}"))?;
                // The view: the editor's full size, inside the clip.
                let aux = CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY).background_pixel(background);
                connection
                    .create_window(depth, window, clip, 0, 0, width, height, 0, WindowClass::INPUT_OUTPUT, 0, &aux)
                    .map_err(error)?
                    .check()
                    .map_err(|e| connect(e.to_string()))?;
                connection
                    .change_property8(
                        PropMode::REPLACE,
                        window,
                        AtomEnum::WM_CLASS,
                        AtomEnum::STRING,
                        b"suhina-plugin-editor\0Suhina\0",
                    )
                    .map_err(error)?;
                connection.map_window(window).map_err(error)?;
                connection.flush().map_err(error)?;
                Ok(())
            };
            if let Err(e) = make() {
                let _ = connection.destroy_window(window);
                let _ = connection.destroy_window(clip);
                let _ = connection.flush();
                return Err(e);
            }
            let fresh = Open { connection, window, atoms, close_requested: false, size, clip: Some(clip) };
            Self::keep(fresh)
        }

        /// Keeps `fresh` in the list of open windows, or closes it when the list is busy.
        fn keep(fresh: Open) -> Result<Self, String> {
            let window = fresh.window;
            let kept = OPEN.with(|open| match open.try_borrow_mut() {
                Ok(mut open) => {
                    open.push(fresh);
                    None
                }
                Err(_) => Some(fresh),
            });
            if let Some(lost) = kept {
                lost.close();
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

    impl Open {
        /// Destroys the window (and an embedded one's clip), and waits until the X server has
        /// done so: it serves its clients in no set order, so without the round trip another
        /// client told the editor closed could still find the window (#237).
        fn close(self) {
            eprintln!("TRACE host close window={}", self.window);
            let _ = self.connection.destroy_window(self.window);
            if let Some(clip) = self.clip {
                let _ = self.connection.destroy_window(clip);
            }
            if let Ok(cookie) = self.connection.get_input_focus() {
                let _ = cookie.reply();
            }
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
                open.close();
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

    /// Puts embedded window `id` at `placement` in the engine's window: the clip at the
    /// visible area, the view inside it so that the editor's origin lands at `(x, y)` of the
    /// engine's window. An empty visible area hides it. Nothing happens to a window that is
    /// not embedded.
    pub(crate) fn place(id: WindowId, placement: Placement) {
        with_open(id, |open| {
            let Some(clip) = open.clip else { return };
            let Placement { x, y, clip_x, clip_y, clip_width, clip_height } = placement;
            if clip_width == 0 || clip_height == 0 {
                let _ = open.connection.unmap_window(clip);
                let _ = open.connection.flush();
                return;
            }
            let view = ConfigureWindowAux::new().x(x.saturating_sub(clip_x)).y(y.saturating_sub(clip_y));
            let _ = open.connection.configure_window(open.window, &view);
            let clip_aux = ConfigureWindowAux::new()
                .x(clip_x)
                .y(clip_y)
                .width(clip_width)
                .height(clip_height)
                .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
            let _ = open.connection.configure_window(clip, &clip_aux);
            let _ = open.connection.map_window(clip);
            let _ = open.connection.flush();
        });
    }

    /// Whether the user asked window `id` to close since the last call.
    pub(crate) fn take_close_request(id: WindowId) -> bool {
        let taken = with_open(id, |open| std::mem::take(&mut open.close_requested));
        if taken != Some(false) {
            eprintln!("TRACE host take_close_request id={} -> {taken:?}", id.0);
        }
        taken.unwrap_or(false)
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
                loop {
                    let polled = open.connection.poll_for_event();
                    static POLLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let n = POLLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if n % 500 == 0 {
                        eprintln!("TRACE host pump heartbeat {n} window={}", open.window);
                    }
                    if !matches!(polled, Ok(None)) {
                        eprintln!("TRACE host pump window={} polled={polled:?}", open.window);
                    }
                    let Ok(Some(event)) = polled else { break };
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

    use super::{Placement, Size, WindowId};

    pub(crate) struct Window;

    impl Window {
        pub(crate) fn open(_title: &str, _size: Size, _resizable: bool) -> Result<Self, String> {
            Err("plug-in editors are not supported on this system yet".into())
        }

        pub(crate) fn open_embedded(_parent: u64, _size: Size) -> Result<Self, String> {
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

    pub(crate) fn place(_id: WindowId, _placement: Placement) {}

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
pub(crate) use imp::{Window, client_size, place, pump, resize, take_close_request};

/// Where an editor opens: a window of its own (owned by the engine's, when it named one with
/// [`set_owner`]), or embedded inside the engine's window `parent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Floating,
    Embedded { parent: u64 },
}

impl Window {
    /// Opens a window for an editor of inside `size` as `mode` says: floating, titled `title`
    /// and resizable by the user when `resizable`; or embedded and hidden until placed.
    pub(crate) fn open_as(mode: Mode, title: &str, size: Size, resizable: bool) -> Result<Self, String> {
        match mode {
            Mode::Floating => Self::open(title, size, resizable),
            Mode::Embedded { parent } => Self::open_embedded(parent, size),
        }
    }
}
