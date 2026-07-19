//! Per-container **left bus rails** (schematic hierarchical buses).
//!
//! Pass 1 — walk matryoshka groups, assign each its own left rail (`bus_x`).
//! Pass 2 — route nets on those rails; never merge different owners onto one X.
//!
//! Rules: KiCad hierarchy splits buses; Altium sheet-entry vertical connectivity.

use crate::collision::Aabb;
use crate::ports::{pick_port_avoiding, side_facing, Port};
use crate::{SceneNode, Side};
use std::collections::{HashMap, HashSet};

type Routed = (Port, Port, Vec<(f64, f64)>);

fn track_signed(track: usize) -> f64 {
    if track == 0 {
        0.0
    } else if track % 2 == 1 {
        track.div_ceil(2) as f64
    } else {
        -((track / 2) as f64)
    }
}

pub(crate) fn ensure_orthogonal_poly(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.is_empty() {
        return vec![];
    }
    let mut out = vec![pts[0]];
    for &p in pts.iter().skip(1) {
        let last = *out.last().unwrap();
        let same_x = (last.0 - p.0).abs() < 0.05;
        let same_y = (last.1 - p.1).abs() < 0.05;
        if same_x {
            out.push((last.0, p.1));
        } else if same_y {
            out.push((p.0, last.1));
        } else {
            out.push((p.0, last.1));
            out.push(p);
        }
    }
    out
}

pub(crate) fn clean_polyline(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for &p in pts {
        if let Some(&last) = out.last() {
            if (last.0 - p.0).abs() < 0.5 && (last.1 - p.1).abs() < 0.5 {
                continue;
            }
        }
        out.push(p);
        while out.len() >= 3 {
            let n = out.len();
            let (a, b, c) = (out[n - 3], out[n - 2], out[n - 1]);
            let col_x = (a.0 - b.0).abs() < 0.5 && (b.0 - c.0).abs() < 0.5;
            let col_y = (a.1 - b.1).abs() < 0.5 && (b.1 - c.1).abs() < 0.5;
            if !(col_x || col_y) {
                break;
            }
            let between = if col_x {
                let (lo, hi) = (a.1.min(c.1), a.1.max(c.1));
                b.1 >= lo - 0.5 && b.1 <= hi + 0.5
            } else {
                let (lo, hi) = (a.0.min(c.0), a.0.max(c.0));
                b.0 >= lo - 0.5 && b.0 <= hi + 0.5
            };
            if between {
                // Redundant midpoint on A—C.
                out.remove(n - 2);
            } else {
                // Elbow outside A—C (left-bus tap) — keep B.
                break;
            }
        }
    }
    out
}

/// Mid-gap trunk between two sibling blocks (does not merge their left rails).
pub(crate) fn channel_rails(
    a: &SceneNode,
    b: &SceneNode,
    from: (f64, f64),
    to: (f64, f64),
    track: usize,
    pitch: f64,
) -> Vec<(f64, f64)> {
    let off = track_signed(track) * pitch;
    let stacked = (b.y >= a.y + a.h - 2.0) || (a.y >= b.y + b.h - 2.0);
    if stacked {
        let (top, bot) = if a.y + a.h <= b.y + 2.0 {
            (a.y + a.h, b.y)
        } else {
            (b.y + b.h, a.y)
        };
        let gap = (bot - top).max(8.0);
        let y = top + gap / 2.0 + off;
        clean_polyline(&[from, (from.0, y), (to.0, y), to])
    } else {
        let (left, right) = if a.x + a.w <= b.x + 2.0 {
            (a.x + a.w, b.x)
        } else {
            (b.x + b.w, a.x)
        };
        let gap = (right - left).max(8.0);
        let x = left + gap / 2.0 + off;
        clean_polyline(&[from, (x, from.1), (x, to.1), to])
    }
}

/// Vertical bus rail owned by one matryoshka group (container / component shell).
#[derive(Debug, Clone)]
pub struct BusRail {
    pub x: f64,
    pub y0: f64,
    pub y1: f64,
}

/// Legacy inset (inside pad); magistrals use [`OUTER_BUS_GUTTER`].
#[allow(dead_code)]
pub const LEFT_BUS_INSET: f64 = 22.0;
/// Distance **left of** the shell border for the outer magistral rail (outside the dashed box).
pub const OUTER_BUS_GUTTER: f64 = 40.0;
/// Parallel nets on the **same** owner rail — further left (never merge owners).
pub const RAIL_TRACK_PITCH: f64 = 10.0;

fn shell_nodes(nodes: &[SceneNode]) -> impl Iterator<Item = &SceneNode> {
    nodes.iter().filter(|n| {
        n.group
            || n.kind == "container"
            || n.kind == "software_system"
            || (n.kind == "component" && n.group)
    })
}

/// Left outer rails (`group.x - gutter`).
pub fn allocate_left_buses(nodes: &[SceneNode]) -> HashMap<String, BusRail> {
    let header: f64 = 48.0;
    let mut rails = HashMap::new();
    for n in shell_nodes(nodes) {
        let x = n.x - OUTER_BUS_GUTTER;
        let y0 = n.y + f64::min(header, n.h * 0.25);
        let y1 = (n.y + n.h - 8.0).max(y0 + 8.0);
        rails.insert(n.id.clone(), BusRail { x, y0, y1 });
    }
    rails
}

/// Right outer rails (`group.x + group.w + gutter`) for long links.
pub fn allocate_right_buses(nodes: &[SceneNode]) -> HashMap<String, BusRail> {
    let header: f64 = 48.0;
    let mut rails = HashMap::new();
    for n in shell_nodes(nodes) {
        let x = n.x + n.w + OUTER_BUS_GUTTER;
        let y0 = n.y + f64::min(header, n.h * 0.25);
        let y1 = (n.y + n.h - 8.0).max(y0 + 8.0);
        rails.insert(n.id.clone(), BusRail { x, y0, y1 });
    }
    rails
}

fn clamp_y(rail: &BusRail, y: f64) -> f64 {
    y.clamp(rail.y0, rail.y1)
}

pub(crate) fn rail_x(rail: &BusRail, track: usize) -> f64 {
    // Left rails: fan further LEFT. Callers for right rails use [`rail_x_right`].
    rail.x - track as f64 * RAIL_TRACK_PITCH
}

pub(crate) fn rail_x_right(rail: &BusRail, track: usize) -> f64 {
    rail.x + track as f64 * RAIL_TRACK_PITCH
}

/// Pick LEFT vs RIGHT magistral by midpoint vs parent center (SNS).
pub fn pick_long_side(a: &SceneNode, b: &SceneNode, parent: Option<&SceneNode>) -> bool {
    // true = LEFT, false = RIGHT
    let mid = (a.x + a.w / 2.0 + b.x + b.w / 2.0) * 0.5;
    if let Some(p) = parent {
        mid < p.x + p.w * 0.5
    } else {
        true
    }
}

/// Adjacent in pack: share a side with a small gap (neighbor channel TOP/BOTTOM or side).
pub fn neighbor_kind(a: &SceneNode, b: &SceneNode) -> Option<&'static str> {
    const ADJ: f64 = 220.0;
    let y_overlap = a.y < b.y + b.h && b.y < a.y + a.h;
    let x_overlap = a.x < b.x + b.w && b.x < a.x + a.w;
    let h_gap = if a.x + a.w <= b.x + 1.0 {
        b.x - (a.x + a.w)
    } else if b.x + b.w <= a.x + 1.0 {
        a.x - (b.x + b.w)
    } else {
        -1.0
    };
    let v_gap = if a.y + a.h <= b.y + 1.0 {
        b.y - (a.y + a.h)
    } else if b.y + b.h <= a.y + 1.0 {
        a.y - (b.y + b.h)
    } else {
        -1.0
    };
    if y_overlap && (0.0..ADJ).contains(&h_gap) {
        Some("horizontal") // channel above/below
    } else if x_overlap && (0.0..ADJ).contains(&v_gap) {
        Some("vertical") // channel left/right between stacked
    } else {
        None
    }
}

/// Border attach points for neighbor routing (never box centers).
/// Returns `(from_pt, from_side, to_pt, to_side)`.
#[allow(clippy::type_complexity)]
pub fn neighbor_attach_points(
    a: &SceneNode,
    b: &SceneNode,
) -> Option<((f64, f64), Side, (f64, f64), Side)> {
    match neighbor_kind(a, b)? {
        "horizontal" => {
            if a.x + a.w <= b.x + 1.0 {
                Some((
                    (a.x + a.w, a.y + a.h / 2.0),
                    Side::E,
                    (b.x, b.y + b.h / 2.0),
                    Side::W,
                ))
            } else {
                Some((
                    (a.x, a.y + a.h / 2.0),
                    Side::W,
                    (b.x + b.w, b.y + b.h / 2.0),
                    Side::E,
                ))
            }
        }
        "vertical" => {
            if a.y + a.h <= b.y + 1.0 {
                Some((
                    (a.x + a.w / 2.0, a.y + a.h),
                    Side::S,
                    (b.x + b.w / 2.0, b.y),
                    Side::N,
                ))
            } else {
                Some((
                    (a.x + a.w / 2.0, a.y),
                    Side::N,
                    (b.x + b.w / 2.0, b.y + b.h),
                    Side::S,
                ))
            }
        }
        _ => None,
    }
}

/// Dead-short channel between neighboring boxes (gap between them — no outer U-turn).
/// Horizontal neighbors: vertical jog in the **middle of the gap**.
/// Vertical neighbors: horizontal jog in the **middle of the gap**.
pub fn route_neighbor_channel(
    a: &SceneNode,
    b: &SceneNode,
    from: (f64, f64),
    to: (f64, f64),
    track: usize,
) -> Vec<(f64, f64)> {
    let off = track as f64 * RAIL_TRACK_PITCH;
    match neighbor_kind(a, b) {
        Some("horizontal") => {
            let (left, right) = if a.x + a.w <= b.x + 1.0 {
                (a, b)
            } else {
                (b, a)
            };
            let mid_x = (left.x + left.w + right.x) * 0.5 + off;
            ensure_orthogonal_poly(&clean_polyline(&[from, (mid_x, from.1), (mid_x, to.1), to]))
        }
        Some("vertical") => {
            let (top, bot) = if a.y + a.h <= b.y + 1.0 {
                (a, b)
            } else {
                (b, a)
            };
            let mid_y = (top.y + top.h + bot.y) * 0.5 + off;
            ensure_orthogonal_poly(&clean_polyline(&[from, (from.0, mid_y), (to.0, mid_y), to]))
        }
        _ => ensure_orthogonal_poly(&clean_polyline(&[from, (to.0, from.1), to])),
    }
}

/// All nodes that are strict descendants of `shell` (any depth).
#[allow(dead_code)]
pub fn obstacles_descendants(
    shell: &str,
    exclude: &HashSet<&str>,
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) -> Vec<Aabb> {
    nodes
        .iter()
        .filter(|n| n.id != shell && !exclude.contains(n.id.as_str()))
        .filter(|n| is_strict_descendant(&n.id, shell, parent_of))
        .map(|n| Aabb::from_node(n, 4.0))
        .collect()
}

pub fn is_strict_descendant(
    id: &str,
    ancestor: &str,
    parent_of: &HashMap<String, Option<String>>,
) -> bool {
    let mut cur = parent_of.get(id).cloned().flatten();
    while let Some(p) = cur {
        if p == ancestor {
            return true;
        }
        cur = parent_of.get(&p).cloned().flatten();
    }
    false
}

pub fn is_ancestor(anc: &str, desc: &str, parent_of: &HashMap<String, Option<String>>) -> bool {
    is_strict_descendant(desc, anc, parent_of) || anc == desc
}

fn take_westish(
    bag: &HashMap<String, Vec<Port>>,
    node: &SceneNode,
    prefer_y: f64,
    used: &mut HashSet<String>,
) -> Option<Port> {
    let ports = bag.get(&node.id)?;
    // Prefer West (toward left bus); fallback facing left edge of node.
    let side = Side::W;
    if let Some(p) = pick_port_avoiding(ports, side, prefer_y, used) {
        used.insert(p.id.clone());
        return Some(p.clone());
    }
    let toward = (node.x - 10.0, prefer_y);
    let side = side_facing(node.x, node.y, node.w, node.h, toward);
    let p = pick_port_avoiding(ports, side, prefer_y, used)
        .cloned()
        .or_else(|| ports.first().cloned())?;
    used.insert(p.id.clone());
    Some(p)
}

/// Leaf/port → vertical rail → other port (same owner rail).
pub fn route_on_rail(from: &Port, to: &Port, rail: &BusRail, track: usize) -> Vec<(f64, f64)> {
    let x = rail_x(rail, track);
    let y0 = clamp_y(rail, from.y);
    let y1 = clamp_y(rail, to.y);
    clean_polyline(&[
        (from.x, from.y),
        (x, from.y),
        (x, y0),
        (x, y1),
        (x, to.y),
        (to.x, to.y),
    ])
}

/// Parent → child (or reverse): stay on **parent** left rail; never pierce grandchildren.
pub fn route_ancestor_on_rail(
    parent: &SceneNode,
    child: &SceneNode,
    parent_is_from: bool,
    bag: &HashMap<String, Vec<Port>>,
    rail: &BusRail,
    track: usize,
    used: &mut HashSet<String>,
) -> Option<Routed> {
    let cy = child.y + child.h / 2.0;
    let py = cy.clamp(parent.y + 8.0, parent.y + parent.h - 8.0);
    let p_parent = take_westish(bag, parent, py, used)?;
    let p_child = take_westish(bag, child, cy, used)?;
    let pts = route_on_rail(&p_parent, &p_child, rail, track);
    if parent_is_from {
        Some((p_parent, p_child, pts))
    } else {
        let mut rev = pts;
        rev.reverse();
        Some((p_child, p_parent, rev))
    }
}

/// Siblings under one shell on the left rail.
/// Kept for inter-container sheet wiring; **not** used for Code↔Code inside a shell
/// (those use classic `route_siblings`).
#[allow(dead_code)]
pub fn route_siblings_on_rail(
    from_n: &SceneNode,
    to_n: &SceneNode,
    bag: &HashMap<String, Vec<Port>>,
    rail: &BusRail,
    track: usize,
    used: &mut HashSet<String>,
) -> Option<Routed> {
    let pa = take_westish(bag, from_n, from_n.y + from_n.h / 2.0, used)?;
    let pb = take_westish(bag, to_n, to_n.y + to_n.h / 2.0, used)?;
    Some((pa.clone(), pb.clone(), route_on_rail(&pa, &pb, rail, track)))
}

fn take_sideish(
    bag: &HashMap<String, Vec<Port>>,
    node: &SceneNode,
    prefer_y: f64,
    left: bool,
    used: &mut HashSet<String>,
) -> Option<Port> {
    let side = if left { Side::W } else { Side::E };
    let ports = bag.get(&node.id)?;
    if let Some(p) = pick_port_avoiding(ports, side, prefer_y, used) {
        used.insert(p.id.clone());
        return Some(p.clone());
    }
    take_westish(bag, node, prefer_y, used)
}

/// Cross-shell: leaf → rail(A) → mid-gap highway → rail(B) → leaf.
/// `use_left`: LEFT outer magistral; otherwise RIGHT.
#[allow(clippy::too_many_arguments)]
pub fn route_cross_on_rails(
    fc: &[String],
    tc: &[String],
    node_by: &HashMap<String, SceneNode>,
    bag: &HashMap<String, Vec<Port>>,
    buses: &HashMap<String, BusRail>,
    track: usize,
    use_left: bool,
    used: &mut HashSet<String>,
) -> Option<Routed> {
    let a_id = fc.last()?;
    let b_id = tc.last()?;
    let a = node_by.get(a_id)?;
    let b = node_by.get(b_id)?;
    let rail_a = buses.get(a_id)?;
    let rail_b = buses.get(b_id)?;

    let leaf_from = node_by.get(&fc[0])?;
    let leaf_to = node_by.get(&tc[0])?;

    let pa = take_sideish(
        bag,
        leaf_from,
        leaf_from.y + leaf_from.h / 2.0,
        use_left,
        used,
    )?;
    let pb = take_sideish(bag, leaf_to, leaf_to.y + leaf_to.h / 2.0, use_left, used)?;

    let mut points = Vec::new();
    let x_a = if use_left {
        rail_x(rail_a, track)
    } else {
        rail_x_right(rail_a, track)
    };
    let x_b = if use_left {
        rail_x(rail_b, track)
    } else {
        rail_x_right(rail_b, track)
    };

    // Ascend fc: leaf → each shell rail (own X!) → next
    // Start: leaf → rail of immediate parent (fc[1] or A if leaf is A)
    let mut cur = (pa.x, pa.y);
    points.push(cur);

    // Hop onto each ancestor rail from leaf up to A (including when leaf == A).
    for shell_id in fc.iter().skip(1).chain(std::iter::once(a_id)) {
        // Avoid double-processing the leaf when it also appears mid-chain.
        if shell_id == &fc[0] && fc.len() > 1 {
            continue;
        }
        let rail = buses.get(shell_id.as_str())?;
        let x = if shell_id == a_id {
            x_a
        } else if use_left {
            rail_x(rail, 0)
        } else {
            rail_x_right(rail, 0)
        };
        let y = clamp_y(rail, cur.1);
        // Always H then V — never diagonal onto the outer rail.
        if (cur.0 - x).abs() > 0.5 {
            points.push((x, cur.1));
        }
        if (cur.1 - y).abs() > 0.5 {
            points.push((x, y));
        }
        cur = (x, y);
    }

    // Exit A toward B: pick Y facing B (mid of A–B gap or centers)
    let exit_y = clamp_y(
        rail_a,
        if (a.y + a.h) <= b.y {
            a.y + a.h
        } else if (b.y + b.h) <= a.y {
            a.y
        } else {
            (a.y + a.h / 2.0 + b.y + b.h / 2.0) / 2.0
        },
    );
    points.push((x_a, exit_y));

    let entry_y = clamp_y(
        rail_b,
        if b.y >= a.y + a.h - 2.0 {
            b.y
        } else if a.y >= b.y + b.h - 2.0 {
            b.y + b.h
        } else {
            exit_y
        },
    );
    // Highway between rails — keeps rail_a.x and rail_b.x distinct (no merge).
    let hwy = channel_rails(a, b, (x_a, exit_y), (x_b, entry_y), track, 14.0);
    for p in hwy.into_iter().skip(1) {
        points.push(p);
    }

    // Descend tc rails from B down to leaf
    let cur = *points.last().unwrap();
    points.push((x_b, cur.1));
    let y_leaf = clamp_y(rail_b, pb.y);
    points.push((x_b, y_leaf));
    let mut cur = (x_b, y_leaf);

    // If deeper than B, hop child rails (tc reversed, skip B)
    for shell_id in tc.iter().rev().skip(1) {
        if shell_id == &tc[0] {
            // final leaf hop below
            break;
        }
        let rail = buses.get(shell_id.as_str())?;
        let x = if use_left {
            rail_x(rail, 0)
        } else {
            rail_x_right(rail, 0)
        };
        let y = clamp_y(rail, pb.y);
        points.push((x, cur.1));
        points.push((x, y));
        cur = (x, y);
    }

    // Final stub to leaf port (H then V).
    if (cur.0 - pb.x).abs() > 0.5 {
        points.push((pb.x, cur.1));
    }
    if (cur.1 - pb.y).abs() > 0.5 || points.last() != Some(&(pb.x, pb.y)) {
        points.push((pb.x, pb.y));
    }

    let points = ensure_orthogonal_poly(&clean_polyline(&points));
    if points.len() < 2 {
        return None;
    }
    Some((pa, pb, points))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, x: f64, y: f64, w: f64, h: f64) -> SceneNode {
        SceneNode {
            id: id.into(),
            kind: "container".into(),
            layer: "container".into(),
            name: id.into(),
            parent_id: None,
            group: true,
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
    fn each_container_gets_own_outer_bus_x() {
        let nodes = vec![
            group("api", 100.0, 0.0, 400.0, 300.0),
            group("db", 600.0, 0.0, 400.0, 300.0),
        ];
        let buses = allocate_left_buses(&nodes);
        let xa = buses["api"].x;
        let xb = buses["db"].x;
        assert!((xa - (100.0 - OUTER_BUS_GUTTER)).abs() < 0.1);
        assert!(xa < 100.0, "rail must be outside shell");
        assert!((xb - (600.0 - OUTER_BUS_GUTTER)).abs() < 0.1);
        assert!((xa - xb).abs() > 100.0, "must not merge into one line");
    }

    #[test]
    fn nested_groups_do_not_share_rail_x() {
        let mut parent = group("rgw", 100.0, 0.0, 800.0, 600.0);
        parent.parent_id = None;
        let mut child = group("usage", 140.0, 80.0, 300.0, 200.0);
        child.parent_id = Some("rgw".into());
        child.depth = 1;
        let buses = allocate_left_buses(&[parent.clone(), child.clone()]);
        assert!(buses["rgw"].x < parent.x);
        assert!(buses["usage"].x < child.x);
        assert!(
            (buses["rgw"].x - buses["usage"].x).abs() > 1.0,
            "nested containers must keep separate rails"
        );
    }

    #[test]
    fn right_bus_sits_outside_shell() {
        let nodes = vec![group("api", 100.0, 0.0, 400.0, 300.0)];
        let right = allocate_right_buses(&nodes);
        assert!((right["api"].x - (100.0 + 400.0 + OUTER_BUS_GUTTER)).abs() < 0.1);
        assert!(rail_x_right(&right["api"], 1) > right["api"].x);
    }

    #[test]
    fn neighbor_kind_detects_horizontal_pair() {
        let a = group("a", 0.0, 0.0, 100.0, 80.0);
        let b = group("b", 140.0, 10.0, 100.0, 80.0);
        assert_eq!(neighbor_kind(&a, &b), Some("horizontal"));
        let c = group("c", 0.0, 200.0, 100.0, 80.0);
        assert_eq!(neighbor_kind(&a, &c), Some("vertical"));
    }
}
