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
    assert_eq!(ring_target(512, 0), 1024);
    assert_eq!(ring_target(512, 128), 1024);
    assert_eq!(ring_target(256, 1024), 1280);
    // A device asking for absurd amounts is capped.
    assert_eq!(ring_target(64, usize::MAX), MAX_DEVICE_FRAMES + 64);
    // The ring has room for its target and a block more, for every block size.
    for block in [64, 256, 1024, 4096] {
        let (producer, _consumer) = ring(block);
        assert!(producer.buffer().capacity() >= (ring_target(block, MAX_DEVICE_FRAMES) + block) * 2);
    }
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
