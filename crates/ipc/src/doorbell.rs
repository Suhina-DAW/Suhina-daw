// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Waking the other process when a word in shared memory changes, and waiting for that with a
//! deadline. Each side first spins briefly (a fast host answers within microseconds), then
//! sleeps in the operating system:
//!
//! - Linux: a futex on the word itself.
//! - Windows: an auto-reset event per direction, named after the region, waited on together
//!   with a high-resolution waitable timer of the waiting side's own. The timer wakes the wait
//!   about [`FINE_TAIL`] before the deadline and the rest is spent yielding, so a wait ends on
//!   time rather than on the system timer's next tick (15.6 ms by default). Without a
//!   high-resolution timer (Windows 10 before 1803) the event's own timeout is cut a tick short
//!   and the rest is yielded the same way.
//! - Elsewhere (macOS for now): yielding the thread until the deadline. A semaphore-based wait
//!   comes with the macOS prototype (ADR 0006, not decided yet).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::IpcError;

/// Checks before going to sleep.
const SPINS: u32 = 256;

/// How long before the deadline a high-resolution timer wakes the wait; the rest is yielded.
/// Such a timer wakes within about half a millisecond, so one millisecond keeps the deadline.
#[cfg_attr(not(windows), allow(dead_code))]
const FINE_TAIL: Duration = Duration::from_millis(1);

/// One tick of the Windows system timer at its default resolution (15.625 ms), rounded up: how
/// late a wait with a millisecond timeout can end.
#[cfg_attr(not(windows), allow(dead_code))]
const TICK: Duration = Duration::from_millis(16);

#[cfg_attr(not(windows), allow(dead_code))]
fn event_name(region: &str, role: &str) -> Vec<u16> {
    format!("Local\\{region}-{role}").encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) struct Doorbell {
    #[cfg(windows)]
    event: windows_sys::Win32::Foundation::HANDLE,
    /// A high-resolution waitable timer for this side's waits, unnamed and not shared; `None`
    /// where Windows cannot make one.
    #[cfg(windows)]
    timer: Option<windows_sys::Win32::Foundation::HANDLE>,
}

// SAFETY: Windows event and waitable timer handles can be set, signalled and waited on from any
// thread (the doorbell is not `Sync`, so one thread at a time arms its timer); on other systems
// the doorbell holds nothing.
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
            Ok(Self { event, timer: high_resolution_timer() })
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
            Ok(Self { event, timer: high_resolution_timer() })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    /// Whether waits end on a high-resolution timer (always, except on Windows that has none).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_high_resolution(&self) -> bool {
        #[cfg(windows)]
        let fine = self.timer.is_some();
        #[cfg(not(windows))]
        let fine = true;
        fine
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
        self.sleep_windows(left);
        #[cfg(not(any(target_os = "linux", windows)))]
        std::thread::yield_now();
    }

    /// [`Doorbell::sleep`] on Windows. A wait with a millisecond timeout ends on the system
    /// timer's tick, up to 15.6 ms late, so it never sleeps into the last [`FINE_TAIL`] (with the
    /// high-resolution timer) or [`TICK`] (without) before the deadline; there it yields, and
    /// the caller checks the word and the deadline again.
    #[cfg(windows)]
    fn sleep_windows(&self, left: Duration) {
        use windows_sys::Win32::System::Threading::{SetWaitableTimer, WaitForMultipleObjects, WaitForSingleObject};
        if let Some(timer) = self.timer
            && let Some(nap) = left.checked_sub(FINE_TAIL)
        {
            // Relative due times are negative, in 100 ns units; at least one.
            let due = -i64::try_from((nap.as_nanos() / 100).max(1)).unwrap_or(i64::MAX);
            // SAFETY: `timer` is a valid waitable timer handle until `drop`; `due` is a live
            // `i64`; no completion routine, so nothing runs on this thread.
            let armed = unsafe { SetWaitableTimer(timer, &raw const due, 0, None, std::ptr::null(), 0) };
            if armed != 0 {
                let handles = [self.event, timer];
                // A backstop in case the timer never fires; it is never shorter than the nap.
                let backstop = millis_rounded_up(left).saturating_add(1);
                // SAFETY: `handles` holds two valid handles and outlives the call; it returns
                // when either is signalled (the event: rung; the timer: the nap is over).
                unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, backstop) };
                return;
            }
        }
        match left.checked_sub(TICK) {
            Some(nap) => {
                let millis = u32::try_from(nap.as_millis()).unwrap_or(u32::MAX - 1).min(u32::MAX - 1);
                // SAFETY: `self.event` is a valid event handle until `drop`.
                unsafe { WaitForSingleObject(self.event, millis) };
            }
            None => std::thread::yield_now(),
        }
    }
}

/// Milliseconds, rounded up, short of `INFINITE`.
#[cfg(windows)]
fn millis_rounded_up(d: Duration) -> u32 {
    u32::try_from(d.as_micros().div_ceil(1_000)).unwrap_or(u32::MAX - 1).min(u32::MAX - 1)
}

/// A high-resolution waitable timer (Windows 10 1803 and later), auto-reset; `None` where
/// Windows cannot make one.
#[cfg(windows)]
fn high_resolution_timer() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, TIMER_ALL_ACCESS,
    };
    // SAFETY: no security attributes and no name (both may be null); the flag asks for a
    // high-resolution timer, which older Windows refuses with a null handle.
    let timer = unsafe {
        CreateWaitableTimerExW(
            std::ptr::null(),
            std::ptr::null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS,
        )
    };
    (!timer.is_null()).then_some(timer)
}

#[cfg(windows)]
impl Drop for Doorbell {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW or OpenEventW and is closed only here.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.event) };
        if let Some(timer) = self.timer {
            // SAFETY: the handle came from CreateWaitableTimerExW and is closed only here.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(timer) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A doorbell nobody rings: each wait must end at its deadline, not before and not a timer
    /// tick after (#211). 40 waits from 1 to 12.7 ms, as plug-in blocks' deadlines are.
    #[test]
    fn an_unanswered_wait_ends_at_its_deadline() {
        let name = format!("suhina-ipc-doorbell-test-{}", std::process::id());
        let bell = Doorbell::create(&name, "test").unwrap_or_else(|e| panic!("{e}"));
        let word = AtomicU32::new(0);
        let mut late = Vec::new();
        for n in 0..40u64 {
            let timeout = Duration::from_micros(1_000 + n * 300);
            let deadline = Instant::now() + timeout;
            assert!(!bell.wait(&word, |v| v != 0, deadline));
            let ended = Instant::now();
            assert!(ended >= deadline, "ended {:?} early", deadline - ended);
            late.push(ended - deadline);
        }
        late.sort();
        let worst = late.last().copied().unwrap_or_default();
        let median = late.get(late.len() / 2).copied().unwrap_or_default();
        // Without the fix a wait on Windows ended on the system timer's next tick, up to
        // 15.6 ms late, so the median lay at several milliseconds. A single wait can still be
        // late when the test machine is busy (the other tests run at the same time), so the
        // worst case only has the bound the link's own test has.
        //
        // Elsewhere (macOS) the wait yields until its deadline, and a yield on a busy shared
        // runner can lose the core for tens of milliseconds, so how late it ends measures the
        // runner's scheduler (#522): there only "never early" above and a loose bound hold.
        if cfg!(any(target_os = "linux", windows)) {
            assert!(median < Duration::from_millis(1), "median {median:?}, all {late:?}");
            assert!(worst < Duration::from_millis(40), "worst {worst:?}, all {late:?}");
        } else {
            assert!(worst < Duration::from_millis(500), "worst {worst:?}, all {late:?}");
        }
    }

    /// A ring wakes a sleeping wait long before its deadline.
    #[test]
    fn a_ring_wakes_the_wait() {
        let name = format!("suhina-ipc-doorbell-ring-{}", std::process::id());
        let bell = Doorbell::create(&name, "test").unwrap_or_else(|e| panic!("{e}"));
        let other = Doorbell::open(&name, "test").unwrap_or_else(|e| panic!("{e}"));
        let word = std::sync::Arc::new(AtomicU32::new(0));
        let ringer = {
            let word = word.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                word.store(1, Ordering::Release);
                other.ring(&word);
            })
        };
        let started = Instant::now();
        assert!(bell.wait(&word, |v| v == 1, started + Duration::from_secs(5)));
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert!(ringer.join().is_ok());
    }

    /// The high-resolution timer exists on every Windows the project supports (10 1803 and
    /// later), so CI checks the fine path rather than the fallback.
    #[test]
    fn waits_are_high_resolution() {
        let name = format!("suhina-ipc-doorbell-hr-{}", std::process::id());
        let bell = Doorbell::create(&name, "test").unwrap_or_else(|e| panic!("{e}"));
        assert!(bell.is_high_resolution());
    }

    #[cfg(windows)]
    #[test]
    fn milliseconds_round_up_and_stay_short_of_infinite() {
        assert_eq!(millis_rounded_up(Duration::ZERO), 0);
        assert_eq!(millis_rounded_up(Duration::from_micros(1)), 1);
        assert_eq!(millis_rounded_up(Duration::from_millis(10)), 10);
        assert_eq!(millis_rounded_up(Duration::MAX), u32::MAX - 1);
    }
}
