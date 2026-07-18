//! Border port slots for matryoshka shells.
//! Viewpoint ◇ = port on border, never geometric center (unless single mid slot).

pub use crate::Side;

#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub id: String,
    pub node_id: String,
    pub side: Side,
    pub slot: u32,
    pub x: f64,
    pub y: f64,
}

/// Place `count` ports evenly along `side` of rect (x,y,w,h).
pub fn allocate_side_ports(
    node_id: &str,
    side: Side,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    count: usize,
) -> Vec<Port> {
    let count = count.max(1);
    let inset = 18.0_f64;
    let mut out = Vec::with_capacity(count);
    for slot in 0..count {
        let t = if count == 1 {
            0.5
        } else {
            (slot as f64 + 1.0) / (count as f64 + 1.0)
        };
        let (px, py) = match side {
            Side::N => (x + inset + t * (w - 2.0 * inset).max(0.0), y),
            Side::S => (x + inset + t * (w - 2.0 * inset).max(0.0), y + h),
            Side::W => (x, y + inset + t * (h - 2.0 * inset).max(0.0)),
            Side::E => (x + w, y + inset + t * (h - 2.0 * inset).max(0.0)),
        };
        out.push(Port {
            id: format!("{}:{}:{}", node_id, side_char(side), slot),
            node_id: node_id.into(),
            side,
            slot: slot as u32,
            x: px,
            y: py,
        });
    }
    out
}

fn side_char(s: Side) -> char {
    match s {
        Side::N => 'n',
        Side::E => 'e',
        Side::S => 's',
        Side::W => 'w',
    }
}

/// Which side of `box` faces point `toward`.
pub fn side_facing(box_x: f64, box_y: f64, box_w: f64, box_h: f64, toward: (f64, f64)) -> Side {
    let cx = box_x + box_w / 2.0;
    let cy = box_y + box_h / 2.0;
    let dx = toward.0 - cx;
    let dy = toward.1 - cy;
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

/// Pick one port from preallocated side list closest to `prefer_y_or_x` along the side.
pub fn pick_port(ports: &[Port], side: Side, prefer: f64) -> Option<&Port> {
    pick_port_avoiding(ports, side, prefer, &std::collections::HashSet::new())
}

/// Like [`pick_port`], but prefer slots whose id is **not** in `used` (ELK/libavoid idea:
/// one edge per pin when degree allows — avoids stacked stubs + stacked labels).
pub fn pick_port_avoiding<'a>(
    ports: &'a [Port],
    side: Side,
    prefer: f64,
    used: &std::collections::HashSet<String>,
) -> Option<&'a Port> {
    let on_side: Vec<_> = ports.iter().filter(|p| p.side == side).collect();
    if on_side.is_empty() {
        return None;
    }
    let free: Vec<_> = on_side
        .iter()
        .copied()
        .filter(|p| !used.contains(&p.id))
        .collect();
    let pool = if free.is_empty() { on_side } else { free };
    pool.into_iter().min_by(|a, b| {
        let da = match side {
            Side::N | Side::S => (a.x - prefer).abs(),
            Side::E | Side::W => (a.y - prefer).abs(),
        };
        let db = match side {
            Side::N | Side::S => (b.x - prefer).abs(),
            Side::E | Side::W => (b.y - prefer).abs(),
        };
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// How many ports to allocate on a side given incident edge count (degree).
pub fn port_count_for_degree(degree: usize) -> usize {
    // Need enough pins for UML arrowheads (implements △) without stacking.
    degree.clamp(4, 20).max(degree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_lie_on_border() {
        let ports = allocate_side_ports("n", Side::E, 10.0, 20.0, 100.0, 80.0, 3);
        assert_eq!(ports.len(), 3);
        for p in &ports {
            assert!((p.x - 110.0).abs() < 0.01);
            assert!(p.y > 20.0 && p.y < 100.0);
        }
        // evenly spaced (slot0 and slot2 away from mid)
        assert!((ports[0].y - ports[2].y).abs() > 10.0);
    }

    #[test]
    fn single_port_is_mid_side() {
        let ports = allocate_side_ports("n", Side::N, 0.0, 0.0, 100.0, 50.0, 1);
        assert_eq!(ports.len(), 1);
        assert!((ports[0].x - 50.0).abs() < 0.01);
        assert!((ports[0].y - 0.0).abs() < 0.01);
    }
}
