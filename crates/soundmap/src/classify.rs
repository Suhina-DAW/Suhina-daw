//! What kind of sound each item of a scope is, before it is laid out (ADR 0013 › Kinds): a kind
//! set by hand, the words of its name, and a light classifier in feature space learned from the
//! scope's own named items, so no model is shipped. And the layout's input, in which each item
//! is drawn towards its kind's region.

use std::path::Path;

use crate::features::{FEATURE_COUNT, Features};
use crate::kinds::{self, Kind};

/// Where a name's or the user's kind comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Set by hand: it wins over everything else.
    Hand,
    /// The file's own name: it stands, whatever the sound.
    FileName,
    /// A folder above the file: a sound that is clearly something else overrules it.
    Folder,
}

/// Where an item's kind came from in the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    Hand,
    Name,
    /// The classifier, from the sound.
    Sound,
    /// Nothing gave one: the kind is [`Kind::Other`].
    Nothing,
}

/// Fewer examples than this of a kind with a file name (or set by hand) and its folder-named
/// items teach as well; fewer than this in all and the kind is not learned.
const MIN_EXAMPLES: usize = 4;
/// At most this many examples of a kind are learned from, taken evenly.
const MAX_EXAMPLES: usize = 2_000;
/// Prototypes per kind (the centres of its sub-groups, by k-means), and the examples each needs.
const PROTOTYPES: usize = 4;
const EXAMPLES_PER_PROTOTYPE: usize = 6;
const KMEANS_ROUNDS: usize = 12;
/// A prototype that holds less than this share of its kind's examples is dropped: a handful of
/// misnamed items must not make a region of their own.
const MIN_PROTOTYPE_SHARE: f32 = 0.15;
/// The share of a kind's examples, nearest its prototypes first, whose distance is its radius.
const RADIUS_SHARE: f32 = 0.9;
/// A kind's radius is at least this share of the scope's typical distance from its centre, so a
/// kind whose examples are all alike still reaches sounds a little different.
const MIN_RADIUS: f32 = 0.25;
/// An item further than this many radii from the nearest kind is not given it.
const REACH: f32 = 2.5;
/// An unnamed item takes the nearest kind when the nearest kind of another family is at least
/// this many times further away.
const GUESS_MARGIN: f32 = 1.15;
/// A folder's kind gives way when the sound's kind is nearer by at least this factor.
const OVERRULE_MARGIN: f32 = 1.4;
/// How far each item is drawn towards its kind's centre in the layout's input (0 none, 1 all
/// the way), and how far each kind's items are moved apart from the others', in the scope's
/// typical distance from the centre.
const PULL: f32 = 0.5;
const SEPARATION: f32 = 1.5;
/// Below this many items, the classifier runs on the caller's thread.
const PARALLEL_ITEMS: usize = 4_096;

/// The kind an item's name gives, with where it came from: its file name first, then its
/// folders below `root`.
pub fn named(path: &Path, root: &Path) -> Option<(Kind, Evidence)> {
    if let Some(kind) = path.file_stem().and_then(|s| s.to_str()).and_then(kinds::kind_of_name) {
        return Some((kind, Evidence::FileName));
    }
    kinds::kind_of_path(path, root).map(|kind| (kind, Evidence::Folder))
}

/// Kinds that are close relatives: telling them apart is not what overrules a name or makes a
/// guess unsure (a closed hat near the hats is still a hat of some sort).
fn family(kind: Kind) -> u8 {
    match kind {
        Kind::ClosedHat | Kind::OpenHat | Kind::Hat => 1,
        Kind::Crash | Kind::Ride | Kind::Cymbal => 2,
        Kind::Snare | Kind::Rim => 3,
        _ => 10 + kind as u8,
    }
}

/// One learned kind.
#[derive(Debug, Clone)]
struct Learned {
    kind: Kind,
    prototypes: Vec<Features>,
    /// How far its own examples are from its prototypes ([`RADIUS_SHARE`] of them nearer).
    radius: f32,
}

/// The kinds learned from a scope's named items.
#[derive(Debug, Clone, Default)]
pub struct Classifier {
    learned: Vec<Learned>,
}

impl Classifier {
    /// Learns the kinds from the items whose kind is known: set by hand or from their file name,
    /// and from their folder for kinds with too few of those. `rows` are standardised features.
    pub fn learn(rows: &[Features], evidence: &[Option<(Kind, Evidence)>]) -> Self {
        let typical = typical_norm(rows);
        let mut learned = Vec::new();
        for kind in Kind::ALL.into_iter().filter(|k| *k != Kind::Other) {
            let of = |strong: bool| -> Vec<usize> {
                evidence
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| match e {
                        Some((k, Evidence::Hand | Evidence::FileName)) => *k == kind,
                        Some((k, Evidence::Folder)) => !strong && *k == kind,
                        None => false,
                    })
                    .map(|(i, _)| i)
                    .collect()
            };
            let mut examples = of(true);
            if examples.len() < MIN_EXAMPLES {
                examples = of(false);
            }
            if examples.len() < MIN_EXAMPLES {
                continue;
            }
            let step = examples.len().div_ceil(MAX_EXAMPLES).max(1);
            let examples: Vec<&Features> = examples.iter().step_by(step).filter_map(|&i| rows.get(i)).collect();
            let count = (examples.len() / EXAMPLES_PER_PROTOTYPE).clamp(1, PROTOTYPES);
            let prototypes = kmeans(&examples, count);
            let mut distances: Vec<f32> = examples.iter().map(|e| nearest(&prototypes, e)).collect();
            distances.sort_by(f32::total_cmp);
            let at = ((distances.len() as f32 * RADIUS_SHARE) as usize).min(distances.len().saturating_sub(1));
            let radius = distances.get(at).copied().unwrap_or(0.0).max(MIN_RADIUS * typical).max(1e-3);
            learned.push(Learned { kind, prototypes, radius });
        }
        Classifier { learned }
    }

    /// The kinds learned, in [`Kind::ALL`]'s order.
    pub fn kinds(&self) -> Vec<Kind> {
        self.learned.iter().map(|l| l.kind).collect()
    }

    /// The kind of one item from its features and what its name or the user says.
    pub fn kind_of(&self, row: &Features, evidence: Option<(Kind, Evidence)>) -> (Kind, From) {
        match evidence {
            Some((kind, Evidence::Hand)) => return (kind, From::Hand),
            Some((kind, Evidence::FileName)) => return (kind, From::Name),
            _ => {}
        }
        let distances: Vec<(Kind, f32, f32)> =
            self.learned.iter().map(|l| (l.kind, nearest(&l.prototypes, row), l.radius)).collect();
        let best = distances.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1));
        let sound = best.filter(|(_, d, radius)| *d <= REACH * radius);
        match (evidence, sound) {
            (Some((named, _)), Some((kind, d, _))) if family(kind) != family(named) => {
                let named_distance =
                    distances.iter().find(|(k, _, _)| *k == named).map_or(f32::INFINITY, |(_, d, _)| *d);
                if named_distance >= OVERRULE_MARGIN * d { (kind, From::Sound) } else { (named, From::Name) }
            }
            (Some((named, _)), _) => (named, From::Name),
            (None, Some((kind, d, _))) => {
                let other = distances
                    .iter()
                    .filter(|(k, _, _)| family(*k) != family(kind))
                    .map(|(_, d, _)| *d)
                    .fold(f32::INFINITY, f32::min);
                if other >= GUESS_MARGIN * d { (kind, From::Sound) } else { (Kind::Other, From::Nothing) }
            }
            (None, None) => (Kind::Other, From::Nothing),
        }
    }
}

/// The kind of every item of a scope ([`Classifier::learn`], then [`Classifier::kind_of`] for
/// each), on several threads for large scopes.
pub fn classify(rows: &[Features], evidence: &[Option<(Kind, Evidence)>]) -> Vec<(Kind, From)> {
    let classifier = Classifier::learn(rows, evidence);
    let one = |(row, e): (&Features, &Option<(Kind, Evidence)>)| classifier.kind_of(row, *e);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    if rows.len() < PARALLEL_ITEMS || threads < 2 {
        return rows.iter().zip(evidence).map(one).collect();
    }
    let chunk = rows.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = rows
            .chunks(chunk)
            .zip(evidence.chunks(chunk))
            .map(|(r, e)| scope.spawn(move || r.iter().zip(e).map(one).collect::<Vec<_>>()))
            .collect();
        let mut out = Vec::with_capacity(rows.len());
        for (handle, (r, e)) in handles.into_iter().zip(rows.chunks(chunk).zip(evidence.chunks(chunk))) {
            match handle.join() {
                Ok(part) => out.extend(part),
                // A thread that failed: its share is worked out here instead.
                Err(_) => out.extend(r.iter().zip(e).map(one)),
            }
        }
        out
    })
}

/// The layout's input: each item drawn [`PULL`] of the way towards its kind's centre and moved
/// [`SEPARATION`] along a direction of its kind's own, so that the kinds form regions of their
/// own on the map and an item of one kind cannot sit inside another's. Items of no kind stay
/// where their sound puts them.
pub fn layout_rows(rows: &[Features], kinds: &[Kind]) -> Vec<Features> {
    let typical = typical_norm(rows);
    let mut centres = vec![([0.0f32; FEATURE_COUNT], 0usize); Kind::ALL.len()];
    for (row, kind) in rows.iter().zip(kinds) {
        if let Some((sum, count)) = centres.get_mut(*kind as usize) {
            for (s, v) in sum.iter_mut().zip(row) {
                *s += v;
            }
            *count += 1;
        }
    }
    for (sum, count) in centres.iter_mut() {
        let c = (*count).max(1) as f32;
        sum.iter_mut().for_each(|s| *s /= c);
    }
    let directions = directions();
    rows.iter()
        .zip(kinds)
        .map(|(row, kind)| {
            let (Some((centre, _)), Some(direction)) = (centres.get(*kind as usize), directions.get(*kind as usize))
            else {
                return *row;
            };
            if *kind == Kind::Other {
                return *row;
            }
            let mut out = *row;
            for ((o, c), d) in out.iter_mut().zip(centre).zip(direction) {
                *o += PULL * (c - *o) + SEPARATION * typical * d;
            }
            out
        })
        .collect()
}

/// A unit direction per kind, all at right angles to each other: seeded vectors made
/// orthonormal (Gram-Schmidt), the same on every run.
fn directions() -> Vec<Features> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let mut out: Vec<Features> = Vec::with_capacity(Kind::ALL.len());
    for _ in Kind::ALL {
        let mut v = [0.0f32; FEATURE_COUNT];
        v.iter_mut().for_each(|x| *x = next());
        for u in &out {
            let dot: f32 = v.iter().zip(u).map(|(a, b)| a * b).sum();
            v.iter_mut().zip(u).for_each(|(a, b)| *a -= dot * b);
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        v.iter_mut().for_each(|x| *x /= norm);
        out.push(v);
    }
    out
}

/// The root mean square distance of standardised rows from their centre (the origin).
fn typical_norm(rows: &[Features]) -> f32 {
    let n = rows.len().max(1) as f32;
    (rows.iter().map(|r| r.iter().map(|v| v * v).sum::<f32>()).sum::<f32>() / n).sqrt()
}

fn distance(a: &Features, b: &Features) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt()
}

/// The distance from `row` to the nearest of `prototypes`.
fn nearest(prototypes: &[Features], row: &Features) -> f32 {
    prototypes.iter().map(|p| distance(p, row)).fold(f32::INFINITY, f32::min)
}

/// `count` centres of `examples` by k-means, started from points spread over them (each the
/// example furthest from those chosen, among the nearer nine tenths, so an odd one out is not a
/// centre of its own); centres left holding less than [`MIN_PROTOTYPE_SHARE`] of the examples
/// are dropped. The same examples always give the same centres.
fn kmeans(examples: &[&Features], count: usize) -> Vec<Features> {
    let Some(first) = examples.first() else { return Vec::new() };
    let n = examples.len() as f32;
    let mut mean = [0.0f32; FEATURE_COUNT];
    for e in examples {
        mean.iter_mut().zip(e.iter()).for_each(|(m, v)| *m += v / n);
    }
    let mut from_mean: Vec<f32> = examples.iter().map(|e| distance(e, &mean)).collect();
    from_mean.sort_by(f32::total_cmp);
    let limit = from_mean.get((from_mean.len() * 9 / 10).min(from_mean.len() - 1)).copied().unwrap_or(f32::MAX);
    let core: Vec<&Features> = examples.iter().copied().filter(|e| distance(e, &mean) <= limit).collect();
    let start = core
        .iter()
        .min_by(|a, b| distance(a, &mean).total_cmp(&distance(b, &mean)))
        .copied()
        .copied()
        .unwrap_or(**first);
    let mut centres = vec![start];
    while centres.len() < count {
        let far = core.iter().max_by(|a, b| nearest(&centres, a).total_cmp(&nearest(&centres, b)));
        match far {
            Some(far) if nearest(&centres, far) > 0.0 => centres.push(**far),
            _ => break,
        }
    }
    let mut held = vec![0usize; centres.len()];
    for _ in 0..KMEANS_ROUNDS {
        let mut sums = vec![([0.0f32; FEATURE_COUNT], 0usize); centres.len()];
        for e in examples {
            let closest = centres
                .iter()
                .enumerate()
                .min_by(|a, b| distance(a.1, e).total_cmp(&distance(b.1, e)))
                .map_or(0, |(i, _)| i);
            if let Some((sum, c)) = sums.get_mut(closest) {
                sum.iter_mut().zip(e.iter()).for_each(|(s, v)| *s += v);
                *c += 1;
            }
        }
        held = sums.iter().map(|(_, c)| *c).collect();
        for (centre, (sum, c)) in centres.iter_mut().zip(sums) {
            if c > 0 {
                centre.iter_mut().zip(sum).for_each(|(x, s)| *x = s / c as f32);
            }
        }
    }
    let largest = held.iter().copied().max().unwrap_or(0);
    let enough = (MIN_PROTOTYPE_SHARE * n).ceil() as usize;
    centres.into_iter().zip(held).filter(|(_, c)| *c >= enough || *c == largest).map(|(centre, _)| centre).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(state: &mut u32) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        *state as f32 / u32::MAX as f32 - 0.5
    }

    /// `per` rows around each of three centres far apart.
    fn three(per: usize) -> Vec<Features> {
        let mut state = 5u32;
        let mut rows = Vec::new();
        for c in 0..3 {
            for _ in 0..per {
                let mut row = [0.0f32; FEATURE_COUNT];
                for (i, v) in row.iter_mut().enumerate() {
                    *v = if i % 3 == c { 3.0 } else { 0.0 } + noise(&mut state) * 0.5;
                }
                rows.push(row);
            }
        }
        rows
    }

    const KINDS: [Kind; 3] = [Kind::Kick, Kind::Tom, Kind::Percussion];

    #[test]
    fn names_say_where_they_come_from() {
        let root = Path::new("/lib");
        assert_eq!(named(Path::new("/lib/Kicks/01.wav"), root), Some((Kind::Kick, Evidence::Folder)));
        assert_eq!(named(Path::new("/lib/Kicks/Snare 1.wav"), root), Some((Kind::Snare, Evidence::FileName)));
        assert_eq!(named(Path::new("/lib/Misc/01.wav"), root), None);
    }

    #[test]
    fn unnamed_items_take_the_kind_they_sound_like() {
        let rows = three(20);
        let evidence: Vec<_> = (0..60).map(|i| (i % 20 != 0).then_some((KINDS[i / 20], Evidence::FileName))).collect();
        let kinds = classify(&rows, &evidence);
        for c in 0..3 {
            assert_eq!(kinds[c * 20], (KINDS[c], From::Sound));
            assert_eq!(kinds[c * 20 + 1], (KINDS[c], From::Name));
        }
    }

    #[test]
    fn a_folder_gives_way_to_a_clear_sound_a_file_name_and_the_hand_do_not() {
        let rows = three(20);
        let mut evidence: Vec<_> = (0..60).map(|i| Some((KINDS[i / 20], Evidence::FileName))).collect();
        // Percussion sounds in a folder of kicks, named as kicks, set to kick by hand.
        evidence[40] = Some((Kind::Kick, Evidence::Folder));
        evidence[41] = Some((Kind::Kick, Evidence::FileName));
        evidence[42] = Some((Kind::Kick, Evidence::Hand));
        let kinds = classify(&rows, &evidence);
        assert_eq!(kinds[40], (Kind::Percussion, From::Sound));
        assert_eq!(kinds[41], (Kind::Kick, From::Name));
        assert_eq!(kinds[42], (Kind::Kick, From::Hand));
    }

    #[test]
    fn a_sound_like_nothing_known_is_not_guessed() {
        let mut rows = three(20);
        let mut evidence: Vec<_> = (0..60).map(|i| Some((KINDS[i / 20], Evidence::FileName))).collect();
        // Far from all three, and halfway between two.
        rows.push([20.0; FEATURE_COUNT]);
        let mut between = [0.0f32; FEATURE_COUNT];
        for (i, v) in between.iter_mut().enumerate() {
            *v = if i % 3 < 2 { 1.5 } else { 0.0 };
        }
        rows.push(between);
        evidence.extend([None, None]);
        let kinds = classify(&rows, &evidence);
        assert_eq!(kinds[60], (Kind::Other, From::Nothing));
        assert_eq!(kinds[61], (Kind::Other, From::Nothing));
        // With nothing named, nothing is learned or guessed.
        let none = classify(&rows, &vec![None; rows.len()]);
        assert!(none.iter().all(|k| *k == (Kind::Other, From::Nothing)));
        assert!(Classifier::learn(&rows, &vec![None; rows.len()]).kinds().is_empty());
    }

    #[test]
    fn too_few_file_names_let_the_folders_teach() {
        let rows = three(20);
        let evidence: Vec<_> = (0..60)
            .map(|i| match i / 20 {
                0 => Some((Kind::Kick, Evidence::Folder)),
                1 if i % 2 == 0 => Some((Kind::Tom, Evidence::FileName)),
                _ => None,
            })
            .collect();
        assert_eq!(Classifier::learn(&rows, &evidence).kinds(), vec![Kind::Kick, Kind::Tom]);
    }

    #[test]
    fn related_kinds_do_not_make_a_guess_unsure() {
        let rows = three(20);
        // The first two groups are closed and open hats, so close relatives.
        let hats = [Kind::ClosedHat, Kind::OpenHat, Kind::Kick];
        let evidence: Vec<_> = (0..60).map(|i| (i % 20 != 0).then_some((hats[i / 20], Evidence::FileName))).collect();
        let kinds = classify(&rows, &evidence);
        assert_eq!(kinds[0], (Kind::ClosedHat, From::Sound));
        // A folder of hats keeps its open hat.
        let mut evidence: Vec<_> = (0..60).map(|i| Some((hats[i / 20], Evidence::FileName))).collect();
        evidence[0] = Some((Kind::Hat, Evidence::Folder));
        assert_eq!(classify(&rows, &evidence)[0], (Kind::Hat, From::Name));
    }

    #[test]
    fn many_items_are_classified_on_several_threads_with_the_same_result() {
        let rows = three(3_000);
        let evidence: Vec<_> =
            (0..rows.len()).map(|i| (i % 7 != 0).then_some((KINDS[i / 3_000], Evidence::FileName))).collect();
        let parallel = classify(&rows, &evidence);
        let classifier = Classifier::learn(&rows, &evidence);
        let serial: Vec<_> = rows.iter().zip(&evidence).map(|(r, e)| classifier.kind_of(r, *e)).collect();
        assert_eq!(parallel, serial);
        assert!(parallel.iter().enumerate().all(|(i, (k, _))| *k == KINDS[i / 3_000]));
    }

    #[test]
    fn the_layout_input_keeps_kinds_apart() {
        let rows = three(10);
        // Every item of the first group but one called a tom: they sit with the toms now.
        let mut kinds: Vec<Kind> = (0..30).map(|i| KINDS[i / 10]).collect();
        kinds[0] = Kind::Tom;
        kinds[29] = Kind::Other;
        let pulled = layout_rows(&rows, &kinds);
        assert_eq!(pulled.len(), 30);
        assert_eq!(pulled[29], rows[29]);
        let nearest_other = |i: usize| {
            (0..29)
                .filter(|&j| j != i)
                .min_by(|&a, &b| distance(&pulled[a], &pulled[i]).total_cmp(&distance(&pulled[b], &pulled[i])))
                .unwrap()
        };
        assert_eq!(kinds[nearest_other(0)], Kind::Tom);
        for i in 1..29 {
            assert_eq!(kinds[nearest_other(i)], kinds[i], "item {i}");
        }
        let d = directions();
        for (i, a) in d.iter().enumerate() {
            for (j, b) in d.iter().enumerate() {
                let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
                assert!((dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn the_kinds_of_150_000_items_are_found_quickly() {
        let n = 150_000;
        let mut state = 11u32;
        let rows: Vec<Features> = (0..n)
            .map(|i| {
                let mut row = [0.0f32; FEATURE_COUNT];
                for (k, v) in row.iter_mut().enumerate() {
                    *v = ((i % 12) * (k % 5)) as f32 * 0.1 + noise(&mut state);
                }
                row
            })
            .collect();
        let evidence: Vec<_> = (0..n)
            .map(|i| match i % 4 {
                0 => Some((Kind::ALL[i % 12], Evidence::FileName)),
                1 => Some((Kind::ALL[(i + 3) % 12], Evidence::Folder)),
                _ => None,
            })
            .collect();
        let started = std::time::Instant::now();
        let kinds = classify(&rows, &evidence);
        assert_eq!(kinds.len(), n);
        // Well under a second in a release build (ADR 0013 › Kinds); a debug build is many
        // times slower.
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", started.elapsed());
    }
}
