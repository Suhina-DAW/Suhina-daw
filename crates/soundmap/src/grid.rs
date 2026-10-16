// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Snapping a layout to a grid, one item per cell, keeping neighbours together (ADR 0013 ›
//! Grid): recursive splitting of the points and the cells down to parts small enough to
//! assign exactly.

/// Parts of at most this many items are assigned exactly.
pub const EXACT_ITEMS: usize = 256;

/// A grid's size and where each item went.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub columns: usize,
    pub rows: usize,
    /// The cell `(column, row)` of each item, in the order the points were given.
    pub cells: Vec<(usize, usize)>,
}

/// Snaps `points` (0–1 in both axes) to a grid whose shape is close to `aspect`
/// (width / height).
pub fn snap(points: &[[f32; 2]], aspect: f32) -> Grid {
    let n = points.len();
    if n == 0 {
        return Grid { columns: 0, rows: 0, cells: Vec::new() };
    }
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.1, 10.0) } else { 1.0 };
    let columns = ((n as f32 * aspect).sqrt().ceil() as usize).clamp(1, n);
    let rows = n.div_ceil(columns);
    let mut cells = vec![(0, 0); n];
    let items: Vec<usize> = (0..n).collect();
    let part = Part { items, columns: (0, columns), rows: (0, rows) };
    assign(points, part, &mut cells);
    Grid { columns, rows, cells }
}

struct Part {
    items: Vec<usize>,
    columns: (usize, usize),
    rows: (usize, usize),
}

impl Part {
    fn width(&self) -> usize {
        self.columns.1 - self.columns.0
    }

    fn height(&self) -> usize {
        self.rows.1 - self.rows.0
    }

    fn capacity(&self) -> usize {
        self.width() * self.height()
    }
}

fn assign(points: &[[f32; 2]], part: Part, out: &mut [(usize, usize)]) {
    if part.items.is_empty() {
        return;
    }
    if part.items.len() <= EXACT_ITEMS || part.capacity() <= 1 {
        assign_exactly(points, &part, out);
        return;
    }
    // Split the cells along the longer side and the points along the same axis, giving each
    // side as many points as its share of the cells, within its capacity.
    let axis = if part.width() >= part.height() { 0 } else { 1 };
    let (first, second) = if axis == 0 {
        let middle = part.columns.0 + part.width() / 2;
        (
            Part { items: Vec::new(), columns: (part.columns.0, middle), rows: part.rows },
            Part { items: Vec::new(), columns: (middle, part.columns.1), rows: part.rows },
        )
    } else {
        let middle = part.rows.0 + part.height() / 2;
        (
            Part { items: Vec::new(), columns: part.columns, rows: (part.rows.0, middle) },
            Part { items: Vec::new(), columns: part.columns, rows: (middle, part.rows.1) },
        )
    };
    let total = part.items.len();
    let share = (total * first.capacity() + part.capacity() / 2) / part.capacity().max(1);
    let share = share.clamp(total.saturating_sub(second.capacity()), first.capacity().min(total));
    let mut items = part.items;
    items.sort_by(|a, b| {
        let pa = points.get(*a).map_or(0.0, |p| p[axis]);
        let pb = points.get(*b).map_or(0.0, |p| p[axis]);
        pa.total_cmp(&pb).then(a.cmp(b))
    });
    let rest = items.split_off(share);
    assign(points, Part { items, ..first }, out);
    assign(points, Part { items: rest, ..second }, out);
}

/// Assigns a part's items to its cells by least total squared distance, after stretching the
/// items' bounding box over the part's cells.
fn assign_exactly(points: &[[f32; 2]], part: &Part, out: &mut [(usize, usize)]) {
    let cells: Vec<(usize, usize)> =
        (part.rows.0..part.rows.1).flat_map(|r| (part.columns.0..part.columns.1).map(move |c| (c, r))).collect();
    let mine: Vec<[f32; 2]> = part.items.iter().map(|i| points.get(*i).copied().unwrap_or([0.5, 0.5])).collect();
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &mine {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    let stretch = |p: [f32; 2], axis: usize, from: usize, length: usize| -> f64 {
        let span = max[axis] - min[axis];
        let t = if span > 1e-12 { (p[axis] - min[axis]) / span } else { 0.5 };
        from as f64 + f64::from(t) * length as f64
    };
    let costs: Vec<Vec<f64>> = mine
        .iter()
        .map(|p| {
            let x = stretch(*p, 0, part.columns.0, part.width());
            let y = stretch(*p, 1, part.rows.0, part.height());
            cells.iter().map(|(c, r)| (x - (*c as f64 + 0.5)).powi(2) + (y - (*r as f64 + 0.5)).powi(2)).collect()
        })
        .collect();
    let chosen = hungarian(&costs);
    for (item, cell) in part.items.iter().zip(chosen) {
        if let (Some(slot), Some(cell)) = (out.get_mut(*item), cells.get(cell)) {
            *slot = *cell;
        }
    }
}

/// The least-cost assignment of every row to a distinct column (rows ≤ columns), by the
/// Hungarian method with potentials. Returns the column of each row.
fn hungarian(costs: &[Vec<f64>]) -> Vec<usize> {
    let n = costs.len();
    let m = costs.first().map_or(0, Vec::len);
    if n == 0 || m < n {
        return (0..n).collect();
    }
    let cost = |i: usize, j: usize| costs.get(i - 1).and_then(|r| r.get(j - 1)).copied().unwrap_or(f64::MAX / 4.0);
    // 1-based, with column 0 as the free start.
    let mut u = vec![0.0f64; n + 1];
    let mut v = vec![0.0f64; m + 1];
    let mut matched = vec![0usize; m + 1];
    let mut way = vec![0usize; m + 1];
    for i in 1..=n {
        matched[0] = i;
        let mut j0 = 0;
        let mut min = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = matched[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=m {
                if !used[j] {
                    let current = cost(i0, j) - u[i0] - v[j];
                    if current < min[j] {
                        min[j] = current;
                        way[j] = j0;
                    }
                    if min[j] < delta {
                        delta = min[j];
                        j1 = j;
                    }
                }
            }
            if j1 == 0 {
                break;
            }
            for j in 0..=m {
                if used[j] {
                    u[matched[j]] += delta;
                    v[j] -= delta;
                } else {
                    min[j] -= delta;
                }
            }
            j0 = j1;
            if matched[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            matched[j0] = matched[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = vec![0usize; n];
    for (j, row) in matched.iter().enumerate().skip(1) {
        if let Some(slot) = row.checked_sub(1).and_then(|i| out.get_mut(i)) {
            *slot = j - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn scattered(n: usize) -> Vec<[f32; 2]> {
        let mut state = 99u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };
        (0..n).map(|_| [next(), next()]).collect()
    }

    fn check(points: &[[f32; 2]], aspect: f32) -> Grid {
        let grid = snap(points, aspect);
        assert_eq!(grid.cells.len(), points.len());
        assert!(grid.columns * grid.rows >= points.len());
        let distinct: HashSet<_> = grid.cells.iter().collect();
        assert_eq!(distinct.len(), points.len(), "two items share a cell");
        assert!(grid.cells.iter().all(|(c, r)| *c < grid.columns && *r < grid.rows));
        grid
    }

    #[test]
    fn every_item_gets_its_own_cell() {
        for n in [1, 2, 3, 7, 100, 257, 1000, 3000] {
            check(&scattered(n), 1.0);
        }
        check(&scattered(500), 2.5);
        check(&vec![[0.5, 0.5]; 300], 1.0);
        assert_eq!(snap(&[], 1.0).cells.len(), 0);
        check(&scattered(10), f32::NAN);
    }

    #[test]
    fn neighbours_stay_near() {
        // Points already on a regular lattice keep their order.
        let points: Vec<[f32; 2]> = (0..100).map(|i| [(i % 10) as f32 / 9.0, (i / 10) as f32 / 9.0]).collect();
        let grid = check(&points, 1.0);
        for (i, cell) in grid.cells.iter().enumerate() {
            assert_eq!(*cell, (i % 10, i / 10));
        }
        // Points in the left half of a large scattered cloud land mostly in the left half.
        let points = scattered(2000);
        let grid = check(&points, 1.0);
        let left = points
            .iter()
            .zip(&grid.cells)
            .filter(|(p, _)| p[0] < 0.4)
            .filter(|(_, c)| (c.0 as f32) < grid.columns as f32 * 0.5)
            .count();
        let all_left = points.iter().filter(|p| p[0] < 0.4).count();
        assert!(left as f32 > all_left as f32 * 0.95, "{left} of {all_left}");
    }

    #[test]
    fn hungarian_finds_the_least_cost() {
        let costs = vec![vec![4.0, 1.0, 3.0], vec![2.0, 0.0, 5.0], vec![3.0, 2.0, 2.0]];
        assert_eq!(hungarian(&costs), vec![1, 0, 2]);
        let wide = vec![vec![9.0, 1.0, 9.0, 9.0], vec![9.0, 1.0, 2.0, 9.0]];
        assert_eq!(hungarian(&wide), vec![1, 2]);
    }
}
