//! What sort of sound an item is, from the words in its name (spec › Kinds and colours).

use std::path::Path;

/// The kinds, in the spec's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Kick,
    Snare,
    Clap,
    ClosedHat,
    OpenHat,
    Hat,
    Rim,
    Tom,
    Crash,
    Ride,
    Cymbal,
    Percussion,
    Bass,
    Tonal,
    Vocal,
    Effect,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 17] = [
        Kind::Kick,
        Kind::Snare,
        Kind::Clap,
        Kind::ClosedHat,
        Kind::OpenHat,
        Kind::Hat,
        Kind::Rim,
        Kind::Tom,
        Kind::Crash,
        Kind::Ride,
        Kind::Cymbal,
        Kind::Percussion,
        Kind::Bass,
        Kind::Tonal,
        Kind::Vocal,
        Kind::Effect,
        Kind::Other,
    ];

    /// The id used in commands and the cache.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Kick => "kick",
            Kind::Snare => "snare",
            Kind::Clap => "clap",
            Kind::ClosedHat => "closedHat",
            Kind::OpenHat => "openHat",
            Kind::Hat => "hat",
            Kind::Rim => "rim",
            Kind::Tom => "tom",
            Kind::Crash => "crash",
            Kind::Ride => "ride",
            Kind::Cymbal => "cymbal",
            Kind::Percussion => "percussion",
            Kind::Bass => "bass",
            Kind::Tonal => "tonal",
            Kind::Vocal => "vocal",
            Kind::Effect => "effect",
            Kind::Other => "other",
        }
    }

    /// The name shown on the map.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Kick => "Kick",
            Kind::Snare => "Snare",
            Kind::Clap => "Clap",
            Kind::ClosedHat => "Closed hat",
            Kind::OpenHat => "Open hat",
            Kind::Hat => "Hat",
            Kind::Rim => "Rim",
            Kind::Tom => "Tom",
            Kind::Crash => "Crash",
            Kind::Ride => "Ride",
            Kind::Cymbal => "Cymbal",
            Kind::Percussion => "Percussion",
            Kind::Bass => "Bass",
            Kind::Tonal => "Tonal",
            Kind::Vocal => "Vocal",
            Kind::Effect => "Effect",
            Kind::Other => "Other",
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// Single words that name a kind.
const WORDS: &[(&str, Kind)] = &[
    ("kick", Kind::Kick),
    ("kik", Kind::Kick),
    ("kck", Kind::Kick),
    ("bd", Kind::Kick),
    ("bassdrum", Kind::Kick),
    ("snare", Kind::Snare),
    ("snr", Kind::Snare),
    ("sd", Kind::Snare),
    ("clap", Kind::Clap),
    ("clp", Kind::Clap),
    ("cp", Kind::Clap),
    ("snap", Kind::Clap),
    ("chh", Kind::ClosedHat),
    ("closedhat", Kind::ClosedHat),
    ("hhc", Kind::ClosedHat),
    ("ohh", Kind::OpenHat),
    ("openhat", Kind::OpenHat),
    ("hho", Kind::OpenHat),
    ("hat", Kind::Hat),
    ("hihat", Kind::Hat),
    ("hh", Kind::Hat),
    ("rim", Kind::Rim),
    ("rimshot", Kind::Rim),
    ("rs", Kind::Rim),
    ("stick", Kind::Rim),
    ("tom", Kind::Tom),
    ("toms", Kind::Tom),
    ("crash", Kind::Crash),
    ("ride", Kind::Ride),
    ("cymbal", Kind::Cymbal),
    ("cym", Kind::Cymbal),
    ("china", Kind::Cymbal),
    ("splash", Kind::Cymbal),
    ("perc", Kind::Percussion),
    ("shaker", Kind::Percussion),
    ("shk", Kind::Percussion),
    ("tamb", Kind::Percussion),
    ("tambourine", Kind::Percussion),
    ("conga", Kind::Percussion),
    ("bongo", Kind::Percussion),
    ("cowbell", Kind::Percussion),
    ("clave", Kind::Percussion),
    ("block", Kind::Percussion),
    ("triangle", Kind::Percussion),
    ("808", Kind::Bass),
    ("bass", Kind::Bass),
    ("sub", Kind::Bass),
    ("stab", Kind::Tonal),
    ("chord", Kind::Tonal),
    ("pluck", Kind::Tonal),
    ("keys", Kind::Tonal),
    ("synth", Kind::Tonal),
    ("lead", Kind::Tonal),
    ("pad", Kind::Tonal),
    ("vox", Kind::Vocal),
    ("vocal", Kind::Vocal),
    ("voice", Kind::Vocal),
    ("chant", Kind::Vocal),
    ("fx", Kind::Effect),
    ("sfx", Kind::Effect),
    ("riser", Kind::Effect),
    ("impact", Kind::Effect),
    ("sweep", Kind::Effect),
    ("noise", Kind::Effect),
];

/// Two words in a row that name a kind ("closed hat", "hat open", "floor tom").
const PAIRS: &[(&str, &str, Kind)] = &[
    ("closed", "hat", Kind::ClosedHat),
    ("hat", "closed", Kind::ClosedHat),
    ("closed", "hihat", Kind::ClosedHat),
    ("hihat", "closed", Kind::ClosedHat),
    ("hh", "closed", Kind::ClosedHat),
    ("open", "hat", Kind::OpenHat),
    ("hat", "open", Kind::OpenHat),
    ("open", "hihat", Kind::OpenHat),
    ("hihat", "open", Kind::OpenHat),
    ("hh", "open", Kind::OpenHat),
    ("floor", "tom", Kind::Tom),
    ("bass", "drum", Kind::Kick),
];

/// The words of a name: split at anything not a letter or digit, at a lower-to-upper case
/// change and between letters and digits; lower case.
pub fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous = None;
            continue;
        }
        if let Some(p) = previous {
            let case_change = p.is_lowercase() && c.is_uppercase();
            let digit_change = p.is_ascii_digit() != c.is_ascii_digit();
            if (case_change || digit_change) && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        }
        current.extend(c.to_lowercase());
        previous = Some(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The kind one name gives, if any: the most specific of the kinds its words name (a pair
/// before a single word, and Closed or Open hat before Hat).
pub fn kind_of_name(name: &str) -> Option<Kind> {
    // A plural counts as its word ("Kicks", "Hats"), for words of three letters or more.
    let words: Vec<String> = words(name)
        .into_iter()
        .map(|w| match w.strip_suffix('s') {
            Some(stem) if stem.len() >= 3 && !WORDS.iter().any(|(x, _)| *x == w) => stem.to_string(),
            _ => w,
        })
        .collect();
    let mut found: Option<Kind> = None;
    let mut consider = |kind: Kind| {
        found = Some(match found {
            Some(Kind::Hat) if matches!(kind, Kind::ClosedHat | Kind::OpenHat) => kind,
            Some(current) => current,
            None => kind,
        });
    };
    for pair in words.windows(2) {
        if let [a, b] = pair
            && let Some((_, _, kind)) = PAIRS.iter().find(|(x, y, _)| x == a && y == b)
        {
            consider(*kind);
        }
    }
    for word in &words {
        if let Some((_, kind)) = WORDS.iter().find(|(w, _)| w == word) {
            consider(*kind);
        }
    }
    found
}

/// The kind a file's name gives: its file name first, then its folders from the nearest up to
/// (not including) `root`.
pub fn kind_of_path(path: &Path, root: &Path) -> Option<Kind> {
    if let Some(kind) = path.file_stem().and_then(|s| s.to_str()).and_then(kind_of_name) {
        return Some(kind);
    }
    let below = path.strip_prefix(root).unwrap_or(path);
    below.parent()?.components().rev().filter_map(|c| c.as_os_str().to_str()).find_map(kind_of_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_into_words() {
        assert_eq!(words("KickHard_02"), ["kick", "hard", "02"]);
        assert_eq!(words("BD-909 (tuned).wav"), ["bd", "909", "tuned", "wav"]);
        assert_eq!(words("808Sub"), ["808", "sub"]);
        assert_eq!(words("HHOpen"), ["hhopen"]);
        assert!(words("  __ ").is_empty());
    }

    #[test]
    fn names_give_kinds() {
        assert_eq!(kind_of_name("KickHard_02"), Some(Kind::Kick));
        assert_eq!(kind_of_name("Snr 4"), Some(Kind::Snare));
        assert_eq!(kind_of_name("closed hat 3"), Some(Kind::ClosedHat));
        assert_eq!(kind_of_name("Hat_Open_1"), Some(Kind::OpenHat));
        assert_eq!(kind_of_name("Hihat 1"), Some(Kind::Hat));
        assert_eq!(kind_of_name("hat chh"), Some(Kind::ClosedHat));
        assert_eq!(kind_of_name("808 boom"), Some(Kind::Bass));
        assert_eq!(kind_of_name("Floor Tom"), Some(Kind::Tom));
        assert_eq!(kind_of_name("Rides"), Some(Kind::Ride));
        assert_eq!(kind_of_name("Hats"), Some(Kind::Hat));
        assert_eq!(kind_of_name("toms"), Some(Kind::Tom));
        assert_eq!(kind_of_name("bass"), Some(Kind::Bass));
        assert_eq!(kind_of_name("sidestick"), None);
        assert_eq!(kind_of_name("thing"), None);
    }

    #[test]
    fn folders_count_when_the_file_name_does_not() {
        let root = Path::new("/lib");
        assert_eq!(kind_of_path(Path::new("/lib/Drums/Kicks/01.wav"), root), Some(Kind::Kick));
        assert_eq!(kind_of_path(Path::new("/lib/Drums/One shots/01.wav"), root), None);
        assert_eq!(kind_of_path(Path::new("/lib/Drums/Kick/01.wav"), root), Some(Kind::Kick));
        assert_eq!(kind_of_path(Path::new("/lib/Snare/Kick/01.wav"), root), Some(Kind::Kick));
        assert_eq!(kind_of_path(Path::new("/lib/Snare/clap 2.wav"), root), Some(Kind::Clap));
        // The library folder's own name does not count.
        assert_eq!(kind_of_path(Path::new("/Kick/01.wav"), Path::new("/Kick")), None);
    }

    #[test]
    fn ids_round_trip() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_id(kind.id()), Some(kind));
        }
    }
}
