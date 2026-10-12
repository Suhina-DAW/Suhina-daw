//! Waking the other process when a word in shared memory changes, and waiting for that with a
//! deadline. Each side first spins briefly (a fast host answers within microseconds), then
//! sleeps in the operating system:
//!
//! - Linux: a futex on the word itself.
//! - Windows: an auto-reset event per direction, named after the region.
//! - Elsewhere (macOS for now): yielding the thread until the deadline. A semaphore-based wait
//!   comes with the macOS prototype (ADR 0006, not decided yet).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use crate::IpcError;

/// Checks before going to sleep.
const SPINS: u32 = 256;

#[cfg_attr(not(windows), allow(dead_code))]
fn event_name(region: &str, role: &str) -> Vec<u16> {
    format!("Local\\{region}-{role}").encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) struct Doorbell {
    #[cfg(windows)]
    event: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a Windows event handle can be signalled and waited on from any thread; on other
// systems the doorbell holds nothing.
#[cfg(windows)]
unsafe impl Send for Doorbell {}

impl Doorbell {
    /// Creates the doorbell `role` of the link `region`.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub(crate) fn create(region: &str, role: &str) -> Result<Self, IpcError> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::CreateEventW;
            let name = event_name(region, role);
            // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; no
            // security attributes; auto-reset, initially not signalled.
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
            if event.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self { event })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    /// Opens the doorbell `role` the other side created.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub(crate) fn open(region: &str, role: &str) -> Result<Self, IpcError> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE};
            let name = event_name(region, role);
            let access = EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE;
            // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
            let event = unsafe { OpenEventW(access, 0, name.as_ptr()) };
            if event.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self { event })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    /// Wakes the other side after `word` was changed.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub(crate) fn ring(&self, word: &AtomicU32) {
        #[cfg(windows)]
        {
            // SAFETY: `self.event` is a valid event handle until `drop`.
            unsafe { windows_sys::Win32::System::Threading::SetEvent(self.event) };
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: FUTEX_WAKE only reads the address, which points at a live, aligned
            // `AtomicU32` in the shared mapping; the shared (non-private) futex reaches waiters
            // in the other process.
            unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAKE, i32::MAX) };
        }
    }

    /// Waits until `done(word)` holds or `deadline` passes; returns whether it held.
    pub(crate) fn wait(&self, word: &AtomicU32, done: impl Fn(u32) -> bool, deadline: Instant) -> bool {
        for _ in 0..SPINS {
            if done(word.load(Ordering::Acquire)) {
                return true;
            }
            std::hint::spin_loop();
        }
        loop {
            let value = word.load(Ordering::Acquire);
            if done(value) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            self.sleep(word, value, deadline - now);
        }
    }

    /// Sleeps until woken, `word` stops being `value`, or `left` has passed (whichever comes
    /// first; spurious wake-ups are fine, the caller checks again).
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    fn sleep(&self, word: &AtomicU32, value: u32, left: std::time::Duration) {
        #[cfg(target_os = "linux")]
        {
            let timeout = libc::timespec {
                tv_sec: left.as_secs().min(i64::MAX as u64) as libc::time_t,
                tv_nsec: libc::c_long::from(left.subsec_nanos() as i32),
            };
            // SAFETY: FUTEX_WAIT reads the aligned `AtomicU32` at the address and sleeps only
            // while it still holds `value`; the timeout points at a live `timespec`.
            unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAIT, value, &raw const timeout) };
        }
        #[cfg(windows)]
        {
            // Milliseconds, rounded up so a short wait still sleeps rather than spinning.
            let millis = left.as_micros().div_ceil(1_000).min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: `self.event` is a valid event handle until `drop`.
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.event, millis) };
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        std::thread::yield_now();
    }
}

#[cfg(windows)]
impl Drop for Doorbell {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW or OpenEventW and is closed only here.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.event) };
    }
}
