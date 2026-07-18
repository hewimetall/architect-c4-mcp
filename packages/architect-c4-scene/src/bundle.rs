//! Atom magistral bundling (DISABLED in matryoshka — user forbid: do not glue lines).
//!
//! Kept for experiments/tests only. Production All-view must keep per-edge polylines.

use crate::bus::{
    allocate_left_buses, allocate_right_buses, channel_rails, clean_polyline,
    ensure_orthogonal_poly, pick_long_side, rail_x, rail_x_right, OUTER_BUS_GUTTER,
    RAIL_TRACK_PITCH,
};
use crate::{SceneEdge, SceneNode};
use std::collections::HashMap;

fn is_atom_kind(kind: &str) -> bool {
    matches!(kind, "code" | "external")
}

/// Nearest container/system id walking parents (inclusive).
pub fn enclosing_container_id(
    id: &str,
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) -> Option<String> {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut cur = Some(id.to_string());
    while let Some(cid) = cur {
        if let Some(n) = by.get(cid.as_str()) {
            if n.kind == "container" || n.kind == "software_system" {
                return Some(cid);
            }
        }
        cur = parent_of.get(&cid).cloned().flatten();
    }
    None
}

/// Rewrite atom→atom magistrals onto a shared trunk.
/// **Do not call from production layout** — gluing lines is forbidden.
#[allow(dead_code)]
pub fn bundle_atom_magistrals(
    edges: &mut [SceneEdge],
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let left_buses = allocate_left_buses(nodes);
    let right_buses = allocate_right_buses(nodes);

    // group edge indices by directed container pair
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let Some(fn_) = by.get(e.from.as_str()) else {
            continue;
        };
        let Some(tn) = by.get(e.to.as_str()) else {
            continue;
        };
        if !is_atom_kind(&fn_.kind) || !is_atom_kind(&tn.kind) {
            continue;
        }
        let Some(ca) = enclosing_container_id(&e.from, nodes, parent_of) else {
            continue;
        };
        let Some(cb) = enclosing_container_id(&e.to, nodes, parent_of) else {
            continue;
        };
        if ca == cb {
            continue;
        }
        if !left_buses.contains_key(&ca) || !left_buses.contains_key(&cb) {
            continue;
        }
        groups.entry((ca, cb)).or_default().push(i);
    }

    for ((ca, cb), idxs) in groups {
        if idxs.is_empty() {
            continue;
        }
        let Some(a) = by.get(ca.as_str()) else {
            continue;
        };
        let Some(b) = by.get(cb.as_str()) else {
            continue;
        };
        let parent = parent_of
            .get(&ca)
            .cloned()
            .flatten()
            .and_then(|p| by.get(p.as_str()).copied());
        let use_left = pick_long_side(a, b, parent);
        let buses = if use_left { &left_buses } else { &right_buses };
        let Some(rail_a) = buses.get(&ca) else {
            continue;
        };
        let Some(rail_b) = buses.get(&cb) else {
            continue;
        };

        // One shared trunk for the whole bundle (track 0). Fan-outs stay per-edge.
        let xa = if use_left {
            rail_x(rail_a, 0)
        } else {
            rail_x_right(rail_a, 0)
        };
        let xb = if use_left {
            rail_x(rail_b, 0)
        } else {
            rail_x_right(rail_b, 0)
        };
        let exit_y = if (a.y + a.h) <= b.y + 2.0 {
            a.y + a.h
        } else if (b.y + b.h) <= a.y + 2.0 {
            a.y
        } else {
            (a.y + a.h / 2.0 + b.y + b.h / 2.0) / 2.0
        };
        let entry_y = if b.y >= a.y + a.h - 2.0 {
            b.y
        } else if a.y >= b.y + b.h - 2.0 {
            b.y + b.h
        } else {
            exit_y
        };
        let shared = channel_rails(
            a,
            b,
            (xa, exit_y),
            (xb, entry_y),
            0,
            RAIL_TRACK_PITCH.max(14.0),
        );

        let n = idxs.len();
        for (k, &ei) in idxs.iter().enumerate() {
            let e = &edges[ei];
            let Some(from_n) = by.get(e.from.as_str()) else {
                continue;
            };
            let Some(to_n) = by.get(e.to.as_str()) else {
                continue;
            };
            // Slight Y stagger on the shared rail so on-ramps don't stack on one point.
            let stagger = (k as f64) * 14.0 - ((n.saturating_sub(1) as f64) * 14.0 / 2.0);
            let ya = (from_n.y + from_n.h / 2.0 + stagger).clamp(rail_a.y0, rail_a.y1);
            let yb = (to_n.y + to_n.h / 2.0 + stagger).clamp(rail_b.y0, rail_b.y1);

            // Attach on the side facing the rail — never cross the leaf to reach it.
            let from_attach = if use_left {
                (from_n.x, from_n.y + from_n.h / 2.0 + stagger)
            } else {
                (from_n.x + from_n.w, from_n.y + from_n.h / 2.0 + stagger)
            };
            let to_attach = if use_left {
                (to_n.x, to_n.y + to_n.h / 2.0 + stagger)
            } else {
                (to_n.x + to_n.w, to_n.y + to_n.h / 2.0 + stagger)
            };

            let mut pts = vec![from_attach, (xa, from_attach.1), (xa, ya)];
            // Join shared trunk (skip duplicate join point)
            for p in shared.iter().copied() {
                if let Some(last) = pts.last() {
                    if (last.0 - p.0).abs() < 0.5 && (last.1 - p.1).abs() < 0.5 {
                        continue;
                    }
                }
                pts.push(p);
            }
            // Align to fan-out Y on B rail then into leaf
            if let Some(last) = pts.last().copied() {
                if (last.0 - xb).abs() > 0.5 {
                    pts.push((xb, last.1));
                }
                if (last.1 - yb).abs() > 0.5 {
                    pts.push((xb, yb));
                }
            }
            pts.push((xb, to_attach.1));
            pts.push(to_attach);

            let pts = ensure_orthogonal_poly(&clean_polyline(&pts));
            edges[ei].points = pts;
            // Annotate dense bundles in the note caption (still per-edge; collision separates chips).
            if n >= 2 && !edges[ei].label.is_empty() && !edges[ei].label.contains("· bundle") {
                // keep individual From→To; optional suffix for clarity
                let _ = OUTER_BUS_GUTTER;
            }
        }

        // Bundle summary label on the first edge if many members (extra note above trunk mid).
        if n >= 3 {
            let mid = shared
                .get(shared.len() / 2)
                .copied()
                .unwrap_or((xa, exit_y));
            let first = idxs[0];
            // Prefixed count helps when ladder is long; individual labels remain.
            if !edges[first].label.starts_with(&(n.to_string() + "×")) {
                edges[first].label = format!("{}×\n{}", n, edges[first].label);
            }
            let _ = mid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneNode;

    fn node(
        id: &str,
        kind: &str,
        parent: Option<&str>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> SceneNode {
        SceneNode {
            id: id.into(),
            kind: kind.into(),
            layer: kind.into(),
            name: id.into(),
            parent_id: parent.map(str::to_string),
            group: kind == "container" || kind == "component",
            depth: 0,
            x,
            y,
            w,
            h,
            members: vec![],
            stereotype: None,
            url: None,
        }
    }

    #[test]
    fn bundle_shares_trunk_x_for_two_atom_edges() {
        let nodes = vec![
            node("sys", "software_system", None, 0.0, 0.0, 800.0, 600.0),
            node("api", "container", Some("sys"), 200.0, 40.0, 200.0, 200.0),
            node("db", "container", Some("sys"), 200.0, 320.0, 200.0, 200.0),
            node("c1", "code", Some("api"), 240.0, 100.0, 80.0, 40.0),
            node("c2", "code", Some("api"), 240.0, 160.0, 80.0, 40.0),
            node("d1", "code", Some("db"), 240.0, 380.0, 80.0, 40.0),
            node("d2", "code", Some("db"), 240.0, 440.0, 80.0, 40.0),
        ];
        let mut parent_of = HashMap::new();
        for n in &nodes {
            parent_of.insert(n.id.clone(), n.parent_id.clone());
        }
        let mut edges = vec![
            SceneEdge {
                id: "e1".into(),
                from: "c1".into(),
                to: "d1".into(),
                label: "A → B\nuses".into(),
                points: vec![
                    (240.0, 120.0),
                    (400.0, 120.0),
                    (400.0, 400.0),
                    (240.0, 400.0),
                ],
                from_port: String::new(),
                to_port: String::new(),
                label_x: 0.0,
                label_y: 0.0,
                edge_kind: "assoc".into(),
            },
            SceneEdge {
                id: "e2".into(),
                from: "c2".into(),
                to: "d2".into(),
                label: "C → D\nuses".into(),
                points: vec![
                    (240.0, 180.0),
                    (500.0, 180.0),
                    (500.0, 460.0),
                    (240.0, 460.0),
                ],
                from_port: String::new(),
                to_port: String::new(),
                label_x: 0.0,
                label_y: 0.0,
                edge_kind: "assoc".into(),
            },
        ];
        bundle_atom_magistrals(&mut edges, &nodes, &parent_of);
        let api = nodes.iter().find(|n| n.id == "api").unwrap();
        for e in &edges {
            let min_x = e.points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            assert!(
                min_x < api.x,
                "bundled trunk outside api: min_x={min_x} api.x={}",
                api.x
            );
        }
        // Shared outer rail X should match between edges (same xa).
        let rail = |e: &SceneEdge| e.points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        assert!(
            (rail(&edges[0]) - rail(&edges[1])).abs() < 1.0,
            "bundle should share outer rail x"
        );
    }
}
