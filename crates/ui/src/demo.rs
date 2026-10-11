//! A small self-made project with a filled clip launcher: four tracks, two return tracks and six
//! scenes. The app opens it with `--demo`, and the UI tests draw it.

use loopwerk_engine::model::{
    Arrangement, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds, ContentLane, Extensions,
    LaneData, Launcher, LauncherColumn, Note, Project, Scene, Send, Slot, Track, TrackId, Transport,
};

/// Clip names per track and scene; `None` is an empty slot with a stop button.
const CLIPS: [(&str, [Option<&str>; 6]); 4] = [
    ("Drums", [Some("Beat A"), Some("Beat A"), Some("Beat B"), Some("Fill"), None, Some("Beat B")]),
    ("Bass", [Some("Root"), Some("Walk"), Some("Walk"), None, Some("Drone"), Some("Root")]),
    ("Keys", [None, Some("Chords 1"), Some("Chords 2"), Some("Chords 2"), Some("Pad"), None]),
    ("Vox", [None, None, Some("Hook"), Some("Hook dub"), None, Some("Verse")]),
];

const SCENES: [&str; 6] = ["Intro", "Groove", "Build", "Break", "Drop", "Outro"];

pub fn demo_project() -> Project {
    let mut project = Project {
        transport: Transport { tempo_bpm: 122.0, ..Transport::default() },
        arrangement: Arrangement::default(),
        ..Project::default()
    };
    let main = ChannelId(100);
    let returns = [(ChannelId(90), "A Reverb"), (ChannelId(91), "B Delay")];

    let mut columns = Vec::new();
    for (index, (name, clips)) in CLIPS.iter().enumerate() {
        let id = index as u32 + 1;
        project.tracks.push(Track {
            id: TrackId(id),
            name: (*name).into(),
            color: None,
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(id)),
            parent: None,
            extensions: Extensions::default(),
        });
        let mut channel = channel(ChannelId(id), name, ChannelRole::Regular, Some(main));
        channel.volume = [1.0, 0.71, 0.5, 0.84][index];
        channel.pan = [0.0, 0.0, -0.3, 0.2][index];
        channel.sends = vec![Send { target: returns[0].0, level: 0.3, pre_fader: false }];
        project.channels.push(channel);

        let slots = clips
            .iter()
            .enumerate()
            .map(|(scene, clip)| match clip {
                None => Slot { clip: None, has_stop: true },
                Some(clip_name) => {
                    let content = ContentId(id * 10 + scene as u32);
                    project.contents.push(content_of(content, clip_name, index as u8));
                    Slot { clip: Some(clip_of(content)), has_stop: true }
                }
            })
            .collect();
        columns.push(LauncherColumn { track: TrackId(id), slots });
    }
    for (offset, (channel_id, name)) in returns.iter().enumerate() {
        let id = 10 + offset as u32;
        project.tracks.push(Track {
            id: TrackId(id),
            name: (*name).into(),
            color: None,
            content: ContentKinds::default(),
            channel: Some(*channel_id),
            parent: None,
            extensions: Extensions::default(),
        });
        project.channels.push(channel(*channel_id, name, ChannelRole::Effect, Some(main)));
    }
    project.channels.push(channel(main, "Main", ChannelRole::Master, None));
    project.launcher = Launcher {
        scenes: SCENES.iter().map(|name| Scene { name: (*name).into(), tempo_bpm: None }).collect(),
        columns,
    };
    project
}

fn channel(id: ChannelId, name: &str, role: ChannelRole, output: Option<ChannelId>) -> Channel {
    Channel {
        id,
        name: name.into(),
        role,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
    }
}

/// A one-bar clip: a short figure around a root that differs per track.
fn content_of(id: ContentId, name: &str, track: u8) -> ClipContent {
    let root = [36u8, 40, 60, 67][usize::from(track) % 4];
    let notes = (0..4)
        .map(|beat| Note {
            time: f64::from(beat),
            duration: 0.5,
            key: root + [0u8, 7, 3, 5][beat as usize % 4],
            velocity: 0.8,
            release_velocity: 0.5,
            channel: 0,
        })
        .collect();
    ClipContent {
        id,
        name: name.into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
    }
}

fn clip_of(content: ContentId) -> Clip {
    Clip {
        content,
        start: 0.0,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        extensions: Extensions::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_project_validates() {
        assert_eq!(demo_project().validate(), Ok(()));
    }
}
