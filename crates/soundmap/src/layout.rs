//! From features to positions: standardising, PCA, t-SNE, similar items and guessed kinds
//! (ADR 0013 › Distances, Layout, Kinds).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bhtsne::tSNE;

use crate::features::{FEATURE_COUNT, Features, GROUPS};
use crate::kinds::{Kind, Source};

/// Dimensions kept by PCA before t-SNE.
const PCA_DIMENSIONS: usize = 20;
const PERPLEXITY: usize = 30;
const EPOCHS: usize = 1000;
const THETA: f32 = 0.5;
/// Below this many items t-SNE says little; the first two principal components are used.
const MIN_TSNE_ITEMS: usize = 16;
/// Neighbours asked when a kind is guessed, and how many of them must agree.
const GUESS_NEIGHBOURS: usize = 7;
const GUESS_AGREE: usize = 4;
/// How many of the neighbours must agree to overrule a kind that came from a folder's name.
const OVERRULE_AGREE: usize = 5;
/// Items per cell of the grid the neighbours are looked up in, on average.
const ITEMS_PER_CELL: usize = 4;

/// How far a layout is, shared between the thread that computes it and the one that shows it.
/// Clones share the same count.
#[derive(Debug, Clone, Default)]
pub struct Progress(Arc<AtomicU32>);

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// From 0 to 1: the share of t-SNE's epochs done. It stays at 0 while the neighbours are
    /// worked out, before the first epoch.
    pub fn fraction(&self) -> f32 {
        (self.0.load(Ordering::Relaxed) as f32 / EPOCHS as f32).clamp(0.0, 1.0)
    }

    fn set(&self, epochs_done: usize) {
        self.0.store(u32::try_from(epochs_done).unwrap_or(u32::MAX), Ordering::Relaxed);
    }
}

/// Features standardised across a scope: each to mean 0 and deviation 1, each group weighted
/// so that it counts as much as any other whatever its size.
#[derive(Debug, Clone, PartialEq)]
pub struct Standardised {
    rows: Vec<Features>,
}

impl Standardised {
    pub fn new(features: &[Features]) -> Self {
        let n = features.len().max(1) as f32;
        let mut mean = [0.0f32; FEATURE_COUNT];
        let mut deviation = [0.0f32; FEATURE_COUNT];
        for row in features {
            for (m, v) in mean.iter_mut().zip(row) {
                *m += v / n;
            }
        }
        for row in features {
            for ((d, m), v) in deviation.iter_mut().zip(&mean).zip(row) {
                *d += (v - m) * (v - m) / n;
            }
        }
        let mut weight = [0.0f32; FEATURE_COUNT];
        for group in GROUPS {
            let w = 1.0 / (group.len().max(1) as f32).sqrt();
            for slot in weight.get_mut(group).into_iter().flatten() {
                *slot = w;
            }
        }
        let rows = features
            .iter()
            .map(|row| {
                let mut out = [0.0f32; FEATURE_COUNT];
                for (i, slot) in out.iter_mut().enumerate() {
                    let (v, m, d, w) = (row[i], mean[i], deviation[i].sqrt(), weight[i]);
                    *slot = if d > 1e-9 { (v - m) / d * w } else { 0.0 };
                }
                out
            })
            .collect();
        Standardised { rows }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The `count` items nearest to item `index`, nearest first, with their distances.
    pub fn similar(&self, index: usize, count: usize) -> Vec<(usize, f32)> {
        let Some(from) = self.rows.get(index) else { return Vec::new() };
        let mut all: Vec<(usize, f32)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(i, row)| (i, distance(from, row)))
            .collect();
        let count = count.min(all.len());
        if count == 0 {
            return Vec::new();
        }
        all.select_nth_unstable_by(count - 1, |a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        all.truncate(count);
        all.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        all
    }

    /// Kinds for the items whose name gives none: the kind most of their nearest named
    /// neighbours have, when enough agree. Returns the kind per item and whether it was guessed.
    pub fn guess_kinds(&self, named: &[Option<Kind>]) -> Vec<(Kind, bool)> {
        let labelled: Vec<usize> = named.iter().enumerate().filter(|(_, k)| k.is_some()).map(|(i, _)| i).collect();
        named
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                if let Some(kind) = kind {
                    return (*kind, false);
                }
                let Some(from) = self.rows.get(i) else { return (Kind::Other, false) };
                let mut nearest: Vec<(f32, Kind)> = labelled
                    .iter()
                    .filter_map(|&j| Some((distance(from, self.rows.get(j)?), (*named.get(j)?)?)))
                    .collect();
                nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
                nearest.truncate(GUESS_NEIGHBOURS);
                let mut best = (0usize, Kind::Other);
                for kind in Kind::ALL {
                    let votes = nearest.iter().filter(|(_, k)| *k == kind).count();
                    if votes > best.0 {
                        best = (votes, kind);
                    }
                }
                if best.0 >= GUESS_AGREE { (best.1, true) } else { (Kind::Other, false) }
            })
            .collect()
    }
}

/// Kinds for every item from the names and the layout (ADR 0013 › Kinds). A kind from the
/// file's own name stands. A kind from a folder stands unless at least [`OVERRULE_AGREE`] of the
/// item's [`GUESS_NEIGHBOURS`] nearest file-named neighbours agree on another one; an item with
/// no kind takes the one at least [`GUESS_AGREE`] of them agree on. Neighbours are found on the
/// map, where t-SNE keeps similar sounds together, through a grid of cells, so the cost grows
/// with the number of items rather than its square. Returns the kind per item and whether it was
/// guessed from the sound.
pub fn kinds_from_neighbours(positions: &[[f32; 2]], named: &[Option<(Kind, Source)>]) -> Vec<(Kind, bool)> {
    let from_file = |k: &Option<(Kind, Source)>| matches!(k, Some((_, Source::FileName)));
    let mut anchors: Vec<usize> = (0..named.len()).filter(|&i| named.get(i).is_some_and(from_file)).collect();
    if anchors.len() < GUESS_NEIGHBOURS {
        // Too few file names to go by: the folders' kinds teach as well.
        anchors = (0..named.len()).filter(|&i| named.get(i).is_some_and(Option::is_some)).collect();
    }
    let grid = Cells::new(positions, &anchors);
    named
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if let Some((kind, Source::FileName)) = name {
                return (*kind, false);
            }
            let Some(at) = positions.get(i) else { return (Kind::Other, false) };
            let near = grid.nearest(positions, *at, i, GUESS_NEIGHBOURS);
            let mut best = (0usize, Kind::Other);
            for kind in Kind::ALL {
                let votes =
                    near.iter().filter(|&&j| named.get(j).and_then(|k| k.map(|(k, _)| k)) == Some(kind)).count();
                if votes > best.0 {
                    best = (votes, kind);
                }
            }
            match name {
                Some((kind, _)) if best.1 != *kind && best.0 >= OVERRULE_AGREE => (best.1, true),
                Some((kind, _)) => (*kind, false),
                None if best.0 >= GUESS_AGREE => (best.1, true),
                None => (Kind::Other, false),
            }
        })
        .collect()
}

/// Items bucketed by where they are on the map, to find the nearest ones quickly.
struct Cells {
    side: usize,
    cells: Vec<Vec<usize>>,
}

impl Cells {
    fn new(positions: &[[f32; 2]], items: &[usize]) -> Self {
        let side = ((items.len() / ITEMS_PER_CELL).max(1) as f64).sqrt().ceil().clamp(1.0, 1024.0) as usize;
        let mut cells = vec![Vec::new(); side * side];
        let grid = Cells { side, cells: Vec::new() };
        for &i in items {
            if let Some(p) = positions.get(i)
                && let Some(cell) = cells.get_mut(grid.index(*p))
            {
                cell.push(i);
            }
        }
        Cells { side, cells }
    }

    fn cell_of(&self, value: f32) -> usize {
        let v = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.5 };
        ((v * self.side as f32) as usize).min(self.side - 1)
    }

    fn index(&self, p: [f32; 2]) -> usize {
        self.cell_of(p[1]) * self.side + self.cell_of(p[0])
    }

    /// The `count` items nearest to `at`, leaving out `skip`: rings of cells around `at` are
    /// searched until no unsearched cell can hold a nearer item.
    fn nearest(&self, positions: &[[f32; 2]], at: [f32; 2], skip: usize, count: usize) -> Vec<usize> {
        let (cx, cy) = (self.cell_of(at[0]) as isize, self.cell_of(at[1]) as isize);
        let cell = 1.0 / self.side as f32;
        let mut found: Vec<(f32, usize)> = Vec::new();
        for ring in 0..self.side as isize {
            for y in cy - ring..=cy + ring {
                for x in cx - ring..=cx + ring {
                    let on_ring = (y - cy).abs() == ring || (x - cx).abs() == ring;
                    if !on_ring || x < 0 || y < 0 || x >= self.side as isize || y >= self.side as isize {
                        continue;
                    }
                    let Some(items) = self.cells.get(y as usize * self.side + x as usize) else { continue };
                    for &j in items {
                        if j == skip {
                            continue;
                        }
                        if let Some(p) = positions.get(j) {
                            let d = (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2);
                            found.push((d, j));
                        }
                    }
                }
            }
            // Every cell outside this ring is at least `ring` cells away.
            let reach = ring as f32 * cell;
            if found.len() >= count {
                found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                found.truncate(count);
                if found.last().is_some_and(|(d, _)| *d <= reach * reach) {
                    break;
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        found.truncate(count);
        found.into_iter().map(|(_, j)| j).collect()
    }
}

fn distance(a: &Features, b: &Features) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt()
}

/// Positions in 0–1 for every item. `seed` comes from the scope; the same input gives the same
/// map, up to t-SNE's parallel sums, whose last digits follow the thread schedule (ADR 0013).
pub fn layout(standardised: &Standardised, seed: u64) -> Vec<[f32; 2]> {
    layout_with_progress(standardised, seed, &Progress::new())
}

/// [`layout`], counting t-SNE's epochs in `progress` as they are done.
pub fn layout_with_progress(standardised: &Standardised, seed: u64, progress: &Progress) -> Vec<[f32; 2]> {
    let n = standardised.len();
    match n {
        0 => return Vec::new(),
        1 => return vec![[0.5, 0.5]],
        _ => {}
    }
    let projected = pca(&standardised.rows, PCA_DIMENSIONS);
    let positions = if n >= MIN_TSNE_ITEMS {
        tsne(&projected, seed, progress).unwrap_or_else(|| first_two(&projected))
    } else {
        first_two(&projected)
    };
    progress.set(EPOCHS);
    normalise(positions)
}

fn first_two(rows: &[Vec<f32>]) -> Vec<[f32; 2]> {
    rows.iter().map(|r| [r.first().copied().unwrap_or(0.0), r.get(1).copied().unwrap_or(0.0)]).collect()
}

fn tsne(rows: &[Vec<f32>], seed: u64, progress: &Progress) -> Option<Vec<[f32; 2]>> {
    let n = rows.len();
    let perplexity = PERPLEXITY.min((n - 1) / 3).max(1);
    // A seeded start, so the same scope gives the same map: the first two principal
    // components, scaled small as t-SNE expects, with a little seeded jitter to break ties.
    let mut state = seed | 1;
    let mut jitter = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let start = first_two(rows);
    let scale = start.iter().flat_map(|p| p.iter()).fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    let initial: Vec<f32> = start
        .iter()
        .flat_map(|p| [p[0] / scale * 1e-2 + jitter() * 1e-4, p[1] / scale * 1e-2 + jitter() * 1e-4])
        .collect();
    let vectors: Vec<&[f32]> = rows.iter().map(Vec::as_slice).collect();
    // The crate checks its inputs with assertions; ours are checked above, and a panic left in
    // it must not take the caller down.
    let counter = progress.clone();
    let result = std::panic::catch_unwind(|| {
        let mut fit: tSNE<f32, &[f32]> = tSNE::new(&vectors);
        fit.perplexity(perplexity as f32)
            .epochs(EPOCHS)
            .epoch_callback(move |epoch, _| counter.set(epoch + 1))
            .initial_embedding(initial)
            .barnes_hut(THETA, |a, b| a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt());
        fit.embedding()
    })
    .ok()?;
    if result.len() != n * 2 || result.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(result.as_chunks::<2>().0.to_vec())
}

/// Scales positions to fit 0–1 in both axes, keeping their aspect ratio, centred.
fn normalise(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &points {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    let span = (max[0] - min[0]).max(max[1] - min[1]);
    if span.is_nan() || span <= 1e-12 {
        return points.iter().map(|_| [0.5, 0.5]).collect();
    }
    let offset = [(span - (max[0] - min[0])) / 2.0, (span - (max[1] - min[1])) / 2.0];
    points.iter().map(|p| [(p[0] - min[0] + offset[0]) / span, (p[1] - min[1] + offset[1]) / span]).collect()
}

/// Projects rows (already centred) onto their first `keep` principal components.
fn pca(rows: &[Features], keep: usize) -> Vec<Vec<f32>> {
    let d = FEATURE_COUNT;
    let n = rows.len().max(1) as f64;
    let mut covariance = vec![vec![0.0f64; d]; d];
    for row in rows {
        for (i, line) in covariance.iter_mut().enumerate() {
            for (j, slot) in line.iter_mut().enumerate() {
                *slot += f64::from(row[i]) * f64::from(row[j]) / n;
            }
        }
    }
    let (values, vectors) = jacobi_eigen(covariance);
    let mut order: Vec<usize> = (0..d).collect();
    order.sort_by(|a, b| values[*b].total_cmp(&values[*a]).then(a.cmp(b)));
    let components: Vec<Vec<f64>> = order
        .iter()
        .take(keep)
        .map(|&c| {
            let mut v: Vec<f64> = vectors.iter().map(|row| row[c]).collect();
            // A fixed sign, so the layout does not flip between runs or platforms.
            let largest = v.iter().copied().fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m });
            if largest < 0.0 {
                v.iter_mut().for_each(|x| *x = -*x);
            }
            v
        })
        .collect();
    rows.iter()
        .map(|row| {
            components.iter().map(|c| c.iter().zip(row).map(|(a, b)| a * f64::from(*b)).sum::<f64>() as f32).collect()
        })
        .collect()
}

/// Eigenvalues and eigenvectors (as columns) of a symmetric matrix, by cyclic Jacobi rotations.
fn jacobi_eigen(mut a: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let d = a.len();
    let mut v: Vec<Vec<f64>> = (0..d).map(|i| (0..d).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for _sweep in 0..64 {
        let off: f64 = (0..d)
            .flat_map(|i| (0..d).filter(move |j| *j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        if off < 1e-18 {
            break;
        }
        for p in 0..d {
            for q in p + 1..d {
                if a[p][q].abs() < 1e-30 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in a.iter_mut() {
                    let (akp, akq) = (row[p], row[q]);
                    row[p] = c * akp - s * akq;
                    row[q] = s * akp + c * akq;
                }
                let (row_p, row_q) = (a[p].clone(), a[q].clone());
                for (k, (apk, aqk)) in row_p.into_iter().zip(row_q).enumerate() {
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ((0..d).map(|i| a[i][i]).collect(), v)
}

#[cfg(test)]
mod tests {

    /// Two groups on the map: kicks on the left, snares on the right.
    type Named = Option<(Kind, Source)>;

    fn kicks_and_snares() -> (Vec<[f32; 2]>, Vec<Named>) {
        let mut positions = Vec::new();
        let mut named = Vec::new();
        for i in 0..20 {
            let jitter = i as f32 * 0.002;
            positions.push([0.1 + jitter, 0.5]);
            named.push(Some((Kind::Kick, Source::FileName)));
            positions.push([0.9 - jitter, 0.5]);
            named.push(Some((Kind::Snare, Source::FileName)));
        }
        (positions, named)
    }

    #[test]
    fn unnamed_items_take_the_kind_of_their_neighbours_on_the_map() {
        let (mut positions, mut named) = kicks_and_snares();
        positions.extend([[0.12, 0.5], [0.88, 0.5], [0.5, 0.5]]);
        named.extend([None, None, None]);
        let kinds = kinds_from_neighbours(&positions, &named);
        let n = kinds.len();
        assert_eq!(kinds[n - 3], (Kind::Kick, true));
        assert_eq!(kinds[n - 2], (Kind::Snare, true));
        // Halfway, the neighbours are split: no kind is guessed.
        assert_eq!(kinds[n - 1].1, kinds[n - 1].0 != Kind::Other);
        // Names from the file stand.
        assert_eq!(kinds[0], (Kind::Kick, false));
    }

    #[test]
    fn a_folders_kind_gives_way_to_a_clear_sound_but_a_file_name_does_not() {
        let (mut positions, mut named) = kicks_and_snares();
        // In a folder called "Bass", but it sits among the kicks.
        positions.push([0.11, 0.5]);
        named.push(Some((Kind::Bass, Source::Folder)));
        // Named "Bass" in its own file name, among the kicks too.
        positions.push([0.11, 0.51]);
        named.push(Some((Kind::Bass, Source::FileName)));
        let kinds = kinds_from_neighbours(&positions, &named);
        let n = kinds.len();
        assert_eq!(kinds[n - 2], (Kind::Kick, true));
        assert_eq!(kinds[n - 1], (Kind::Bass, false));
    }

    #[test]
    fn kinds_of_many_items_are_found_quickly() {
        let n = 150_000;
        let mut state = 9u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32
        };
        let positions: Vec<[f32; 2]> = (0..n).map(|_| [next(), next()]).collect();
        let named: Vec<Option<(Kind, Source)>> = (0..n)
            .map(|i| match i % 3 {
                0 => Some((Kind::Kick, Source::FileName)),
                1 => Some((Kind::Hat, Source::Folder)),
                _ => None,
            })
            .collect();
        let started = std::time::Instant::now();
        let kinds = kinds_from_neighbours(&positions, &named);
        assert_eq!(kinds.len(), n);
        // Brute force took minutes for this many; the grid takes well under a few seconds even
        // in a debug build.
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
    }

    use super::*;

    /// Items in three tight clusters far apart in feature space.
    fn clusters(per: usize) -> (Vec<Features>, Vec<usize>) {
        let mut rows = Vec::new();
        let mut labels = Vec::new();
        let mut state = 7u32;
        let mut noise = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32 - 0.5
        };
        for cluster in 0..3 {
            for _ in 0..per {
                let mut row = [0.0f32; FEATURE_COUNT];
                for (i, v) in row.iter_mut().enumerate() {
                    let centre = if (i + cluster * 5) % 3 == 0 { 10.0 } else { 0.0 };
                    *v = centre + noise();
                }
                rows.push(row);
                labels.push(cluster);
            }
        }
        (rows, labels)
    }

    #[test]
    fn standardising_centres_and_scales() {
        let rows: Vec<Features> = (0..10).map(|i| [i as f32; FEATURE_COUNT]).collect();
        let s = Standardised::new(&rows);
        for f in 0..FEATURE_COUNT {
            let mean: f32 = s.rows.iter().map(|r| r[f]).sum::<f32>() / 10.0;
            assert!(mean.abs() < 1e-5);
        }
        // Every group has total variance 1, so it counts as much as any other.
        for group in GROUPS {
            let total: f32 = group.clone().map(|f| s.rows.iter().map(|r| r[f] * r[f]).sum::<f32>() / 10.0).sum();
            assert!((total - 1.0).abs() < 1e-3, "{total}");
        }
        // A feature that never changes contributes nothing.
        let flat = Standardised::new(&[[3.0; FEATURE_COUNT], [3.0; FEATURE_COUNT]]);
        assert!(flat.rows.iter().all(|r| r.iter().all(|v| *v == 0.0)));
    }

    #[test]
    fn similar_items_are_from_the_same_cluster() {
        let (rows, labels) = clusters(20);
        let s = Standardised::new(&rows);
        let similar = s.similar(0, 10);
        assert_eq!(similar.len(), 10);
        assert!(similar.iter().all(|(i, _)| labels[*i] == 0));
        assert!(similar.windows(2).all(|w| w[0].1 <= w[1].1));
        assert!(s.similar(999, 5).is_empty());
        assert_eq!(s.similar(0, 1000).len(), 59);
    }

    #[test]
    fn kinds_are_guessed_from_named_neighbours() {
        let (rows, labels) = clusters(10);
        let s = Standardised::new(&rows);
        let named: Vec<Option<Kind>> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| if i % 10 == 0 { None } else { Some([Kind::Kick, Kind::Snare, Kind::Hat][*l]) })
            .collect();
        let guessed = s.guess_kinds(&named);
        assert_eq!(guessed[0], (Kind::Kick, true));
        assert_eq!(guessed[10], (Kind::Snare, true));
        assert_eq!(guessed[20], (Kind::Hat, true));
        assert_eq!(guessed[1], (Kind::Kick, false));
        // With nothing named, nothing is guessed.
        let none = s.guess_kinds(&vec![None; rows.len()]);
        assert!(none.iter().all(|k| *k == (Kind::Other, false)));
    }

    #[test]
    fn clusters_stay_apart_on_the_map_and_the_map_is_repeatable() {
        let (rows, labels) = clusters(30);
        let s = Standardised::new(&rows);
        let a = layout(&s, 42);
        assert_eq!(a.len(), rows.len());
        assert!(a.iter().all(|p| (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])));
        // Each point's nearest point on the map is from its own cluster.
        for (i, p) in a.iter().enumerate() {
            let nearest = a
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .min_by(|x, y| {
                    let dx = (x.1[0] - p[0]).powi(2) + (x.1[1] - p[1]).powi(2);
                    let dy = (y.1[0] - p[0]).powi(2) + (y.1[1] - p[1]).powi(2);
                    dx.total_cmp(&dy)
                })
                .map(|(j, _)| j)
                .unwrap();
            assert_eq!(labels[nearest], labels[i]);
        }
        // t-SNE sums in parallel, and the order of a float sum follows the thread schedule, so
        // two runs may differ in the last digits, which its epochs can grow by any amount (the
        // sound map keeps layouts, so a scope opens with the map it had). On one thread the order
        // is fixed: the layout is then exactly a function of its input and seed.
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let (b, c) = one.install(|| (layout(&s, 42), layout(&s, 42)));
        assert_eq!(b, c);
    }

    #[test]
    fn small_scopes_are_laid_out_without_tsne() {
        assert!(layout(&Standardised::new(&[]), 1).is_empty());
        assert_eq!(layout(&Standardised::new(&[[1.0; FEATURE_COUNT]]), 1), vec![[0.5, 0.5]]);
        let (rows, _) = clusters(2);
        let positions = layout(&Standardised::new(&rows), 1);
        assert_eq!(positions.len(), 6);
        assert!(positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn pca_finds_the_main_direction() {
        // Points along (1, 1, 0, …) with a little spread in the third feature.
        let rows: Vec<Features> = (0..50)
            .map(|i| {
                let t = i as f32 - 25.0;
                let mut r = [0.0; FEATURE_COUNT];
                r[0] = t;
                r[1] = t;
                r[2] = if i % 2 == 0 { 0.1 } else { -0.1 };
                r
            })
            .collect();
        let projected = pca(&rows, 2);
        let first: Vec<f32> = projected.iter().map(|r| r[0]).collect();
        let expected = 2f32.sqrt();
        assert!((first[49] - first[48] - expected).abs() < 1e-3, "{}", first[49] - first[48]);
    }
}
