// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time scheduling on Linux: `SCHED_FIFO` set by the process itself, or `SCHED_RR`
//! granted by rtkit.

use std::time::Duration;

use rustix::process::{Resource, Rlimit};
use thread_priority::{
    NormalThreadSchedulePolicy, RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityOsValue,
    ThreadPriorityValue, ThreadSchedulePolicy,
};

/// The real-time priority asked for: low among real-time threads, so the sound server's own
/// threads (PipeWire's or JACK's, which feed the device) stay above the ones that feed them, and
/// within rtkit's usual ceiling of 20.
pub(crate) const RT_PRIORITY: u8 = 10;

/// How long rtkit may take to answer before the thread plays on without it. rtkit is started
/// on demand by D-Bus, so the first answer can take a moment.
const RTKIT_WAIT: Duration = Duration::from_secs(2);

const RTKIT_NAME: &str = "org.freedesktop.RealtimeKit1";
const RTKIT_PATH: &str = "/org/freedesktop/RealtimeKit1";

/// Puts the calling thread in `SCHED_FIFO` at [`RT_PRIORITY`]: allowed when the user has
/// real-time rights (`RLIMIT_RTPRIO` at least that high, or `CAP_SYS_NICE`).
pub(crate) fn fifo() -> Result<(), String> {
    let value = ThreadPriorityValue::try_from(RT_PRIORITY).map_err(|e| e.to_string())?;
    thread_priority::set_thread_priority_and_policy(
        thread_priority::thread_native_id(),
        ThreadPriority::Crossplatform(value),
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    )
    .map_err(|e| match e {
        thread_priority::Error::OS(code) => std::io::Error::from_raw_os_error(code).to_string(),
        other => other.to_string(),
    })
}

/// Puts the calling thread back in normal scheduling at nice 0.
pub(crate) fn normal() -> Result<(), thread_priority::Error> {
    thread_priority::set_thread_priority_and_policy(
        thread_priority::thread_native_id(),
        ThreadPriority::Os(ThreadPriorityOsValue::default()),
        ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Other),
    )
}

/// Asks rtkit to put the calling thread in real-time scheduling (`SCHED_RR`, which rtkit
/// always grants, reset in threads it starts). The D-Bus exchange runs on a helper
/// thread, rtkit being told this thread's id, so a system bus that never answers costs at most
/// [`RTKIT_WAIT`] and never holds this thread. Gives the CPU time the thread may then take
/// without waiting (`RLIMIT_RTTIME`).
pub(crate) fn rtkit() -> Result<Duration, String> {
    let thread = u64::try_from(rustix::thread::gettid().as_raw_nonzero().get())
        .map_err(|_| "the thread has no id rtkit takes".to_string())?;
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("suhina-rtkit".into())
        .spawn(move || {
            let _late = answer.send(ask_rtkit(thread));
        })
        .map_err(|e| format!("could not start a thread to ask it: {e}"))?;
    answered.recv_timeout(RTKIT_WAIT).unwrap_or_else(|_| Err("no answer in time".to_string()))
}

/// The exchange with rtkit: its limits, `RLIMIT_RTTIME` within them, then the thread.
fn ask_rtkit(thread: u64) -> Result<Duration, String> {
    let connection = zbus::blocking::connection::Builder::system()
        .and_then(|builder| builder.method_timeout(RTKIT_WAIT).build())
        .map_err(|e| format!("no system D-Bus: {e}"))?;
    // rtkit answers `Get` but not `GetAll`, so its properties are read one by one, uncached.
    let rtkit: zbus::blocking::Proxy<'_> = zbus::blocking::proxy::Builder::new(&connection)
        .destination(RTKIT_NAME)
        .and_then(|b| b.path(RTKIT_PATH))
        .and_then(|b| b.interface(RTKIT_NAME))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?;
    let ceiling: i32 = rtkit.get_property("MaxRealtimePriority").map_err(|e| e.to_string())?;
    let rttime: i64 = rtkit.get_property("RTTimeUSecMax").map_err(|e| e.to_string())?;
    let priority = rtkit_priority(ceiling).ok_or_else(|| format!("it allows no real-time priority ({ceiling})"))?;
    let limit = rttime_limit(rustix::process::getrlimit(Resource::Rttime), rttime)
        .ok_or_else(|| format!("it allows no real-time CPU time ({rttime} µs)"))?;
    rustix::process::setrlimit(Resource::Rttime, limit).map_err(|e| format!("could not limit RLIMIT_RTTIME: {e}"))?;
    rtkit.call_method("MakeThreadRealtime", &(thread, priority)).map_err(|e| e.to_string())?;
    // Over the soft limit the kernel ends the process; the hard one is never lower.
    Ok(Duration::from_micros(limit.current.unwrap_or(0)))
}

/// The priority to ask rtkit for: [`RT_PRIORITY`], or its ceiling when that is lower;
/// `None` when it allows none (real-time priorities start at 1).
pub(crate) fn rtkit_priority(ceiling: i32) -> Option<u32> {
    let priority = ceiling.min(i32::from(RT_PRIORITY));
    u32::try_from(priority).ok().filter(|p| *p >= 1)
}

/// `RLIMIT_RTTIME` within `max_us`, the most rtkit accepts: a limit already lower is kept.
/// It bounds the CPU time a real-time thread may take without blocking: over it the kernel
/// ends the process (rtkit's guard against a runaway thread taking the machine). An audio thread
/// waits between blocks, far within it (rtkit's default is 200 ms; a block is a few ms). `None`
/// when rtkit allows no time at all.
pub(crate) fn rttime_limit(now: Rlimit, max_us: i64) -> Option<Rlimit> {
    let max = u64::try_from(max_us).ok().filter(|m| *m > 0)?;
    let maximum = now.maximum.map_or(max, |hard| hard.min(max));
    let current = now.current.map_or(maximum, |soft| soft.min(maximum));
    Some(Rlimit { current: Some(current), maximum: Some(maximum) })
}
