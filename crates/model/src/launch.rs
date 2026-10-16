// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How a clip in a launcher slot responds to being launched: its launch mode, legato and its own
//! launch quantisation (`docs/spec/clip-launching.md` › Launch settings).

/// What pressing and releasing a clip's launch button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
    /// A press launches the clip; it plays until it is stopped or replaced.
    #[default]
    Trigger,
    /// A press launches the clip, the release stops it.
    Gate,
    /// A press launches the clip, the next press stops it.
    Toggle,
    /// While held, the clip is launched again at every boundary of its quantisation.
    Repeat,
}

impl LaunchMode {
    pub const ALL: [Self; 4] = [Self::Trigger, Self::Gate, Self::Toggle, Self::Repeat];

    pub fn name(self) -> &'static str {
        match self {
            Self::Trigger => "trigger",
            Self::Gate => "gate",
            Self::Toggle => "toggle",
            Self::Repeat => "repeat",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }

    /// Whether the clip needs to hear when its button is released.
    pub fn uses_release(self) -> bool {
        matches!(self, Self::Gate | Self::Repeat)
    }
}

/// The grid a clip's launch waits for: the launcher's global value, or one of the clip's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchQuantization {
    /// The launcher's global launch quantisation.
    #[default]
    Global,
    /// No waiting: the launch happens at once.
    None,
    /// A number of bars: 8, 4, 2 or 1.
    Bars(u8),
    /// A note value: 2 is a half note, 4 a quarter note, up to 32.
    Note(u8),
    /// A triplet note value: 2, 4, 8 or 16.
    Triplet(u8),
}

impl LaunchQuantization {
    /// Every value, in the order a menu shows them.
    pub const ALL: [Self; 15] = [
        Self::Global,
        Self::None,
        Self::Bars(8),
        Self::Bars(4),
        Self::Bars(2),
        Self::Bars(1),
        Self::Note(2),
        Self::Triplet(2),
        Self::Note(4),
        Self::Triplet(4),
        Self::Note(8),
        Self::Triplet(8),
        Self::Note(16),
        Self::Triplet(16),
        Self::Note(32),
    ];

    /// The value as `clip.setQuantization` and the project file write it: "global", "none",
    /// "1 bar", "2 bars", "1/4", "1/8T" and so on.
    pub fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::None => "none",
            Self::Bars(8) => "8 bars",
            Self::Bars(4) => "4 bars",
            Self::Bars(2) => "2 bars",
            Self::Bars(_) => "1 bar",
            Self::Note(2) => "1/2",
            Self::Note(4) => "1/4",
            Self::Note(8) => "1/8",
            Self::Note(16) => "1/16",
            Self::Note(_) => "1/32",
            Self::Triplet(2) => "1/2T",
            Self::Triplet(4) => "1/4T",
            Self::Triplet(8) => "1/8T",
            Self::Triplet(_) => "1/16T",
        }
    }

    /// Reads a value written by [`LaunchQuantization::name`], in any letter case and with
    /// spaces around it; `off` is `none` too. Anything else is `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        let normal = name.trim().to_ascii_lowercase();
        if normal == "off" {
            return Some(Self::None);
        }
        Self::ALL.into_iter().find(|q| q.name().to_ascii_lowercase() == normal)
    }

    /// The grid step in beats (quarter notes) with bars of `bar_beats`: `None` for
    /// [`LaunchQuantization::Global`], 0 for no waiting. A count outside the menu's values counts
    /// as the value [`LaunchQuantization::name`] gives it, so the name and the step always agree.
    pub fn beats(self, bar_beats: f64) -> Option<f64> {
        let bars = |n: u8| if matches!(n, 8 | 4 | 2) { f64::from(n) } else { 1.0 };
        let note = |n: u8, last: u8| 4.0 / f64::from(if matches!(n, 2 | 4 | 8 | 16) { n } else { last });
        match self {
            Self::Global => None,
            Self::None => Some(0.0),
            Self::Bars(n) => Some(bars(n) * bar_beats),
            Self::Note(n) => Some(note(n, 32)),
            Self::Triplet(n) => Some(note(n, 16) * 2.0 / 3.0),
        }
    }
}

/// A clip's launch settings. A clip on the arrangement keeps them but does not use them; they
/// count when the clip is in a launcher slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchSettings {
    pub mode: LaunchMode,
    /// The clip starts at the position of the clip it replaces on its track instead of at its
    /// own start.
    pub legato: bool,
    pub quantization: LaunchQuantization,
}

impl LaunchSettings {
    /// Whether these are the settings of a new clip.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_back() {
        for mode in LaunchMode::ALL {
            assert_eq!(LaunchMode::from_name(mode.name()), Some(mode));
        }
        assert_eq!(LaunchMode::from_name("Gate"), None);
        for q in LaunchQuantization::ALL {
            assert_eq!(LaunchQuantization::from_name(q.name()), Some(q), "{}", q.name());
        }
        assert_eq!(LaunchQuantization::from_name(" 1/8t "), Some(LaunchQuantization::Triplet(8)));
        assert_eq!(LaunchQuantization::from_name("OFF"), Some(LaunchQuantization::None));
        for bad in ["", "3 bars", "1/3", "1/32T", "global!", "1/64"] {
            assert_eq!(LaunchQuantization::from_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn steps_are_in_beats() {
        assert_eq!(LaunchQuantization::Global.beats(4.0), None);
        assert_eq!(LaunchQuantization::None.beats(4.0), Some(0.0));
        assert_eq!(LaunchQuantization::Bars(2).beats(3.0), Some(6.0));
        assert_eq!(LaunchQuantization::Note(16).beats(4.0), Some(0.25));
        let triplet = LaunchQuantization::Triplet(8).beats(4.0).unwrap_or(0.0);
        assert!((triplet - 1.0 / 3.0).abs() < 1e-12);
        // Counts outside the menu read as the value their name gives.
        assert_eq!(LaunchQuantization::Note(0).beats(4.0), Some(0.125));
        assert_eq!(LaunchQuantization::Bars(3).name(), "1 bar");
        assert_eq!(LaunchQuantization::Bars(3).beats(4.0), Some(4.0));
    }
}
