//! Compact C1→C4 shell packing: shape catalog + pick-min-cost Embed.
//!
//! Research notes (see docs/research/olympiad-c4-shapes.md):
//! - ELK Layered/box: hierarchical + orthogonal (inspiration; not a dep)
//! - Squarified treemaps (Bruls): compact sibling packing / aspect
//! - Polyomino packing: compact placement of unequal rects
//!
//! Chat lessons: never force infinite Row (no_wrap); never glue edges;
//! prefer mid-gap short links for geometric neighbors.

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Row,
    Col,
    Grid { cols: usize },
    Cross,
    Diamond,
}

#[derive(Debug, Clone)]
pub struct Placed {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone)]
pub struct EmbedResult {
    pub shape: ShapeKind,
    pub placed: Vec<Placed>,
    pub width: f64,
    pub height: f64,
    pub cost: f64,
}

/// Consecutive-pair affinity: distance between centers of order-neighbors
/// (MinLA order already clusters linked siblings).
fn neighbor_path_cost(placed: &[Placed]) -> f64 {
    let n = placed.len();
    if n < 2 {
        return 0.0;
    }
    let mut c = 0.0;
    for i in 0..n - 1 {
        let a = &placed[i];
        let b = &placed[i + 1];
        let ax = a.x + a.w * 0.5;
        let ay = a.y + a.h * 0.5;
        let bx = b.x + b.w * 0.5;
        let by = b.y + b.h * 0.5;
        c += (ax - bx).abs() + (ay - by).abs();
    }
    c
}

fn bbox_cost(w: f64, h: f64) -> f64 {
    let area = w.max(1.0) * h.max(1.0);
    let aspect = (w.max(h) / w.min(h).max(1.0)).max(1.0);
    // Prefer near-square packs (squarified treemap insight) over ribbons.
    area * (1.0 + 0.65 * (aspect - 1.0).powi(2))
}

fn shape_penalty(shape: ShapeKind, n: usize) -> f64 {
    match shape {
        ShapeKind::Row | ShapeKind::Col => {
            if n >= 5 {
                50_000.0 // strongly discourage infinite ribbon
            } else if n >= 3 {
                5_000.0
            } else {
                0.0
            }
        }
        ShapeKind::Grid { .. } => 0.0,
        ShapeKind::Cross => {
            if n < 3 {
                400.0
            } else {
                50.0
            }
        }
        ShapeKind::Diamond => {
            if n < 4 {
                300.0
            } else {
                40.0
            }
        }
    }
}

fn embed_row(sizes: &[(f64, f64)], gap: f64) -> (Vec<Placed>, f64, f64) {
    let mut x = 0.0;
    let mut h = 0.0_f64;
    let mut out = Vec::with_capacity(sizes.len());
    for &(w, hh) in sizes {
        out.push(Placed {
            x,
            y: 0.0,
            w,
            h: hh,
        });
        x += w + gap;
        h = h.max(hh);
    }
    let width = if sizes.is_empty() { 0.0 } else { x - gap };
    (out, width.max(0.0), h)
}

fn embed_col(sizes: &[(f64, f64)], gap: f64) -> (Vec<Placed>, f64, f64) {
    let mut y = 0.0;
    let mut w = 0.0_f64;
    let mut out = Vec::with_capacity(sizes.len());
    for &(ww, h) in sizes {
        out.push(Placed {
            x: 0.0,
            y,
            w: ww,
            h,
        });
        y += h + gap;
        w = w.max(ww);
    }
    let height = if sizes.is_empty() { 0.0 } else { y - gap };
    (out, w, height.max(0.0))
}

/// Classic wrap grid with fixed column count (squarified-inspired: try several cols).
fn embed_grid(sizes: &[(f64, f64)], cols: usize, gap: f64) -> (Vec<Placed>, f64, f64) {
    let cols = cols.max(1);
    let mut col_w = vec![0.0_f64; cols];
    let mut row_h = Vec::<f64>::new();
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let c = i % cols;
        let r = i / cols;
        if row_h.len() <= r {
            row_h.push(0.0);
        }
        col_w[c] = col_w[c].max(w);
        row_h[r] = row_h[r].max(h);
    }
    let mut out = Vec::with_capacity(sizes.len());
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let c = i % cols;
        let r = i / cols;
        let x: f64 = col_w.iter().take(c).sum::<f64>() + gap * c as f64;
        let y: f64 = row_h.iter().take(r).sum::<f64>() + gap * r as f64;
        // center in cell
        let cx = x + (col_w[c] - w) * 0.5;
        let cy = y + (row_h[r] - h) * 0.5;
        out.push(Placed { x: cx, y: cy, w, h });
    }
    let width = col_w.iter().sum::<f64>() + gap * (cols.saturating_sub(1) as f64);
    let height = row_h.iter().sum::<f64>() + gap * (row_h.len().saturating_sub(1) as f64);
    (out, width, height)
}

/// Cross: center = middle of MinLA order; then N,E,S,W; overflow as grid under south.
fn embed_cross(sizes: &[(f64, f64)], gap: f64) -> (Vec<Placed>, f64, f64) {
    let n = sizes.len();
    if n == 0 {
        return (vec![], 0.0, 0.0);
    }
    if n == 1 {
        return (
            vec![Placed {
                x: 0.0,
                y: 0.0,
                w: sizes[0].0,
                h: sizes[0].1,
            }],
            sizes[0].0,
            sizes[0].1,
        );
    }
    let mid = n / 2;
    let (cw, ch) = sizes[mid];
    // Arms in order: remaining by distance from mid in π
    let mut arms: Vec<usize> = (0..n).filter(|&i| i != mid).collect();
    arms.sort_by_key(|&i| (i as i32 - mid as i32).unsigned_abs());

    let mut placed = vec![
        Placed {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        n
    ];
    // provisional center
    placed[mid] = Placed {
        x: 0.0,
        y: 0.0,
        w: cw,
        h: ch,
    };

    let mut north: Option<usize> = None;
    let mut east: Option<usize> = None;
    let mut south: Option<usize> = None;
    let mut west: Option<usize> = None;
    let mut overflow = Vec::new();
    for (k, &i) in arms.iter().enumerate() {
        match k {
            0 => north = Some(i),
            1 => east = Some(i),
            2 => south = Some(i),
            3 => west = Some(i),
            _ => overflow.push(i),
        }
    }

    let n_h = north.map(|i| sizes[i].1).unwrap_or(0.0);
    let s_h = south.map(|i| sizes[i].1).unwrap_or(0.0);
    let w_w = west.map(|i| sizes[i].0).unwrap_or(0.0);
    let e_w = east.map(|i| sizes[i].0).unwrap_or(0.0);

    let cx = w_w + if w_w > 0.0 { gap } else { 0.0 };
    let cy = n_h + if n_h > 0.0 { gap } else { 0.0 };
    placed[mid].x = cx;
    placed[mid].y = cy;

    if let Some(i) = north {
        let (w, h) = sizes[i];
        placed[i] = Placed {
            x: cx + (cw - w) * 0.5,
            y: 0.0,
            w,
            h,
        };
    }
    if let Some(i) = west {
        let (w, h) = sizes[i];
        placed[i] = Placed {
            x: 0.0,
            y: cy + (ch - h) * 0.5,
            w,
            h,
        };
    }
    if let Some(i) = east {
        let (w, h) = sizes[i];
        placed[i] = Placed {
            x: cx + cw + gap,
            y: cy + (ch - h) * 0.5,
            w,
            h,
        };
    }
    if let Some(i) = south {
        let (w, h) = sizes[i];
        placed[i] = Placed {
            x: cx + (cw - w) * 0.5,
            y: cy + ch + gap,
            w,
            h,
        };
    }

    let mut width = cx + cw + if e_w > 0.0 { gap + e_w } else { 0.0 };
    let mut height = cy + ch + if s_h > 0.0 { gap + s_h } else { 0.0 };
    width = width.max(north.map(|i| placed[i].x + placed[i].w).unwrap_or(0.0));
    height = height.max(west.map(|i| placed[i].y + placed[i].h).unwrap_or(0.0));

    if !overflow.is_empty() {
        let ov_sizes: Vec<(f64, f64)> = overflow.iter().map(|&i| sizes[i]).collect();
        let cols = (ov_sizes.len() as f64).sqrt().ceil() as usize;
        let (ov_placed, ow, oh) = embed_grid(&ov_sizes, cols.max(1), gap);
        let base_y = height + gap;
        for (k, &i) in overflow.iter().enumerate() {
            placed[i] = Placed {
                x: ov_placed[k].x,
                y: base_y + ov_placed[k].y,
                w: ov_placed[k].w,
                h: ov_placed[k].h,
            };
        }
        width = width.max(ow);
        height = base_y + oh;
    }

    (placed, width, height)
}

/// Diamond / Manhattan rings around center (slot order = MinLA order spiral).
fn embed_diamond(sizes: &[(f64, f64)], gap: f64) -> (Vec<Placed>, f64, f64) {
    let n = sizes.len();
    if n == 0 {
        return (vec![], 0.0, 0.0);
    }
    // Collect ring slots: (dx, dy) in grid units
    let mut slots: Vec<(i32, i32)> = vec![(0, 0)];
    let mut ring = 1_i32;
    while slots.len() < n {
        // walk ring perimeter
        for dx in -ring..=ring {
            let dy = ring - dx.abs();
            slots.push((dx, dy));
            if dy != 0 {
                slots.push((dx, -dy));
            }
        }
        ring += 1;
        if ring > 20 {
            break;
        }
    }
    slots.truncate(n);

    // Cell size = max child + gap
    let cell = sizes.iter().map(|(w, h)| w.max(*h)).fold(0.0_f64, f64::max) + gap;

    let mut placed = Vec::with_capacity(n);
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let (dx, dy) = slots[i];
        let cx = dx as f64 * cell;
        let cy = dy as f64 * cell;
        let x = cx - w * 0.5;
        let y = cy - h * 0.5;
        placed.push(Placed { x, y, w, h });
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);
    }
    // shift to origin
    for p in &mut placed {
        p.x -= min_x;
        p.y -= min_y;
    }
    (placed, (max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
}

fn link_cost(placed: &[Placed], weights: &[Vec<f64>]) -> f64 {
    let n = placed.len();
    if n == 0 || weights.len() != n {
        return 0.0;
    }
    let mut c = 0.0;
    for i in 0..n {
        for j in (i + 1)..n {
            let w = weights[i][j];
            if w <= 0.0 {
                continue;
            }
            let a = &placed[i];
            let b = &placed[j];
            let d = (a.x + a.w * 0.5 - (b.x + b.w * 0.5)).abs()
                + (a.y + a.h * 0.5 - (b.y + b.h * 0.5)).abs();
            // Direct links must dominate packing: heavy pairs sit close.
            c += w * d;
        }
    }
    c
}

fn finalize(
    shape: ShapeKind,
    placed: Vec<Placed>,
    width: f64,
    height: f64,
    weights: &[Vec<f64>],
) -> EmbedResult {
    // Link term MUST dominate bbox: area is O(1e6–1e7) for C4 shells, while
    // Σ m·manhattan is O(1e3–1e4). Scale links so a 1k-px move of a weight-6
    // pair (~12e6) beats typical packing-area deltas.
    let cost = bbox_cost(width, height)
        + 2_000.0 * link_cost(&placed, weights)
        + 0.01 * neighbor_path_cost(&placed)
        + shape_penalty(shape, placed.len());
    EmbedResult {
        shape,
        placed,
        width,
        height,
        cost,
    }
}

fn permute_sizes(sizes: &[(f64, f64)], order: &[usize]) -> Vec<(f64, f64)> {
    order.iter().map(|&i| sizes[i]).collect()
}

fn permute_weights(weights: &[Vec<f64>], order: &[usize]) -> Vec<Vec<f64>> {
    let n = order.len();
    let mut w = vec![vec![0.0; n]; n];
    for (ni, &i) in order.iter().enumerate() {
        for (nj, &j) in order.iter().enumerate() {
            w[ni][nj] = weights[i][j];
        }
    }
    w
}

fn embed_all_shapes(sizes: &[(f64, f64)], gap: f64, weights: &[Vec<f64>]) -> EmbedResult {
    let n = sizes.len();
    let mut cands: Vec<EmbedResult> = Vec::new();
    let (p, w, h) = embed_row(sizes, gap);
    cands.push(finalize(ShapeKind::Row, p, w, h, weights));
    let (p, w, h) = embed_col(sizes, gap);
    cands.push(finalize(ShapeKind::Col, p, w, h, weights));
    let sqrt_c = (n as f64).sqrt().ceil() as usize;
    for cols in [1, 2, 3, sqrt_c, sqrt_c + 1, n.div_ceil(2), n.min(4)] {
        let cols = cols.clamp(1, n.max(1));
        let (p, w, h) = embed_grid(sizes, cols, gap);
        cands.push(finalize(ShapeKind::Grid { cols }, p, w, h, weights));
    }
    if n >= 3 {
        let (p, w, h) = embed_cross(sizes, gap);
        cands.push(finalize(ShapeKind::Cross, p, w, h, weights));
    }
    if n >= 4 {
        let (p, w, h) = embed_diamond(sizes, gap);
        cands.push(finalize(ShapeKind::Diamond, p, w, h, weights));
    }
    cands
        .into_iter()
        .min_by(|a, b| {
            a.cost
                .partial_cmp(&b.cost)
                .unwrap_or(Ordering::Equal)
                .then_with(|| format!("{:?}", a.shape).cmp(&format!("{:?}", b.shape)))
        })
        .unwrap()
}

/// Try shape catalog; MinLA order + local swaps so heavy m(i,j) pairs sit close.
#[allow(dead_code)]
pub fn pick_best_embed(sizes: &[(f64, f64)], gap: f64) -> EmbedResult {
    let n = sizes.len();
    let zeros = vec![vec![0.0; n]; n];
    pick_best_embed_weighted(sizes, gap, &zeros)
}

pub fn pick_best_embed_weighted(
    sizes: &[(f64, f64)],
    gap: f64,
    weights: &[Vec<f64>],
) -> EmbedResult {
    let n = sizes.len();
    if n == 0 {
        return EmbedResult {
            shape: ShapeKind::Row,
            placed: vec![],
            width: 0.0,
            height: 0.0,
            cost: 0.0,
        };
    }
    let mut order: Vec<usize> = (0..n).collect();
    let sizes0 = permute_sizes(sizes, &order);
    let w0 = permute_weights(weights, &order);
    let mut best_emb = embed_all_shapes(&sizes0, gap, &w0);
    let mut best_order = order.clone();
    let mut best_cost = best_emb.cost;

    // Local search on permutation (olympiad: pull heavy direct links together).
    let rounds = if n <= 8 { n * 4 } else { n * 2 };
    for _ in 0..rounds {
        let mut improved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                order.swap(i, j);
                let s = permute_sizes(sizes, &order);
                let w = permute_weights(weights, &order);
                let emb = embed_all_shapes(&s, gap, &w);
                if emb.cost + 1e-6 < best_cost {
                    best_cost = emb.cost;
                    best_emb = emb;
                    best_order = order.clone();
                    improved = true;
                } else {
                    order.swap(i, j);
                }
            }
        }
        if !improved {
            break;
        }
        order = best_order.clone();
    }

    // Map placed slots back to original child indices.
    let mut placed_orig = vec![
        Placed {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        n
    ];
    for (slot, &orig) in best_order.iter().enumerate() {
        placed_orig[orig] = best_emb.placed[slot].clone();
    }
    best_emb.placed = placed_orig;
    best_emb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_not_infinite_row_for_many_equal() {
        let sizes: Vec<(f64, f64)> = (0..9).map(|_| (100.0, 80.0)).collect();
        let best = pick_best_embed(&sizes, 20.0);
        // Should prefer compact grid over long row
        assert!(
            best.height > 100.0,
            "expected multi-row compact pack, got {:?}",
            best.shape
        );
        assert!(
            best.width < 9.0 * 120.0,
            "width should be compact, got {}",
            best.width
        );
    }

    #[test]
    fn cross_places_center_and_arms() {
        let sizes = vec![
            (80.0, 60.0),
            (80.0, 60.0),
            (100.0, 80.0),
            (80.0, 60.0),
            (80.0, 60.0),
        ];
        let (p, w, h) = embed_cross(&sizes, 16.0);
        assert_eq!(p.len(), 5);
        assert!(w > 100.0 && h > 80.0);
        // center index 2 should not be at origin-only corner necessarily, but inside bbox
        assert!(p[2].x >= 0.0 && p[2].y >= 0.0);
    }

    #[test]
    fn huge_shells_still_pull_heavy_pair() {
        // Mimic ceph: one giant OSD-like box + smaller siblings; heavy pair must win.
        let sizes = vec![
            (420.0, 560.0),   // librados
            (2056.0, 1352.0), // mon
            (3112.0, 3872.0), // osd (giant)
            (420.0, 560.0),   // mgr
            (728.0, 776.0),   // rgw
            (1443.0, 1088.0), // rgw_log_pool
        ];
        let mut w = vec![vec![0.0; 6]; 6];
        w[4][5] = 6.0;
        w[5][4] = 6.0;
        let best = pick_best_embed_weighted(&sizes, 120.0, &w);
        let a = &best.placed[4];
        let b = &best.placed[5];
        let d = (a.x + a.w * 0.5 - (b.x + b.w * 0.5)).abs()
            + (a.y + a.h * 0.5 - (b.y + b.h * 0.5)).abs();
        // Adjacent in a gap ~ (728+1443)/2 + gap ≈ 1200; far diagonal was ~2200+.
        assert!(
            d < 1800.0,
            "rgw-like heavy pair too far under giant sibling: d={d} shape={:?}",
            best.shape
        );
    }

    #[test]
    fn heavy_pair_pulled_adjacent() {
        // 4 equal boxes; pair (0,3) has weight 10 — must sit close after pack.
        let sizes = vec![(100.0, 80.0); 4];
        let mut w = vec![vec![0.0; 4]; 4];
        w[0][3] = 10.0;
        w[3][0] = 10.0;
        let best = pick_best_embed_weighted(&sizes, 20.0, &w);
        let a = &best.placed[0];
        let b = &best.placed[3];
        let d = (a.x + a.w * 0.5 - (b.x + b.w * 0.5)).abs()
            + (a.y + a.h * 0.5 - (b.y + b.h * 0.5)).abs();
        // Far opposite corners of a 2x2 would be ~200+; adjacent ~120.
        assert!(
            d < 220.0,
            "heavy pair too far: d={d} shape={:?}",
            best.shape
        );
    }

    #[test]
    fn diamond_has_positive_bbox() {
        let sizes: Vec<(f64, f64)> = (0..7).map(|_| (60.0, 50.0)).collect();
        let (p, w, h) = embed_diamond(&sizes, 12.0);
        assert_eq!(p.len(), 7);
        assert!(w > 0.0 && h > 0.0);
    }
}
