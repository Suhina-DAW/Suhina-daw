use super::*;

#[test]
fn stereo_maps_onto_any_channel_count() {
    let stereo = [0.5f32, -0.5, 1.0, 0.0];

    let mut mono = [0.0f32; 2];
    write_frames(&mut mono, 1, &stereo);
    assert_eq!(mono, [0.0, 0.5]);

    let mut two = [0.0f32; 4];
    write_frames(&mut two, 2, &stereo);
    assert_eq!(two, stereo);

    let mut four = [9.0f32; 8];
    write_frames(&mut four, 4, &stereo);
    assert_eq!(four, [0.5, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);

    let mut ints = [0i16; 4];
    write_frames(&mut ints, 2, &stereo);
    assert_eq!(ints[2], i16::MAX);
}

#[test]
fn the_engine_block_follows_the_chosen_buffer() {
    assert_eq!(block_for(None, None), ENGINE_BLOCK);
    assert_eq!(block_for(Some(256), None), 256);
    assert_eq!(block_for(Some(32), None), 64, "bounded below");
    assert_eq!(block_for(Some(10_000), None), 4096, "bounded above");
    // The test override wins when it makes sense, and is ignored when it does not.
    assert_eq!(block_for(Some(256), Some("512")), 512);
    assert_eq!(block_for(Some(256), Some("16")), 256);
    assert_eq!(block_for(None, Some("lots")), ENGINE_BLOCK);
}

#[test]
fn the_ring_keeps_two_blocks_or_a_device_buffer_and_a_block() {
    assert_eq!(ring_target(512, 0, false), 1024);
    assert_eq!(ring_target(512, 128, false), 1024);
    assert_eq!(ring_target(256, 1024, false), 1280);
    // A device asking for absurd amounts is capped.
    assert_eq!(ring_target(64, usize::MAX, false), MAX_DEVICE_FRAMES + 64);
    // The ring has room for its target and a block more, for every block size, live or not.
    for block in [64, 256, 1024, 4096] {
        let (producer, _consumer) = ring(block);
        for live in [false, true] {
            assert!(producer.buffer().capacity() >= (ring_target(block, MAX_DEVICE_FRAMES, live) + block) * 2);
        }
    }
}

#[test]
fn while_a_track_is_armed_the_ring_keeps_one_block_or_a_device_buffer() {
    assert_eq!(ring_target(128, 0, true), 128, "before the first callback");
    assert_eq!(ring_target(128, 128, true), 128);
    assert_eq!(ring_target(128, 64, true), 128);
    assert_eq!(ring_target(128, 512, true), 512, "a device taking more at once gets it");
    assert_eq!(ring_target(64, usize::MAX, true), MAX_DEVICE_FRAMES);
    // Never more than without anyone playing.
    for block in [64, 128, 256, 1024, 4096] {
        for device in [0, 64, 128, 441, 1024, 8192] {
            assert!(ring_target(block, device, true) < ring_target(block, device, false), "{block} {device}");
        }
    }
}

#[test]
fn a_keys_latency_is_the_measured_latency_and_one_block() {
    let progress = Progress::default();
    progress.set_latency(Duration::from_millis(5));
    progress.block.store(480, Ordering::Relaxed);
    assert_eq!(progress.live_latency(48_000), Duration::from_millis(15));
    assert_eq!(progress.live_latency(0), Duration::from_millis(5) + Duration::from_secs(480), "a rate of 0");
    assert!(!progress.live());
}

#[test]
fn reading_the_ring_counts_what_was_missing() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(16);
    let progress = Progress::default();
    for sample in [0.1, 0.2, 0.3, 0.4] {
        producer.push(sample).unwrap();
    }
    let mut out = [9.0f32; 4];
    read_ring(&mut consumer, &mut out, &progress);
    assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(progress.underruns(), 0);

    producer.push(0.5).unwrap();
    producer.push(0.6).unwrap();
    producer.push(0.7).unwrap();
    let mut out = [9.0f32; 6];
    read_ring(&mut consumer, &mut out, &progress);
    // Whole frames only: the odd sample waits for its pair.
    assert_eq!(out, [0.5, 0.6, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(progress.underruns(), 1);
    assert_eq!(consumer.slots(), 1);
}

#[test]
fn latency_is_the_ring_and_the_device() {
    let latency = measure_latency(441, Duration::from_millis(5), 44_100);
    assert_eq!(latency, Duration::from_millis(15));
    assert_eq!(measure_latency(100, Duration::ZERO, 0), Duration::from_secs(100), "a rate of 0 does not divide by 0");
    let progress = Progress::default();
    progress.set_latency(Duration::from_micros(23_456));
    assert_eq!(progress.latency(), Duration::from_micros(23_456));
}

/// A choice that names nothing on this system falls back to the default with notices, or
/// fails cleanly where there is no audio device at all (CI).
#[test]
fn a_choice_of_missing_things_falls_back_to_the_default() {
    let choice = OutputChoice {
        host: Some("No Such Host".into()),
        device: Some("No Such Device".into()),
        sample_rate: Some(12_345),
        buffer_frames: Some(300),
        exclusive: false,
        input: suhina_session::InputChoice::Off,
    };
    match Output::choose(&choice) {
        Ok(output) => {
            assert!(output.notices().iter().any(|n| n.contains("No Such Host")), "{:?}", output.notices());
            assert!(output.notices().iter().any(|n| n.contains("No Such Device")));
            assert!(output.sample_rate() > 0 && output.sample_rate() != 12_345);
            assert!((64..=4096).contains(&output.block()));
        }
        Err(e) => assert!(!e.is_empty()),
    }
}

#[test]
fn the_priority_notice_is_told_once() {
    let told = AtomicBool::new(false);
    assert_eq!(tell_once(&told, None), None, "nothing to tell leaves it untold");
    assert_eq!(tell_once(&told, Some("first".into())), Some("first".into()));
    assert_eq!(tell_once(&told, Some("again".into())), None);
}

/// The render thread says what priority it got before it renders anything, so the stream can
/// tell the user before the device starts; it plays on whatever it got.
#[test]
fn the_render_thread_says_its_priority_first() {
    let said = std::thread::spawn(|| {
        let renderer = Renderer::stopped(&suhina_model::Project::default(), &AudioPool::new(), 48_000).unwrap();
        let (producer, _consumer) = ring(256);
        let (stop, progress) = (AtomicBool::new(true), Progress::default());
        let (joined, said) = mpsc::sync_channel(1);
        let _renderer = render_ahead(renderer, producer, 256, &stop, &progress, &joined);
        said.try_recv().unwrap()
    })
    .join()
    .unwrap();
    let (priority, notice) = said;
    if priority.is_real_time() || priority == Priority::Off {
        assert_eq!(notice, None);
    }
    if let Some(notice) = notice {
        assert!(notice.contains(priority.describe()), "{notice}");
    }
}

#[test]
fn an_output_plays_in_place_of_a_chosen_device_that_is_not_there() {
    let choice = |host: Option<&str>, device: Option<&str>| OutputChoice {
        host: host.map(String::from),
        device: device.map(String::from),
        ..OutputChoice::default()
    };
    assert!(!in_place_of(&OutputChoice::default(), "ALSA", "default"), "nothing chosen");
    assert!(!in_place_of(&choice(Some("alsa "), Some("USB Audio")), "ALSA", "usb audio"), "names as compared");
    assert!(in_place_of(&choice(None, Some("USB Audio")), "ALSA", "default"), "another device");
    assert!(in_place_of(&choice(Some("ASIO"), None), "WASAPI", "Speakers"), "another host");
}

#[test]
fn the_chosen_device_is_looked_for_in_the_background() {
    let system = DeviceOutput::new();
    let gone = OutputChoice { host: Some("No such host".into()), ..OutputChoice::default() };
    // The first ask starts a look and does not wait for it.
    assert_eq!(system.chosen_present(&gone), None);
    let mut answer = None;
    for _ in 0..500 {
        std::thread::sleep(Duration::from_millis(10));
        answer = system.chosen_present(&gone);
        if answer.is_some() {
            break;
        }
    }
    assert_eq!(answer, Some(false));
    // An answer is for the choice it was asked for: another choice waits for its own look.
    let other = OutputChoice { device: Some("Elsewhere".into()), ..gone.clone() };
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(system.chosen_present(&other), None);
}

/// Waits up to two seconds for `done`, looking every millisecond.
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    done()
}

#[test]
fn the_render_thread_keeps_less_ahead_while_a_track_is_armed() {
    use suhina_engine::{LaunchCommand, model};
    const BLOCK: usize = 128;
    let mut renderer = Renderer::stopped(&model::Project::default(), &AudioPool::new(), 48_000).unwrap();
    let mut controller = renderer.controller();
    let (producer, mut consumer) = ring(BLOCK);
    let progress = Arc::new(Progress::default());
    let stop = Arc::new(AtomicBool::new(false));
    let (stopping, watching) = (Arc::clone(&stop), Arc::clone(&progress));
    let (joined, _said) = mpsc::sync_channel(1);
    let render = std::thread::spawn(move || render_ahead(renderer, producer, BLOCK, &stopping, &watching, &joined));
    let queued = |c: &Ring| c.slots() / 2;

    // Nobody plays: two blocks ahead.
    assert!(eventually(|| queued(&consumer) >= 2 * BLOCK));
    assert!(!progress.live());

    // Armed: the device drains the queue down to one block, which the thread keeps.
    controller
        .send(LaunchCommand::SetArmed { track: model::TrackId(1), armed: true, input: Default::default() })
        .unwrap();
    let mut device = vec![0.0f32; BLOCK * 2];
    assert!(eventually(|| {
        read_ring(&mut consumer, &mut device, &progress);
        progress.live() && queued(&consumer) < 2 * BLOCK
    }));
    std::thread::sleep(Duration::from_millis(20));
    assert!(queued(&consumer) >= BLOCK && queued(&consumer) < 2 * BLOCK, "{}", queued(&consumer));

    // Disarmed: back to two blocks.
    controller
        .send(LaunchCommand::SetArmed { track: model::TrackId(1), armed: false, input: Default::default() })
        .unwrap();
    assert!(eventually(|| {
        if !progress.live() && queued(&consumer) >= 2 * BLOCK {
            return true;
        }
        read_ring(&mut consumer, &mut device, &progress);
        false
    }));
    stop.store(true, Ordering::Release);
    progress.wake_render();
    assert!(render.join().is_ok());
}
