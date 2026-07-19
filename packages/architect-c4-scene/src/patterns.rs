//! Monotonic pattern repairs (olympiad post-process).
//! Only accept edits that strictly improve a simple cost.

use crate::bus::{
    ensure_orthogonal_poly, neighbor_attach_points, neighbor_kind, route_neighbor_channel,
};
use crate::collision::{segment_hits_aabb, simplify_polyline, Aabb};
use crate::labels::text_aabb;
use crate::{SceneEdge, SceneNode};
use std::collections::HashMap;

fn path_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs())
        .sum()
}

fn manhattan(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

fn has_uturn(pts: &[(f64, f64)]) -> bool {
    for w in pts.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let h = (a.1 - b.1).abs() < 0.5 && (b.1 - c.1).abs() < 0.5;
        let v = (a.0 - b.0).abs() < 0.5 && (b.0 - c.0).abs() < 0.5;
        if h && (b.0 - a.0) * (c.0 - b.0) < -1.0 {
            return true;
        }
        if v && (b.1 - a.1) * (c.1 - b.1) < -1.0 {
            return true;
        }
    }
    false
}

fn interior(n: &SceneNode) -> Aabb {
    Aabb {
        x0: n.x + 4.0,
        y0: n.y + 4.0,
        x1: (n.x + n.w - 4.0).max(n.x + 5.0),
        y1: (n.y + n.h - 4.0).max(n.y + 5.0),
    }
}

fn path_stabs_endpoints(pts: &[(f64, f64)], from: &SceneNode, to: &SceneNode) -> bool {
    let boxes = [interior(from), interior(to)];
    pts.windows(2)
        .any(|w| boxes.iter().any(|b| segment_hits_aabb(w[0], w[1], b)))
}

/// Exit south of `from`, run below it, then into `to` (never through either box).
/// Channel stays **well under** the source so long horizontals don't skim the class bottom
/// (UsageLogger "VERY LONG line" bug).
fn south_then_across(from: &SceneNode, to: &SceneNode) -> Vec<(f64, f64)> {
    let out = (from.x + from.w / 2.0, from.y + from.h);
    const DROP: f64 = 96.0;
    let mut y = out.1 + DROP;
    // Target far below: use mid-gap, but never shallower than DROP under source.
    if to.y > out.1 + DROP + 40.0 {
        let mid = (out.1 + DROP + to.y) * 0.5;
        y = mid.clamp(out.1 + DROP, (to.y - 48.0).max(out.1 + DROP));
    }
    let entry = if to.x >= from.x + from.w - 2.0 {
        (to.x, to.y + to.h / 2.0) // approach from west
    } else if to.x + to.w <= from.x + 2.0 {
        (to.x + to.w, to.y + to.h / 2.0)
    } else if to.y >= out.1 {
        (to.x + to.w / 2.0, to.y) // from north
    } else {
        (to.x + to.w / 2.0, to.y + to.h) // from south
    };
    simplify_polyline(&[out, (out.0, y), (entry.0, y), entry])
}

/// P_uturn: remove 180° chelnok — never accept elbows that stab endpoints.
fn fix_uturn(pts: &[(f64, f64)], from: &SceneNode, to: &SceneNode) -> Option<Vec<(f64, f64)>> {
    if pts.len() < 3 || !has_uturn(pts) {
        return None;
    }
    let a = pts[0];
    let b = pts[pts.len() - 1];
    let cands = [
        simplify_polyline(&[a, (b.0, a.1), b]),
        simplify_polyline(&[a, (a.0, b.1), b]),
        south_then_across(from, to),
    ];
    cands
        .into_iter()
        .filter(|c| !path_stabs_endpoints(c, from, to))
        .filter(|c| path_len(c) + 1.0 < path_len(pts))
        .min_by(|a, b| {
            path_len(a)
                .partial_cmp(&path_len(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// P_through_self: any stab of from/to interior → rebuild via south channel.
fn fix_through_self(e: &SceneEdge, nodes: &HashMap<&str, &SceneNode>) -> Option<Vec<(f64, f64)>> {
    let from = *nodes.get(e.from.as_str())?;
    let to = *nodes.get(e.to.as_str())?;
    if e.points.len() < 2 || !path_stabs_endpoints(&e.points, from, to) {
        return None;
    }
    let pts = south_then_across(from, to);
    if path_stabs_endpoints(&pts, from, to) {
        return None;
    }
    Some(pts)
}

/// Same-side spur / "hairpin": start & end on one vertical (or horizontal) line,
/// path juts out and returns (orange U on a purple class edge).
fn is_same_side_spur(pts: &[(f64, f64)]) -> bool {
    if pts.len() < 4 {
        return false;
    }
    let a = pts[0];
    let b = pts[pts.len() - 1];
    const SIDE_EPS: f64 = 14.0;
    const BULGE_MIN: f64 = 20.0;
    if (a.0 - b.0).abs() <= SIDE_EPS {
        let min_x = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        if max_x - min_x >= BULGE_MIN {
            return true;
        }
    }
    if (a.1 - b.1).abs() <= SIDE_EPS {
        let min_y = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        if max_y - min_y >= BULGE_MIN {
            return true;
        }
    }
    false
}

/// Collapse spur to a short path along the shared side (untangle).
fn fix_same_side_spur(
    pts: &[(f64, f64)],
    from: &SceneNode,
    to: &SceneNode,
) -> Option<Vec<(f64, f64)>> {
    if !is_same_side_spur(pts) {
        return None;
    }
    let a = pts[0];
    let b = pts[pts.len() - 1];
    let mut cands: Vec<Vec<(f64, f64)>> = Vec::new();
    if (a.0 - b.0).abs() <= 14.0 {
        let x = (a.0 + b.0) * 0.5;
        cands.push(simplify_polyline(&[
            (a.0, a.1),
            (x, a.1),
            (x, b.1),
            (b.0, b.1),
        ]));
        cands.push(simplify_polyline(&[(a.0, a.1), (a.0, b.1), (b.0, b.1)]));
        cands.push(simplify_polyline(&[(a.0, a.1), (b.0, a.1), (b.0, b.1)]));
    }
    if (a.1 - b.1).abs() <= 14.0 {
        let y = (a.1 + b.1) * 0.5;
        cands.push(simplify_polyline(&[
            (a.0, a.1),
            (a.0, y),
            (b.0, y),
            (b.0, b.1),
        ]));
        cands.push(simplify_polyline(&[(a.0, a.1), (b.0, a.1), (b.0, b.1)]));
        cands.push(simplify_polyline(&[(a.0, a.1), (a.0, b.1), (b.0, b.1)]));
    }
    cands.push(south_then_across(from, to));

    cands
        .into_iter()
        .filter(|c| c.len() >= 2)
        .filter(|c| !is_same_side_spur(c))
        .filter(|c| !path_stabs_endpoints(c, from, to))
        .filter(|c| path_len(c) + 1.0 < path_len(pts))
        .min_by(|x, y| {
            path_len(x)
                .partial_cmp(&path_len(y))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| x.len().cmp(&y.len()))
        })
}

/// P_long_detour: path much longer than manhattan → neighbor channel or south elbow.
fn fix_long_detour(e: &SceneEdge, nodes: &HashMap<&str, &SceneNode>) -> Option<Vec<(f64, f64)>> {
    if e.points.len() < 2 {
        return None;
    }
    let a = e.points[0];
    let b = e.points[e.points.len() - 1];
    let man = manhattan(a, b).max(1.0);
    if path_len(&e.points) <= 2.5 * man {
        return None;
    }
    let fn_ = *nodes.get(e.from.as_str())?;
    let tn = *nodes.get(e.to.as_str())?;
    if let Some((fp, _, tp, _)) = neighbor_attach_points(fn_, tn) {
        let pts = route_neighbor_channel(fn_, tn, fp, tp, 0);
        if !path_stabs_endpoints(&pts, fn_, tn) && path_len(&pts) + 1.0 < path_len(&e.points) {
            return Some(pts);
        }
    }
    let pts = south_then_across(fn_, tn);
    if !path_stabs_endpoints(&pts, fn_, tn) && path_len(&pts) + 1.0 < path_len(&e.points) {
        Some(pts)
    } else {
        None
    }
}

/// P_corridor_stack: spread near-coincident horizontal segments onto tracks.
fn assign_corridor_tracks(edges: &mut [SceneEdge]) {
    const BAND: f64 = 10.0;
    const PITCH: f64 = 14.0;
    // Collect (edge_idx, seg_idx, y, x0, x1)
    let mut segs: Vec<(usize, usize, f64, f64, f64)> = Vec::new();
    for (ei, e) in edges.iter().enumerate() {
        for (si, w) in e.points.windows(2).enumerate() {
            if (w[0].1 - w[1].1).abs() < 0.5 && (w[0].0 - w[1].0).abs() > 8.0 {
                let y = w[0].1;
                let x0 = w[0].0.min(w[1].0);
                let x1 = w[0].0.max(w[1].0);
                segs.push((ei, si, y, x0, x1));
            }
        }
    }
    if segs.len() < 2 {
        return;
    }
    segs.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut i = 0;
    while i < segs.len() {
        let y0 = segs[i].2;
        let mut j = i + 1;
        while j < segs.len() && (segs[j].2 - y0).abs() < BAND {
            j += 1;
        }
        let band = &segs[i..j];
        if band.len() >= 2 {
            // Overlap in X?
            let mut overlapping = false;
            for a in 0..band.len() {
                for b in (a + 1)..band.len() {
                    let (x0a, x1a) = (band[a].3, band[a].4);
                    let (x0b, x1b) = (band[b].3, band[b].4);
                    if x0a < x1b - 1.0 && x0b < x1a - 1.0 {
                        overlapping = true;
                    }
                }
            }
            if overlapping {
                // Sort by edge id for stability, assign tracks
                let mut members: Vec<_> = band.to_vec();
                members.sort_by_key(|t| t.0);
                for (k, &(ei, si, _y, _, _)) in members.iter().enumerate() {
                    let dy = (k as f64) * PITCH - ((members.len() - 1) as f64) * PITCH / 2.0;
                    let new_y = y0 + dy;
                    if si + 1 < edges[ei].points.len() {
                        edges[ei].points[si].1 = new_y;
                        edges[ei].points[si + 1].1 = new_y;
                        // keep orthogonal: fix adjacent vertical stubs if needed
                        if si > 0 {
                            let prev = edges[ei].points[si - 1];
                            if (prev.0 - edges[ei].points[si].0).abs() < 0.5 {
                                // vertical into this H — ok
                            } else if (prev.1 - new_y).abs() > 0.5 {
                                // insert elbow
                            }
                        }
                    }
                }
            }
        }
        i = j;
    }
    for e in edges.iter_mut() {
        e.points = simplify_polyline(&e.points);
    }
}

fn label_chip(e: &SceneEdge) -> Option<Aabb> {
    if e.label.trim().is_empty() {
        return None;
    }
    Some(text_aabb(e.label_x, e.label_y, &e.label, 10.0).inflate(8.0))
}

/// P_label_stab: shift stabbed H segment to free track (monotonic).
fn dodge_foreign_labels(edges: &mut [SceneEdge]) {
    const PITCH: f64 = 16.0;
    let chips: Vec<Option<Aabb>> = edges.iter().map(label_chip).collect();
    for (ei, edge) in edges.iter_mut().enumerate() {
        for (ci, chip_opt) in chips.iter().enumerate() {
            if ci == ei {
                continue;
            }
            let Some(chip) = chip_opt else {
                continue;
            };
            let pts = edge.points.clone();
            for si in 0..pts.len().saturating_sub(1) {
                if !segment_hits_aabb(pts[si], pts[si + 1], chip) {
                    continue;
                }
                // Only dodge horizontal segments
                if (pts[si].1 - pts[si + 1].1).abs() > 0.5 {
                    continue;
                }
                let y = pts[si].1;
                for dir in [-1.0_f64, 1.0] {
                    let mut trial = pts.clone();
                    let ny = y + dir * PITCH;
                    trial[si].1 = ny;
                    trial[si + 1].1 = ny;
                    if !segment_hits_aabb(trial[si], trial[si + 1], chip)
                        && path_len(&trial) <= path_len(&pts) + PITCH + 1.0
                    {
                        edge.points = simplify_polyline(&trial);
                        break;
                    }
                }
                break;
            }
        }
    }
}

/// U-notch on a channel: leave axis → jog → return (often skims a foreign class edge).
/// Example: horizontal at y=615 dips to Consumer.y=700 then returns — must become a straight.
fn collapse_channel_notches(pts: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    if pts.len() < 5 {
        return None;
    }
    const EPS: f64 = 2.0;
    const MIN_DEPTH: f64 = 8.0;
    const RETURN_TOL: f64 = 48.0;
    let mut out = pts.to_vec();
    let mut changed = false;
    let mut i = 0;
    while i + 5 <= out.len() {
        let a = out[i];
        let b = out[i + 1];
        let c = out[i + 2];
        let d = out[i + 3];
        let e = out[i + 4];
        // Horizontal channel notch: a-b horizontal, b-c vertical, c-d horizontal, d-e vertical.
        let h_notch = (a.1 - b.1).abs() <= EPS
            && (b.0 - c.0).abs() <= EPS
            && (c.1 - d.1).abs() <= EPS
            && (d.0 - e.0).abs() <= EPS
            && (b.1 - a.1).abs() < 0.5 // a-b horizontal (redundant with a.1≈b.1)
            && (c.1 - a.1).abs() >= MIN_DEPTH
            && (e.1 - a.1).abs() <= RETURN_TOL;
        // Vertical channel notch (symmetric).
        let v_notch = (a.0 - b.0).abs() <= EPS
            && (b.1 - c.1).abs() <= EPS
            && (c.0 - d.0).abs() <= EPS
            && (d.1 - e.1).abs() <= EPS
            && (c.0 - a.0).abs() >= MIN_DEPTH
            && (e.0 - a.0).abs() <= RETURN_TOL;

        if h_notch {
            // Straight along entry channel y to exit x, then to e.
            let y = a.1;
            let repl = simplify_polyline(&[a, (e.0, y), e]);
            out.splice(i..i + 5, repl.iter().copied());
            changed = true;
            continue; // re-examine from i
        }
        if v_notch {
            let x = a.0;
            let repl = simplify_polyline(&[a, (x, e.1), e]);
            out.splice(i..i + 5, repl.iter().copied());
            changed = true;
            continue;
        }
        i += 1;
    }
    if !changed {
        return None;
    }
    let out = simplify_polyline(&out);
    if path_len(&out) + 1.0 < path_len(pts) {
        Some(out)
    } else {
        None
    }
}

fn fix_channel_notch(
    pts: &[(f64, f64)],
    from: &SceneNode,
    to: &SceneNode,
) -> Option<Vec<(f64, f64)>> {
    let cand = collapse_channel_notches(pts)?;
    if path_stabs_endpoints(&cand, from, to) {
        return None;
    }
    Some(cand)
}

/// Which border of `node` (if any) contains point `p`.
fn point_on_side(node: &SceneNode, p: (f64, f64), eps: f64) -> Option<&'static str> {
    let on_y = p.1 >= node.y - eps && p.1 <= node.y + node.h + eps;
    let on_x = p.0 >= node.x - eps && p.0 <= node.x + node.w + eps;
    if on_y && (p.0 - node.x).abs() <= eps {
        return Some("W");
    }
    if on_y && (p.0 - (node.x + node.w)).abs() <= eps {
        return Some("E");
    }
    if on_x && (p.1 - node.y).abs() <= eps {
        return Some("N");
    }
    if on_x && (p.1 - (node.y + node.h)).abs() <= eps {
        return Some("S");
    }
    None
}

/// Nudge polyline endpoint along a vertical (W/E) or horizontal (N/S) side.
fn set_end_along_side(pts: &mut [(f64, f64)], side: &str, along: f64) {
    if pts.len() < 2 {
        return;
    }
    let n = pts.len();
    let end = pts[n - 1];
    let prev = pts[n - 2];
    match side {
        "W" | "E" => {
            pts[n - 1].1 = along;
            if (prev.1 - end.1).abs() < 1.0 {
                // horizontal approach — keep orthogonality
                pts[n - 2].1 = along;
            } else if (prev.0 - end.0).abs() < 1.0 {
                // already vertical on/near side — only move tip
            } else {
                pts[n - 2] = (prev.0, along);
                pts[n - 1] = (end.0, along);
            }
        }
        "N" | "S" => {
            pts[n - 1].0 = along;
            if (prev.0 - end.0).abs() < 1.0 {
                pts[n - 2].0 = along;
            } else if (prev.1 - end.1).abs() < 1.0 {
            } else {
                pts[n - 2] = (along, prev.1);
                pts[n - 1] = (along, end.1);
            }
        }
        _ => {}
    }
}

fn set_start_along_side(pts: &mut [(f64, f64)], side: &str, along: f64) {
    if pts.len() < 2 {
        return;
    }
    let start = pts[0];
    let next = pts[1];
    match side {
        "W" | "E" => {
            pts[0].1 = along;
            if (next.1 - start.1).abs() < 1.0 {
                pts[1].1 = along;
            } else if (next.0 - start.0).abs() < 1.0 {
            } else {
                pts[1] = (next.0, along);
                pts[0] = (start.0, along);
            }
        }
        "N" | "S" => {
            pts[0].0 = along;
            if (next.0 - start.0).abs() < 1.0 {
                pts[1].0 = along;
            } else if (next.1 - start.1).abs() < 1.0 {
            } else {
                pts[1] = (along, next.1);
                pts[0] = (along, start.1);
            }
        }
        _ => {}
    }
}

fn uml_arrow_pitch(edge_kind: &str) -> f64 {
    match edge_kind {
        "implements" | "extends" | "composition" | "aggregation" => 40.0,
        _ => 28.0,
    }
}

/// P_arrow_fan: redistribute endpoints that share a node side so arrowheads don't stack.
fn spread_arrow_fans(edges: &mut [SceneEdge], nodes: &[SceneNode]) -> bool {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut changed = false;
    // key: (node_id, side, is_end)
    let mut groups: HashMap<(String, String, bool), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if e.points.len() < 2 {
            continue;
        }
        if let Some(n) = by.get(e.to.as_str()) {
            if let Some(side) = point_on_side(n, *e.points.last().unwrap(), 3.0) {
                groups
                    .entry((e.to.clone(), side.to_string(), true))
                    .or_default()
                    .push(ei);
            }
        }
        if let Some(n) = by.get(e.from.as_str()) {
            if let Some(side) = point_on_side(n, e.points[0], 3.0) {
                groups
                    .entry((e.from.clone(), side.to_string(), false))
                    .or_default()
                    .push(ei);
            }
        }
    }

    for ((nid, side, is_end), mut idxs) in groups {
        if idxs.len() < 2 {
            continue;
        }
        let Some(node) = by.get(nid.as_str()) else {
            continue;
        };
        let inset = 16.0_f64;
        let (lo, hi) = match side.as_str() {
            "W" | "E" => (node.y + inset, node.y + node.h - inset),
            _ => (node.x + inset, node.x + node.w - inset),
        };
        if hi - lo < 8.0 {
            continue;
        }
        // Sort by current along-side coordinate.
        idxs.sort_by(|&a, &b| {
            let pa = if is_end {
                *edges[a].points.last().unwrap()
            } else {
                edges[a].points[0]
            };
            let pb = if is_end {
                *edges[b].points.last().unwrap()
            } else {
                edges[b].points[0]
            };
            let ka = if side == "W" || side == "E" {
                pa.1
            } else {
                pa.0
            };
            let kb = if side == "W" || side == "E" {
                pb.1
            } else {
                pb.0
            };
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Required pitch = max of member edge kinds.
        let mut pitch = 28.0_f64;
        for &i in &idxs {
            pitch = pitch.max(uml_arrow_pitch(&edges[i].edge_kind));
        }
        let n = idxs.len();
        let need = pitch * (n as f64 - 1.0);
        let span = hi - lo;
        // Even fan across usable side (prefer over min-pitch pack at center).
        let positions: Vec<f64> = if need <= span {
            (0..n)
                .map(|i| lo + (i as f64 + 1.0) / (n as f64 + 1.0) * span)
                .collect()
        } else {
            // Side too short: pack with reduced pitch still monotonic.
            let p = span / (n as f64 + 1.0);
            (0..n).map(|i| lo + (i as f64 + 1.0) * p).collect()
        };
        for (k, &ei) in idxs.iter().enumerate() {
            let along = positions[k];
            let before = edges[ei].points.clone();
            if is_end {
                set_end_along_side(&mut edges[ei].points, &side, along);
            } else {
                set_start_along_side(&mut edges[ei].points, &side, along);
            }
            edges[ei].points = simplify_polyline(&edges[ei].points);
            if edges[ei].points != before {
                changed = true;
            }
        }
    }
    changed
}

/// Run all monotonic pattern fixes. Returns true if any edge changed.
pub fn apply_patterns(edges: &mut [SceneEdge], nodes: &[SceneNode]) -> bool {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut changed = false;

    for e in edges.iter_mut() {
        let (Some(f), Some(t)) = (by.get(e.from.as_str()), by.get(e.to.as_str())) else {
            continue;
        };
        // Adjacent siblings: ALWAYS dead-short mid-gap path (never south U / long detour).
        if neighbor_kind(f, t).is_some() {
            if let Some((fp, _, tp, _)) = neighbor_attach_points(f, t) {
                let p = route_neighbor_channel(f, t, fp, tp, 0);
                if !path_stabs_endpoints(&p, f, t)
                    && (e.points != p)
                    && (path_len(&p) + 8.0 < path_len(&e.points)
                        || has_uturn(&e.points)
                        || path_stabs_endpoints(&e.points, f, t)
                        || e.points.len() > 4)
                {
                    e.points = p;
                    changed = true;
                }
            }
            continue; // do not apply south_then_across to neighbors
        }
        // Through-self first (e.g. libradosApi → right across its own box).
        if let Some(p) = fix_through_self(e, &by) {
            e.points = p;
            changed = true;
        }
        // Same-side hairpin on one class/shell edge → collapse to straight.
        if let Some(p) = fix_same_side_spur(&e.points, f, t) {
            e.points = p;
            changed = true;
        }
        // Channel U-notch (skims foreign class border) → straight line.
        if let Some(p) = fix_channel_notch(&e.points, f, t) {
            e.points = p;
            changed = true;
        }
        if let Some(p) = fix_uturn(&e.points, f, t) {
            e.points = p;
            changed = true;
        }
        if let Some(p) = fix_long_detour(e, &by) {
            e.points = p;
            changed = true;
        }
    }

    let before: Vec<_> = edges.iter().map(|e| e.points.clone()).collect();
    assign_corridor_tracks(edges);
    if spread_arrow_fans(edges, nodes) {
        changed = true;
    }
    // Second pass: fans may leave skims; collapse channel notches again.
    for e in edges.iter_mut() {
        let (Some(f), Some(t)) = (by.get(e.from.as_str()), by.get(e.to.as_str())) else {
            continue;
        };
        if let Some(p) = fix_channel_notch(&e.points, f, t) {
            e.points = p;
            changed = true;
        }
    }
    dodge_foreign_labels(edges);
    for e in edges.iter_mut() {
        if e.points.len() >= 2 {
            let ortho = ensure_orthogonal_poly(&e.points);
            if ortho != e.points {
                e.points = ortho;
                changed = true;
            }
        }
    }
    if edges.iter().zip(before.iter()).any(|(e, b)| e.points != *b) {
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str, x: f64, y: f64) -> SceneNode {
        SceneNode {
            id: id.into(),
            kind: "code".into(),
            layer: "code".into(),
            name: id.into(),
            parent_id: Some("p".into()),
            group: false,
            depth: 1,
            x,
            y,
            w: 80.0,
            h: 40.0,
            members: vec![],
            stereotype: None,
            url: None,
        }
    }

    fn edge(id: &str, from: &str, to: &str, points: Vec<(f64, f64)>) -> SceneEdge {
        SceneEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            label: String::new(),
            points,
            from_port: String::new(),
            to_port: String::new(),
            label_x: 0.0,
            label_y: 0.0,
            edge_kind: "assoc".into(),
        }
    }

    #[test]
    fn uturn_is_cut_to_short_elbow() {
        let nodes = vec![leaf("a", 0.0, 0.0), leaf("b", 200.0, 0.0)];
        // Classic chelnok on one horizontal: right endpoint via a left spur.
        let mut edges = vec![edge(
            "e",
            "a",
            "b",
            vec![
                (80.0, 50.0),
                (-120.0, 50.0), // reverse left
                (280.0, 50.0),  // then past target
                (280.0, 20.0),
            ],
        )];
        assert!(has_uturn(&edges[0].points));
        let before = path_len(&edges[0].points);
        apply_patterns(&mut edges, &nodes);
        assert!(
            path_len(&edges[0].points) + 1.0 < before,
            "pts={:?}",
            edges[0].points
        );
        assert!(!has_uturn(&edges[0].points), "pts={:?}", edges[0].points);
        assert!(!path_stabs_endpoints(
            &edges[0].points,
            &nodes[0],
            &nodes[1]
        ));
    }

    #[test]
    fn same_side_spur_is_collapsed() {
        // Orange U on a purple right edge: out → down → back on same x.
        let mut a = leaf("a", 100.0, 100.0);
        let mut b = leaf("b", 100.0, 300.0);
        a.w = 120.0;
        a.h = 80.0;
        b.w = 120.0;
        b.h = 80.0;
        let spur = vec![
            (a.x + a.w, a.y + 40.0),
            (a.x + a.w + 48.0, a.y + 40.0),
            (a.x + a.w + 48.0, b.y + 40.0),
            (b.x + b.w, b.y + 40.0),
        ];
        assert!(is_same_side_spur(&spur));
        let nodes = vec![a, b];
        let mut edges = vec![edge("e", "a", "b", spur.clone())];
        let before = path_len(&edges[0].points);
        assert!(apply_patterns(&mut edges, &nodes));
        assert!(
            !is_same_side_spur(&edges[0].points),
            "still a spur: {:?}",
            edges[0].points
        );
        assert!(
            path_len(&edges[0].points) + 1.0 < before,
            "expected shorter: {:?} vs {:?}",
            edges[0].points,
            spur
        );
    }

    #[test]
    fn adjacent_neighbors_get_short_midgap_path() {
        // Side-by-side classes must NOT get a south U-turn (Backend→IBackend bug).
        let a = leaf("a", 0.0, 0.0);
        let mut b = leaf("b", 120.0, 0.0);
        b.w = 80.0;
        let nodes = vec![a, b];
        let mut edges = vec![edge(
            "e",
            "a",
            "b",
            // Bad U under the boxes
            vec![(80.0, 20.0), (80.0, 100.0), (120.0, 100.0), (120.0, 20.0)],
        )];
        apply_patterns(&mut edges, &nodes);
        let pts = &edges[0].points;
        let min_y = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_y - min_y < 50.0,
            "path should stay in the gap, not U under: {:?}",
            pts
        );
        assert!(
            path_len(pts) < 80.0,
            "should be short: len={} {:?}",
            path_len(pts),
            pts
        );
    }

    #[test]
    fn through_self_horizontal_reroutes_south() {
        // West port → shoot east through own box (libradosApi bug).
        let a = leaf("a", 100.0, 100.0);
        let b = leaf("b", 800.0, 400.0);
        let mut a = a;
        a.w = 180.0;
        a.h = 88.0;
        let nodes = vec![a, b];
        let mut edges = vec![edge(
            "e",
            "a",
            "b",
            vec![(100.0, 144.0), (800.0, 144.0), (800.0, 440.0)],
        )];
        assert!(path_stabs_endpoints(&edges[0].points, &nodes[0], &nodes[1]));
        apply_patterns(&mut edges, &nodes);
        assert!(
            !path_stabs_endpoints(&edges[0].points, &nodes[0], &nodes[1]),
            "pts={:?}",
            edges[0].points
        );
        // Must go below source (south exit) with real drop (not skim).
        let channel_y = edges[0]
            .points
            .windows(2)
            .filter(|w| (w[0].1 - w[1].1).abs() < 0.5 && (w[0].0 - w[1].0).abs() > 50.0)
            .map(|w| w[0].1)
            .next();
        let cy = channel_y.expect("expected horizontal channel");
        assert!(
            cy >= nodes[0].y + nodes[0].h + 80.0,
            "channel too shallow under source: y={cy} pts={:?}",
            edges[0].points
        );
    }

    #[test]
    fn corridor_tracks_separate_stacked_horizontals() {
        let nodes = vec![
            leaf("a", 0.0, 0.0),
            leaf("b", 300.0, 0.0),
            leaf("c", 0.0, 100.0),
            leaf("d", 300.0, 100.0),
        ];
        let mut edges = vec![
            edge("e1", "a", "b", vec![(80.0, 50.0), (300.0, 50.0)]),
            edge("e2", "c", "d", vec![(80.0, 52.0), (300.0, 52.0)]),
            edge("e3", "a", "d", vec![(80.0, 51.0), (300.0, 51.0)]),
        ];
        apply_patterns(&mut edges, &nodes);
        let ys: Vec<f64> = edges.iter().map(|e| e.points[0].1).collect();
        let mut uniq = ys.clone();
        uniq.sort_by(|a, b| a.partial_cmp(b).unwrap());
        uniq.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert!(uniq.len() >= 2, "expected spread tracks, ys={ys:?}");
    }
    #[test]
    fn arrow_fan_spreads_stacked_ends() {
        // Several implements into Broker west — must not share nearly the same y.
        let broker = SceneNode {
            id: "Broker".into(),
            kind: "code".into(),
            layer: "code".into(),
            name: "Broker".into(),
            parent_id: Some("p".into()),
            group: false,
            depth: 1,
            x: 200.0,
            y: 100.0,
            w: 120.0,
            h: 260.0,
            members: vec![],
            stereotype: Some("Interface".into()),
            url: None,
        };
        let mk = |id: &str, y: f64| SceneNode {
            id: id.into(),
            kind: "code".into(),
            layer: "code".into(),
            name: id.into(),
            parent_id: Some("p".into()),
            group: false,
            depth: 1,
            x: 20.0,
            y,
            w: 100.0,
            h: 40.0,
            members: vec![],
            stereotype: None,
            url: None,
        };
        let nodes = vec![
            mk("StubBroker", 160.0),
            mk("RedisBroker", 220.0),
            mk("A", 280.0),
            broker,
        ];
        // All ends stacked ~ mid west of Broker
        let mut edges = vec![
            edge(
                "e1",
                "StubBroker",
                "Broker",
                vec![(120.0, 180.0), (200.0, 180.0)],
            ),
            edge(
                "e2",
                "RedisBroker",
                "Broker",
                vec![(120.0, 185.0), (200.0, 185.0)],
            ),
            edge("e3", "A", "Broker", vec![(120.0, 190.0), (200.0, 190.0)]),
        ];
        edges[0].edge_kind = "implements".into();
        edges[1].edge_kind = "implements".into();
        edges[2].edge_kind = "implements".into();
        assert!(spread_arrow_fans(&mut edges, &nodes));
        let mut ys: Vec<f64> = edges.iter().map(|e| e.points.last().unwrap().1).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in ys.windows(2) {
            assert!(w[1] - w[0] >= 24.0, "arrow tips too close: {ys:?}");
        }
    }
    #[test]
    fn channel_notch_skimming_foreign_border_becomes_straight() {
        // Actor→Broker style: horizontal dips onto Consumer top then continues.
        let pts = vec![
            (855.0, 615.0),
            (1099.0, 615.0),
            (1099.0, 700.0), // dip onto foreign N
            (1127.0, 700.0), // skim
            (1127.0, 645.0),
            (1475.0, 645.0),
        ];
        let out = collapse_channel_notches(&pts).expect("notch should collapse");
        let skim = out.windows(2).any(|w| {
            (w[0].1 - 700.0).abs() < 2.0
                && (w[1].1 - 700.0).abs() < 2.0
                && (w[0].0 - w[1].0).abs() > 10.0
        });
        assert!(!skim, "still skims y=700: {out:?}");
        assert!(
            path_len(&out) + 1.0 < path_len(&pts),
            "expected shorter: {out:?}"
        );
        // Prefer a single horizontal run near the entry channel.
        let has_615 = out.windows(2).any(|w| {
            (w[0].1 - 615.0).abs() < 2.0
                && (w[1].1 - 615.0).abs() < 2.0
                && (w[0].0 - w[1].0).abs() > 50.0
        });
        assert!(has_615, "expected straight at y=615: {out:?}");
    }
}
