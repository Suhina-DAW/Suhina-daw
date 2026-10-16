// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launch mappings: a computer key or a MIDI note that launches a slot, a scene or a track's stop
//! button (`docs/spec/clip-launching.md` › Launching from keys and notes).

use crate::TrackId;

/// The longest key name a mapping keeps.
pub const KEY_NAME_MAX: usize = 24;

/// What a mapping listens to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MappingSource {
    /// A computer key by its name: "Q", "1", "F5", "Space". Pressed without Ctrl, Alt or Cmd.
    Key(String),
    /// A MIDI note: `channel` 1 to 16, `note` 0 to 127.
    Note { channel: u8, note: u8 },
}

impl MappingSource {
    /// A key source, when `name` is 1 to [`KEY_NAME_MAX`] printable ASCII characters.
    pub fn key(name: &str) -> Option<Self> {
        let ok = !name.is_empty() && name.len() <= KEY_NAME_MAX && name.bytes().all(|b| b.is_ascii_graphic());
        ok.then(|| Self::Key(name.to_string()))
    }

    /// A note source, when the channel is 1 to 16 and the note 0 to 127.
    pub fn note(channel: u8, note: u8) -> Option<Self> {
        ((1..=16).contains(&channel) && note <= 127).then_some(Self::Note { channel, note })
    }

    /// The source as the commands and the project file write it: `key:Q`, `note:1:60`.
    pub fn name(&self) -> String {
        match self {
            Self::Key(key) => format!("key:{key}"),
            Self::Note { channel, note } => format!("note:{channel}:{note}"),
        }
    }

    /// Reads a name written by [`MappingSource::name`]; anything else is `None`.
    pub fn from_name(text: &str) -> Option<Self> {
        if let Some(key) = text.strip_prefix("key:") {
            return Self::key(key);
        }
        let (channel, note) = text.strip_prefix("note:")?.split_once(':')?;
        let whole = |s: &str| if s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<u8>().ok() } else { None };
        Self::note(whole(channel)?, whole(note)?)
    }

    /// What the window shows on a mapped control: `Q`, `Note 60`, `Ch 2 Note 60`.
    pub fn label(&self) -> String {
        match self {
            Self::Key(key) => key.clone(),
            Self::Note { channel: 1, note } => format!("Note {note}"),
            Self::Note { channel, note } => format!("Ch {channel} Note {note}"),
        }
    }
}

/// What a mapping launches. Scenes count from 0, like [`crate::Launcher::scenes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MappingTarget {
    /// The slot of `track` in `scene`: the press is `clip.launch`, the release `clip.release`.
    Slot { track: TrackId, scene: usize },
    /// A scene's launch button: `scene.launch`.
    Scene { scene: usize },
    /// A track's stop button: `track.stop`.
    Stop { track: TrackId },
}

impl MappingTarget {
    /// The kind as the commands and the project file write it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Slot { .. } => "slot",
            Self::Scene { .. } => "scene",
            Self::Stop { .. } => "stop",
        }
    }

    /// The command a press runs.
    pub fn command(&self) -> &'static str {
        match self {
            Self::Slot { .. } => "clip.launch",
            Self::Scene { .. } => "scene.launch",
            Self::Stop { .. } => "track.stop",
        }
    }

    pub fn track(&self) -> Option<TrackId> {
        match self {
            Self::Slot { track, .. } | Self::Stop { track } => Some(*track),
            Self::Scene { .. } => None,
        }
    }

    pub fn scene(&self) -> Option<usize> {
        match self {
            Self::Slot { scene, .. } | Self::Scene { scene } => Some(*scene),
            Self::Stop { .. } => None,
        }
    }
}

/// One source that launches one target. A project keeps at most one mapping per source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchMapping {
    pub source: MappingSource,
    pub target: MappingTarget,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_read_back() {
        for source in [
            MappingSource::key("Q").unwrap(),
            MappingSource::key("Space").unwrap(),
            MappingSource::key(":").unwrap(),
            MappingSource::note(1, 0).unwrap(),
            MappingSource::note(16, 127).unwrap(),
        ] {
            assert_eq!(MappingSource::from_name(&source.name()), Some(source.clone()), "{}", source.name());
        }
        assert_eq!(MappingSource::from_name("note:10:36"), Some(MappingSource::Note { channel: 10, note: 36 }));
        for bad in [
            "",
            "key:",
            "key:two words",
            "key:é",
            "Q",
            "note:0:60",
            "note:17:60",
            "note:1:128",
            "note:1",
            "note:1:+5",
            "note:1:60:2",
            "note::60",
            "cc:1:20",
        ] {
            assert_eq!(MappingSource::from_name(bad), None, "{bad}");
        }
        assert!(MappingSource::key(&"A".repeat(KEY_NAME_MAX)).is_some());
        assert!(MappingSource::key(&"A".repeat(KEY_NAME_MAX + 1)).is_none());
    }

    #[test]
    fn labels_are_short() {
        assert_eq!(MappingSource::key("Q").unwrap().label(), "Q");
        assert_eq!(MappingSource::note(1, 36).unwrap().label(), "Note 36");
        assert_eq!(MappingSource::note(10, 36).unwrap().label(), "Ch 10 Note 36");
    }

    #[test]
    fn targets_name_their_command() {
        let slot = MappingTarget::Slot { track: TrackId(2), scene: 1 };
        assert_eq!(
            (slot.kind(), slot.command(), slot.track(), slot.scene()),
            ("slot", "clip.launch", Some(TrackId(2)), Some(1))
        );
        let scene = MappingTarget::Scene { scene: 3 };
        assert_eq!(
            (scene.kind(), scene.command(), scene.track(), scene.scene()),
            ("scene", "scene.launch", None, Some(3))
        );
        let stop = MappingTarget::Stop { track: TrackId(4) };
        assert_eq!(
            (stop.kind(), stop.command(), stop.track(), stop.scene()),
            ("stop", "track.stop", Some(TrackId(4)), None)
        );
    }
}
