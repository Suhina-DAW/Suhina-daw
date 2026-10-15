use super::*;

/// Two ring frames: main (0.5, -0.5) with cue (0.25, -0.25), then main (1, 0) with cue (0.1, 0.2).
const RINGED: [f32; 8] = [0.5, -0.5, 0.25, -0.25, 1.0, 0.0, 0.1, 0.2];

#[test]
fn the_main_output_maps_onto_any_channel_count() {
    let mut mono = [0.0f32; 2];
    write_frames(&mut mono, 1, &RINGED, None);
    assert_eq!(mono, [0.0, 0.5]);

    let mut two = [0.0f32; 4];
    write_frames(&mut two, 2, &RINGED, None);
    assert_eq!(two, [0.5, -0.5, 1.0, 0.0]);

    // Without a cue pair, the cue is not written: the engine has put the preview in the main.
    let mut four = [9.0f32; 8];
    write_frames(&mut four, 4, &RINGED, None);
    assert_eq!(four, [0.5, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);

    let mut ints = [0i16; 4];
    write_frames(&mut ints, 2, &RINGED, None);
    assert_eq!(ints[2], i16::MAX);

    let mut none: [f32; 0] = [];
    write_frames(&mut none, 0, &RINGED, None);
}

/// A fake eight-channel interface with the cue on pair 2 (channels 5 and 6): the main output
/// stays on 1/2, the cue goes to 5/6, everything else is silent.
#[test]
fn the_cue_goes_to_its_pair_on_a_multi_channel_device() {
    let cue = cue_channel(Some(2), 8);
    assert_eq!(cue, Some(4));
    let mut eight = [9.0f32; 16];
    write_frames(&mut eight, 8, &RINGED, cue);
    assert_eq!(eight[..8], [0.5, -0.5, 0.0, 0.0, 0.25, -0.25, 0.0, 0.0]);
    assert_eq!(eight[8..], [1.0, 0.0, 0.0, 0.0, 0.1, 0.2, 0.0, 0.0]);
    // The last pair of the device.
    let mut last = [9.0f32; 8];
    write_frames(&mut last, 4, &RINGED[..4], cue_channel(Some(1), 4));
    assert_eq!(last[..4], [0.5, -0.5, 0.25, -0.25]);
    assert_eq!(last[4..], [9.0; 4], "frames past the ring's are left alone");
}

#[test]
fn a_cue_pair_the_device_does_not_have_plays_with_the_main_output() {
    assert_eq!(cue_channel(None, 8), None);
    assert_eq!(cue_channel(Some(0), 8), None, "pair 0 is the main output");
    assert_eq!(cue_channel(Some(1), 2), None);
    assert_eq!(cue_channel(Some(1), 3), None, "half a pair is not a pair");
    assert_eq!(cue_channel(Some(3), 8), Some(6));
    assert_eq!(cue_channel(Some(4), 8), None);
    assert_eq!(cue_channel(Some(u16::MAX), u16::MAX), None);
    // Opening more channels than the default gives, when the device offers them.
    assert_eq!(channels_for_cue(1, 2, &[2, 8, 4]), Some(4));
    assert_eq!(channels_for_cue(1, 6, &[2]), Some(6), "the default is enough");
    assert_eq!(channels_for_cue(3, 2, &[2, 4, 6]), None);
    assert_eq!(channels_for_cue(u16::MAX, 2, &[u16::MAX]), None);
    assert!(cue_missing("Interface", 3, 4).contains("not 7/8"));
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
        assert!(producer.buffer().capacity() >= (ring_target(block, MAX_DEVICE_FRAMES) + block) * RING_CHANNELS);
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

    for sample in [0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
        producer.push(sample).unwrap();
    }
    let mut out = [9.0f32; 12];
    read_ring(&mut consumer, &mut out, &progress);
    // Whole frames only: the half frame waits for the rest of it.
    assert_eq!(out, [0.5, 0.6, 0.7, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(progress.underruns(), 1);
    assert_eq!(consumer.slots(), 2);
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
        cue_pair: Some(31),
        cue_gain: 1.0,
    };
    match Output::choose(&choice) {
        Ok(output) => {
            assert!(output.notices().iter().any(|n| n.contains("No Such Host")), "{:?}", output.notices());
            assert!(output.notices().iter().any(|n| n.contains("No Such Device")));
            assert!(output.sample_rate() > 0 && output.sample_rate() != 12_345);
            assert!((64..=4096).contains(&output.block()));
            // No default device has 64 channels: the cue plays with the main output, and says so.
            assert_eq!(output.cue_pair(), None);
            assert!(output.notices().iter().any(|n| n.contains("63/64")), "{:?}", output.notices());
        }
        Err(e) => assert!(!e.is_empty()),
    }
}
