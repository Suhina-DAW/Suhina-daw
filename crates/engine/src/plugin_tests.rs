//! Plug-in slots in the renderer (ADR 0006) with stand-in processors: effects and instruments,
//! a plug-in that misses its deadline or is gone, latency compensation, processors arriving
//! while playing and carried through edits.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use loopwerk_device_api::NoteEvent;
use loopwerk_model::*;

use crate::chain_tests::{BEAT, SR, blocks, device, left, max_diff, note, peak, project};
use crate::{
    AudioPool, PluginProcessor, PluginSlotInfo, Plugins, Renderer, plugin_devices, render_offline,
    render_offline_with_plugins,
};

/// What a stand-in plug-in does with a block.
#[derive(Clone, Copy)]
enum Behaviour {
    /// Scales the sound.
    Gain(f32),
    /// Plays a constant 0.5 on both sides from each note on until its note off.
    Instrument,
    /// Delays the sound by this many frames, as a plug-in with latency does.
    Delay(usize),
    /// Never answers.
    Gone,
}

struct Stand {
    behaviour: Behaviour,
    sounding: bool,
    history: Vec<f32>,
    /// Every event it got, with its frame in the renderer's output.
    seen: Arc<std::sync::Mutex<Vec<(usize, NoteEvent)>>>,
    /// Frames it has processed.
    frames: usize,
    /// Set false to make it stop answering.
    answering: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
}

impl Stand {
    fn new(behaviour: Behaviour) -> Self {
        Self {
            behaviour,
            sounding: false,
            history: Vec::new(),
            seen: Arc::default(),
            frames: 0,
            answering: Arc::new(AtomicBool::new(true)),
            dropped: Arc::default(),
        }
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl PluginProcessor for Stand {
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], _timeout: Duration) -> bool {
        if !self.answering.load(Ordering::SeqCst) {
            return false;
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.extend(events.iter().map(|&(at, e)| (self.frames + at, e)));
        }
        let frames = buffer.len() / 2;
        match self.behaviour {
            Behaviour::Gain(gain) => buffer.iter_mut().for_each(|s| *s *= gain),
            Behaviour::Instrument => {
                let mut next = events.iter().peekable();
                for (frame, pair) in buffer.chunks_mut(2).enumerate() {
                    while let Some((_, event)) = next.next_if(|(at, _)| *at <= frame) {
                        match event {
                            NoteEvent::On { .. } => self.sounding = true,
                            NoteEvent::Off { .. } | NoteEvent::AllNotesOff => self.sounding = false,
                            _ => {}
                        }
                    }
                    let value = if self.sounding { 0.5 } else { 0.0 };
                    pair.iter_mut().for_each(|s| *s = value);
                }
            }
            Behaviour::Delay(by) => {
                // `history` holds the last `by` frames.
                if self.history.len() != by * 2 {
                    self.history = vec![0.0; by * 2];
                }
                for pair in buffer.chunks_mut(2) {
                    self.history.extend_from_slice(pair);
                    let oldest: Vec<f32> = self.history.drain(..2).collect();
                    pair.copy_from_slice(&oldest);
                }
            }
            Behaviour::Gone => return false,
        }
        self.frames += frames;
        true
    }
}

fn plug(kind: DeviceKind) -> Device {
    let mut d = device("plug-in", &[]);
    d.kind = kind;
    d
}

fn vst3() -> DeviceKind {
    DeviceKind::Vst3 { id: "565354416263646566".into() }
}

/// The plug-ins of `project` (in order) running in `stands`.
fn plugins(project: &Project, stands: Vec<(Stand, PluginSlotInfo)>) -> Plugins {
    let mut plugins = Plugins::new();
    for (found, (stand, info)) in plugin_devices(project).into_iter().zip(stands) {
        plugins.insert(found.key, info, Box::new(stand));
    }
    plugins
}

const EFFECT: PluginSlotInfo = PluginSlotInfo { instrument: false, latency: 0 };
const INSTRUMENT: PluginSlotInfo = PluginSlotInfo { instrument: true, latency: 0 };

#[test]
fn a_plug_in_effect_processes_the_tracks_sound() {
    let notes = vec![note(0.0, 2.0, 69)];
    let p = project(notes.clone(), 2.0, vec![plug(vst3())]);
    let dry = render_offline(&project(notes, 2.0, vec![device("utility", &[])]), SR, 512).unwrap();
    let wet = render_offline_with_plugins(
        &p,
        &AudioPool::new(),
        SR,
        512,
        plugins(&p, vec![(Stand::new(Behaviour::Gain(0.5)), EFFECT)]),
    )
    .unwrap();
    assert_eq!(wet.len(), dry.len());
    let half: Vec<f32> = dry.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&wet, &half) < 1e-6, "{}", max_diff(&wet, &half));

    // A plug-in with built-in devices around it: one after the other.
    let p = project(vec![note(0.0, 2.0, 69)], 2.0, vec![device("utility", &[("gain", -6.0206)]), plug(vst3())]);
    let both = render_offline_with_plugins(
        &p,
        &AudioPool::new(),
        SR,
        333,
        plugins(&p, vec![(Stand::new(Behaviour::Gain(0.5)), EFFECT)]),
    )
    .unwrap();
    let quarter: Vec<f32> = dry.iter().map(|s| s * 0.25).collect();
    assert!(max_diff(&both, &quarter) < 1e-4, "{}", max_diff(&both, &quarter));
}

#[test]
fn a_plug_in_instrument_gets_the_notes_at_their_frames() {
    let p = project(vec![note(0.0, 1.0, 60), note(1.5, 0.25, 64)], 2.0, vec![plug(vst3())]);
    let seen: Arc<std::sync::Mutex<Vec<(usize, NoteEvent)>>> = Arc::default();
    for block in [64, 512, 1_000] {
        let mut stand = Stand::new(Behaviour::Instrument);
        stand.seen = Arc::clone(&seen);
        let out = render_offline_with_plugins(&p, &AudioPool::new(), SR, block, plugins(&p, vec![(stand, INSTRUMENT)]))
            .unwrap();
        let out = left(&out);
        let second = BEAT * 3 / 2;
        // No sine voices: the plug-in plays the notes, at the track's unity gain.
        assert!((out[0] - 0.5).abs() < 1e-6, "block {block}: {}", out[0]);
        assert_eq!(out[BEAT - 1], out[0]);
        assert_eq!(out[BEAT], 0.0, "released on its frame");
        assert_eq!(out[second - 1], 0.0);
        assert!(out[second] > 0.0, "the second note starts on its frame");
        let events = std::mem::take(&mut *seen.lock().unwrap());
        let ons: Vec<usize> =
            events.iter().filter(|(_, e)| matches!(e, NoteEvent::On { .. })).map(|(at, _)| *at).collect();
        assert_eq!(ons, [0, second], "block {block}");
    }
}

#[test]
fn a_plug_in_that_misses_passes_the_sound_or_stays_silent_and_the_engine_plays_on() {
    let notes = vec![note(0.0, 4.0, 69)];
    let dry_project = project(notes.clone(), 4.0, vec![device("utility", &[])]);
    let mut dry = Renderer::with_audio(&dry_project, &AudioPool::new(), SR).unwrap();
    let dry = blocks(&mut dry, 40, 480);

    let p = project(notes.clone(), 4.0, vec![plug(vst3())]);
    let effect = Stand::new(Behaviour::Gain(0.5));
    let answering = Arc::clone(&effect.answering);
    let mut renderer = Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins(&p, vec![(effect, EFFECT)])).unwrap();
    let mut out = blocks(&mut renderer, 20, 480);
    answering.store(false, Ordering::SeqCst);
    out.extend(blocks(&mut renderer, 20, 480));
    let at = 20 * 480 * 2;
    let half: Vec<f32> = dry[..at].iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&out[..at], &half) < 1e-6);
    // A crashed or hung effect: the dry sound, every block, without waiting.
    assert!(max_diff(&out[at..], &dry[at..]) < 1e-6);
    assert_eq!(renderer.graph.chains[0].missed(), 20);

    // An instrument that is gone is silent; a track without one still plays.
    let p = project(notes, 4.0, vec![plug(vst3())]);
    let mut renderer =
        Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins(&p, vec![(Stand::new(Behaviour::Gone), INSTRUMENT)]))
            .unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 20, 480)), 0.0);
}

#[test]
fn a_disabled_or_missing_plug_in_passes_the_sound_through() {
    let notes = vec![note(0.0, 1.0, 60)];
    let plain = render_offline(&project(notes.clone(), 1.0, vec![device("utility", &[])]), SR, 512).unwrap();
    let mut off = plug(vst3());
    off.enabled = false;
    let p = project(notes.clone(), 1.0, vec![off]);
    let out = render_offline_with_plugins(
        &p,
        &AudioPool::new(),
        SR,
        512,
        plugins(&p, vec![(Stand::new(Behaviour::Gain(0.0)), EFFECT)]),
    )
    .unwrap();
    assert!(max_diff(&out, &plain) < 1e-6);
    // Missing: no slot at all, the track plays as if it were not there.
    let p = project(notes, 1.0, vec![plug(vst3())]);
    let bare = render_offline(&p, SR, 512).unwrap();
    assert!(peak(&bare) > 0.1);
}

/// `notes` on two tracks: the first with `devices`, the second with a utility.
fn two_tracks(notes: Vec<Note>, devices: Vec<Device>) -> Project {
    let mut p = project(notes, 2.0, devices);
    let mut track = p.tracks[0].clone();
    track.id = TrackId(2);
    track.channel = Some(ChannelId(2));
    let mut channel = p.channels[0].clone();
    channel.id = ChannelId(2);
    channel.devices = vec![device("utility", &[])];
    let mut lane = p.arrangement.lanes[0].clone();
    lane.track = Some(TrackId(2));
    p.tracks.push(track);
    p.channels.push(channel);
    p.arrangement.lanes.push(lane);
    p.launcher.columns.clear();
    p
}

#[test]
fn latency_compensation_lines_every_track_up_with_the_slowest() {
    const LATENCY: usize = 300;
    let p = two_tracks(vec![note(0.0, 1.0, 69)], vec![plug(vst3())]);
    let out = render_offline_with_plugins(
        &p,
        &AudioPool::new(),
        SR,
        256,
        plugins(
            &p,
            vec![(Stand::new(Behaviour::Delay(LATENCY)), PluginSlotInfo { latency: LATENCY as u32, ..EFFECT })],
        ),
    )
    .unwrap();
    // Both tracks play the same note: lined up, the sum is twice one track, delayed by the
    // latency.
    let one = render_offline(&project(vec![note(0.0, 1.0, 69)], 2.0, vec![device("utility", &[])]), SR, 256).unwrap();
    let out = left(&out);
    let one = left(&one);
    assert!(peak(&out[..LATENCY]) == 0.0, "everything waits for the slowest track");
    let doubled: Vec<f32> = one[..BEAT].iter().map(|s| s * 2.0).collect();
    assert!(max_diff(&out[LATENCY..LATENCY + BEAT], &doubled) < 1e-5);
    let renderer = Renderer::with_plugins(
        &p,
        &AudioPool::new(),
        SR,
        plugins(&p, vec![(Stand::new(Behaviour::Gone), PluginSlotInfo { latency: LATENCY as u32, ..EFFECT })]),
    )
    .unwrap();
    assert_eq!(renderer.latency(), LATENCY as u32);
}

#[test]
fn a_plug_in_arrives_while_playing_and_stays_through_edits_and_restarts() {
    let notes = vec![note(0.0, 8.0, 69)];
    let p = project(notes.clone(), 8.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let mut renderer = Renderer::with_audio(&p, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    let mut info = std::collections::HashMap::new();
    info.insert(key, EFFECT);
    controller.set_plugin_info(info);
    // Before it has loaded, the plug-in is not there: the sound passes.
    let before = blocks(&mut renderer, 5, 480);
    assert!(peak(&before) > 0.1);
    // The plug-in loads: it waits for a graph with its slot, then plays.
    let first = Stand::new(Behaviour::Gain(0.0));
    let first_dropped = Arc::clone(&first.dropped);
    controller.send_plugin(key, Box::new(first)).unwrap();
    blocks(&mut renderer, 2, 480);
    controller.update(&p).unwrap();
    blocks(&mut renderer, 2, 480);
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0, "the plug-in silences the track");

    // An edit around it (a device added in front) keeps it running.
    let mut edited = p.clone();
    edited.channels[0].devices.insert(0, device("utility", &[]));
    controller.update(&edited).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0);
    controller.collect();
    assert_eq!(first_dropped.load(Ordering::SeqCst), 0, "the plug-in carried over");

    // A restarted plug-in replaces it; the old one goes back to be dropped off the audio
    // thread.
    controller.send_plugin(key, Box::new(Stand::new(Behaviour::Gain(1.0)))).unwrap();
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1);
    controller.collect();
    assert_eq!(first_dropped.load(Ordering::SeqCst), 1);

    // Removing the device ends it with the graph it was in.
    let mut removed = edited.clone();
    removed.channels[0].devices.remove(1);
    let second_dropped = {
        let stand = Stand::new(Behaviour::Gain(1.0));
        let dropped = Arc::clone(&stand.dropped);
        controller.send_plugin(key, Box::new(stand)).unwrap();
        dropped
    };
    blocks(&mut renderer, 2, 480);
    controller.update(&removed).unwrap();
    blocks(&mut renderer, 4, 480);
    controller.update(&removed).unwrap();
    blocks(&mut renderer, 4, 480);
    controller.collect();
    assert_eq!(second_dropped.load(Ordering::SeqCst), 1);
}

/// A muted track (or one left out by a solo) keeps its chain, so its plug-in keeps its
/// process and sounds again when the track does; a chain left out of the graph would give the
/// processor back and the plug-in would stay silent for good.
#[test]
fn a_plug_in_on_a_muted_track_keeps_running_and_sounds_again() {
    let p = project(vec![note(0.0, 16.0, 60)], 16.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let stand = Stand::new(Behaviour::Instrument);
    let dropped = Arc::clone(&stand.dropped);
    let mut renderer =
        Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins(&p, vec![(stand, INSTRUMENT)])).unwrap();
    let mut controller = renderer.controller();
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1);
    let mut muted = p.clone();
    muted.channels[0].mute = true;
    controller.update(&muted).unwrap();
    blocks(&mut renderer, 2, 480);
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0, "muted");
    controller.update(&p).unwrap();
    blocks(&mut renderer, 2, 480);
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1, "sounds again");
    controller.collect();
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "the plug-in carried over");
    let _ = key;
}

/// A stopped preview's note-off reaches the track's instrument on the first frame of the block
/// the stop is applied in, whatever the block size (#156); it came one block late.
#[test]
fn a_stopped_preview_releases_its_note_on_the_frame_the_stop_applies() {
    for size in [512usize, 300] {
        let p = project(Vec::new(), 8.0, vec![plug(vst3())]);
        let stand = Stand::new(Behaviour::Instrument);
        let seen = Arc::clone(&stand.seen);
        let mut renderer =
            Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins(&p, vec![(stand, INSTRUMENT)])).unwrap();
        let mut controller = renderer.controller();
        let notes = crate::preview_notes([(0.0, 64.0, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
        controller.preview_notes(notes, 1.0).unwrap();
        blocks(&mut renderer, 2, size);
        controller.preview_stop().unwrap();
        blocks(&mut renderer, 2, size);
        let seen = seen.lock().unwrap();
        let off = seen.iter().find(|(_, e)| matches!(e, NoteEvent::Off { key: 60, .. })).map(|(at, _)| *at);
        assert_eq!(off, Some(2 * size), "block {size}: {seen:?}");
    }
}

#[test]
fn a_plug_in_instrument_is_released_when_the_transport_stops() {
    let p = project(vec![note(0.0, 8.0, 60)], 8.0, vec![plug(vst3())]);
    let stand = Stand::new(Behaviour::Instrument);
    let seen = Arc::clone(&stand.seen);
    let mut renderer =
        Renderer::with_plugins(&p, &AudioPool::new(), SR, plugins(&p, vec![(stand, INSTRUMENT)])).unwrap();
    assert!(peak(&blocks(&mut renderer, 10, 480)) > 0.1);
    renderer.handle(crate::LaunchCommand::Stop);
    assert_eq!(peak(&blocks(&mut renderer, 10, 480)[960..]), 0.0);
    // Each held key is released on its own before the all-notes-off, for instruments that
    // ignore it.
    let seen = seen.lock().unwrap();
    let off = seen.iter().position(|(_, e)| matches!(e, NoteEvent::Off { key: 60, .. }));
    let all = seen.iter().position(|(_, e)| *e == NoteEvent::AllNotesOff);
    assert!(off.is_some() && all.is_some() && off < all, "{seen:?}");
}
