//! Square grids of `f64` and the neighbourhood operations the basin bake is
//! made of.
//!
//! Every operation here is *position-local*: the value it writes at a cell
//! depends only on the input within a fixed radius of that cell, computed in
//! an order fixed relative to the cell. Two tiles whose domains overlap
//! therefore compute the same bits for a cell both hold far enough from
//! their domain edges, which is what makes neighbouring tiles meet with no
//! seam. That rules out running sums (a sliding box sum's rounding depends
//! on where the scan started) and anything whose result depends on the
//! order cells are visited in; maxima, minima, direct window sums and
//! integer path lengths are all exact or order-fixed.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A square grid, row-major with `x` fastest: `v[j * n + i]`.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    pub n: usize,
    pub v: Vec<f64>,
}

impl Grid {
    pub fn new(n: usize, v: Vec<f64>) -> Self {
        debug_assert_eq!(v.len(), n * n);
        Self { n, v }
    }

    fn map2(&self, other: &Grid, f: impl Fn(f64, f64) -> f64) -> Grid {
        Grid::new(
            self.n,
            self.v
                .iter()
                .zip(&other.v)
                .map(|(&a, &b)| f(a, b))
                .collect(),
        )
    }

    /// Cell-wise minimum of two grids.
    pub fn min(&self, other: &Grid) -> Grid {
        self.map2(other, f64::min)
    }
}

/// The half-widths of a disk of radius `r` cells, one per row offset
/// `-r..=r`: the largest `w` with `w^2 + dy^2 <= r^2`. Integer arithmetic,
/// so the disk is the same on every machine.
fn disk_half_widths(r: usize) -> Vec<usize> {
    let r2 = r * r;
    (0..=2 * r)
        .map(|k| {
            let dy = k.abs_diff(r);
            let mut w = 0;
            while (w + 1) * (w + 1) + dy * dy <= r2 {
                w += 1;
            }
            w
        })
        .collect()
}

/// Grey dilation (`max`) or erosion (`min`) by a disk of radius `r` cells.
///
/// Near the grid's edge the disk is clipped to the grid; those cells are the
/// apron a caller discards. The row maxima for every half-width are built
/// incrementally (`m_w[x] = ext(m_{w-1}[x], v[x - w], v[x + w])`), so a disk
/// costs one lookup per row instead of one per cell.
pub(crate) fn disk_extreme(grid: &Grid, r: usize, max: bool) -> Grid {
    let n = grid.n;
    let pick = |a: f64, b: f64| if max { a.max(b) } else { a.min(b) };
    let widths = disk_half_widths(r);
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(r + 1);
    rows.push(grid.v.clone());
    for w in 1..=r {
        let prev = &rows[w - 1];
        let mut next = prev.clone();
        for j in 0..n {
            let row = &grid.v[j * n..(j + 1) * n];
            for i in 0..n {
                let mut e = prev[j * n + i];
                if i >= w {
                    e = pick(e, row[i - w]);
                }
                if i + w < n {
                    e = pick(e, row[i + w]);
                }
                next[j * n + i] = e;
            }
        }
        rows.push(next);
    }
    let mut out = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            let mut e = grid.v[j * n + i];
            for (k, &w) in widths.iter().enumerate() {
                // Row `j + k - r`, skipped where it falls off the grid.
                let Some(jj) = (j + k).checked_sub(r) else {
                    continue;
                };
                if jj >= n {
                    break;
                }
                e = pick(e, rows[w][jj * n + i]);
            }
            out[j * n + i] = e;
        }
    }
    Grid::new(n, out)
}

/// A separable box average of radius `s` cells, each output a direct sum of
/// its window in a fixed order (never a running sum) divided by the number
/// of cells the window kept on the grid.
pub(crate) fn box_blur(grid: &Grid, s: usize) -> Grid {
    let n = grid.n;
    let pass = |src: &[f64], horizontal: bool| {
        let mut out = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let (along, fixed) = if horizontal { (i, j) } else { (j, i) };
                let lo = along.saturating_sub(s);
                let hi = (along + s).min(n - 1);
                let mut sum = 0.0;
                for a in lo..=hi {
                    let idx = if horizontal {
                        fixed * n + a
                    } else {
                        a * n + fixed
                    };
                    sum += src[idx];
                }
                // Window counts are tiny; the cast is exact.
                #[allow(clippy::cast_precision_loss)]
                let count = (hi - lo + 1) as f64;
                out[j * n + i] = sum / count;
            }
        }
        out
    };
    let h = pass(&grid.v, true);
    Grid::new(n, pass(&h, false))
}

/// The length of the shortest 8-connected path from each cell to a `source`
/// cell through cells that are `passable`, in whole metres, capped at `cap`.
/// A cell that is not passable is the end of a path, never the middle of
/// one: it gets the distance of its nearest reached neighbour plus the step,
/// and what lies behind it stays at the cap.
///
/// Steps cost `cell` metres straight and `cell * 1.4142` diagonally, rounded
/// to integers first, so the result is a unique integer whatever order the
/// queue pops equal keys in. A path no longer than `cap` stays within
/// `cap / cell` cells of where it ends, which is the apron this needs.
pub(crate) fn capped_distance(
    n: usize,
    source: &[bool],
    passable: &[bool],
    cell: u32,
    cap: u32,
) -> Vec<u32> {
    // 1.41421 * cell in integer metres: 181 for 128 m cells.
    let diagonal = (cell * 141_421 + 50_000) / 100_000;
    let mut dist = vec![cap; n * n];
    let mut heap = BinaryHeap::new();
    for (idx, &s) in source.iter().enumerate() {
        if s {
            dist[idx] = 0;
            heap.push(Reverse((0u32, idx)));
        }
    }
    while let Some(Reverse((d, idx))) = heap.pop() {
        if d > dist[idx] {
            continue;
        }
        let (i, j) = (idx % n, idx / n);
        for (di, dj) in [
            (-1isize, 0isize),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let (Some(ii), Some(jj)) = (i.checked_add_signed(di), j.checked_add_signed(dj)) else {
                continue;
            };
            if ii >= n || jj >= n {
                continue;
            }
            let next = jj * n + ii;
            let step = if di != 0 && dj != 0 { diagonal } else { cell };
            let nd = d + step;
            if nd < dist[next] {
                dist[next] = nd;
                // A wall cell takes its distance, so the field climbs
                // smoothly into it, but passes nothing on.
                if passable[next] {
                    heap.push(Reverse((nd, next)));
                }
            }
        }
    }
    dist
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_is_round() {
        assert_eq!(disk_half_widths(2), vec![0, 1, 2, 1, 0]);
        assert_eq!(disk_half_widths(0), vec![0]);
    }

    #[test]
    fn dilation_of_a_spike_is_a_disk() {
        let n = 9;
        let mut v = vec![0.0; n * n];
        v[4 * n + 4] = 1.0;
        let d = disk_extreme(&Grid::new(n, v), 2, true);
        let count = d.v.iter().filter(|&&x| x > 0.5).count();
        // 1 + 3 + 5 + 3 + 1 cells for radius 2.
        assert_eq!(count, 13);
        assert!(d.v[4 * n + 6] > 0.5 && d.v[4 * n + 7] < 0.5);
    }

    #[test]
    fn blur_keeps_a_constant() {
        let g = Grid::new(5, vec![3.0; 25]);
        assert!(box_blur(&g, 2).v.iter().all(|&x| (x - 3.0).abs() < 1e-12));
    }

    #[test]
    fn distance_walks_round_walls() {
        let n = 5;
        let mut source = vec![false; n * n];
        source[0] = true;
        let mut passable = vec![true; n * n];
        // A wall down column 1 except the bottom row.
        for j in 0..4 {
            passable[j * n + 1] = false;
        }
        let d = capped_distance(n, &source, &passable, 100, 10_000);
        assert_eq!(d[n * 4], 400);
        // Down to row 3, two diagonals round the wall's foot, up three rows.
        assert_eq!(d[2], 300 + 141 + 141 + 300);
        // The wall's own cell is reached, what is behind it only round it.
        assert_eq!(d[1], 100);
        assert_eq!(d[3], 300 + 141 + 141 + 141 + 200);
    }
}
