//! One track's arrangement clips rendered on their own, as they sound before the track's mixer
//! strip, for `clip.consolidate` (`docs/spec/arrangement-editing.md` › Consolidating). The same
//! renderer as playback and offline renders.

use suhina_model::{Arrangement, Channel, ChannelId, ChannelRole, ClipContent, Extensions, Project, Track, TrackId};

use crate::{AUDIO_HEADROOM, AudioPool, MAX_RENDER_SECONDS, RenderError, Renderer, track_gains};

/// Frames rendered per call to the renderer.
const BLOCK_FRAMES: usize = 512;

/// Renders the first `frames` of `track`'s arrangement lane into interleaved stereo: each audio
/// clip with its own gain, fades and warping, at unity gain and centred, with no devices, no
/// other track and no headroom, so a full-scale file plays back at full scale. Notes on the lane
/// play the engine's test voice; `clip.consolidate` renders only audio lanes.
pub fn render_track(
    project: &Project,
    audio: &AudioPool,
    track: TrackId,
    sample_rate: u32,
    frames: usize,
) -> Result<Vec<f32>, RenderError> {
    let seconds = frames as f64 / f64::from(sample_rate.max(1));
    if seconds > MAX_RENDER_SECONDS {
        return Err(RenderError::TooLong { seconds });
    }
    let alone = alone(project, track);
    let (left, right) = track_gains(&alone, track).unwrap_or((0.0, 0.0));
    let mut renderer = Renderer::with_audio(&alone, audio, sample_rate)?;
    renderer.set_offline(true);
    let mut out = vec![0.0f32; frames.saturating_mul(2)];
    for block in out.chunks_mut(BLOCK_FRAMES * 2) {
        renderer.process(block);
    }
    // Undo the centred pan and the headroom the mix gives every audio clip.
    let undo = |gain: f32| if gain > 0.0 { 1.0 / (gain * AUDIO_HEADROOM) } else { 0.0 };
    let (left, right) = (undo(left), undo(right));
    for frame in out.chunks_exact_mut(2) {
        if let [l, r] = frame {
            *l *= left;
            *r *= right;
        }
    }
    Ok(out)
}

/// The project with only `track` and its arrangement lane, playing straight to the output
/// through a unity channel.
fn alone(project: &Project, track: TrackId) -> Project {
    let channel = ChannelId(0);
    let source = project.tracks.iter().find(|t| t.id == track);
    Project {
        transport: project.transport.clone(),
        tracks: vec![Track {
            id: track,
            name: source.map(|t| t.name.clone()).unwrap_or_default(),
            color: None,
            content: source.map(|t| t.content).unwrap_or_default(),
            channel: Some(channel),
            parent: None,
            extensions: Extensions::default(),
        }],
        channels: vec![Channel {
            id: channel,
            name: String::new(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: Vec::new(),
            devices: Vec::new(),
            extensions: Extensions::default(),
        }],
        // Lanes that play on another track (a shared pattern) are left out with that track.
        contents: project
            .contents
            .iter()
            .map(|c| ClipContent {
                lanes: c.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)).cloned().collect(),
                ..c.clone()
            })
            .collect(),
        arrangement: Arrangement {
            lanes: project.arrangement.lanes.iter().filter(|l| l.track == Some(track)).cloned().collect(),
        },
        launcher: Default::default(),
        extensions: Extensions::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use suhina_model::{AudioRegion, ContentId, ContentLane, LaneData, StretchMode};

    use super::*;
    use crate::{AudioData, render_offline_with_audio};

    fn region(file: &str) -> AudioRegion {
        AudioRegion {
            file: file.into(),
            absolute_file: None,
            sample_rate: Some(48_000),
            channels: Some(1),
            duration_seconds: Some(1.0),
            warped: false,
            stretch: StretchMode::default(),
            warp_markers: Vec::new(),
            gain: 0.5,
            transpose: 0.0,
            stream: None,
        }
    }

    fn project() -> (Project, AudioPool) {
        let mut audio = AudioPool::new();
        let samples: Vec<f32> = (0..48_000).map(|i| ((i as f32) * 0.01).sin() * 0.8).collect();
        audio.files.insert("a.wav".into(), Arc::new(AudioData { sample_rate: 48_000, channels: 1, samples }));
        let track = TrackId(3);
        let project = Project {
            tracks: vec![Track {
                id: track,
                name: "Audio".into(),
                color: None,
                content: suhina_model::ContentKinds { notes: false, audio: true },
                channel: Some(ChannelId(7)),
                parent: None,
                extensions: Extensions::default(),
            }],
            channels: vec![Channel {
                id: ChannelId(7),
                name: "Audio".into(),
                role: ChannelRole::Regular,
                volume: 0.3,
                pan: -0.5,
                mute: false,
                solo: false,
                output: None,
                sends: Vec::new(),
                devices: Vec::new(),
                extensions: Extensions::default(),
            }],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "a".into(),
                length: 2.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Audio(region("a.wav")) }],
                extensions: Extensions::default(),
            }],
            arrangement: Arrangement {
                lanes: vec![suhina_model::ArrangementLane {
                    name: "Audio".into(),
                    track: Some(track),
                    clips: vec![suhina_model::Clip {
                        content: ContentId(1),
                        start: 0.5,
                        length: 1.0,
                        offset: 0.25,
                        loop_range: None,
                        enabled: true,
                        fade_in: 0.25,
                        fade_out: 0.0,
                        fade_in_curve: Default::default(),
                        fade_out_curve: Default::default(),
                        extensions: Extensions::default(),
                    }],
                }],
            },
            ..Default::default()
        };
        (project, audio)
    }

    #[test]
    fn a_track_renders_without_its_channel_and_the_mix_headroom() {
        let (project, audio) = project();
        let frames = 48_000;
        let alone = render_track(&project, &audio, TrackId(3), 48_000, frames).unwrap_or_default();
        assert_eq!(alone.len(), frames * 2);
        // The mix renders the same clip through its channel: scaled by its gains and headroom.
        let mixed = render_offline_with_audio(&project, &audio, 48_000, 512).unwrap_or_default();
        let (l, r) = track_gains(&project, TrackId(3)).unwrap_or((0.0, 0.0));
        let peak = alone.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "the clip is heard at its own level, peak {peak}");
        for (frame, (a, m)) in alone.chunks_exact(2).zip(mixed.chunks_exact(2)).enumerate() {
            let expected = (a[0] * l * AUDIO_HEADROOM, a[1] * r * AUDIO_HEADROOM);
            assert!((expected.0 - m[0]).abs() < 1e-5 && (expected.1 - m[1]).abs() < 1e-5, "frame {frame}");
        }
        // Left and right are the same: centred.
        assert!(alone.chunks_exact(2).all(|f| (f[0] - f[1]).abs() < 1e-6));
    }

    #[test]
    fn a_render_longer_than_the_limit_is_refused() {
        let (project, audio) = project();
        let too_long = (MAX_RENDER_SECONDS as usize + 1) * 8_000;
        assert!(matches!(
            render_track(&project, &audio, TrackId(3), 8_000, too_long),
            Err(RenderError::TooLong { .. })
        ));
    }
}
