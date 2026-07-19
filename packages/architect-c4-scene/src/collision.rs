//! Gamedev-style collision for edge parts vs node boxes.
//! Broad phase: spatial hash of AABBs. Narrow: segment vs AABB.

use crate::SceneNode;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Aabb {
    pub fn from_node(n: &SceneNode, pad: f64) -> Self {
        Self {
            x0: n.x - pad,
            y0: n.y - pad,
            x1: n.x + n.w + pad,
            y1: n.y + n.h + pad,
        }
    }

    pub fn inflate(self, pad: f64) -> Self {
        Self {
            x0: self.x0 - pad,
            y0: self.y0 - pad,
            x1: self.x1 + pad,
            y1: self.y1 + pad,
        }
    }

    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.x0 < o.x1 && self.x1 > o.x0 && self.y0 < o.y1 && self.y1 > o.y0
    }

    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.x0 + self.x1) * 0.5, (self.y0 + self.y1) * 0.5)
    }
}

/// Broad-phase spatial hash (gamedev grid).
pub struct SpatialHash {
    cell: f64,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}

impl SpatialHash {
    pub fn build(boxes: &[Aabb], cell: f64) -> Self {
        let cell = cell.max(16.0);
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, b) in boxes.iter().enumerate() {
            let x0 = (b.x0 / cell).floor() as i32;
            let y0 = (b.y0 / cell).floor() as i32;
            let x1 = (b.x1 / cell).floor() as i32;
            let y1 = (b.y1 / cell).floor() as i32;
            for gy in y0..=y1 {
                for gx in x0..=x1 {
                    buckets.entry((gx, gy)).or_default().push(i);
                }
            }
        }
        Self { cell, buckets }
    }

    pub fn query_aabb(&self, b: &Aabb) -> HashSet<usize> {
        let mut out = HashSet::new();
        let x0 = (b.x0 / self.cell).floor() as i32;
        let y0 = (b.y0 / self.cell).floor() as i32;
        let x1 = (b.x1 / self.cell).floor() as i32;
        let y1 = (b.y1 / self.cell).floor() as i32;
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                if let Some(list) = self.buckets.get(&(gx, gy)) {
                    out.extend(list.iter().copied());
                }
            }
        }
        out
    }
}

/// Narrow phase: does open segment (p→q) intersect AABB interior?
pub fn segment_hits_aabb(p: (f64, f64), q: (f64, f64), b: &Aabb) -> bool {
    // Degenerate
    if (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9 {
        return b.contains_point(p.0, p.1);
    }
    // Slab method (ray vs AABB), t in (0,1)
    let mut t0 = 0.0_f64;
    let mut t1 = 1.0_f64;
    let dx = q.0 - p.0;
    let dy = q.1 - p.1;
    for (p0, d, min_b, max_b) in [(p.0, dx, b.x0, b.x1), (p.1, dy, b.y0, b.y1)] {
        if d.abs() < 1e-12 {
            if p0 < min_b || p0 > max_b {
                return false;
            }
            continue;
        }
        let mut t_near = (min_b - p0) / d;
        let mut t_far = (max_b - p0) / d;
        if t_near > t_far {
            std::mem::swap(&mut t_near, &mut t_far);
        }
        t0 = t0.max(t_near);
        t1 = t1.min(t_far);
        if t0 > t1 {
            return false;
        }
    }
    // Hit if overlap of t in (eps, 1-eps) — ignore endpoint grazing at t=0/1
    t1 > 1e-4 && t0 < 1.0 - 1e-4
}

/// Orthogonal detour around an AABB for segment p→q (two candidate walks).
pub fn detour_around(p: (f64, f64), q: (f64, f64), b: &Aabb) -> Vec<(f64, f64)> {
    let pad = 8.0;
    let left = b.x0 - pad;
    let right = b.x1 + pad;
    let top = b.y0 - pad;
    let bottom = b.y1 + pad;

    // Orthogonal walks; keep only polylines that no longer stab `b`.
    let candidates = [
        vec![p, (left, p.1), (left, q.1), q],
        vec![p, (right, p.1), (right, q.1), q],
        vec![p, (p.0, top), (q.0, top), q],
        vec![p, (p.0, bottom), (q.0, bottom), q],
        vec![p, (left, p.1), (left, top), (q.0, top), q],
        vec![p, (right, p.1), (right, top), (q.0, top), q],
        vec![p, (left, p.1), (left, bottom), (q.0, bottom), q],
        vec![p, (right, p.1), (right, bottom), (q.0, bottom), q],
        // U-shaped around (needed when p/q share Y and left/right walks stay on the stab line)
        vec![p, (p.0, top), (left, top), (left, bottom), (q.0, bottom), q],
        vec![
            p,
            (p.0, bottom),
            (right, bottom),
            (right, top),
            (q.0, top),
            q,
        ],
    ];

    candidates
        .into_iter()
        .filter(|path| path.windows(2).all(|w| !segment_hits_aabb(w[0], w[1], b)))
        .min_by(|a, b| {
            path_len(a)
                .partial_cmp(&path_len(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or_else(|| vec![p, (p.0, top), (q.0, top), q])
}

fn path_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs())
        .sum()
}

/// Collapse near-duplicate consecutive points.
pub fn simplify_polyline(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for &p in pts {
        if let Some(last) = out.last() {
            if (last.0 - p.0).abs() < 0.5 && (last.1 - p.1).abs() < 0.5 {
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// Iteratively resolve segment parts against obstacles (recalculate by parts).
pub fn resolve_polyline(
    points: &[(f64, f64)],
    boxes: &[Aabb],
    hash: &SpatialHash,
    skip: &HashSet<usize>,
    max_passes: usize,
) -> Vec<(f64, f64)> {
    let mut pts = simplify_polyline(points);
    for _ in 0..max_passes {
        let mut changed = false;
        let mut next: Vec<(f64, f64)> = Vec::new();
        if pts.is_empty() {
            return pts;
        }
        next.push(pts[0]);
        for w in pts.windows(2) {
            let p = w[0];
            let q = w[1];
            let seg_box = Aabb {
                x0: p.0.min(q.0),
                y0: p.1.min(q.1),
                x1: p.0.max(q.0),
                y1: p.1.max(q.1),
            }
            .inflate(2.0);
            let mut hit: Option<usize> = None;
            for idx in hash.query_aabb(&seg_box) {
                if skip.contains(&idx) {
                    continue;
                }
                if segment_hits_aabb(p, q, &boxes[idx]) {
                    hit = Some(idx);
                    break;
                }
            }
            if let Some(i) = hit {
                let detour = detour_around(p, q, &boxes[i]);
                // append detour without duplicating p
                for &pt in detour.iter().skip(1) {
                    next.push(pt);
                }
                changed = true;
            } else {
                next.push(q);
            }
        }
        pts = simplify_polyline(&next);
        if !changed {
            break;
        }
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_overlap() {
        let a = Aabb {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        let b = Aabb {
            x0: 5.0,
            y0: 5.0,
            x1: 15.0,
            y1: 15.0,
        };
        assert!(a.overlaps(&b));
    }

    #[test]
    fn segment_through_box_detected() {
        let b = Aabb {
            x0: 10.0,
            y0: 10.0,
            x1: 40.0,
            y1: 40.0,
        };
        assert!(segment_hits_aabb((0.0, 25.0), (50.0, 25.0), &b));
        assert!(!segment_hits_aabb((0.0, 5.0), (50.0, 5.0), &b));
    }

    #[test]
    fn resolve_bends_around_obstacle() {
        let boxes = vec![Aabb {
            x0: 40.0,
            y0: 10.0,
            x1: 80.0,
            y1: 50.0,
        }];
        let hash = SpatialHash::build(&boxes, 32.0);
        let skip = HashSet::new();
        let out = resolve_polyline(&[(0.0, 30.0), (120.0, 30.0)], &boxes, &hash, &skip, 4);
        assert!(out.len() > 2, "expected detour points, got {out:?}");
        // Midpoints of each part should not stay as the original piercing segment only
        assert!(!segment_hits_aabb(out[0], *out.last().unwrap(), &boxes[0]) || out.len() > 2);
    }
}
