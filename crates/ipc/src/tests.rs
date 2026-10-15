use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

const BLOCK: usize = 256;
const LONG: Duration = Duration::from_secs(5);

/// A host on a thread that halves every sample, and pauses for `pause` on block `slow`.
fn halving_host(address: String, blocks: usize, slow: Option<(usize, Duration)>) -> thread::JoinHandle<usize> {
    thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut count = 0;
        while count < blocks {
            let pause = slow.filter(|(at, _)| *at == count).map(|(_, d)| d);
            if served.serve(LONG, |input, output| {
                if let Some(pause) = pause {
                    thread::sleep(pause);
                }
                for (o, i) in output.iter_mut().zip(input) {
                    *o = i * 0.5;
                }
            }) {
                count += 1;
            }
        }
        count
    })
}

fn ramp(len: usize, offset: f32) -> Vec<f32> {
    (0..len).map(|n| n as f32 + offset).collect()
}

#[test]
fn blocks_go_there_and_back() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let host = halving_host(link.address(), 100, None);
    let mut output = vec![0.0; BLOCK * 2];
    for block in 0..100 {
        let input = ramp(BLOCK * 2, block as f32);
        assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done, "block {block}");
        assert!(output.iter().zip(&input).all(|(o, i)| *o == i * 0.5), "block {block}");
    }
    assert_eq!(host.join().ok(), Some(100));
}

#[test]
fn shorter_blocks_and_odd_lengths() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let host = halving_host(link.address(), 2, None);
    // 7 samples are 3 whole frames; the rest of the output stays as it was.
    let mut output = vec![9.0; 7];
    assert_eq!(link.process(&ramp(7, 1.0), &mut output, LONG), Outcome::Done);
    assert_eq!(output, [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 9.0]);
    // Longer than the link's block: only the first BLOCK frames go.
    let mut output = vec![9.0; BLOCK * 4];
    assert_eq!(link.process(&ramp(BLOCK * 4, 0.0), &mut output, LONG), Outcome::Done);
    assert_eq!(output[BLOCK * 2 - 1], (BLOCK * 2 - 1) as f32 * 0.5);
    assert_eq!(output[BLOCK * 2], 9.0);
    assert_eq!(host.join().ok(), Some(2));
}

#[test]
fn a_slow_host_misses_then_the_link_is_busy_until_it_answers() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let host = halving_host(link.address(), 3, Some((1, Duration::from_millis(200))));
    let input = ramp(BLOCK * 2, 1.0);
    let mut output = vec![0.0; BLOCK * 2];
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);

    let timeout = Duration::from_millis(10);
    let started = Instant::now();
    output.fill(7.0);
    assert_eq!(link.process(&input, &mut output, timeout), Outcome::Missed);
    assert!(started.elapsed() < timeout + Duration::from_millis(30), "{:?}", started.elapsed());
    assert!(output.iter().all(|&o| o == 7.0), "a missed block leaves the output alone");

    let started = Instant::now();
    assert_eq!(link.process(&input, &mut output, timeout), Outcome::Busy);
    assert!(started.elapsed() < Duration::from_millis(5), "busy returns at once: {:?}", started.elapsed());

    // The link stays busy until the slow host answers; a loaded machine may take longer than the
    // host's pause, so wait for that rather than for a fixed time.
    let waiting = Instant::now();
    let mut outcome = link.process(&input, &mut output, LONG);
    while outcome == Outcome::Busy && waiting.elapsed() < LONG {
        thread::sleep(Duration::from_millis(10));
        outcome = link.process(&input, &mut output, LONG);
    }
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(output[3], input[3] * 0.5);
    assert_eq!(host.join().ok(), Some(3));
}

#[test]
fn without_a_host_every_block_misses_or_is_busy_on_time() {
    let mut link = Link::create(1, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut output = [0.0; 64];
    let timeout = Duration::from_millis(5);
    let started = Instant::now();
    assert_eq!(link.process(&[1.0; 64], &mut output, timeout), Outcome::Missed);
    for _ in 0..100 {
        assert_eq!(link.process(&[1.0; 64], &mut output, timeout), Outcome::Busy);
    }
    assert!(started.elapsed() < Duration::from_millis(100), "{:?}", started.elapsed());
}

#[test]
fn a_host_waiting_for_blocks_gives_up_at_its_timeout() {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let mut served = Served::open(&link.address()).unwrap_or_else(|e| panic!("{e}"));
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    assert!(!served.serve(Duration::from_millis(5), move |_, _| flag.store(true, Ordering::Relaxed)));
    assert!(!called.load(Ordering::Relaxed));
    assert_eq!(served.channels(), 2);
}

#[test]
fn sizes_are_checked() {
    for (channels, frames) in [(0, 64), (2, 0), (MAX_CHANNELS + 1, 64), (2, MAX_FRAMES + 1)] {
        assert!(matches!(Link::create(channels, frames), Err(IpcError::BadSize { .. })), "{channels} {frames}");
    }
    let link = Link::create(MAX_CHANNELS, 16).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((link.channels(), link.max_frames()), (MAX_CHANNELS, 16));
}

#[test]
fn opening_something_else_fails_cleanly() {
    let dir = std::env::temp_dir();
    let stranger = dir.join(format!("not-a-link-{}", std::process::id()));
    let _ = std::fs::write(&stranger, [0u8; 1024]);
    assert!(matches!(Served::open(&stranger.display().to_string()), Err(IpcError::BadLink(_))));
    let _ = std::fs::remove_file(&stranger);

    let short = dir.join(format!("suhina-ipc-short-{}", std::process::id()));
    let _ = std::fs::write(&short, [0u8; 16]);
    assert!(matches!(Served::open(&short.display().to_string()), Err(IpcError::BadLink(_))));
    let _ = std::fs::remove_file(&short);

    let garbage = dir.join(format!("suhina-ipc-garbage-{}", std::process::id()));
    let _ = std::fs::write(&garbage, [0xAAu8; 4096]);
    assert!(matches!(Served::open(&garbage.display().to_string()), Err(IpcError::BadLink(_))));
    let _ = std::fs::remove_file(&garbage);

    assert!(Served::open("suhina-ipc-does-not-exist").is_err());
}

#[test]
fn the_region_is_removed_with_the_link() {
    let link = Link::create(2, 64).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    assert!(std::path::Path::new(&address).exists());
    drop(link);
    assert!(!std::path::Path::new(&address).exists());
}

#[test]
fn events_go_with_their_block() {
    let mut link = Link::create(2, BLOCK as u32).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    // A host that writes, for each note-on, its key at the event's frame (left channel).
    let host = thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut seen = Vec::new();
        while seen.len() < 3 {
            served.serve_with_events(LONG, |_, events, output| {
                for event in events {
                    if let EventKind::NoteOn { key, .. } = event.kind
                        && let Some(sample) = output.get_mut(event.frame as usize * 2)
                    {
                        *sample = f32::from(key);
                    }
                }
                seen.push(events.len());
            });
        }
        seen
    });
    let input = vec![0.0; BLOCK * 2];
    let mut output = vec![0.0; BLOCK * 2];
    let events = [Event::note_on(0, 0, 60, 1.0), Event::note_on(10, 0, 64, 0.5), Event::note_off(20, 0, 60, 0.0)];
    assert_eq!(link.process_with_events(&input, &events, &mut output, LONG), Outcome::Done);
    assert_eq!((output[0], output[20], output[40]), (60.0, 64.0, 0.0));
    // The next block has none; one past the block's end lands on its last frame.
    assert_eq!(link.process(&input, &mut output, LONG), Outcome::Done);
    assert!(output.iter().all(|s| *s == 0.0));
    let late = [Event::note_on(5_000, 0, 72, 1.0)];
    assert_eq!(link.process_with_events(&input[..20], &late, &mut output[..20], LONG), Outcome::Done);
    assert_eq!(output[18], 72.0);
    assert_eq!(host.join().ok(), Some(vec![3, 0, 1]));

    // Past MAX_EVENTS, the rest are dropped.
    let many = vec![Event::note_on(0, 0, 1, 1.0); MAX_EVENTS + 5];
    let mut link = Link::create(1, 16).unwrap_or_else(|e| panic!("{e}"));
    let address = link.address();
    let host = thread::spawn(move || {
        let mut served = Served::open(&address).unwrap_or_else(|e| panic!("{e}"));
        let mut count = 0;
        while !served.serve_with_events(LONG, |_, events, _| count = events.len()) {}
        count
    });
    assert_eq!(link.process_with_events(&[0.0; 16], &many, &mut [0.0; 16], LONG), Outcome::Done);
    assert_eq!(host.join().ok(), Some(MAX_EVENTS));
}
