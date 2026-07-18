//! Exact Minimum Linear Arrangement (+ crossing penalty) for sibling order.
//! Olympiad-style: n ≤ 10 → full search; larger → local search.

/// Undirected edge weight between sibling indices.
pub type WeightMatrix = Vec<Vec<f64>>;

pub fn hop_cost(order: &[usize], w: &WeightMatrix) -> f64 {
    let n = order.len();
    let mut pos = vec![0usize; n];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p;
    }
    let mut c = 0.0;
    for i in 0..n {
        for j in (i + 1)..n {
            c += w[i][j] * (pos[i] as f64 - pos[j] as f64).abs();
        }
    }
    c
}

/// Count chord crossings for edges with weight > 0 (as unit edges).
pub fn crossing_cost(order: &[usize], w: &WeightMatrix) -> f64 {
    let n = order.len();
    let mut pos = vec![0usize; n];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p;
    }
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            if w[i][j] > 0.0 {
                let (a, b) = if pos[i] < pos[j] {
                    (pos[i], pos[j])
                } else {
                    (pos[j], pos[i])
                };
                edges.push((a, b));
            }
        }
    }
    let mut crosses = 0.0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let (a, b) = edges[i];
            let (c, d) = edges[j];
            // proper cross: a < c < b < d or c < a < d < b
            if (a < c && c < b && b < d) || (c < a && a < d && d < b) {
                crosses += 1.0;
            }
        }
    }
    crosses
}

pub fn arrangement_cost(order: &[usize], w: &WeightMatrix) -> f64 {
    hop_cost(order, w) + 3.0 * crossing_cost(order, w)
}

/// Prefer high-`ext` nodes near either end of the row (for external/magistral links).
pub fn border_pull_cost(order: &[usize], ext: &[f64]) -> f64 {
    let n = order.len();
    if n == 0 {
        return 0.0;
    }
    let mut pos = vec![0usize; n];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p;
    }
    let last = (n - 1) as f64;
    let mut c = 0.0;
    for (i, &p) in pos.iter().enumerate() {
        let pf = p as f64;
        let dist_end = pf.min(last - pf);
        c += ext.get(i).copied().unwrap_or(0.0) * dist_end;
    }
    c
}

pub fn arrangement_cost_full(order: &[usize], w: &WeightMatrix, ext: &[f64]) -> f64 {
    // hop + crossings + 2× border pull (olympiad two-level model)
    arrangement_cost(order, w) + 2.0 * border_pull_cost(order, ext)
}

/// Pull toward a specific side: left_pull wants small pos, right_pull wants large pos.
pub fn directed_border_cost(order: &[usize], left_pull: &[f64], right_pull: &[f64]) -> f64 {
    let n = order.len();
    if n == 0 {
        return 0.0;
    }
    let mut pos = vec![0usize; n];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p;
    }
    let last = (n - 1) as f64;
    let mut c = 0.0;
    for (i, &p) in pos.iter().enumerate() {
        let pf = p as f64;
        c += left_pull.get(i).copied().unwrap_or(0.0) * pf;
        c += right_pull.get(i).copied().unwrap_or(0.0) * (last - pf);
    }
    c
}

pub fn arrangement_cost_directed(
    order: &[usize],
    w: &WeightMatrix,
    left_pull: &[f64],
    right_pull: &[f64],
) -> f64 {
    arrangement_cost(order, w) + 2.5 * directed_border_cost(order, left_pull, right_pull)
}

/// MinLA with directed left/right pull (hot edge toward neighbor component).
pub fn best_order_directed(
    n: usize,
    w: &WeightMatrix,
    left_pull: &[f64],
    right_pull: &[f64],
    tie_ids: &[&str],
) -> Vec<usize> {
    assert_eq!(w.len(), n);
    assert_eq!(left_pull.len(), n);
    assert_eq!(right_pull.len(), n);
    let mut order: Vec<usize> = (0..n).collect();
    if n <= 1 {
        return order;
    }
    let cost = |o: &[usize]| arrangement_cost_directed(o, w, left_pull, right_pull);
    if n <= 10 {
        let mut best = order.clone();
        let mut best_c = cost(&best);
        let mut cur = order.clone();
        let mut c = vec![0usize; n];
        let mut i = 0;
        while i < n {
            if c[i] < i {
                if i % 2 == 0 {
                    cur.swap(0, i);
                } else {
                    cur.swap(c[i], i);
                }
                let cc = cost(&cur);
                if cc < best_c - 1e-9
                    || ((cc - best_c).abs() < 1e-9
                        && order_key(&cur, tie_ids) < order_key(&best, tie_ids))
                {
                    best_c = cc;
                    best = cur.clone();
                }
                c[i] += 1;
                i = 0;
            } else {
                c[i] = 0;
                i += 1;
            }
        }
        let cc0 = cost(&order);
        if cc0 < best_c - 1e-9
            || ((cc0 - best_c).abs() < 1e-9
                && order_key(&order, tie_ids) < order_key(&best, tie_ids))
        {
            best = order;
        }
        return best;
    }
    let mut best_c = cost(&order);
    for _ in 0..n * 4 {
        let mut improved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                order.swap(i, j);
                let cc = cost(&order);
                if cc + 1e-9 < best_c {
                    best_c = cc;
                    improved = true;
                } else {
                    order.swap(i, j);
                }
            }
        }
        if !improved {
            break;
        }
    }
    order
}

fn order_key<'a>(order: &[usize], ids: &[&'a str]) -> Vec<&'a str> {
    order.iter().map(|&i| ids[i]).collect()
}

/// Exact MinLA for n ≤ 10; pairwise local search otherwise.
#[allow(dead_code)]
pub fn best_order(n: usize, w: &WeightMatrix, tie_ids: &[&str]) -> Vec<usize> {
    best_order_full(n, w, &vec![0.0; n], tie_ids)
}

/// MinLA + border pull (`ext[i]` = weight of edges leaving the parent shell).
pub fn best_order_full(n: usize, w: &WeightMatrix, ext: &[f64], tie_ids: &[&str]) -> Vec<usize> {
    assert_eq!(w.len(), n);
    assert_eq!(ext.len(), n);
    let mut order: Vec<usize> = (0..n).collect();
    if n <= 1 {
        return order;
    }
    let cost = |o: &[usize]| arrangement_cost_full(o, w, ext);
    if n <= 10 {
        let mut best = order.clone();
        let mut best_c = cost(&best);
        let mut cur = order.clone();
        let mut c = vec![0usize; n];
        let mut i = 0;
        while i < n {
            if c[i] < i {
                if i % 2 == 0 {
                    cur.swap(0, i);
                } else {
                    cur.swap(c[i], i);
                }
                let cc = cost(&cur);
                if cc < best_c - 1e-9
                    || ((cc - best_c).abs() < 1e-9
                        && order_key(&cur, tie_ids) < order_key(&best, tie_ids))
                {
                    best_c = cc;
                    best = cur.clone();
                }
                c[i] += 1;
                i = 0;
            } else {
                c[i] = 0;
                i += 1;
            }
        }
        let cc0 = cost(&order);
        if cc0 < best_c - 1e-9
            || ((cc0 - best_c).abs() < 1e-9
                && order_key(&order, tie_ids) < order_key(&best, tie_ids))
        {
            best = order;
        }
        return best;
    }
    let mut best_c = cost(&order);
    for _ in 0..n * 4 {
        let mut improved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                order.swap(i, j);
                let cc = cost(&order);
                if cc + 1e-9 < best_c {
                    best_c = cc;
                    improved = true;
                } else {
                    order.swap(i, j);
                }
            }
        }
        if !improved {
            break;
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_pair_adjacent_exact() {
        // 3 nodes: edges only 0—2 → order should put 0 and 2 adjacent
        let w = vec![
            vec![0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
        ];
        let ids = ["a", "c", "b"];
        let order = best_order(3, &w, &ids);
        let pos: Vec<_> = {
            let mut p = vec![0; 3];
            for (i, &v) in order.iter().enumerate() {
                p[v] = i;
            }
            p
        };
        assert_eq!((pos[0] as i32 - pos[2] as i32).abs(), 1);
    }

    #[test]
    fn crossing_penalty_prefers_nested_not_cross() {
        // edges 0-2 and 1-3: order 0,1,2,3 crosses; 0,2,1,3 nested (no cross)
        let mut w = vec![vec![0.0; 4]; 4];
        w[0][2] = 1.0;
        w[2][0] = 1.0;
        w[1][3] = 1.0;
        w[3][1] = 1.0;
        let ids = ["a", "b", "c", "d"];
        let order = best_order(4, &w, &ids);
        assert!(
            crossing_cost(&order, &w) < 0.5,
            "order={order:?} crosses={}",
            crossing_cost(&order, &w)
        );
    }

    #[test]
    fn hot_edge_pulls_atom_right() {
        let w = vec![vec![0.0; 3]; 3];
        let left = vec![0.0, 0.0, 0.0];
        let right = vec![0.0, 4.0, 0.0]; // atom 1 wants right
        let ids = ["a", "hot", "c"];
        let order = best_order_directed(3, &w, &left, &right, &ids);
        assert_eq!(order[2], 1, "hot should be rightmost, got {order:?}");
    }

    #[test]
    fn external_atom_pulled_to_row_end() {
        // No internal edges; atom 1 has heavy external pull → must be at an end.
        let w = vec![vec![0.0; 3]; 3];
        let ext = vec![0.0, 5.0, 0.0];
        let ids = ["a", "hot", "c"];
        let order = best_order_full(3, &w, &ext, &ids);
        let pos_hot = order.iter().position(|&i| i == 1).unwrap();
        assert!(
            pos_hot == 0 || pos_hot == 2,
            "hot external atom at end, order={order:?}"
        );
    }
}
