//! The host's share of a plug-in's own event loop. On Linux a plug-in's editor has no message
//! loop of its own: it hands the host the file descriptors it waits on (its X11 connection) and
//! the timers it needs, and the host calls it back from its main loop (VST3 `IRunLoop`, CLAP's
//! `posix-fd-support` and `timer-support`). Timers are also how CLAP plug-ins get periodic main
//! thread calls on every system.

use std::time::{Duration, Instant};

/// Timers registered by a plug-in, each with its period and when it is next due.
pub(crate) struct Timers<K> {
    list: Vec<(K, Duration, Instant)>,
}

/// Shortest period a timer gets: plug-ins asking for less get this.
const MIN_PERIOD: Duration = Duration::from_millis(10);
/// Most timers one plug-in may register.
const MAX_TIMERS: usize = 256;

impl<K: Clone> Timers<K> {
    pub(crate) fn new() -> Self {
        Self { list: Vec::new() }
    }

    /// Adds a timer `key` firing every `period`; whether there was room.
    pub(crate) fn add(&mut self, key: K, period: Duration) -> bool {
        if self.list.len() >= MAX_TIMERS {
            return false;
        }
        let period = period.max(MIN_PERIOD);
        self.list.push((key, period, Instant::now() + period));
        true
    }

    /// Removes the timers `key` matches.
    pub(crate) fn remove(&mut self, matches: impl Fn(&K) -> bool) {
        self.list.retain(|(key, _, _)| !matches(key));
    }

    /// The timers due at `now`, each moved on to its next time.
    pub(crate) fn due(&mut self, now: Instant) -> Vec<K> {
        let mut due = Vec::new();
        for (key, period, next) in &mut self.list {
            if *next <= now {
                due.push(key.clone());
                // A late timer fires once and starts over, rather than catching up.
                *next = now + *period;
            }
        }
        due
    }
}

/// The descriptors among `fds` with something to read (or an error), without waiting.
#[cfg(target_os = "linux")]
pub(crate) fn ready(fds: &[i32]) -> Vec<i32> {
    let mut polled: Vec<libc::pollfd> =
        fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    if polled.is_empty() {
        return Vec::new();
    }
    // SAFETY: a valid array of `polled.len()` entries the call fills in; a zero timeout returns
    // at once.
    let count = unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, 0) };
    if count <= 0 {
        return Vec::new();
    }
    polled.iter().filter(|p| p.revents != 0).map(|p| p.fd).collect()
}

/// Elsewhere plug-ins have their own message loops and register no descriptors.
#[cfg(not(target_os = "linux"))]
pub(crate) fn ready(_fds: &[i32]) -> Vec<i32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_fire_when_due_and_start_over() {
        let mut timers = Timers::new();
        assert!(timers.add("fast", Duration::from_millis(1)));
        assert!(timers.add("slow", Duration::from_secs(60)));
        let start = Instant::now();
        assert!(timers.due(start).is_empty());
        // The fast one fires (its period raised to the minimum); the slow one waits.
        assert_eq!(timers.due(start + Duration::from_millis(20)), ["fast"]);
        assert!(timers.due(start + Duration::from_millis(21)).is_empty());
        timers.remove(|k| *k == "fast");
        assert!(timers.due(start + Duration::from_secs(1)).is_empty());
        assert_eq!(timers.due(start + Duration::from_secs(61)), ["slow"]);
        timers.remove(|_| true);
        assert!(timers.due(start + Duration::from_secs(3600)).is_empty());
        for n in 0..MAX_TIMERS {
            assert!(timers.add("many", Duration::from_secs(n as u64 + 1)));
        }
        assert!(!timers.add("one too many", Duration::from_secs(1)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_descriptor_with_data_is_ready() {
        let mut ends = [0; 2];
        // SAFETY: an array of two descriptors for the call to fill.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        let [read, write] = ends;
        assert!(ready(&[read]).is_empty());
        // SAFETY: one byte from a live buffer to the pipe's write end.
        assert_eq!(unsafe { libc::write(write, [7u8].as_ptr().cast(), 1) }, 1);
        assert_eq!(ready(&[read]), [read]);
        // SAFETY: both ends are ours and closed once.
        unsafe { libc::close(read) };
        // SAFETY: as above.
        unsafe { libc::close(write) };
    }
}
