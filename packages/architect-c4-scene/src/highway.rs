//! Hierarchical highway + **per-container left bus rails**.
//!
//! - **Inside** a container/shell: classic matryoshka (`route_siblings` /
//!   `route_highway_path`) — classes connect leaf-port → leaf-port.
//! - **Between** containers / parent→child: left bus rails (no pierce).
//!
//! See `docs/research/schematic-left-bus-rails.md`.

use crate::bus::{
    allocate_left_buses, allocate_right_buses, is_ancestor, neighbor_attach_points, pick_long_side,
    route_ancestor_on_rail, route_cross_on_rails, route_neighbor_channel,
};
use crate::collision::Aabb;
use crate::ports::{
    allocate_side_ports, pick_port_avoiding, port_count_for_degree, side_facing, Port,
};
use crate::router::{route_port_to_port, route_port_to_port_stubbed};
use crate::{SceneEdge, SceneNode, ScenePort, Side};
use architect_c4_domain::Relationship;
use std::collections::{HashMap, HashSet};

/// Walk up to the nearest Container (or SoftwareSystem) ancestor id.
fn enclosing_container_id(
    id: &str,
    node_by: &HashMap<String, SceneNode>,
    parent_of: &HashMap<String, Option<String>>,
) -> Option<String> {
    let mut cur = Some(id.to_string());
    while let Some(cid) = cur {
        if let Some(n) = node_by.get(&cid) {
            if n.kind == "container" || n.kind == "software_system" {
                return Some(cid);
            }
        }
        cur = parent_of.get(&cid).cloned().flatten();
    }
    None
}

/// True when both ends live under the same Container/System — use classic matryoshka
/// routing inside (classes must not ride the left bus).
fn same_enclosing_container(
    a: &str,
    b: &str,
    node_by: &HashMap<String, SceneNode>,
    parent_of: &HashMap<String, Option<String>>,
) -> bool {
    match (
        enclosing_container_id(a, node_by, parent_of),
        enclosing_container_id(b, node_by, parent_of),
    ) {
        (Some(ca), Some(cb)) => ca == cb,
        _ => false,
    }
}

/// `[leaf, …, direct_child_of_lca]` (or `[lca]` when id == lca).
pub fn chain_under_lca(
    id: &str,
    lca: &str,
    parent_of: &HashMap<String, Option<String>>,
) -> Option<Vec<String>> {
    if id == lca {
        return Some(vec![id.to_string()]);
    }
    let mut chain = vec![id.to_string()];
    let mut cur = id.to_string();
    for _ in 0..64 {
        let p = parent_of.get(&cur).cloned().flatten()?;
        if p == lca {
            return Some(chain);
        }
        chain.push(p.clone());
        cur = p;
    }
    None
}

fn append_poly(out: &mut Vec<(f64, f64)>, seg: Vec<(f64, f64)>) {
    if seg.is_empty() {
        return;
    }
    if out.is_empty() {
        out.extend(seg);
        return;
    }
    let last = *out.last().unwrap();
    let skip = if (last.0 - seg[0].0).abs() < 0.75 && (last.1 - seg[0].1).abs() < 0.75 {
        1
    } else {
        0
    };
    out.extend(seg.into_iter().skip(skip));
}

fn center_of(n: &SceneNode) -> (f64, f64) {
    (n.x + n.w / 2.0, n.y + n.h / 2.0)
}

fn obstacles_children(shell: &str, exclude: &HashSet<&str>, nodes: &[SceneNode]) -> Vec<Aabb> {
    nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == Some(shell))
        .filter(|n| !exclude.contains(n.id.as_str()))
        .map(|n| Aabb::from_node(n, 4.0))
        .collect()
}

fn take_port(
    bag: &HashMap<String, Vec<Port>>,
    node_id: &str,
    side: Side,
    prefer: f64,
    used: &mut HashSet<String>,
) -> Option<Port> {
    let ports = bag.get(node_id)?;
    let p = pick_port_avoiding(ports, side, prefer, used)
        .cloned()
        .or_else(|| ports.first().cloned())?;
    used.insert(p.id.clone());
    Some(p)
}

fn prefer_on_side(side: Side, toward: (f64, f64)) -> f64 {
    match side {
        Side::N | Side::S => toward.0,
        Side::E | Side::W => toward.1,
    }
}

/// Signed track offset: 0, +1, −1, +2, −2, …
fn track_signed(track: usize) -> f64 {
    if track == 0 {
        0.0
    } else if track % 2 == 1 {
        track.div_ceil(2) as f64
    } else {
        -((track / 2) as f64)
    }
}

/// Remove U-turns / backtracks from concatenated hierarchical segments.
/// Example: (x,876)→(x,848)→(x,932) collapses to (x,876)→(x,932).
pub fn clean_polyline(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for &p in pts {
        if let Some(&last) = out.last() {
            if (last.0 - p.0).abs() < 0.5 && (last.1 - p.1).abs() < 0.5 {
                continue; // duplicate
            }
        }
        out.push(p);
        // Collapse collinear backtracks: A→B→C on one axis where B is not between A and C.
        while out.len() >= 3 {
            let n = out.len();
            let (a, b, c) = (out[n - 3], out[n - 2], out[n - 1]);
            let col_x = (a.0 - b.0).abs() < 0.5 && (b.0 - c.0).abs() < 0.5;
            let col_y = (a.1 - b.1).abs() < 0.5 && (b.1 - c.1).abs() < 0.5;
            if !col_x && !col_y {
                break;
            }
            let between = if col_x {
                let (lo, hi) = (a.1.min(c.1), a.1.max(c.1));
                b.1 >= lo - 0.5 && b.1 <= hi + 0.5
            } else {
                let (lo, hi) = (a.0.min(c.0), a.0.max(c.0));
                b.0 >= lo - 0.5 && b.0 <= hi + 0.5
            };
            if between && !((a.0 - c.0).abs() < 0.5 && (a.1 - c.1).abs() < 0.5) {
                // B is on the segment A–C: drop B only if A–C is the direct path (collinear progress)
                // Keep B if it's a necessary corner — but on a pure line, B is redundant.
                out.remove(n - 2);
                continue;
            }
            if !between {
                // U-turn: drop B
                out.remove(n - 2);
                continue;
            }
            break;
        }
    }
    // Second pass: drop pure collinear midpoints (A–B–C progressive).
    if out.len() <= 2 {
        return out;
    }
    let mut slim = vec![out[0]];
    for w in out.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let col = ((a.0 - b.0).abs() < 0.5 && (b.0 - c.0).abs() < 0.5)
            || ((a.1 - b.1).abs() < 0.5 && (b.1 - c.1).abs() < 0.5);
        if !col {
            slim.push(b);
        }
    }
    slim.push(*out.last().unwrap());
    slim
}

/// Explicit mid-gap channel between two sibling blocks (schematic магистраль).
/// Trunk sits in the gutter — not on the dashed group border.
pub fn channel_between(
    a: &SceneNode,
    b: &SceneNode,
    from: &Port,
    to: &Port,
    track: usize,
    pitch: f64,
) -> Vec<(f64, f64)> {
    let off = track_signed(track) * pitch;
    let a_cx = a.x + a.w / 2.0;
    let b_cx = b.x + b.w / 2.0;
    let a_cy = a.y + a.h / 2.0;
    let b_cy = b.y + b.h / 2.0;

    // Vertical stack → horizontal trunk in the Y gap.
    let stacked = (b.y >= a.y + a.h - 2.0) || (a.y >= b.y + b.h - 2.0);
    // Side-by-side → vertical trunk in the X gap.
    let side_by_side = (b.x >= a.x + a.w - 2.0) || (a.x >= b.x + b.w - 2.0);

    if stacked || (!side_by_side && (a_cy - b_cy).abs() >= (a_cx - b_cx).abs()) {
        let (top, bot) = if a.y + a.h <= b.y + 2.0 {
            (a.y + a.h, b.y)
        } else if b.y + b.h <= a.y + 2.0 {
            (b.y + b.h, a.y)
        } else {
            // Overlap / nested fallback: mid between centers
            let mid = (a_cy + b_cy) / 2.0 + off;
            return clean_polyline(&[(from.x, from.y), (from.x, mid), (to.x, mid), (to.x, to.y)]);
        };
        let gap = (bot - top).max(8.0);
        let y = top + gap / 2.0 + off;
        clean_polyline(&[(from.x, from.y), (from.x, y), (to.x, y), (to.x, to.y)])
    } else {
        let (left, right) = if a.x + a.w <= b.x + 2.0 {
            (a.x + a.w, b.x)
        } else if b.x + b.w <= a.x + 2.0 {
            (b.x + b.w, a.x)
        } else {
            let mid = (a_cx + b_cx) / 2.0 + off;
            return clean_polyline(&[(from.x, from.y), (mid, from.y), (mid, to.y), (to.x, to.y)]);
        };
        let gap = (right - left).max(8.0);
        let x = left + gap / 2.0 + off;
        clean_polyline(&[(from.x, from.y), (x, from.y), (x, to.y), (to.x, to.y)])
    }
}

fn edge_caption(from_name: &str, to_name: &str, desc: &str, kind: &str) -> String {
    let trunc = |s: &str, max: usize| {
        let t = s.trim();
        if t.chars().count() <= max {
            t.to_string()
        } else {
            let mut o: String = t.chars().take(max.saturating_sub(3)).collect();
            o.push_str("...");
            o
        }
    };
    let kind_tag = match kind {
        "implements" => "«implements»",
        "extends" => "«extends»",
        "composition" => "«composition»",
        "aggregation" => "«aggregation»",
        _ => "",
    };
    let body = trunc(desc, 22);
    let second = if kind_tag.is_empty() {
        body
    } else if body.is_empty() || body.eq_ignore_ascii_case(kind) {
        kind_tag.to_string()
    } else {
        format!("{kind_tag} {body}")
    };
    format!(
        "{} → {}\n{}",
        trunc(from_name, 18),
        trunc(to_name, 18),
        second
    )
}

fn record_port(used_ports: &mut Vec<ScenePort>, seen: &mut HashSet<String>, p: &Port) {
    if seen.insert(p.id.clone()) {
        used_ports.push(ScenePort {
            id: p.id.clone(),
            node_id: p.node_id.clone(),
            x: p.x,
            y: p.y,
        });
    }
}

struct RouteCtx<'a> {
    node_by: &'a HashMap<String, SceneNode>,
    port_bag: &'a HashMap<String, Vec<Port>>,
    nodes_acc: &'a [SceneNode],
    used: &'a mut HashSet<String>,
    track: usize,
}

type RoutedEdge = (Port, Port, Vec<(f64, f64)>);

/// Direct sibling route inside one shell (same parent).
fn route_siblings(
    from_id: &str,
    to_id: &str,
    shell: &str,
    ctx: &mut RouteCtx<'_>,
) -> Option<RoutedEdge> {
    let lf = ctx.node_by.get(from_id)?;
    let lt = ctx.node_by.get(to_id)?;
    let t_to = center_of(lt);
    let t_from = center_of(lf);
    let sa = side_facing(lf.x, lf.y, lf.w, lf.h, t_to);
    let sb = side_facing(lt.x, lt.y, lt.w, lt.h, t_from);
    let pa = take_port(
        ctx.port_bag,
        from_id,
        sa,
        prefer_on_side(sa, t_to),
        ctx.used,
    )?;
    let pb = take_port(
        ctx.port_bag,
        to_id,
        sb,
        prefer_on_side(sb, t_from),
        ctx.used,
    )?;
    let mut excl = HashSet::new();
    excl.insert(from_id);
    excl.insert(to_id);
    let obstacles = obstacles_children(shell, &excl, ctx.nodes_acc);
    // Sibling routes: keep leaf stubs; nudge parallel tracks via channel when gap is clear.
    let mut seg = route_port_to_port(&pa, &pb, &obstacles, 6.0);
    if ctx.track > 0 {
        // Mild lateral nudge of the longest segment so parallel siblings don't paint as one stroke.
        seg = nudge_longest(&seg, ctx.track, 12.0);
    }
    Some((pa, pb, clean_polyline(&seg)))
}

/// Nudge longest axis-aligned segment by track pitch (fallback when not using channel_between).
fn nudge_longest(pts: &[(f64, f64)], track: usize, pitch: f64) -> Vec<(f64, f64)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let off = track_signed(track) * pitch;
    let mut best_i = 0usize;
    let mut best_len = 0.0;
    for (i, w) in pts.windows(2).enumerate() {
        let len = (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs();
        if len > best_len {
            best_len = len;
            best_i = i;
        }
    }
    let a = pts[best_i];
    let b = pts[best_i + 1];
    let horizontal = (a.1 - b.1).abs() < 0.05;
    let mut rebuilt = Vec::with_capacity(pts.len() + 4);
    rebuilt.extend_from_slice(&pts[..=best_i]);
    if horizontal {
        let y = a.1 + off;
        let last = *rebuilt.last().unwrap();
        if (last.1 - y).abs() > 0.05 {
            rebuilt.push((last.0, y));
        } else {
            rebuilt.last_mut().unwrap().1 = y;
        }
        rebuilt.push((b.0, y));
        if (b.1 - y).abs() > 0.05 {
            rebuilt.push(b);
        }
    } else {
        let x = a.0 + off;
        let last = *rebuilt.last().unwrap();
        if (last.0 - x).abs() > 0.05 {
            rebuilt.push((x, last.1));
        } else {
            rebuilt.last_mut().unwrap().0 = x;
        }
        rebuilt.push((x, b.1));
        if (b.0 - x).abs() > 0.05 {
            rebuilt.push(b);
        }
    }
    if best_i + 2 < pts.len() {
        append_poly(&mut rebuilt, pts[best_i + 2..].to_vec());
    }
    rebuilt
}

/// Hierarchical: ascend shells → LCA highway → descend shells.
fn route_highway_path(
    fc: &[String],
    tc: &[String],
    _lca: &str,
    ctx: &mut RouteCtx<'_>,
) -> Option<RoutedEdge> {
    let a_id = fc.last()?;
    let b_id = tc.last()?;
    let a_node = ctx.node_by.get(a_id)?;
    let b_node = ctx.node_by.get(b_id)?;
    let toward_b = center_of(b_node);
    let toward_a = center_of(a_node);

    // Sheet-entry joins use stub≈0 so stubs do not U-turn on the dashed border.
    const SHEET_STUB: f64 = 4.0;
    const LEAF_STUB: f64 = 16.0;
    const TRACK_PITCH: f64 = 14.0;

    let mut points = Vec::new();

    // Leaf / start port on fc[0]
    let leaf = ctx.node_by.get(&fc[0])?;
    let side0 = side_facing(leaf.x, leaf.y, leaf.w, leaf.h, toward_b);
    let mut prev = take_port(
        ctx.port_bag,
        &fc[0],
        side0,
        prefer_on_side(side0, toward_b),
        ctx.used,
    )?;
    let start_port = prev.clone();

    // Ascend: inside each shell fc[j], child fc[j-1] → exit pin on fc[j]
    for j in 1..fc.len() {
        let shell_id = &fc[j];
        let child_id = &fc[j - 1];
        let shell = ctx.node_by.get(shell_id)?;
        let exit_side = side_facing(shell.x, shell.y, shell.w, shell.h, toward_b);
        let exit = take_port(
            ctx.port_bag,
            shell_id,
            exit_side,
            prefer_on_side(exit_side, toward_b),
            ctx.used,
        )?;
        let mut excl = HashSet::new();
        excl.insert(child_id.as_str());
        let obstacles = obstacles_children(shell_id, &excl, ctx.nodes_acc);
        // Leaf end keeps a visible stub; sheet-entry end is short.
        let stub = if j == 1 { LEAF_STUB } else { SHEET_STUB };
        let seg = route_port_to_port_stubbed(&prev, &exit, &obstacles, 6.0, stub);
        append_poly(&mut points, seg);
        prev = exit;
    }
    if points.is_empty() {
        points.push((prev.x, prev.y));
    }

    // Highway at LCA: explicit mid-gap channel (not Dijkstra along the border).
    if a_id != b_id {
        let entry_side = side_facing(b_node.x, b_node.y, b_node.w, b_node.h, toward_a);
        let entry = take_port(
            ctx.port_bag,
            b_id,
            entry_side,
            prefer_on_side(entry_side, toward_a),
            ctx.used,
        )?;
        let seg = channel_between(a_node, b_node, &prev, &entry, ctx.track, TRACK_PITCH);
        append_poly(&mut points, seg);
        prev = entry;
    }

    // Descend: for j from len-1 down to 1 — reverse(child → shell_border)
    let mut final_port = prev.clone();
    for j in (1..tc.len()).rev() {
        let shell_id = &tc[j];
        let child_id = &tc[j - 1];
        let child = ctx.node_by.get(child_id)?;
        let toward_entry = (prev.x, prev.y);
        let child_side = side_facing(child.x, child.y, child.w, child.h, toward_entry);
        let child_port = take_port(
            ctx.port_bag,
            child_id,
            child_side,
            prefer_on_side(child_side, toward_entry),
            ctx.used,
        )?;
        let shell_port = if j == tc.len() - 1 && a_id != b_id {
            prev.clone()
        } else {
            let shell = ctx.node_by.get(shell_id)?;
            let side = side_facing(shell.x, shell.y, shell.w, shell.h, toward_a);
            take_port(
                ctx.port_bag,
                shell_id,
                side,
                prefer_on_side(side, toward_a),
                ctx.used,
            )?
        };
        let mut excl = HashSet::new();
        excl.insert(child_id.as_str());
        let obstacles = obstacles_children(shell_id, &excl, ctx.nodes_acc);
        let stub = if j == 1 { LEAF_STUB } else { SHEET_STUB };
        let mut seg = route_port_to_port_stubbed(&child_port, &shell_port, &obstacles, 6.0, stub);
        seg.reverse();
        append_poly(&mut points, seg);
        prev = child_port.clone();
        final_port = child_port;
    }

    let points = clean_polyline(&points);
    if points.len() < 2 {
        return None;
    }
    Some((start_port, final_port, points))
}

/// Route all relationships with leaf pins + hierarchical highways.
pub fn route_all_highway(
    nodes_acc: &[SceneNode],
    relationships: &[Relationship],
    parent_of: &HashMap<String, Option<String>>,
    lca_fn: &dyn Fn(&str, &str) -> Option<String>,
) -> (Vec<SceneEdge>, Vec<ScenePort>) {
    let node_by: HashMap<String, SceneNode> = nodes_acc
        .iter()
        .cloned()
        .map(|n| (n.id.clone(), n))
        .collect();
    let visible: HashSet<&str> = nodes_acc.iter().map(|n| n.id.as_str()).collect();

    let mut degree: HashMap<String, usize> = HashMap::new();
    let mut track_of: HashMap<usize, usize> = HashMap::new();
    let mut hwy_count: HashMap<(String, String, String), usize> = HashMap::new();
    let mut rail_count: HashMap<String, usize> = HashMap::new();

    for (idx, rel) in relationships.iter().enumerate() {
        if !visible.contains(rel.from_id.as_str()) || !visible.contains(rel.to_id.as_str()) {
            continue;
        }
        let Some(lca) = lca_fn(&rel.from_id, &rel.to_id) else {
            continue;
        };
        let Some(fc) = chain_under_lca(&rel.from_id, &lca, parent_of) else {
            continue;
        };
        let Some(tc) = chain_under_lca(&rel.to_id, &lca, parent_of) else {
            continue;
        };
        for id in fc.iter().chain(tc.iter()) {
            *degree.entry(id.clone()).or_default() += 1;
        }

        // Ancestor / same-parent → tracks on one owner rail (do not merge to 1 X).
        // Cross-shell → tracks per (lca, A, B) highway pair.
        if is_ancestor(&rel.from_id, &rel.to_id, parent_of)
            || is_ancestor(&rel.to_id, &rel.from_id, parent_of)
            || parent_of.get(&rel.from_id).cloned().flatten()
                == parent_of.get(&rel.to_id).cloned().flatten()
        {
            let owner = if is_ancestor(&rel.from_id, &rel.to_id, parent_of) {
                rel.from_id.clone()
            } else if is_ancestor(&rel.to_id, &rel.from_id, parent_of) {
                rel.to_id.clone()
            } else {
                parent_of
                    .get(&rel.from_id)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| lca.clone())
            };
            let t = *rail_count.get(&owner).unwrap_or(&0);
            track_of.insert(idx, t);
            rail_count.insert(owner, t + 1);
        } else {
            let a = fc.last().cloned().unwrap_or_default();
            let b = tc.last().cloned().unwrap_or_default();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let key = (lca, lo, hi);
            let t = *hwy_count.get(&key).unwrap_or(&0);
            track_of.insert(idx, t);
            hwy_count.insert(key, t + 1);
        }
    }

    // Pass 1 — LEFT and RIGHT outer rails per shell (SNS).
    let left_buses = allocate_left_buses(nodes_acc);
    let right_buses = allocate_right_buses(nodes_acc);

    let mut port_bag: HashMap<String, Vec<Port>> = HashMap::new();
    for n in nodes_acc {
        let d = degree.get(&n.id).copied().unwrap_or(1);
        let count = port_count_for_degree(d);
        let mut ports = Vec::new();
        for side in [Side::N, Side::E, Side::S, Side::W] {
            ports.extend(allocate_side_ports(&n.id, side, n.x, n.y, n.w, n.h, count));
        }
        port_bag.insert(n.id.clone(), ports);
    }

    let mut scene_edges = Vec::new();
    let mut used_ports = Vec::new();
    let mut used_ids = HashSet::new();
    let mut port_seen = HashSet::new();

    for (idx, rel) in relationships.iter().enumerate() {
        if !visible.contains(rel.from_id.as_str()) || !visible.contains(rel.to_id.as_str()) {
            continue;
        }
        if rel.from_id == rel.to_id {
            continue;
        }
        let Some(lca) = lca_fn(&rel.from_id, &rel.to_id) else {
            continue;
        };
        let Some(fc) = chain_under_lca(&rel.from_id, &lca, parent_of) else {
            continue;
        };
        let Some(tc) = chain_under_lca(&rel.to_id, &lca, parent_of) else {
            continue;
        };
        let track = track_of.get(&idx).copied().unwrap_or(0);

        let routed: Option<RoutedEdge> = if is_ancestor(&rel.from_id, &rel.to_id, parent_of) {
            match (
                node_by.get(&rel.from_id),
                node_by.get(&rel.to_id),
                left_buses.get(&rel.from_id),
            ) {
                (Some(parent), Some(child), Some(rail)) => route_ancestor_on_rail(
                    parent,
                    child,
                    true,
                    &port_bag,
                    rail,
                    track,
                    &mut used_ids,
                ),
                _ => None,
            }
        } else if is_ancestor(&rel.to_id, &rel.from_id, parent_of) {
            match (
                node_by.get(&rel.to_id),
                node_by.get(&rel.from_id),
                left_buses.get(&rel.to_id),
            ) {
                (Some(parent), Some(child), Some(rail)) => route_ancestor_on_rail(
                    parent,
                    child,
                    false,
                    &port_bag,
                    rail,
                    track,
                    &mut used_ids,
                ),
                _ => None,
            }
        } else {
            let same_parent = parent_of.get(&rel.from_id).cloned().flatten()
                == parent_of.get(&rel.to_id).cloned().flatten();
            // Sibling containers under one system → outer magistral (not classic siblings).
            let sibling_shells = same_parent
                && node_by.get(&rel.from_id).is_some_and(|n| {
                    n.kind == "container"
                        || (n.group && (n.kind == "component" || n.kind == "software_system"))
                })
                && node_by.get(&rel.to_id).is_some_and(|n| {
                    n.kind == "container"
                        || (n.group && (n.kind == "component" || n.kind == "software_system"))
                })
                && left_buses.contains_key(&rel.from_id)
                && left_buses.contains_key(&rel.to_id);

            if sibling_shells {
                let a = node_by.get(&rel.from_id).unwrap();
                let b = node_by.get(&rel.to_id).unwrap();
                let parent = parent_of
                    .get(&rel.from_id)
                    .cloned()
                    .flatten()
                    .and_then(|p| node_by.get(&p));
                let use_left = pick_long_side(a, b, parent);
                let buses = if use_left { &left_buses } else { &right_buses };
                route_cross_on_rails(
                    std::slice::from_ref(&rel.from_id),
                    std::slice::from_ref(&rel.to_id),
                    &node_by,
                    &port_bag,
                    buses,
                    track,
                    use_left,
                    &mut used_ids,
                )
            } else if same_parent {
                // Inside one shell: neighbors → TOP/BOTTOM (or side) gutter; else classic.
                let shell = parent_of
                    .get(&rel.from_id)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| lca.clone());
                if let (Some(fn_), Some(tn)) = (node_by.get(&rel.from_id), node_by.get(&rel.to_id))
                {
                    if let Some((from_pt, from_side, to_pt, to_side)) =
                        neighbor_attach_points(fn_, tn)
                    {
                        let pts = route_neighbor_channel(fn_, tn, from_pt, to_pt, track);
                        let pa = Port {
                            id: format!("{}:nb:0", rel.from_id),
                            node_id: rel.from_id.clone(),
                            side: from_side,
                            slot: 0,
                            x: from_pt.0,
                            y: from_pt.1,
                        };
                        let pb = Port {
                            id: format!("{}:nb:0", rel.to_id),
                            node_id: rel.to_id.clone(),
                            side: to_side,
                            slot: 0,
                            x: to_pt.0,
                            y: to_pt.1,
                        };
                        used_ids.insert(pa.id.clone());
                        used_ids.insert(pb.id.clone());
                        Some((pa, pb, pts))
                    } else {
                        let mut ctx = RouteCtx {
                            node_by: &node_by,
                            port_bag: &port_bag,
                            nodes_acc,
                            used: &mut used_ids,
                            track,
                        };
                        route_siblings(&rel.from_id, &rel.to_id, &shell, &mut ctx)
                    }
                } else {
                    let mut ctx = RouteCtx {
                        node_by: &node_by,
                        port_bag: &port_bag,
                        nodes_acc,
                        used: &mut used_ids,
                        track,
                    };
                    route_siblings(&rel.from_id, &rel.to_id, &shell, &mut ctx)
                }
            } else if same_enclosing_container(&rel.from_id, &rel.to_id, &node_by, parent_of) {
                // e.g. Code under CompA → Code under CompB, still inside one Container.
                let mut ctx = RouteCtx {
                    node_by: &node_by,
                    port_bag: &port_bag,
                    nodes_acc,
                    used: &mut used_ids,
                    track,
                };
                route_highway_path(&fc, &tc, &lca, &mut ctx)
            } else if fc.last().is_some_and(|a| left_buses.contains_key(a))
                && tc.last().is_some_and(|b| left_buses.contains_key(b))
            {
                // Distinct containers: LEFT or RIGHT outer magistral (SNS).
                let a = node_by.get(fc.last().unwrap()).unwrap();
                let b = node_by.get(tc.last().unwrap()).unwrap();
                let parent = node_by.get(&lca);
                let use_left = pick_long_side(a, b, parent);
                let buses = if use_left { &left_buses } else { &right_buses };
                route_cross_on_rails(
                    &fc,
                    &tc,
                    &node_by,
                    &port_bag,
                    buses,
                    track,
                    use_left,
                    &mut used_ids,
                )
            } else {
                let mut ctx = RouteCtx {
                    node_by: &node_by,
                    port_bag: &port_bag,
                    nodes_acc,
                    used: &mut used_ids,
                    track,
                };
                route_highway_path(&fc, &tc, &lca, &mut ctx)
            }
        };

        let Some((pa, pb, points)) = routed else {
            continue;
        };
        if points.len() < 2 {
            continue;
        }

        record_port(&mut used_ports, &mut port_seen, &pa);
        record_port(&mut used_ports, &mut port_seen, &pb);

        let from_name = node_by
            .get(&rel.from_id)
            .map(|n| n.name.as_str())
            .unwrap_or(rel.from_id.as_str());
        let to_name = node_by
            .get(&rel.to_id)
            .map(|n| n.name.as_str())
            .unwrap_or(rel.to_id.as_str());
        let desc = rel.description.as_deref().unwrap_or("uses");
        let ek = {
            let d = desc.to_ascii_lowercase();
            if d.contains("composition") || d.contains("composed of") {
                "composition"
            } else if d.contains("aggregation") || d.contains("aggregat") {
                "aggregation"
            } else if d.contains("extends") || d.contains("inherit") {
                "extends"
            } else if d.contains("implements") {
                "implements"
            } else {
                "assoc"
            }
        };

        scene_edges.push(SceneEdge {
            id: rel.id.clone(),
            from: rel.from_id.clone(),
            to: rel.to_id.clone(),
            label: edge_caption(from_name, to_name, desc, ek),
            points,
            from_port: pa.id,
            to_port: pb.id,
            label_x: 0.0,
            label_y: 0.0,
            edge_kind: ek.into(),
        });
    }

    (scene_edges, used_ports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_stops_at_lca_child() {
        let mut parent_of = HashMap::new();
        parent_of.insert("code".into(), Some("comp".into()));
        parent_of.insert("comp".into(), Some("api".into()));
        parent_of.insert("api".into(), Some("sys".into()));
        parent_of.insert("sys".into(), None);
        let c = chain_under_lca("code", "sys", &parent_of).unwrap();
        assert_eq!(c, vec!["code".to_string(), "comp".into(), "api".into()]);
    }

    #[test]
    fn clean_polyline_removes_uturn() {
        let pts = vec![
            (100.0, 876.0),
            (100.0, 848.0), // stub back into block
            (100.0, 932.0),
            (80.0, 932.0),
        ];
        let c = clean_polyline(&pts);
        assert!(
            !c.iter().any(|p| (p.1 - 848.0).abs() < 0.1),
            "U-turn point should be gone: {c:?}"
        );
        assert!(c.len() <= 3, "expected collapsed path, got {c:?}");
    }

    #[test]
    fn channel_between_stacked_uses_mid_gap() {
        let a = SceneNode {
            id: "a".into(),
            kind: "container".into(),
            layer: "container".into(),
            name: "A".into(),
            parent_id: Some("sys".into()),
            group: true,
            depth: 1,
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
            members: vec![],
            stereotype: None,
            url: None,
        };
        let b = SceneNode {
            id: "b".into(),
            kind: "container".into(),
            layer: "container".into(),
            name: "B".into(),
            parent_id: Some("sys".into()),
            group: true,
            depth: 1,
            x: 0.0,
            y: 140.0, // 40px gutter
            w: 200.0,
            h: 100.0,
            members: vec![],
            stereotype: None,
            url: None,
        };
        let from = Port {
            id: "a:s:0".into(),
            node_id: "a".into(),
            side: Side::S,
            slot: 0,
            x: 100.0,
            y: 100.0,
        };
        let to = Port {
            id: "b:n:0".into(),
            node_id: "b".into(),
            side: Side::N,
            slot: 0,
            x: 120.0,
            y: 140.0,
        };
        let path = channel_between(&a, &b, &from, &to, 0, 14.0);
        // Trunk Y must sit in (100, 140) gutter — not on either border.
        let trunk_y = path
            .windows(2)
            .find(|w| (w[0].1 - w[1].1).abs() < 0.05 && (w[0].0 - w[1].0).abs() > 1.0)
            .map(|w| w[0].1)
            .expect("horizontal trunk");
        assert!(
            trunk_y > 100.0 + 1.0 && trunk_y < 140.0 - 1.0,
            "trunk y={trunk_y} not in mid-gap, path={path:?}"
        );
        // Track 1 shifts
        let path1 = channel_between(&a, &b, &from, &to, 1, 14.0);
        let y1 = path1
            .windows(2)
            .find(|w| (w[0].1 - w[1].1).abs() < 0.05 && (w[0].0 - w[1].0).abs() > 1.0)
            .map(|w| w[0].1)
            .unwrap();
        assert!(
            (y1 - trunk_y).abs() > 5.0,
            "tracks must diverge {trunk_y} vs {y1}"
        );
    }
}
