//! Edge routing with border **viewpoints** (ports).
//! Rhombus = connection point on the node edge — never the node center.
//! See `docs/research/edge-routing-arrows.md`.

use crate::{SceneEdge, SceneNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeClass {
    Local,
    InterComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    N,
    E,
    S,
    W,
}

/// Attachment point on a node's border (drawn as a small rhombus).
#[derive(Debug, Clone, PartialEq)]
pub struct Viewpoint {
    pub node_id: String,
    pub side: Side,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    pub points: Vec<(f64, f64)>,
    pub class: EdgeClass,
    pub from_vp: Viewpoint,
    pub to_vp: Viewpoint,
}

pub fn is_inter_component(from: &SceneNode, to: &SceneNode) -> bool {
    let fc = from.kind.as_str();
    let tc = to.kind.as_str();
    matches!(
        (fc, tc),
        ("component", "component")
            | ("component", "container")
            | ("container", "component")
            | ("component", "code")
            | ("code", "component")
    ) && from.parent_id != to.parent_id
}

fn center(n: &SceneNode) -> (f64, f64) {
    (n.x + n.w / 2.0, n.y + n.h / 2.0)
}

fn facing_side(from: &SceneNode, to: &SceneNode) -> Side {
    let (fx, fy) = center(from);
    let (tx, ty) = center(to);
    let dx = tx - fx;
    let dy = ty - fy;
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            Side::E
        } else {
            Side::W
        }
    } else if dy >= 0.0 {
        Side::S
    } else {
        Side::N
    }
}

/// Port on the **border** of the node (never interior / center).
fn viewpoint_on(n: &SceneNode, side: Side, lane: f64) -> Viewpoint {
    let inset = 10.0;
    let (x, y) = match side {
        Side::N => {
            let x = (n.x + n.w / 2.0 + lane).clamp(n.x + inset, n.x + n.w - inset);
            (x, n.y)
        }
        Side::S => {
            let x = (n.x + n.w / 2.0 + lane).clamp(n.x + inset, n.x + n.w - inset);
            (x, n.y + n.h)
        }
        Side::W => {
            let y = (n.y + n.h / 2.0 + lane).clamp(n.y + inset, n.y + n.h - inset);
            (n.x, y)
        }
        Side::E => {
            let y = (n.y + n.h / 2.0 + lane).clamp(n.y + inset, n.y + n.h - inset);
            (n.x + n.w, y)
        }
    };
    Viewpoint {
        node_id: n.id.clone(),
        side,
        x,
        y,
    }
}

/// Orthogonal route between border viewpoints (not centers).
pub fn route_orthogonal(
    from: &SceneNode,
    to: &SceneNode,
    lane_index: usize,
    lane_count: usize,
) -> EdgeRoute {
    let class = if is_inter_component(from, to) {
        EdgeClass::InterComponent
    } else {
        EdgeClass::Local
    };
    let spread = 16.0;
    let mid = (lane_count.saturating_sub(1) as f64) / 2.0;
    let lane = (lane_index as f64 - mid) * spread;

    let s0 = facing_side(from, to);
    let s1 = facing_side(to, from);
    let from_vp = viewpoint_on(from, s0, lane);
    let to_vp = viewpoint_on(to, s1, lane);
    let (x0, y0) = (from_vp.x, from_vp.y);
    let (x1, y1) = (to_vp.x, to_vp.y);

    // Short stub out of the port so the polyline leaves the border cleanly.
    let stub = 12.0;
    let (sx0, sy0) = match s0 {
        Side::E => (x0 + stub, y0),
        Side::W => (x0 - stub, y0),
        Side::N => (x0, y0 - stub),
        Side::S => (x0, y0 + stub),
    };
    let (sx1, sy1) = match s1 {
        Side::E => (x1 + stub, y1),
        Side::W => (x1 - stub, y1),
        Side::N => (x1, y1 - stub),
        Side::S => (x1, y1 + stub),
    };

    let points = match (s0, s1) {
        (Side::E, Side::W) | (Side::W, Side::E) => {
            let mx = (sx0 + sx1) / 2.0 + lane * 0.25;
            vec![
                (x0, y0),
                (sx0, sy0),
                (mx, sy0),
                (mx, sy1),
                (sx1, sy1),
                (x1, y1),
            ]
        }
        (Side::N, Side::S) | (Side::S, Side::N) => {
            let my = (sy0 + sy1) / 2.0 + lane * 0.25;
            vec![
                (x0, y0),
                (sx0, sy0),
                (sx0, my),
                (sx1, my),
                (sx1, sy1),
                (x1, y1),
            ]
        }
        (Side::E, Side::N) | (Side::E, Side::S) | (Side::W, Side::N) | (Side::W, Side::S) => {
            vec![(x0, y0), (sx0, sy0), (sx1, sy0), (sx1, sy1), (x1, y1)]
        }
        (Side::N, Side::E) | (Side::N, Side::W) | (Side::S, Side::E) | (Side::S, Side::W) => {
            vec![(x0, y0), (sx0, sy0), (sx0, sy1), (sx1, sy1), (x1, y1)]
        }
        _ => vec![(x0, y0), (sx0, sy0), (sx1, sy1), (x1, y1)],
    };
    EdgeRoute {
        points,
        class,
        from_vp,
        to_vp,
    }
}

fn ancestor_ids(
    id: &str,
    by_id: &std::collections::HashMap<&str, &SceneNode>,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let mut cur = by_id.get(id).and_then(|n| n.parent_id.as_deref());
    while let Some(p) = cur {
        out.insert(p.to_string());
        cur = by_id.get(p).and_then(|n| n.parent_id.as_deref());
    }
    out
}

pub fn route_all_edges(nodes: &[SceneNode], edges: &[SceneEdge]) -> Vec<EdgeRoute> {
    use crate::collision::{resolve_polyline, Aabb, SpatialHash};
    use std::collections::HashSet;

    let by_id: std::collections::HashMap<&str, &SceneNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    // Obstacle boxes: all nodes (leaves + groups) with label padding.
    let boxes: Vec<Aabb> = nodes.iter().map(|n| Aabb::from_node(n, 6.0)).collect();
    let hash = SpatialHash::build(&boxes, 96.0);

    let mut buckets: std::collections::HashMap<(String, String), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let mut a = e.from.clone();
        let mut b = e.to.clone();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        buckets.entry((a, b)).or_default().push(i);
    }
    let mut out: Vec<Option<EdgeRoute>> = (0..edges.len()).map(|_| None).collect();
    for idxs in buckets.into_values() {
        let n = idxs.len();
        for (lane_i, &ei) in idxs.iter().enumerate() {
            let e = &edges[ei];
            let Some(a) = by_id.get(e.from.as_str()) else {
                continue;
            };
            let Some(b) = by_id.get(e.to.as_str()) else {
                continue;
            };
            let mut route = route_orthogonal(a, b, lane_i, n);
            // Skip endpoints + their ancestor groups (parent boundaries).
            let mut skip: HashSet<usize> = HashSet::new();
            for (i, node) in nodes.iter().enumerate() {
                if node.id == e.from || node.id == e.to {
                    skip.insert(i);
                }
            }
            let anc = ancestor_ids(&e.from, &by_id)
                .union(&ancestor_ids(&e.to, &by_id))
                .cloned()
                .collect::<HashSet<_>>();
            for (i, node) in nodes.iter().enumerate() {
                if anc.contains(&node.id) {
                    skip.insert(i);
                }
            }
            // Part-wise collision resolve + recalculate (gamedev broad/narrow).
            route.points = resolve_polyline(&route.points, &boxes, &hash, &skip, 5);
            // Keep first/last glued to viewpoints.
            if let Some(first) = route.points.first_mut() {
                *first = (route.from_vp.x, route.from_vp.y);
            }
            if let Some(last) = route.points.last_mut() {
                *last = (route.to_vp.x, route.to_vp.y);
            }
            out[ei] = Some(route);
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(i, r)| {
            r.unwrap_or_else(|| {
                let e = &edges[i];
                let dummy = Viewpoint {
                    node_id: e.from.clone(),
                    side: Side::E,
                    x: 0.0,
                    y: 0.0,
                };
                EdgeRoute {
                    points: vec![],
                    class: EdgeClass::Local,
                    from_vp: dummy.clone(),
                    to_vp: dummy,
                }
            })
        })
        .collect()
}

/// Unique viewpoints used by routes (for drawing rhombus ports).
pub fn collect_viewpoints(routes: &[EdgeRoute]) -> Vec<Viewpoint> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in routes {
        for vp in [&r.from_vp, &r.to_vp] {
            // Quantize to avoid near-duplicates from float noise.
            let key = (
                vp.node_id.clone(),
                (vp.x * 10.0).round() as i64,
                (vp.y * 10.0).round() as i64,
            );
            if seen.insert(key) {
                out.push(vp.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: &str, parent: Option<&str>, x: f64, y: f64) -> SceneNode {
        SceneNode {
            id: id.into(),
            kind: kind.into(),
            layer: "container".into(),
            name: id.into(),
            parent_id: parent.map(str::to_string),
            group: false,
            depth: 0,
            x,
            y,
            w: 100.0,
            h: 60.0,
            members: vec![],
            stereotype: None,
            url: None,
        }
    }

    #[test]
    fn viewpoints_lie_on_border_not_center() {
        let a = node("a", "component", Some("c1"), 0.0, 0.0);
        let b = node("b", "component", Some("c2"), 300.0, 0.0);
        let r = route_orthogonal(&a, &b, 0, 1);
        // From is on left node → east border (x == a.x + a.w)
        assert!((r.from_vp.x - (a.x + a.w)).abs() < 0.01);
        assert!(r.from_vp.y > a.y && r.from_vp.y < a.y + a.h);
        // Not center
        assert!((r.from_vp.x - (a.x + a.w / 2.0)).abs() > 1.0);
        assert!((r.to_vp.x - b.x).abs() < 0.01);
    }

    #[test]
    fn inter_component_across_parents() {
        let a = node("a", "component", Some("c1"), 0.0, 0.0);
        let b = node("b", "component", Some("c2"), 200.0, 0.0);
        assert!(is_inter_component(&a, &b));
    }

    #[test]
    fn collect_viewpoints_dedups() {
        let a = node("a", "component", Some("c1"), 0.0, 0.0);
        let b = node("b", "component", Some("c2"), 300.0, 200.0);
        let r = route_orthogonal(&a, &b, 0, 1);
        let vps = collect_viewpoints(std::slice::from_ref(&r));
        assert_eq!(vps.len(), 2);
    }
}
