//! Post-build CollisionEngine: edge⊗box, edge⊗label (foreign), edge⊗edge, interior ban.

use crate::collision::{detour_around, segment_hits_aabb, simplify_polyline, Aabb};
use crate::labels::text_aabb;
use crate::{SceneEdge, SceneNode};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ClearancePush {
    pub parent_id: String,
    pub left_id: String,
    pub right_id: String,
    pub need_gap: f64,
}

fn interior_aabb(n: &SceneNode) -> Aabb {
    Aabb {
        x0: n.x + 4.0,
        y0: n.y + 4.0,
        x1: (n.x + n.w - 4.0).max(n.x + 5.0),
        y1: (n.y + n.h - 4.0).max(n.y + 5.0),
    }
}

fn label_chip(e: &SceneEdge) -> Option<Aabb> {
    if e.label.trim().is_empty() {
        return None;
    }
    Some(text_aabb(e.label_x, e.label_y, &e.label, 10.0).inflate(10.0))
}

fn point_in_interior(pt: (f64, f64), n: &SceneNode) -> bool {
    interior_aabb(n).contains_point(pt.0, pt.1)
}

fn nearest_border_point(pt: (f64, f64), n: &SceneNode) -> (f64, f64) {
    let cands = [
        (n.x, pt.1.clamp(n.y, n.y + n.h)),
        (n.x + n.w, pt.1.clamp(n.y, n.y + n.h)),
        (pt.0.clamp(n.x, n.x + n.w), n.y),
        (pt.0.clamp(n.x, n.x + n.w), n.y + n.h),
    ];
    cands
        .into_iter()
        .min_by(|a, b| {
            let da = (a.0 - pt.0).abs() + (a.1 - pt.1).abs();
            let db = (b.0 - pt.0).abs() + (b.1 - pt.1).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(pt)
}

fn exterior_channel_path(from: &SceneNode, to: &SceneNode) -> Vec<(f64, f64)> {
    // Prefer bottom gutter outside both boxes (never enters interiors).
    let y = from.y.max(to.y) + from.h.max(to.h) + 28.0;
    let a = (from.x + from.w / 2.0, from.y + from.h);
    let b = (to.x + to.w / 2.0, to.y + to.h);
    simplify_polyline(&[a, (a.0, y), (b.0, y), b])
}

fn escape_endpoint_interiors(pts: &mut Vec<(f64, f64)>, from: &SceneNode, to: &SceneNode) {
    if pts.is_empty() {
        return;
    }
    if point_in_interior(pts[0], from) {
        pts[0] = nearest_border_point(pts[0], from);
    }
    let last = pts.len() - 1;
    if point_in_interior(pts[last], to) {
        pts[last] = nearest_border_point(pts[last], to);
    }
    if pts.len() > 2 {
        let head = pts[0];
        let tail = pts[pts.len() - 1];
        let mut mid: Vec<(f64, f64)> = pts[1..pts.len() - 1]
            .iter()
            .copied()
            .filter(|p| !point_in_interior(*p, from) && !point_in_interior(*p, to))
            .collect();
        let mut next = vec![head];
        next.append(&mut mid);
        next.push(tail);
        *pts = next;
    }
    *pts = simplify_polyline(pts);
    if pts.len() < 2 {
        *pts = exterior_channel_path(from, to);
        return;
    }
    // If any segment still stabs from/to interior — rebuild outside channel.
    let interiors = [interior_aabb(from), interior_aabb(to)];
    let still = pts
        .windows(2)
        .any(|w| interiors.iter().any(|b| segment_hits_aabb(w[0], w[1], b)));
    if still {
        *pts = exterior_channel_path(from, to);
    }
}

fn path_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs())
        .sum()
}

fn apply_detour(pts: &[(f64, f64)], wi: usize, obstacle: &Aabb) -> Vec<(f64, f64)> {
    let p = pts[wi];
    let q = pts[wi + 1];
    let detour = detour_around(p, q, &obstacle.inflate(12.0));
    let mut next = Vec::with_capacity(pts.len() + detour.len());
    next.extend_from_slice(&pts[..=wi]);
    for dp in detour.into_iter().skip(1) {
        next.push(dp);
    }
    if wi + 2 < pts.len() {
        next.extend_from_slice(&pts[wi + 2..]);
    }
    simplify_polyline(&next)
}

fn obstacles_for_edge(
    e: &SceneEdge,
    edge_idx: usize,
    nodes: &[SceneNode],
    edges: &[SceneEdge],
    include_foreign_labels: bool,
) -> Vec<(Aabb, &'static str)> {
    let mut out = Vec::new();
    for n in nodes.iter().filter(|n| !n.group) {
        if n.id == e.from || n.id == e.to {
            out.push((interior_aabb(n), "interior"));
        } else {
            out.push((Aabb::from_node(n, 2.0), "leaf"));
        }
    }
    if include_foreign_labels {
        for (i, other) in edges.iter().enumerate() {
            if i == edge_idx {
                continue; // own note sits on the wire by design — don't fight it here
            }
            if let Some(chip) = label_chip(other) {
                out.push((chip, "label"));
            }
        }
    }
    out
}

fn first_hit(pts: &[(f64, f64)], obstacles: &[(Aabb, &str)]) -> Option<(usize, usize)> {
    for wi in 0..pts.len().saturating_sub(1) {
        for (oi, (b, _)) in obstacles.iter().enumerate() {
            if segment_hits_aabb(pts[wi], pts[wi + 1], b) {
                return Some((wi, oi));
            }
        }
    }
    None
}

fn count_hits(pts: &[(f64, f64)], obstacles: &[(Aabb, &str)]) -> usize {
    let mut n = 0usize;
    for w in pts.windows(2) {
        for (b, _) in obstacles {
            if segment_hits_aabb(w[0], w[1], b) {
                n += 1;
            }
        }
    }
    n
}

fn repair_edge(
    pts_in: &[(f64, f64)],
    obstacles: &[(Aabb, &str)],
    max_iters: usize,
) -> Vec<(f64, f64)> {
    let mut pts = simplify_polyline(pts_in);
    let base_len = path_len(&pts).max(1.0);
    let mut best = pts.clone();
    let mut best_hits = count_hits(&best, obstacles);
    for _ in 0..max_iters {
        if best_hits == 0 {
            break;
        }
        let Some((wi, oi)) = first_hit(&pts, obstacles) else {
            break;
        };
        let next = apply_detour(&pts, wi, &obstacles[oi].0);
        if next.len() > pts.len() + 10 || path_len(&next) > base_len * 3.0 {
            break;
        }
        if next == pts {
            break;
        }
        let h = count_hits(&next, obstacles);
        // Strict improvement only — kills left/right oscillation.
        if h < best_hits {
            best = next.clone();
            best_hits = h;
            pts = next;
        } else {
            break;
        }
    }
    best
}

fn hv_crossing(
    a0: (f64, f64),
    a1: (f64, f64),
    b0: (f64, f64),
    b1: (f64, f64),
) -> Option<(f64, f64)> {
    let a_h = (a0.1 - a1.1).abs() < 0.5 && (a0.0 - a1.0).abs() > 0.5;
    let a_v = (a0.0 - a1.0).abs() < 0.5 && (a0.1 - a1.1).abs() > 0.5;
    let b_h = (b0.1 - b1.1).abs() < 0.5 && (b0.0 - b1.0).abs() > 0.5;
    let b_v = (b0.0 - b1.0).abs() < 0.5 && (b0.1 - b1.1).abs() > 0.5;
    let (h0, h1, v0, v1) = if a_h && b_v {
        (a0, a1, b0, b1)
    } else if a_v && b_h {
        (b0, b1, a0, a1)
    } else {
        return None;
    };
    let hy = h0.1;
    let (hx0, hx1) = (h0.0.min(h1.0), h0.0.max(h1.0));
    let vx = v0.0;
    let (vy0, vy1) = (v0.1.min(v1.1), v0.1.max(v1.1));
    if vx > hx0 + 1.0 && vx < hx1 - 1.0 && hy > vy0 + 1.0 && hy < vy1 - 1.0 {
        Some((vx, hy))
    } else {
        None
    }
}

fn jog_horizontal_around_vertical(
    pts: &mut Vec<(f64, f64)>,
    wi: usize,
    cx: f64,
    v_y0: f64,
    v_y1: f64,
) {
    let p = pts[wi];
    let q = pts[wi + 1];
    if (p.1 - q.1).abs() > 0.5 {
        return;
    }
    let y = p.1;
    let gap = 14.0;
    let bridge_y = if (y - v_y0).abs() <= (y - v_y1).abs() {
        v_y0 - gap
    } else {
        v_y1 + gap
    };
    let left = cx - gap;
    let right = cx + gap;
    let going_right = p.0 <= q.0;
    let (start, end) = if going_right { (p, q) } else { (q, p) };
    let mut mid = vec![
        start,
        (left, y),
        (left, bridge_y),
        (right, bridge_y),
        (right, y),
        end,
    ];
    if !going_right {
        mid.reverse();
    }
    let mut next = pts[..wi].to_vec();
    next.extend(mid);
    next.extend_from_slice(&pts[wi + 2..]);
    *pts = simplify_polyline(&next);
}

fn separate_edge_crossings(edges: &mut [SceneEdge], max_pairs: usize) {
    let mut fixed = 0usize;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            if fixed >= max_pairs {
                return;
            }
            let share_end = edges[i].from == edges[j].from
                || edges[i].from == edges[j].to
                || edges[i].to == edges[j].from
                || edges[i].to == edges[j].to;
            if share_end {
                continue;
            }
            let mut hit: Option<(usize, usize, usize, usize, f64)> = None;
            'find: for (wi, wa) in edges[i].points.windows(2).enumerate() {
                for (wj, wb) in edges[j].points.windows(2).enumerate() {
                    if hv_crossing(wa[0], wa[1], wb[0], wb[1]).is_none() {
                        continue;
                    }
                    let i_h = (wa[0].1 - wa[1].1).abs() < 0.5;
                    if i_h {
                        hit = Some((i, wi, j, wj, wb[0].0));
                    } else {
                        hit = Some((j, wj, i, wi, wa[0].0));
                    }
                    break 'find;
                }
            }
            let Some((eh, wi_h, ev, wi_v, cx)) = hit else {
                continue;
            };
            if wi_h + 1 >= edges[eh].points.len() || wi_v + 1 >= edges[ev].points.len() {
                continue;
            }
            let v0 = edges[ev].points[wi_v];
            let v1 = edges[ev].points[wi_v + 1];
            let (vy0, vy1) = (v0.1.min(v1.1), v0.1.max(v1.1));
            let before = edges[eh].points.clone();
            let mut pts = before.clone();
            jog_horizontal_around_vertical(&mut pts, wi_h, cx, vy0, vy1);
            // Accept only if it removes this crossing and doesn't explode.
            if pts.len() > before.len() + 8 || path_len(&pts) > path_len(&before) * 2.5 {
                continue;
            }
            let still = edges[ev].points.windows(2).any(|wb| {
                pts.windows(2)
                    .any(|wa| hv_crossing(wa[0], wa[1], wb[0], wb[1]).is_some())
            });
            if still {
                continue;
            }
            edges[eh].points = pts;
            fixed += 1;
        }
    }
}

/// Full collision pass.
pub fn fix_edge_box_collisions(
    edges: &mut [SceneEdge],
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) -> Vec<ClearancePush> {
    let mut pushes = Vec::new();
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Phase A — escape interiors + foreign leaves (no labels yet).
    for ei in 0..edges.len() {
        if edges[ei].points.len() < 2 {
            continue;
        }
        if let (Some(f), Some(t)) = (
            by.get(edges[ei].from.as_str()),
            by.get(edges[ei].to.as_str()),
        ) {
            escape_endpoint_interiors(&mut edges[ei].points, f, t);
        }
        // clearance hints
        {
            let e = &edges[ei];
            for n in nodes.iter().filter(|n| !n.group) {
                if n.id == e.from || n.id == e.to {
                    continue;
                }
                let b = Aabb::from_node(n, 2.0);
                for w in e.points.windows(2) {
                    if !segment_hits_aabb(w[0], w[1], &b) {
                        continue;
                    }
                    if let Some(pid) = n.parent_id.clone() {
                        let other = if parent_of.get(&e.from).cloned().flatten().as_deref()
                            == Some(pid.as_str())
                        {
                            e.from.clone()
                        } else if parent_of.get(&e.to).cloned().flatten().as_deref()
                            == Some(pid.as_str())
                        {
                            e.to.clone()
                        } else {
                            continue;
                        };
                        let (left, right) = {
                            let o = nodes.iter().find(|x| x.id == other);
                            let h = nodes.iter().find(|x| x.id == n.id);
                            match (o, h) {
                                (Some(o), Some(h)) if o.x <= h.x => (other.clone(), n.id.clone()),
                                (Some(_), Some(_)) => (n.id.clone(), other),
                                _ => continue,
                            }
                        };
                        pushes.push(ClearancePush {
                            parent_id: pid,
                            left_id: left,
                            right_id: right,
                            need_gap: 96.0,
                        });
                    }
                }
            }
        }
        let obstacles = obstacles_for_edge(&edges[ei], ei, nodes, edges, false);
        edges[ei].points = repair_edge(&edges[ei].points, &obstacles, 10);
    }

    // Phase B — foreign label chips only (one repair round).
    for ei in 0..edges.len() {
        let obstacles = obstacles_for_edge(&edges[ei], ei, nodes, edges, true);
        edges[ei].points = repair_edge(&edges[ei].points, &obstacles, 8);
    }

    // Phase C — limited edge⊗edge separation.
    separate_edge_crossings(edges, 32);

    // Phase D — final leaf/interior cleanup after jogs (no labels — notes re-placed after).
    for ei in 0..edges.len() {
        if let (Some(f), Some(t)) = (
            by.get(edges[ei].from.as_str()),
            by.get(edges[ei].to.as_str()),
        ) {
            escape_endpoint_interiors(&mut edges[ei].points, f, t);
        }
        let obstacles = obstacles_for_edge(&edges[ei], ei, nodes, edges, false);
        edges[ei].points = repair_edge(&edges[ei].points, &obstacles, 6);
    }

    let mut best: HashMap<(String, String, String), f64> = HashMap::new();
    for p in pushes {
        let key = (p.parent_id, p.left_id, p.right_id);
        let cur = best.get(&key).copied().unwrap_or(0.0);
        if p.need_gap > cur {
            best.insert(key, p.need_gap);
        }
    }
    best.into_iter()
        .map(|((parent_id, left_id, right_id), need_gap)| ClearancePush {
            parent_id,
            left_id,
            right_id,
            need_gap,
        })
        .collect()
}

#[cfg(test)]
pub fn count_edge_box_hits(edges: &[SceneEdge], nodes: &[SceneNode]) -> usize {
    let mut hits = 0usize;
    for e in edges {
        for n in nodes.iter().filter(|n| !n.group) {
            if n.id == e.from || n.id == e.to {
                continue;
            }
            let b = Aabb::from_node(n, 2.0);
            for w in e.points.windows(2) {
                if segment_hits_aabb(w[0], w[1], &b) {
                    hits += 1;
                }
            }
        }
    }
    hits
}

#[cfg(test)]
pub fn count_interior_hits(edges: &[SceneEdge], nodes: &[SceneNode]) -> usize {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut hits = 0usize;
    for e in edges {
        for id in [&e.from, &e.to] {
            let Some(n) = by.get(id.as_str()) else {
                continue;
            };
            let b = interior_aabb(n);
            for w in e.points.windows(2) {
                if segment_hits_aabb(w[0], w[1], &b) {
                    hits += 1;
                }
            }
        }
    }
    hits
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

    fn edge(
        id: &str,
        from: &str,
        to: &str,
        points: Vec<(f64, f64)>,
        label: &str,
        lx: f64,
        ly: f64,
    ) -> SceneEdge {
        SceneEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            label: label.into(),
            points,
            from_port: String::new(),
            to_port: String::new(),
            label_x: lx,
            label_y: ly,
            edge_kind: "assoc".into(),
        }
    }

    #[test]
    fn fixes_horizontal_stab_through_middle_box() {
        let nodes = vec![
            leaf("a", 0.0, 40.0),
            leaf("c", 120.0, 40.0),
            leaf("b", 240.0, 40.0),
        ];
        let mut edges = vec![edge(
            "e",
            "a",
            "b",
            vec![(80.0, 60.0), (240.0, 60.0)],
            "",
            0.0,
            0.0,
        )];
        let parent_of = HashMap::from([
            ("a".into(), Some("p".into())),
            ("b".into(), Some("p".into())),
            ("c".into(), Some("p".into())),
        ]);
        fix_edge_box_collisions(&mut edges, &nodes, &parent_of);
        assert_eq!(count_edge_box_hits(&edges, &nodes), 0);
    }

    #[test]
    fn bans_polyline_through_own_endpoint_interior() {
        let mut a = leaf("a", 0.0, 0.0);
        a.h = 80.0;
        let b = leaf("b", 200.0, 0.0);
        let nodes = vec![a, b];
        let mut edges = vec![edge(
            "e",
            "a",
            "b",
            vec![(40.0, 40.0), (40.0, 120.0), (240.0, 120.0), (240.0, 40.0)],
            "",
            0.0,
            0.0,
        )];
        let parent_of = HashMap::from([
            ("a".into(), Some("p".into())),
            ("b".into(), Some("p".into())),
        ]);
        assert!(count_interior_hits(&edges, &nodes) >= 1);
        fix_edge_box_collisions(&mut edges, &nodes, &parent_of);
        assert_eq!(
            count_interior_hits(&edges, &nodes),
            0,
            "pts={:?}",
            edges[0].points
        );
    }

    #[test]
    fn detours_around_foreign_label_chip() {
        let nodes = vec![leaf("a", 0.0, 40.0), leaf("b", 400.0, 40.0)];
        let mut edges = vec![
            edge(
                "note_owner",
                "a",
                "b",
                vec![(80.0, 10.0), (400.0, 10.0)],
                "RGWUsage → RGWRados\nread_usage",
                200.0,
                30.0,
            ),
            edge(
                "through",
                "a",
                "b",
                vec![(80.0, 28.0), (400.0, 28.0)],
                "",
                0.0,
                0.0,
            ),
        ];
        // share endpoints → edge-edge skip; label still applies
        let parent_of = HashMap::from([
            ("a".into(), Some("p".into())),
            ("b".into(), Some("p".into())),
        ]);
        fix_edge_box_collisions(&mut edges, &nodes, &parent_of);
        let chip = label_chip(&edges[0]).unwrap();
        let hits = edges[1]
            .points
            .windows(2)
            .filter(|w| segment_hits_aabb(w[0], w[1], &chip))
            .count();
        assert_eq!(hits, 0, "through={:?}", edges[1].points);
    }

    #[test]
    fn separates_crossing_orthogonal_edges() {
        let nodes = vec![
            leaf("a", 0.0, 0.0),
            leaf("b", 300.0, 0.0),
            leaf("c", 100.0, -100.0),
            leaf("d", 100.0, 200.0),
        ];
        let mut edges = vec![
            edge(
                "h",
                "a",
                "b",
                vec![(80.0, 50.0), (300.0, 50.0)],
                "",
                0.0,
                0.0,
            ),
            edge(
                "v",
                "c",
                "d",
                vec![(150.0, -60.0), (150.0, 220.0)],
                "",
                0.0,
                0.0,
            ),
        ];
        fix_edge_box_collisions(&mut edges, &nodes, &HashMap::new());
        let mut still = false;
        for wa in edges[0].points.windows(2) {
            for wb in edges[1].points.windows(2) {
                if hv_crossing(wa[0], wa[1], wb[0], wb[1]).is_some() {
                    still = true;
                }
            }
        }
        assert!(!still, "{:?} / {:?}", edges[0].points, edges[1].points);
    }
}
