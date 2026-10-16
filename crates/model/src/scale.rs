// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's musical scale: a root and a mode (ADR 0001 › Amendment: clip scales).

/// The names of the twelve roots, from C, as the piano roll and the commands write them.
pub const ROOT_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The mode of a [`Scale`]: which pitch classes above its root it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleMode {
    #[default]
    Major,
    Minor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
}

impl ScaleMode {
    /// Every mode, in the order menus list them.
    pub const ALL: [ScaleMode; 12] = [
        Self::Major,
        Self::Minor,
        Self::HarmonicMinor,
        Self::MelodicMinor,
        Self::Dorian,
        Self::Phrygian,
        Self::Lydian,
        Self::Mixolydian,
        Self::Locrian,
        Self::MajorPentatonic,
        Self::MinorPentatonic,
        Self::Blues,
    ];

    /// The stable name commands and the project file use, such as `"harmonicMinor"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::HarmonicMinor => "harmonicMinor",
            Self::MelodicMinor => "melodicMinor",
            Self::Dorian => "dorian",
            Self::Phrygian => "phrygian",
            Self::Lydian => "lydian",
            Self::Mixolydian => "mixolydian",
            Self::Locrian => "locrian",
            Self::MajorPentatonic => "majorPentatonic",
            Self::MinorPentatonic => "minorPentatonic",
            Self::Blues => "blues",
        }
    }

    /// The name as a menu shows it, such as "Harmonic minor".
    pub fn label(self) -> &'static str {
        match self {
            Self::Major => "Major",
            Self::Minor => "Minor",
            Self::HarmonicMinor => "Harmonic minor",
            Self::MelodicMinor => "Melodic minor",
            Self::Dorian => "Dorian",
            Self::Phrygian => "Phrygian",
            Self::Lydian => "Lydian",
            Self::Mixolydian => "Mixolydian",
            Self::Locrian => "Locrian",
            Self::MajorPentatonic => "Major pentatonic",
            Self::MinorPentatonic => "Minor pentatonic",
            Self::Blues => "Blues",
        }
    }

    /// The mode a stable name names; `None` for a name that is not one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// The semitones above the root the mode holds, from 0.
    pub fn semitones(self) -> &'static [u8] {
        match self {
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Self::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Self::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Self::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
            Self::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }
}

/// A musical scale: a root pitch class (0 is C, 11 is B) and a mode. The root is always 0 to 11:
/// [`Scale::new`] refuses anything else, so a scale in the model is always valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scale {
    root: u8,
    pub mode: ScaleMode,
}

impl Scale {
    /// A scale over `root` (0 to 11); `None` for a root outside that.
    pub fn new(root: u8, mode: ScaleMode) -> Option<Self> {
        (root <= 11).then_some(Self { root, mode })
    }

    /// The root pitch class, 0 (C) to 11 (B).
    pub fn root(self) -> u8 {
        self.root
    }

    /// The root's name, such as "F#".
    pub fn root_name(self) -> &'static str {
        ROOT_NAMES.get(usize::from(self.root)).copied().unwrap_or("C")
    }

    /// Whether MIDI key `key` is in the scale.
    pub fn contains(self, key: u8) -> bool {
        let class = (key % 12 + 12 - self.root) % 12;
        self.mode.semitones().contains(&class)
    }

    /// The key nearest to `key` that the scale holds, within 0 to 127; a tie goes to the lower key.
    pub fn nearest(self, key: u8) -> u8 {
        let key = key.min(127);
        for distance in 0..=127u8 {
            if let Some(lower) = key.checked_sub(distance)
                && self.contains(lower)
            {
                return lower;
            }
            if let Some(upper) = key.checked_add(distance).filter(|k| *k <= 127)
                && self.contains(upper)
            {
                return upper;
            }
        }
        key
    }

    /// The scale as people say it, such as "F# minor" or "C harmonic minor".
    pub fn describe(self) -> String {
        format!("{} {}", self.root_name(), self.mode.label().to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_names_are_refused() {
        for mode in ScaleMode::ALL {
            assert_eq!(ScaleMode::from_name(mode.name()), Some(mode));
            assert_eq!(mode.semitones().first(), Some(&0));
        }
        assert_eq!(ScaleMode::from_name("Major"), None);
        assert_eq!(ScaleMode::from_name(""), None);
    }

    #[test]
    fn a_root_is_0_to_11() {
        assert!(Scale::new(11, ScaleMode::Major).is_some());
        assert!(Scale::new(12, ScaleMode::Major).is_none());
        assert!(Scale::new(255, ScaleMode::Blues).is_none());
    }

    #[test]
    fn keys_in_a_scale_and_the_nearest_one() {
        let d_minor = Scale::new(2, ScaleMode::Minor).unwrap();
        // D minor: D E F G A Bb C.
        let held: Vec<u8> = (60..72).filter(|k| d_minor.contains(*k)).collect();
        assert_eq!(held, [60, 62, 64, 65, 67, 69, 70]);
        assert_eq!(d_minor.nearest(61), 60, "a tie goes to the lower key");
        assert_eq!(d_minor.nearest(127), 127);
        assert_eq!(d_minor.describe(), "D minor");
        let g = Scale::new(7, ScaleMode::Major).unwrap();
        assert_eq!(g.nearest(65), 64);
        assert_eq!(g.root_name(), "G");
    }
}
