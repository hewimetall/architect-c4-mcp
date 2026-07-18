//! Per-shell orthogonal router: port → port with child AABBs as obstacles.
//! Uses escape-graph + Dijkstra (visibility-lite).

use crate::collision::{segment_hits_aabb, Aabb, SpatialHash};
use crate::ports::Port;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
struct NodeKey {
    x: i64,
    y: i64,
}

impl NodeKey {
    fn from_f(x: f64, y: f64) -> Self {
        Self {
            x: (x * 2.0).round() as i64,
            y: (y * 2.0).round() as i64,
        }
    }
    fn to_f(self) -> (f64, f64) {
        (self.x as f64 / 2.0, self.y as f64 / 2.0)
    }
}

impl PartialEq for NodeKey {
    fn eq(&self, o: &Self) -> bool {
        self.x == o.x && self.y == o.y
    }
}
impl Eq for NodeKey {}
impl std::hash::Hash for NodeKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
    }
}

#[derive(Clone, PartialEq)]
struct State {
    cost: f64,
    key: NodeKey,
}
impl Eq for State {}
impl Ord for State {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// Route from `from` port to `to` port inside free space; `obstacles` are child cells.
pub fn route_port_to_port(from: &Port, to: &Port, obstacles: &[Aabb], pad: f64) -> Vec<(f64, f64)> {
    // Leaf↔leaf: long stubs so the eye sees which box owns the wire.
    route_port_to_port_stubbed(from, to, obstacles, pad, 28.0)
}

/// Like [`route_port_to_port`], but hierarchical sheet-entry joins use a short/zero stub
/// so concatenated ascend→highway→descend segments do not U-turn on the border.
pub fn route_port_to_port_stubbed(
    from: &Port,
    to: &Port,
    obstacles: &[Aabb],
    pad: f64,
    stub: f64,
) -> Vec<(f64, f64)> {
    let obstacles: Vec<Aabb> = obstacles.iter().map(|b| b.inflate(pad)).collect();
    let hash = SpatialHash::build(&obstacles, 64.0);

    let start = stub_point(from, stub);
    let goal = stub_point(to, stub);

    // Escape nodes: stubs + inflated corners
    let mut points: Vec<(f64, f64)> = vec![(from.x, from.y), start, goal, (to.x, to.y)];
    for b in &obstacles {
        points.push((b.x0, b.y0));
        points.push((b.x1, b.y0));
        points.push((b.x0, b.y1));
        points.push((b.x1, b.y1));
        // mid-side escapes
        points.push((b.x0, (b.y0 + b.y1) * 0.5));
        points.push((b.x1, (b.y0 + b.y1) * 0.5));
        points.push(((b.x0 + b.x1) * 0.5, b.y0));
        points.push(((b.x0 + b.x1) * 0.5, b.y1));
    }

    // Dedup keys
    let mut keys: Vec<NodeKey> = points.iter().map(|p| NodeKey::from_f(p.0, p.1)).collect();
    keys.sort_by_key(|a| (a.x, a.y));
    keys.dedup();

    let clear = |a: (f64, f64), b: (f64, f64)| -> bool {
        let seg = Aabb {
            x0: a.0.min(b.0),
            y0: a.1.min(b.1),
            x1: a.0.max(b.0),
            y1: a.1.max(b.1),
        }
        .inflate(1.0);
        for idx in hash.query_aabb(&seg) {
            if segment_hits_aabb(a, b, &obstacles[idx]) {
                return false;
            }
        }
        true
    };

    // Strict H/V only (same quantized x OR same quantized y) — no diagonal shortcuts.
    let mut adj: HashMap<NodeKey, Vec<(NodeKey, f64)>> = HashMap::new();
    let link = |adj: &mut HashMap<NodeKey, Vec<(NodeKey, f64)>>, a: NodeKey, b: NodeKey| {
        if a == b {
            return;
        }
        let af = a.to_f();
        let bf = b.to_f();
        let orth = a.x == b.x || a.y == b.y;
        if !orth || !clear(af, bf) {
            return;
        }
        let dist = (af.0 - bf.0).abs() + (af.1 - bf.1).abs();
        adj.entry(a).or_default().push((b, dist));
        adj.entry(b).or_default().push((a, dist));
    };
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            link(&mut adj, keys[i], keys[j]);
        }
    }

    let start_k = NodeKey::from_f(from.x, from.y);
    let goal_k = NodeKey::from_f(to.x, to.y);
    let start_stub = NodeKey::from_f(start.0, start.1);
    let goal_stub = NodeKey::from_f(goal.0, goal.1);
    // Port ↔ stub only (never stub→stub diagonal).
    link(&mut adj, start_k, start_stub);
    link(&mut adj, goal_stub, goal_k);
    // Orthogonal elbow between stubs if they share a row/col; else via a corner key.
    if start_stub.x == goal_stub.x || start_stub.y == goal_stub.y {
        link(&mut adj, start_stub, goal_stub);
    } else {
        let corner_hv = NodeKey::from_f(goal.0, start.1); // H then V
        let corner_vh = NodeKey::from_f(start.0, goal.1); // V then H
        keys.push(corner_hv);
        keys.push(corner_vh);
        link(&mut adj, start_stub, corner_hv);
        link(&mut adj, corner_hv, goal_stub);
        link(&mut adj, start_stub, corner_vh);
        link(&mut adj, corner_vh, goal_stub);
    }

    let path = dijkstra(&adj, start_k, goal_k).unwrap_or_else(|| {
        // Fallback: pure HV around obstacles (always right angles).
        let top = obstacles
            .iter()
            .map(|b| b.y0)
            .fold(start.1.min(goal.1), f64::min)
            - 20.0;
        vec![
            (from.x, from.y),
            start,
            (start.0, top),
            (goal.0, top),
            goal,
            (to.x, to.y),
        ]
    });
    simplify(&ensure_orthogonal(&path))
}

/// Insert corner bends so every segment is axis-aligned (no diagonals).
fn ensure_orthogonal(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.is_empty() {
        return vec![];
    }
    let mut out = vec![pts[0]];
    for &p in pts.iter().skip(1) {
        let last = *out.last().unwrap();
        let same_x = (last.0 - p.0).abs() < 0.05;
        let same_y = (last.1 - p.1).abs() < 0.05;
        if same_x || same_y {
            // Snap to exact axis to avoid hairline diagonals from float noise.
            if same_x {
                out.push((last.0, p.1));
            } else {
                out.push((p.0, last.1));
            }
        } else {
            // HV elbow (horizontal first).
            out.push((p.0, last.1));
            out.push(p);
        }
    }
    out
}

fn stub_point(p: &Port, stub: f64) -> (f64, f64) {
    match p.side {
        Side::E => (p.x + stub, p.y),
        Side::W => (p.x - stub, p.y),
        Side::N => (p.x, p.y - stub),
        Side::S => (p.x, p.y + stub),
    }
}

use crate::Side;

fn dijkstra(
    adj: &HashMap<NodeKey, Vec<(NodeKey, f64)>>,
    start: NodeKey,
    goal: NodeKey,
) -> Option<Vec<(f64, f64)>> {
    let mut dist: HashMap<NodeKey, f64> = HashMap::new();
    let mut prev: HashMap<NodeKey, NodeKey> = HashMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(start, 0.0);
    heap.push(State {
        cost: 0.0,
        key: start,
    });
    let mut seen = HashSet::new();
    while let Some(State { cost, key }) = heap.pop() {
        if !seen.insert(key) {
            continue;
        }
        if key == goal {
            let mut path = vec![goal.to_f()];
            let mut cur = goal;
            while let Some(&p) = prev.get(&cur) {
                path.push(p.to_f());
                cur = p;
            }
            path.reverse();
            return Some(path);
        }
        for (n, w) in adj.get(&key).into_iter().flatten() {
            let nd = cost + *w;
            if dist.get(n).map(|d| nd < *d).unwrap_or(true) {
                dist.insert(*n, nd);
                prev.insert(*n, key);
                heap.push(State { cost: nd, key: *n });
            }
        }
    }
    None
}

fn simplify(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for w in pts.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let colinear = ((a.0 - b.0).abs() < 0.6 && (b.0 - c.0).abs() < 0.6)
            || ((a.1 - b.1).abs() < 0.6 && (b.1 - c.1).abs() < 0.6);
        if !colinear {
            out.push(b);
        }
    }
    out.push(*pts.last().unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::allocate_side_ports;
    use crate::Side;

    #[test]
    fn routes_around_blocker() {
        let from = &allocate_side_ports("a", Side::E, 0.0, 0.0, 40.0, 40.0, 1)[0];
        let to = &allocate_side_ports("b", Side::W, 200.0, 0.0, 40.0, 40.0, 1)[0];
        let blocker = Aabb {
            x0: 70.0,
            y0: 0.0,
            x1: 130.0,
            y1: 80.0,
        };
        let path = route_port_to_port(from, to, &[blocker], 4.0);
        assert!(path.len() >= 3);
        // Should not be a single piercing horizontal through blocker center
        // Path should detour above the blocker (min y below blocker's top).
        let min_y = path.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!(
            min_y < blocker.y0 - 1.0,
            "expected detour above blocker, path={path:?}"
        );
    }

    #[test]
    fn all_segments_are_orthogonal() {
        let from = &allocate_side_ports("a", Side::E, 0.0, 10.0, 40.0, 40.0, 1)[0];
        let to = &allocate_side_ports("b", Side::S, 120.0, 0.0, 40.0, 40.0, 1)[0];
        let path = route_port_to_port(from, to, &[], 4.0);
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let orth = (a.0 - b.0).abs() < 0.05 || (a.1 - b.1).abs() < 0.05;
            assert!(orth, "diagonal segment {a:?} -> {b:?} in {path:?}");
        }
    }
}
