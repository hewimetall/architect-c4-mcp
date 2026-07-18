//! Text AABB sizing + edge-label placement (collision with nodes/labels).
//! Approximate metrics match WASM canvas fonts (13px bold / 11px / 10px).

use crate::collision::Aabb;
use crate::{SceneEdge, SceneNode, ScenePort};

/// Approx advance width for UI sans at `font_px`.
pub fn text_width(s: &str, font_px: f64) -> f64 {
    let mut w = 0.0;
    for ch in s.chars() {
        w += if ch.is_ascii_uppercase() {
            font_px * 0.66
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            font_px * 0.56
        } else {
            font_px * 0.4
        };
    }
    w
}

pub fn text_aabb(x: f64, y: f64, s: &str, font_px: f64) -> Aabb {
    // y is baseline of first line; support `\n` wrapped captions.
    let lines: Vec<&str> = s.split('\n').collect();
    let w = lines
        .iter()
        .map(|ln| text_width(ln, font_px))
        .fold(0.0_f64, f64::max)
        + 8.0;
    let line_h = font_px * 1.2;
    let h = line_h * lines.len().max(1) as f64;
    Aabb {
        x0: x - 2.0,
        y0: y - font_px * 0.85,
        x1: x - 2.0 + w,
        y1: y - font_px * 0.85 + h,
    }
}

/// Leaf box size that fits name + kind with padding for border ports.
pub fn leaf_size_for_text(name: &str, kind: &str, min_w: f64, min_h: f64) -> (f64, f64) {
    let tw = text_width(name, 13.0)
        .max(text_width(kind, 11.0))
        .max(min_w - 36.0);
    // side padding 18*2 + room so text doesn't hit ◇ on E/W
    let w = (tw + 40.0).max(min_w);
    let h = min_h.max(72.0);
    (w, h)
}

/// Group header needs room for "[layer] name".
pub fn header_min_width(name: &str, layer: &str) -> f64 {
    let title = format!("[{layer}] {name}");
    text_width(&title, 13.0) + 48.0
}

fn overlaps_any(box_: &Aabb, others: &[Aabb]) -> bool {
    others.iter().any(|o| box_.overlaps(o))
}

/// Border/header strips for a group — full group AABB is too big (blocks all inner notes),
/// but the dashed frame + title band must still collide (user: container collisions count).
fn group_frame_obstacles(n: &SceneNode) -> Vec<Aabb> {
    let t = 18.0; // border thickness
    let header = 42.0; // "[layer] Name" band
    vec![
        // header / top border (title + dashed line)
        Aabb {
            x0: n.x - 2.0,
            y0: n.y - 2.0,
            x1: n.x + n.w + 2.0,
            y1: n.y + header,
        },
        // bottom border
        Aabb {
            x0: n.x - 2.0,
            y0: n.y + n.h - t,
            x1: n.x + n.w + 2.0,
            y1: n.y + n.h + 2.0,
        },
        // left / right borders
        Aabb {
            x0: n.x - 2.0,
            y0: n.y,
            x1: n.x + t,
            y1: n.y + n.h,
        },
        Aabb {
            x0: n.x + n.w - t,
            y0: n.y,
            x1: n.x + n.w + 2.0,
            y1: n.y + n.h,
        },
    ]
}

/// Place edge labels **anchored to their polyline** (Structurizr: mid-edge).
/// Chips must stay near the edge — never eject to the top of the canvas.
pub fn place_edge_labels(edges: &mut [SceneEdge], nodes: &[SceneNode], ports: &[ScenePort]) {
    let mut placed: Vec<Aabb> = Vec::new();
    const FONT: f64 = 10.0;
    const CHIP_PAD: f64 = 6.0;
    // Max distance from segment midpoint — leash so chips don't float away.
    const LEASH: f64 = 72.0;

    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&i, &j| {
        edges[j]
            .label
            .len()
            .cmp(&edges[i].label.len())
            .then(edges[i].id.cmp(&edges[j].id))
    });

    for &ei in &order {
        let e = &edges[ei];
        if e.points.len() < 2 || e.label.trim().is_empty() {
            edges[ei].label_x = 0.0;
            edges[ei].label_y = 0.0;
            continue;
        }
        let label = e.label.clone();
        let points = e.points.clone();
        let from_id = e.from.clone();
        let to_id = e.to.clone();
        let tw = lines_max_width(&label, FONT);

        // Soft-ignore endpoints so the chip can sit near the edge stubs.
        let endpoint_ids: std::collections::HashSet<&str> =
            [from_id.as_str(), to_id.as_str()].into_iter().collect();
        let local_obs: Vec<Aabb> = nodes
            .iter()
            .filter(|n| !endpoint_ids.contains(n.id.as_str()))
            .flat_map(|n| {
                if n.group {
                    group_frame_obstacles(n)
                } else {
                    vec![Aabb::from_node(n, 4.0)]
                }
            })
            .chain(ports.iter().filter_map(|p| {
                if endpoint_ids.contains(p.node_id.as_str()) {
                    None
                } else {
                    Some(Aabb {
                        x0: p.x - 8.0,
                        y0: p.y - 8.0,
                        x1: p.x + 8.0,
                        y1: p.y + 8.0,
                    })
                }
            }))
            .collect();

        // Anchor = midpoint of longest horizontal segment (else longest any).
        let (ax, ay, bx, by, seg_len, horiz) = longest_segment(&points);
        let mx = (ax + bx) * 0.5;
        let my = (ay + by) * 0.5;

        // Candidates: ONLY local offsets around the anchor (leashed).
        let mut candidates: Vec<(f64, f64, f64)> = Vec::new();
        // Dead rule: prefer exact segment center above the wire, then collide outward.
        if horiz {
            candidates.push((mx - tw * 0.5, my - 14.0, 1000.0 + seg_len));
            for t in [0.5, 0.4, 0.6, 0.35, 0.65] {
                let cx = ax + (bx - ax) * t;
                for (dy, score) in [(-14.0, 100.0), (-22.0, 80.0), (16.0, 40.0), (26.0, 20.0)] {
                    candidates.push((cx - tw * 0.5, my + dy, score + seg_len));
                }
            }
        } else {
            candidates.push((mx - tw - 8.0, my, 1000.0 + seg_len));
            for t in [0.5, 0.4, 0.6, 0.35, 0.65] {
                let cy = ay + (by - ay) * t;
                for (dx, score) in [(-tw - 10.0, 90.0), (10.0, 70.0)] {
                    candidates.push((mx + dx, cy, score + seg_len));
                }
            }
        }
        // Tiny leash spiral around anchor (max LEASH)
        for ring in 1..=3 {
            let step = 12.0 * ring as f64;
            if step > LEASH {
                break;
            }
            for (dx, dy) in [
                (0.0, -step),
                (step, -step),
                (-step, -step),
                (step, 0.0),
                (-step, 0.0),
                (0.0, step),
            ] {
                candidates.push((mx - tw * 0.5 + dx, my - 16.0 + dy, 30.0 - ring as f64));
            }
        }
        candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let free = |box_: &Aabb| {
            let fat = box_.inflate(CHIP_PAD);
            !overlaps_any(&fat, &local_obs) && !overlaps_any(&fat, &placed)
        };
        let leashed = |x: f64, y: f64| {
            let cx = x + tw * 0.5;
            let cy = y;
            (cx - mx).hypot(cy - my) <= LEASH + tw * 0.5
        };

        let mut chosen = None;
        for (x, y, _) in &candidates {
            if !leashed(*x, *y) {
                continue;
            }
            let box_ = text_aabb(*x, *y, &label, FONT);
            if free(&box_) {
                chosen = Some((*x, *y, box_));
                break;
            }
        }
        // Hard anchor: stay on the edge even if slightly overlapping a frame
        if chosen.is_none() {
            let x = mx - tw * 0.5;
            let y = if horiz { my - 16.0 } else { my };
            let box_ = text_aabb(x, y, &label, FONT);
            chosen = Some((x, y, box_));
        }
        if let Some((x, y, box_)) = chosen {
            edges[ei].label_x = x;
            edges[ei].label_y = y;
            placed.push(box_.inflate(CHIP_PAD));
        }
    }
}

fn lines_max_width(s: &str, font_px: f64) -> f64 {
    s.split('\n')
        .map(|ln| text_width(ln, font_px))
        .fold(0.0_f64, f64::max)
}

fn longest_segment(points: &[(f64, f64)]) -> (f64, f64, f64, f64, f64, bool) {
    let mut best = (
        points[0].0,
        points[0].1,
        points[1].0,
        points[1].1,
        0.0,
        false,
    );
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (a.0 - b.0).abs() + (a.1 - b.1).abs();
        let horiz = (a.1 - b.1).abs() < 1.0;
        let score = if horiz { len * 3.0 } else { len };
        let best_score = if best.5 { best.4 * 3.0 } else { best.4 };
        if score > best_score {
            best = (a.0, a.1, b.0, b.1, len, horiz);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_name_widens_leaf() {
        let (w, _) = leaf_size_for_text("BcryptPasswordEncoder", "code", 150.0, 70.0);
        assert!(w > 180.0, "expected wide box, got {w}");
    }

    #[test]
    fn label_stays_near_edge_not_canvas_top() {
        let nodes = vec![SceneNode {
            id: "sec".into(),
            kind: "component".into(),
            layer: "component".into(),
            name: "Security Component".into(),
            parent_id: None,
            group: true,
            depth: 0,
            x: 100.0,
            y: 200.0,
            w: 400.0,
            h: 300.0,

            members: vec![],
            stereotype: None,
            url: None,
        }];
        let mut edges = vec![SceneEdge {
            id: "e".into(),
            from: "a".into(),
            to: "b".into(),
            label: "ActorProxy → Actor\nextends".into(),
            points: vec![(150.0, 350.0), (350.0, 350.0)],
            from_port: String::new(),
            to_port: String::new(),
            label_x: 0.0,
            label_y: 0.0,

            edge_kind: String::new(),
        }];
        place_edge_labels(&mut edges, &nodes, &[]);
        // Must stay near the segment (y≈350), not float to canvas top
        assert!(
            (edges[0].label_y - 350.0).abs() < 40.0,
            "chip escaped edge: y={}",
            edges[0].label_y
        );
    }

    #[test]
    fn label_prefers_above_horizontal_segment() {
        let nodes = vec![];
        let mut edges = vec![SceneEdge {
            id: "e".into(),
            from: "a".into(),
            to: "b".into(),
            label: "uses".into(),
            points: vec![(0.0, 100.0), (200.0, 100.0)],
            from_port: String::new(),
            to_port: String::new(),
            label_x: 0.0,
            label_y: 0.0,

            edge_kind: String::new(),
        }];
        place_edge_labels(&mut edges, &nodes, &[]);
        assert!(
            edges[0].label_y < 100.0,
            "expected above arrow, got y={}",
            edges[0].label_y
        );
        assert!(
            (edges[0].label_y - 100.0).abs() < 40.0,
            "too far from edge: y={}",
            edges[0].label_y
        );
    }

    #[test]
    fn labels_do_not_stack_same_point() {
        let nodes = vec![SceneNode {
            id: "a".into(),
            kind: "container".into(),
            layer: "container".into(),
            name: "A".into(),
            parent_id: None,
            group: false,
            depth: 0,
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 60.0,

            members: vec![],
            stereotype: None,
            url: None,
        }];
        let mut edges = vec![
            SceneEdge {
                id: "e1".into(),
                from: "a".into(),
                to: "a".into(),
                label: "extends".into(),
                points: vec![(120.0, 100.0), (220.0, 100.0)],
                from_port: String::new(),
                to_port: String::new(),
                label_x: 0.0,
                label_y: 0.0,

                edge_kind: String::new(),
            },
            SceneEdge {
                id: "e2".into(),
                from: "a".into(),
                to: "a".into(),
                label: "implements".into(),
                points: vec![(120.0, 100.0), (220.0, 100.0)],
                from_port: String::new(),
                to_port: String::new(),
                label_x: 0.0,
                label_y: 0.0,

                edge_kind: String::new(),
            },
            SceneEdge {
                id: "e3".into(),
                from: "a".into(),
                to: "a".into(),
                label: "Consumes via".into(),
                points: vec![(120.0, 100.0), (220.0, 100.0)],
                from_port: String::new(),
                to_port: String::new(),
                label_x: 0.0,
                label_y: 0.0,

                edge_kind: String::new(),
            },
        ];
        place_edge_labels(&mut edges, &nodes, &[]);
        for i in 0..edges.len() {
            for j in (i + 1)..edges.len() {
                let a = text_aabb(edges[i].label_x, edges[i].label_y, &edges[i].label, 10.0)
                    .inflate(6.0);
                let b = text_aabb(edges[j].label_x, edges[j].label_y, &edges[j].label, 10.0)
                    .inflate(6.0);
                assert!(
                    !a.overlaps(&b),
                    "chip overlap {} @({},{}) vs {} @({},{})",
                    edges[i].label,
                    edges[i].label_x,
                    edges[i].label_y,
                    edges[j].label,
                    edges[j].label_x,
                    edges[j].label_y
                );
            }
        }
    }
}
