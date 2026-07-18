//! Matryoshka (inside-out) All-layers layout + hierarchical highway routing.
//! See ADR 0006 and `highway.rs`.

use crate::collision::Aabb;
use crate::highway::route_all_highway;
use crate::labels::{
    header_min_width, leaf_size_for_text, place_edge_labels, text_aabb, text_width,
};
use crate::uml::{class_box_size, class_members_for};
use crate::{SceneEdge, SceneGraph, SceneNode, ViewMode};
use architect_c4_domain::{Element, ElementKind, Relationship};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct Cell {
    id: String,
    kind: String,
    name: String,
    parent_id: Option<String>,
    group: bool,
    depth: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    children: Vec<Cell>,
    members: Vec<String>,
    stereotype: Option<String>,
    url: Option<String>,
}

fn min_leaf(kind: ElementKind) -> (f64, f64) {
    match kind {
        ElementKind::Person => (160.0, 70.0),
        ElementKind::SoftwareSystem => (200.0, 70.0),
        ElementKind::External => (180.0, 70.0),
        ElementKind::Container => (190.0, 70.0),
        ElementKind::Component => (170.0, 70.0),
        ElementKind::Code => (150.0, 70.0),
    }
}

fn sized_leaf(el: &Element) -> (f64, f64, Vec<String>, Option<String>) {
    if el.kind == ElementKind::Code {
        let members = class_members_for(el);
        let stereo = el.technology.as_deref().and_then(|t| {
            let low = t.to_ascii_lowercase();
            match low.as_str() {
                "interface" | "trait" | "protocol" => Some("Interface".into()),
                "class" | "struct" | "type" => Some("Class".into()),
                "function" | "fn" | "method" => Some("Function".into()),
                "base" | "abstract" => Some("Base".into()),
                "enum" => Some("Enum".into()),
                _ => None,
            }
        });
        let (w, h) = class_box_size(&el.name, stereo.as_deref(), &members);
        return (w, h, members, stereo);
    }
    let (mw, mh) = min_leaf(el.kind);
    let (w, h) = leaf_size_for_text(&el.name, el.kind.as_str(), mw, mh);
    (w, h, vec![], None)
}

fn layer_of(kind: &str) -> &'static str {
    match kind {
        "person" | "software_system" => "context",
        "external" => "external",
        "container" => "container",
        "component" => "component",
        "code" => "code",
        _ => "context",
    }
}

/// Build All-layers scene inside-out, route each edge in its LCA shell.
pub fn build_matryoshka(
    elements: &[Element],
    relationships: &[Relationship],
    focus: Option<&str>,
) -> SceneGraph {
    let by_id: HashMap<&str, &Element> = elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut children_map: HashMap<String, Vec<&Element>> = HashMap::new();
    for e in elements {
        let key = e.parent_id.clone().unwrap_or_default();
        children_map.entry(key).or_default().push(e);
    }

    // Roots: full forest, or focused matryoshka (focus + ancestors, no siblings).
    let mut root_cells: Vec<Cell> = if let Some(f) = focus {
        let Some(fe) = by_id.get(f).copied() else {
            return SceneGraph {
                mode: ViewMode::All.as_str().into(),
                focus: Some(f.into()),
                width: 480.0,
                height: 360.0,
                nodes: vec![],
                edges: vec![],
                ports: vec![],
            };
        };
        let mut cell = build_cell(fe, &children_map, 0, relationships);
        // Wrap ancestors outside-in so path sys→…→focus exists without siblings.
        let mut ancestors = Vec::new();
        let mut cur = fe.parent_id.clone();
        while let Some(pid) = cur {
            if let Some(pe) = by_id.get(pid.as_str()).copied() {
                ancestors.push(pe);
                cur = pe.parent_id.clone();
            } else {
                break;
            }
        }
        for pe in ancestors {
            let (mw, mh) = min_leaf(pe.kind);
            let pad = 28.0;
            let header = 48.0;
            cell.x = pad;
            cell.y = header + pad;
            let w = (cell.w + pad * 2.0).max(mw);
            let h = (cell.h + header + pad * 2.0).max(mh);
            cell = Cell {
                id: pe.id.clone(),
                kind: pe.kind.as_str().into(),
                name: pe.name.clone(),
                parent_id: pe.parent_id.clone(),
                group: true,
                depth: 0,
                x: 0.0,
                y: 0.0,
                w,
                h,
                children: vec![cell],
                members: vec![],
                stereotype: None,
                url: pe.url.clone(),
            };
        }
        // Fix depths
        fn redepth(c: &mut Cell, d: u32) {
            c.depth = d;
            for ch in &mut c.children {
                redepth(ch, d + 1);
            }
        }
        redepth(&mut cell, 0);
        vec![cell]
    } else {
        children_map
            .get("")
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|e| build_cell(e, &children_map, 0, relationships))
            .collect()
    };

    let parent_of: HashMap<String, Option<String>> = elements
        .iter()
        .map(|e| (e.id.clone(), e.parent_id.clone()))
        .collect();

    // Component shells with a relationship leaving their subtree → stack vertically.
    // Containers pack L→R (C4 container view); vertical stack is for components/classes.
    let stack_ids = external_link_shells(elements, relationships, &parent_of);

    // Stereotypes from Rel keywords (same as Mermaid classDiagram / uml.rs).
    annotate_code_stereotypes(&mut root_cells, relationships);
    // SNS: pull strongly-linked siblings together before packing.
    for c in &mut root_cells {
        reorder_by_affinity(c, relationships);
    }
    // Second pass: pull atoms toward the hot edge facing neighbor components.
    for c in &mut root_cells {
        refine_atoms_toward_comp_neighbors(c, relationships);
    }

    // Iteratively: flatten → route → labels → line collision → gap boosts.
    let mut gap_boost: HashMap<(String, String, String), f64> = HashMap::new();
    let mut row_boost: HashMap<String, f64> = HashMap::new();
    let mut nodes_acc = Vec::new();
    let mut scene_edges = Vec::new();
    let mut used_ports = Vec::new();

    for _iter in 0..8 {
        apply_gap_boosts(
            &mut root_cells,
            &gap_boost,
            &row_boost,
            &stack_ids,
            relationships,
        );
        place_root_cells(&mut root_cells);

        nodes_acc.clear();
        for c in &root_cells {
            flatten_cell(c, 0.0, 0.0, &mut nodes_acc);
        }

        let (edges, ports) = route_all_in_scene(&nodes_acc, relationships, &parent_of);
        scene_edges = edges;
        used_ports = ports;
        // FORBIDDEN: do not glue/bundle atom magistrals into one trunk (user rule).
        // Local notes via place_edge_labels; magistral notes at on-ramp (WASM scene only).
        let mut magistral_saved: HashMap<String, String> = HashMap::new();
        for e in &mut scene_edges {
            if is_magistral_edge(&e.from, &e.to, &nodes_acc, &parent_of) {
                magistral_saved.insert(e.id.clone(), std::mem::take(&mut e.label));
            }
        }
        place_edge_labels(&mut scene_edges, &nodes_acc, &used_ports);
        for e in &mut scene_edges {
            if let Some(l) = magistral_saved.remove(&e.id) {
                e.label = l;
            }
        }
        place_magistral_onramp_notes(&mut scene_edges, &nodes_acc, &parent_of);

        // CollisionEngine (boxes/interior) then olympiad pattern pass (uturn/tracks/labels).
        let clearance = crate::collision_pass::fix_edge_box_collisions(
            &mut scene_edges,
            &nodes_acc,
            &parent_of,
        );
        crate::patterns::apply_patterns(&mut scene_edges, &nodes_acc);
        // Re-seat notes to the repaired polylines (no second heavy collision).
        let mut magistral_saved2: HashMap<String, String> = HashMap::new();
        for e in &mut scene_edges {
            if is_magistral_edge(&e.from, &e.to, &nodes_acc, &parent_of) {
                magistral_saved2.insert(e.id.clone(), std::mem::take(&mut e.label));
            }
        }
        place_edge_labels(&mut scene_edges, &nodes_acc, &used_ports);
        for e in &mut scene_edges {
            if let Some(l) = magistral_saved2.remove(&e.id) {
                e.label = l;
            }
        }
        place_magistral_onramp_notes(&mut scene_edges, &nodes_acc, &parent_of);
        // Patterns may unbundle shared trunks into separate mega-horizontals —
        // re-bundle atom magistrals, then final note ladder + port sync.
        crate::patterns::apply_patterns(&mut scene_edges, &nodes_acc);
        place_magistral_onramp_notes(&mut scene_edges, &nodes_acc, &parent_of);
        used_ports = ports_from_edge_ends(&scene_edges);

        let (needed, rows) = sibling_gaps_for_labels(&nodes_acc, &scene_edges, &parent_of);
        let (frame_pairs, frame_rows) = sibling_frame_collisions(&nodes_acc);
        let mut grew = false;
        for (k, need) in needed.into_iter().chain(frame_pairs) {
            let cur = gap_boost.get(&k).copied().unwrap_or(0.0);
            if need > cur + 1.0 {
                gap_boost.insert(k, need);
                grew = true;
            }
        }
        for p in clearance {
            let key = (p.parent_id, p.left_id, p.right_id);
            let cur = gap_boost.get(&key).copied().unwrap_or(0.0);
            if p.need_gap > cur + 1.0 {
                gap_boost.insert(key, p.need_gap);
                grew = true;
            }
        }
        for (pid, need) in rows.into_iter().chain(frame_rows) {
            let cur = row_boost.get(&pid).copied().unwrap_or(0.0);
            if need > cur + 1.0 {
                row_boost.insert(pid, need);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    shift_scene_positive(&mut nodes_acc, &mut scene_edges, &mut used_ports);
    used_ports = ports_from_edge_ends(&scene_edges);

    let width = nodes_acc
        .iter()
        .map(|n| n.x + n.w)
        .chain(
            scene_edges
                .iter()
                .flat_map(|e| e.points.iter().map(|p| p.0)),
        )
        .fold(480.0_f64, f64::max)
        + 48.0;
    let height = nodes_acc
        .iter()
        .map(|n| n.y + n.h)
        .chain(scene_edges.iter().map(|e| {
            if e.label.is_empty() {
                0.0
            } else {
                e.label_y + 40.0
            }
        }))
        .fold(360.0_f64, f64::max)
        + 48.0;

    SceneGraph {
        mode: ViewMode::All.as_str().into(),
        focus: focus.map(str::to_string),
        width,
        height,
        nodes: nodes_acc,
        edges: scene_edges,
        ports: used_ports,
    }
}

fn subtree_ids(cell: &Cell) -> HashSet<String> {
    let mut s = HashSet::new();
    fn walk(c: &Cell, s: &mut HashSet<String>) {
        s.insert(c.id.clone());
        for ch in &c.children {
            walk(ch, s);
        }
    }
    walk(cell, &mut s);
    s
}

/// Two-level olympiad MinLA (bottom-up):
/// 1) reorder atoms / nested shells inside this cell;
/// 2) MinLA siblings by internal edge weights + border-pull for edges leaving the parent.
fn reorder_by_affinity(cell: &mut Cell, relationships: &[Relationship]) {
    for ch in &mut cell.children {
        reorder_by_affinity(ch, relationships);
    }
    let n = cell.children.len();
    if n < 2 {
        return;
    }
    let ids: Vec<HashSet<String>> = cell.children.iter().map(subtree_ids).collect();
    let parent_set = subtree_ids(cell);
    let mut weight = vec![vec![0.0_f64; n]; n];
    let mut ext = vec![0.0_f64; n];
    for r in relationships {
        let mut from_i = None;
        let mut to_i = None;
        for (i, set) in ids.iter().enumerate() {
            if set.contains(&r.from_id) {
                from_i = Some(i);
            }
            if set.contains(&r.to_id) {
                to_i = Some(i);
            }
        }
        match (from_i, to_i) {
            (Some(i), Some(j)) if i != j => {
                // Internal to this shell's children — hop/crossing cost.
                // Containers: slightly heavier so long jumps are expensive (magistral).
                let w = if cell.kind == "software_system" {
                    2.0
                } else if cell.kind == "container" {
                    3.0 // component MinLA inside container (olympiad level-2)
                } else {
                    1.0
                };
                weight[i][j] += w;
                weight[j][i] += w;
            }
            (Some(i), None) if !parent_set.contains(&r.to_id) => {
                // Edge leaves this parent shell → pull child toward a row end.
                ext[i] += 1.0;
            }
            (None, Some(j)) if !parent_set.contains(&r.from_id) => {
                ext[j] += 1.0;
            }
            _ => {}
        }
    }
    let tie: Vec<&str> = cell.children.iter().map(|c| c.id.as_str()).collect();
    let order = crate::minla::best_order_full(n, &weight, &ext, &tie);
    let old = std::mem::take(&mut cell.children);
    cell.children = order.into_iter().map(|i| old[i].clone()).collect();
}

/// After component order is known, re-MinLA atoms with directed pull toward
/// the neighboring component they talk to (hot left/right edge).
fn refine_atoms_toward_comp_neighbors(cell: &mut Cell, relationships: &[Relationship]) {
    for ch in &mut cell.children {
        refine_atoms_toward_comp_neighbors(ch, relationships);
    }
    if cell.kind != "container" && cell.kind != "software_system" {
        return;
    }
    let n = cell.children.len();
    if n == 0 {
        return;
    }
    // Refine each child component/shell in place using left/right siblings.
    for i in 0..n {
        let left_ids = if i > 0 {
            subtree_ids(&cell.children[i - 1])
        } else {
            HashSet::new()
        };
        let right_ids = if i + 1 < n {
            subtree_ids(&cell.children[i + 1])
        } else {
            HashSet::new()
        };
        refine_one_shell_atoms(&mut cell.children[i], &left_ids, &right_ids, relationships);
    }
}

fn refine_one_shell_atoms(
    shell: &mut Cell,
    left_ids: &HashSet<String>,
    right_ids: &HashSet<String>,
    relationships: &[Relationship],
) {
    // Recurse into nested groups first.
    for ch in &mut shell.children {
        if ch.group || ch.kind == "component" {
            // nested: no horizontal sibling context at this level
            refine_one_shell_atoms(ch, &HashSet::new(), &HashSet::new(), relationships);
        }
    }
    let n = shell.children.len();
    if n < 2 {
        return;
    }
    // Only meaningful for code-ish leaves / small shells under a component.
    if shell.kind != "component" && shell.kind != "container" {
        // still allow under component groups that hold code
        if !shell.children.iter().any(|c| c.kind == "code") {
            return;
        }
    }
    let ids: Vec<HashSet<String>> = shell.children.iter().map(subtree_ids).collect();
    let mut weight = vec![vec![0.0_f64; n]; n];
    let mut left_pull = vec![0.0_f64; n];
    let mut right_pull = vec![0.0_f64; n];
    for r in relationships {
        let mut fi = None;
        let mut ti = None;
        for (i, set) in ids.iter().enumerate() {
            if set.contains(&r.from_id) {
                fi = Some(i);
            }
            if set.contains(&r.to_id) {
                ti = Some(i);
            }
        }
        match (fi, ti) {
            (Some(i), Some(j)) if i != j => {
                weight[i][j] += 1.0;
                weight[j][i] += 1.0;
            }
            (Some(i), None) => {
                if right_ids.contains(&r.to_id) {
                    right_pull[i] += 2.0;
                } else if left_ids.contains(&r.to_id) {
                    left_pull[i] += 2.0;
                }
            }
            (None, Some(j)) => {
                if right_ids.contains(&r.from_id) {
                    right_pull[j] += 2.0;
                } else if left_ids.contains(&r.from_id) {
                    left_pull[j] += 2.0;
                }
            }
            _ => {}
        }
    }
    if left_pull.iter().all(|&x| x == 0.0) && right_pull.iter().all(|&x| x == 0.0) {
        return; // nothing to refine
    }
    let tie: Vec<&str> = shell.children.iter().map(|c| c.id.as_str()).collect();
    let order = crate::minla::best_order_directed(n, &weight, &left_pull, &right_pull, &tie);
    let old = std::mem::take(&mut shell.children);
    shell.children = order.into_iter().map(|i| old[i].clone()).collect();
}

fn annotate_code_stereotypes(cells: &mut [Cell], relationships: &[Relationship]) {
    let mut interfaces = HashSet::new();
    let mut bases = HashSet::new();
    for r in relationships {
        let d = r.description.as_deref().unwrap_or("").to_ascii_lowercase();
        if d.contains("implements") {
            interfaces.insert(r.to_id.clone());
        } else if d.contains("extends") || d.contains("inherit") {
            bases.insert(r.to_id.clone());
        }
    }
    fn walk(c: &mut Cell, interfaces: &HashSet<String>, bases: &HashSet<String>) {
        if c.kind == "code" {
            if interfaces.contains(&c.id) {
                c.stereotype = Some("Interface".into());
            } else if bases.contains(&c.id) {
                c.stereotype = Some("Base".into());
            }
            if c.stereotype.is_some() {
                let (w, h) = class_box_size(&c.name, c.stereotype.as_deref(), &c.members);
                c.w = c.w.max(w);
                c.h = c.h.max(h);
            }
        }
        for ch in &mut c.children {
            walk(ch, interfaces, bases);
        }
    }
    for c in cells {
        walk(c, &interfaces, &bases);
    }
}

fn place_root_cells(root_cells: &mut [Cell]) {
    let root_gap = 64.0;
    // Leave room for outer magistral rails + on-ramp notes left of systems.
    let left_margin = 40.0 + crate::bus::OUTER_BUS_GUTTER + 80.0;
    let mut person_y = 40.0;
    let mut max_person_w = 0.0_f64;
    for c in root_cells.iter_mut().filter(|c| c.kind == "person") {
        c.x = 40.0;
        c.y = person_y;
        person_y += c.h + root_gap;
        max_person_w = max_person_w.max(c.w);
    }
    let mut sx = if max_person_w > 0.0 {
        40.0 + max_person_w + root_gap
    } else {
        left_margin
    }
    .max(left_margin);
    for c in root_cells.iter_mut().filter(|c| c.kind != "person") {
        c.x = sx;
        c.y = 40.0;
        sx += c.w + root_gap;
    }
}

/// Shells that participate in a relationship leaving their subtree (external link).
/// Those containers are packed **one per row** (друг над другом) inside the parent.
fn external_link_shells(
    elements: &[Element],
    relationships: &[Relationship],
    parent_of: &HashMap<String, Option<String>>,
) -> HashSet<String> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for e in elements {
        if let Some(p) = &e.parent_id {
            children.entry(p.clone()).or_default().push(e.id.clone());
        }
    }
    let mut subtree: HashMap<String, HashSet<String>> = HashMap::new();
    fn collect(
        id: &str,
        children: &HashMap<String, Vec<String>>,
        cache: &mut HashMap<String, HashSet<String>>,
    ) -> HashSet<String> {
        if let Some(s) = cache.get(id) {
            return s.clone();
        }
        let mut set = HashSet::new();
        set.insert(id.to_string());
        if let Some(chs) = children.get(id) {
            for c in chs {
                set.extend(collect(c, children, cache));
            }
        }
        cache.insert(id.to_string(), set.clone());
        set
    }
    for e in elements {
        let _ = collect(&e.id, &children, &mut subtree);
    }

    let mut out = HashSet::new();
    for e in elements {
        // Stack containers (and component shells) that talk outside their boundary.
        if e.kind != ElementKind::Container && e.kind != ElementKind::Component {
            continue;
        }
        let ids = subtree.get(&e.id).cloned().unwrap_or_default();
        for r in relationships {
            let a = ids.contains(&r.from_id);
            let b = ids.contains(&r.to_id);
            if a ^ b {
                out.insert(e.id.clone());
                break;
            }
        }
        let _ = parent_of; // kept for API symmetry / future filters
    }
    out
}

/// Re-pack each shell using boosted gaps between specific sibling pairs.
fn apply_gap_boosts(
    cells: &mut [Cell],
    boosts: &HashMap<(String, String, String), f64>,
    row_boost: &HashMap<String, f64>,
    stack_ids: &HashSet<String>,
    relationships: &[Relationship],
) {
    for cell in cells.iter_mut() {
        apply_gap_boosts(
            &mut cell.children,
            boosts,
            row_boost,
            stack_ids,
            relationships,
        );
        if cell.children.is_empty() {
            continue;
        }
        let kind = match cell.kind.as_str() {
            "software_system" => ElementKind::SoftwareSystem,
            "container" => ElementKind::Container,
            "component" => ElementKind::Component,
            _ => ElementKind::SoftwareSystem,
        };
        let (pad, base_gap, _max_inner, bottom_extra) = shell_metrics(kind);
        let v_gap = base_gap
            .max(56.0)
            .max(row_boost.get(&cell.id).copied().unwrap_or(0.0));
        let header = 48.0;
        pack_children_into(
            cell,
            kind,
            boosts,
            v_gap.max(base_gap),
            stack_ids,
            pad,
            header,
            bottom_extra,
            relationships,
        );
    }
}

/// Packing axis per C4 level:
/// - **containers** → left→right
/// - **components** → left→right with wrap (olympiad PackComps); no force column
/// - **code** leaves wrap L→R inside their shell
#[allow(dead_code)]
fn should_stack_child(ch: &Cell, stack_ids: &HashSet<String>) -> bool {
    let _ = stack_ids;
    match ch.kind.as_str() {
        "container" | "component" => false,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn pack_children_into(
    cell: &mut Cell,
    kind: ElementKind,
    boosts: &HashMap<(String, String, String), f64>,
    v_gap: f64,
    stack_ids: &HashSet<String>,
    pad: f64,
    header: f64,
    bottom_extra: f64,
    relationships: &[Relationship],
) {
    let (pad_m, base_gap, _max_inner, _) = shell_metrics(kind);
    let pad = pad.max(pad_m);
    let gap = base_gap.max(v_gap * 0.5);
    let _ = (boosts, stack_ids);
    let n = cell.children.len();
    if n == 0 {
        let header_w = header_min_width(&cell.name, layer_of(&cell.kind));
        cell.w = (min_leaf(kind).0 + pad * 2.0).max(header_w);
        cell.h = header + min_leaf(kind).1 + bottom_extra;
        return;
    }

    let sizes: Vec<(f64, f64)> = cell.children.iter().map(|c| (c.w, c.h)).collect();
    // Inter-child link weights (direct edges between subtrees) — dominate placement.
    let ids: Vec<HashSet<String>> = cell.children.iter().map(subtree_ids).collect();
    let mut weights = vec![vec![0.0_f64; n]; n];
    for r in relationships {
        let mut fi = None;
        let mut ti = None;
        for (i, set) in ids.iter().enumerate() {
            if set.contains(&r.from_id) {
                fi = Some(i);
            }
            if set.contains(&r.to_id) {
                ti = Some(i);
            }
        }
        if let (Some(i), Some(j)) = (fi, ti) {
            if i != j {
                weights[i][j] += 1.0;
                weights[j][i] += 1.0;
            }
        }
    }

    let best = crate::shapes::pick_best_embed_weighted(&sizes, gap, &weights);
    let origin_x = pad;
    let origin_y = header + pad;
    for (i, pl) in best.placed.iter().enumerate() {
        cell.children[i].x = origin_x + pl.x;
        cell.children[i].y = origin_y + pl.y;
    }
    // ShellContainment: no sibling AABB overlap; parent fits union(children)+pad.
    separate_overlapping_children(&mut cell.children, gap * 0.5);
    fit_shell_to_children(cell, pad, header, bottom_extra, kind);
}

/// Arcade AABB separation for sibling cells (olympiad ShellContainment).
fn separate_overlapping_children(children: &mut [Cell], min_gap: f64) {
    let min_gap = min_gap.max(24.0);
    let rounds = children.len().saturating_mul(6).max(6);
    for _ in 0..rounds {
        let mut moved = false;
        for i in 0..children.len() {
            for j in (i + 1)..children.len() {
                let (ax0, ay0) = (children[i].x, children[i].y);
                let (ax1, ay1) = (ax0 + children[i].w, ay0 + children[i].h);
                let (bx0, by0) = (children[j].x, children[j].y);
                let (bx1, by1) = (bx0 + children[j].w, by0 + children[j].h);
                // Inflate by half-gap so we enforce clearance.
                let g = min_gap * 0.5;
                let ox0 = ax0 - g;
                let oy0 = ay0 - g;
                let ox1 = ax1 + g;
                let oy1 = ay1 + g;
                if ox1 <= bx0 || bx1 <= ox0 || oy1 <= by0 || by1 <= oy0 {
                    continue;
                }
                let overlap_x = (ox1.min(bx1) - ox0.max(bx0)).max(0.0);
                let overlap_y = (oy1.min(by1) - oy0.max(by0)).max(0.0);
                if overlap_x <= 0.0 || overlap_y <= 0.0 {
                    continue;
                }
                if overlap_x <= overlap_y {
                    let push = overlap_x + 1.0;
                    if children[i].x + children[i].w * 0.5 <= children[j].x + children[j].w * 0.5 {
                        children[j].x += push;
                    } else {
                        children[i].x += push;
                    }
                } else {
                    let push = overlap_y + 1.0;
                    if children[i].y + children[i].h * 0.5 <= children[j].y + children[j].h * 0.5 {
                        children[j].y += push;
                    } else {
                        children[i].y += push;
                    }
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Parent AABB = union(children) + pad/header/bottom (containment invariant).
fn fit_shell_to_children(
    cell: &mut Cell,
    pad: f64,
    header: f64,
    bottom_extra: f64,
    kind: ElementKind,
) {
    if cell.children.is_empty() {
        return;
    }
    let min_x = cell
        .children
        .iter()
        .map(|c| c.x)
        .fold(f64::INFINITY, f64::min);
    let min_y = cell
        .children
        .iter()
        .map(|c| c.y)
        .fold(f64::INFINITY, f64::min);
    if min_x < pad {
        let dx = pad - min_x;
        for c in &mut cell.children {
            c.x += dx;
        }
    }
    let top = header + pad;
    if min_y < top {
        let dy = top - min_y;
        for c in &mut cell.children {
            c.y += dy;
        }
    }
    let max_r = cell
        .children
        .iter()
        .map(|c| c.x + c.w)
        .fold(0.0_f64, f64::max);
    let max_b = cell
        .children
        .iter()
        .map(|c| c.y + c.h)
        .fold(0.0_f64, f64::max);
    let header_w = header_min_width(&cell.name, layer_of(&cell.kind));
    cell.w = (max_r + pad)
        .max(min_leaf(kind).0 + pad * 2.0)
        .max(header_w);
    cell.h = (max_b + pad + bottom_extra).max(header + min_leaf(kind).1);
}

fn shell_metrics(kind: ElementKind) -> (f64, f64, f64, f64) {
    // (pad, gap, max_inner, bottom_extra)
    // Left pad must fit a distinct left bus rail (schematic sheet bus) — see bus.rs.
    match kind {
        ElementKind::SoftwareSystem => (72.0, 120.0, 2600.0, 96.0),
        ElementKind::Container => (64.0, 112.0, 2400.0, 80.0),
        ElementKind::Component => (56.0, 128.0, 2000.0, 56.0),
        _ => (40.0, 72.0, 1400.0, 40.0),
    }
}

/// (parent_id, left_child_id, right_child_id) → required gap.
type SiblingGapBoost = HashMap<(String, String, String), f64>;
/// parent_id → required vertical row gap.
type RowGapBoost = HashMap<String, f64>;

/// Demand sibling gaps wide enough for edge caption chips (nodes must push apart).
/// Returns (horizontal pair gaps, vertical row gaps per parent shell).
fn sibling_gaps_for_labels(
    nodes: &[SceneNode],
    edges: &[SceneEdge],
    parent_of: &HashMap<String, Option<String>>,
) -> (SiblingGapBoost, RowGapBoost) {
    let by_id: HashMap<String, SceneNode> =
        nodes.iter().cloned().map(|n| (n.id.clone(), n)).collect();
    let mut out: SiblingGapBoost = HashMap::new();
    let mut rows: RowGapBoost = HashMap::new();
    for e in edges {
        let Some(lca) = lca_id(&e.from, &e.to, parent_of) else {
            continue;
        };
        let Some(fa) = anchor_under_lca(&e.from, &lca, parent_of, &by_id) else {
            continue;
        };
        let Some(ta) = anchor_under_lca(&e.to, &lca, parent_of, &by_id) else {
            continue;
        };
        if fa == ta {
            continue;
        }
        let Some(a) = by_id.get(&fa) else { continue };
        let Some(b) = by_id.get(&ta) else { continue };
        let label_w = text_aabb(0.0, 0.0, &e.label, 10.0).width() + 56.0;
        let label_h = text_aabb(0.0, 0.0, &e.label, 10.0).height() + 28.0;
        let chip = text_aabb(e.label_x, e.label_y, &e.label, 10.0).inflate(8.0);
        // Any leaf under LCA that the chip hits (not only endpoints) must force expansion.
        let hit_foreign = nodes.iter().any(|n| {
            !n.group && n.id != e.from && n.id != e.to && chip.overlaps(&Aabb::from_node(n, 0.0))
        });
        let overlaps = chip.overlaps(&Aabb::from_node(a, 0.0))
            || chip.overlaps(&Aabb::from_node(b, 0.0))
            || hit_foreign;
        let same_row = (a.y - b.y).abs() < (a.h.max(b.h) * 0.55);
        if same_row {
            let need = if overlaps {
                label_w.max(text_width(e.label.lines().next().unwrap_or(""), 10.0) + 80.0)
            } else {
                label_w
            };
            let (left, right) = if a.x <= b.x {
                (fa.clone(), ta.clone())
            } else {
                (ta.clone(), fa.clone())
            };
            let key = (lca.clone(), left, right);
            let cur = out.get(&key).copied().unwrap_or(0.0);
            if need > cur {
                out.insert(key, need);
            }
        } else {
            // Stacked shells / vertical highway: always reserve a collision channel
            // between rows (label + edge clearance), not only when chip already overlaps.
            let need = label_h.max(56.0) + if overlaps || hit_foreign { 32.0 } else { 16.0 };
            let cur = rows.get(&lca).copied().unwrap_or(0.0);
            if need > cur {
                rows.insert(lca, need);
            }
        }
    }
    (out, rows)
}

/// If sibling group frames overlap, demand vertical row gap AND horizontal pair gap.
fn sibling_frame_collisions(nodes: &[SceneNode]) -> (SiblingGapBoost, RowGapBoost) {
    let mut by_parent: HashMap<String, Vec<&SceneNode>> = HashMap::new();
    for n in nodes.iter().filter(|n| n.group) {
        if let Some(p) = &n.parent_id {
            by_parent.entry(p.clone()).or_default().push(n);
        }
    }
    let mut rows = RowGapBoost::new();
    let mut pairs = SiblingGapBoost::new();
    for (pid, sibs) in by_parent {
        for i in 0..sibs.len() {
            for j in (i + 1)..sibs.len() {
                let a = sibs[i];
                let b = sibs[j];
                let aa = Aabb::from_node(a, 4.0);
                let bb = Aabb::from_node(b, 4.0);
                if !aa.overlaps(&bb) {
                    continue;
                }
                let overlap_x = aa.x1.min(bb.x1) - aa.x0.max(bb.x0);
                let overlap_y = aa.y1.min(bb.y1) - aa.y0.max(bb.y0);
                if overlap_x <= 0.0 || overlap_y <= 0.0 {
                    continue;
                }
                // Prefer separating on the thinner penetration axis.
                if overlap_x <= overlap_y {
                    let (left, right) = if a.x <= b.x {
                        (a.id.clone(), b.id.clone())
                    } else {
                        (b.id.clone(), a.id.clone())
                    };
                    let need = overlap_x + 64.0;
                    let key = (pid.clone(), left, right);
                    let cur = pairs.get(&key).copied().unwrap_or(0.0);
                    if need > cur {
                        pairs.insert(key, need);
                    }
                } else {
                    let need = overlap_y + 48.0;
                    let cur = rows.get(&pid).copied().unwrap_or(0.0);
                    if need > cur {
                        rows.insert(pid.clone(), need);
                    }
                }
            }
        }
    }
    (pairs, rows)
}

fn route_all_in_scene(
    nodes_acc: &[SceneNode],
    relationships: &[Relationship],
    parent_of: &HashMap<String, Option<String>>,
) -> (Vec<SceneEdge>, Vec<crate::ScenePort>) {
    // Schematic-style: leaf pins + sheet-entry ports + LCA highway (see highway.rs).
    let parent_of = parent_of.clone();
    route_all_highway(nodes_acc, relationships, &parent_of, &|a, b| {
        lca_id(a, b, &parent_of)
    })
}

/// Nearest container / software_system ancestor (inclusive if the node itself is one).
fn enclosing_container_id(
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

/// Cross-container / sibling-container edge = магистраль (on-ramp notes).
fn is_magistral_edge(
    from: &str,
    to: &str,
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) -> bool {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let pf = parent_of.get(from).cloned().flatten();
    let pt = parent_of.get(to).cloned().flatten();
    let shellish = |id: &str| {
        by.get(id).is_some_and(|n| {
            n.kind == "container"
                || (n.group && (n.kind == "component" || n.kind == "software_system"))
        })
    };
    // Sibling containers under one system still use outer magistral + on-ramp notes.
    if pf.is_some() && pf == pt && shellish(from) && shellish(to) {
        return true;
    }
    if pf.is_some() && pf == pt {
        return false; // leaf↔leaf inside one component/shell
    }
    match (
        enclosing_container_id(from, nodes, parent_of),
        enclosing_container_id(to, nodes, parent_of),
    ) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

/// First polyline vertex at/outside the `from` shell's left wall (= on-ramp onto outer rail).
#[allow(dead_code)]
fn find_onramp(points: &[(f64, f64)], shell_left: f64) -> (f64, f64) {
    for &p in points {
        if p.0 <= shell_left + 0.5 {
            return p;
        }
    }
    points
        .iter()
        .copied()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((0.0, 0.0))
}

/// Rebuild viewpoint diamonds from polyline ends (dedupe near-identical tips).
fn ports_from_edge_ends(edges: &[SceneEdge]) -> Vec<crate::ScenePort> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, i64, i64)> = HashSet::new();
    for e in edges {
        if e.points.len() < 2 {
            continue;
        }
        let tips = [
            (e.from.as_str(), e.points[0], e.from_port.as_str()),
            (
                e.to.as_str(),
                e.points[e.points.len() - 1],
                e.to_port.as_str(),
            ),
        ];
        for (node, pt, pid) in tips {
            let key = (
                node.to_string(),
                (pt.0 * 2.0).round() as i64,
                (pt.1 * 2.0).round() as i64,
            );
            if seen.insert(key) {
                out.push(crate::ScenePort {
                    id: if pid.is_empty() {
                        format!("{}:tip:{}", node, out.len())
                    } else {
                        pid.to_string()
                    },
                    node_id: node.to_string(),
                    x: pt.0,
                    y: pt.1,
                });
            }
        }
    }
    out
}

/// Chip AABB matching WASM `draw_label_chip` (ly baseline, rect at ly-11).
fn note_chip_aabb(label_x: f64, label_y: f64, label: &str) -> crate::collision::Aabb {
    let lines: Vec<&str> = label.split('\n').collect();
    let mut max_w = 0.0_f64;
    for ln in &lines {
        max_w = max_w.max(ln.chars().count() as f64 * 5.6);
    }
    let tw = max_w + 12.0;
    let th = 12.0 * lines.len().max(1) as f64 + 4.0;
    crate::collision::Aabb {
        x0: label_x - 4.0,
        y0: label_y - 11.0,
        x1: label_x - 4.0 + tw,
        y1: label_y - 11.0 + th,
    }
}

/// Notes for magistral (and cross) edges: always at **center** of longest segment,
/// then AABB ladder collision (WASM chip-sized).
fn place_magistral_onramp_notes(
    edges: &mut [SceneEdge],
    nodes: &[SceneNode],
    parent_of: &HashMap<String, Option<String>>,
) {
    const ABOVE: f64 = 14.0;
    const PITCH: f64 = 18.0;
    let _ = (nodes, parent_of);

    let mut idxs: Vec<usize> = Vec::new();
    for (i, e) in edges.iter_mut().enumerate() {
        if !is_magistral_edge(&e.from, &e.to, nodes, parent_of) {
            continue;
        }
        if e.label.trim().is_empty() || e.points.len() < 2 {
            continue;
        }
        // Dead rule: note at center of longest segment of the polyline.
        let (ax, ay, bx, by, _, _) = longest_poly_segment(&e.points);
        e.label_x = (ax + bx) * 0.5;
        e.label_y = (ay + by) * 0.5 - ABOVE;
        idxs.push(i);
    }

    idxs.sort_by(|&a, &b| {
        edges[a]
            .label_y
            .partial_cmp(&edges[b].label_y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(edges[a].id.cmp(&edges[b].id))
    });

    for _ in 0..64 {
        let mut moved = false;
        for ai in 0..idxs.len() {
            for bi in (ai + 1)..idxs.len() {
                let i = idxs[ai];
                let j = idxs[bi];
                let a = note_chip_aabb(edges[i].label_x, edges[i].label_y, &edges[i].label)
                    .inflate(4.0);
                let b = note_chip_aabb(edges[j].label_x, edges[j].label_y, &edges[j].label)
                    .inflate(4.0);
                if !a.overlaps(&b) {
                    continue;
                }
                let (upper, lower) = if edges[i].label_y <= edges[j].label_y {
                    (i, j)
                } else {
                    (j, i)
                };
                let uh = note_chip_aabb(
                    edges[upper].label_x,
                    edges[upper].label_y,
                    &edges[upper].label,
                )
                .height();
                let need_y = edges[upper].label_y - 11.0 + uh + PITCH + 11.0;
                if need_y > edges[lower].label_y + 0.5 {
                    edges[lower].label_y = need_y;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
        idxs.sort_by(|&a, &b| {
            edges[a]
                .label_y
                .partial_cmp(&edges[b].label_y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn longest_poly_segment(points: &[(f64, f64)]) -> (f64, f64, f64, f64, f64, bool) {
    let mut best = (
        points[0].0,
        points[0].1,
        points[1].0,
        points[1].1,
        0.0,
        false,
    );
    for w in points.windows(2) {
        let (ax, ay, bx, by) = (w[0].0, w[0].1, w[1].0, w[1].1);
        let len = (ax - bx).abs() + (ay - by).abs();
        let horiz = (ay - by).abs() < 0.5;
        if len > best.4 {
            best = (ax, ay, bx, by, len, horiz);
        }
    }
    best
}

/// Shift scene so outer rails / notes are not clipped at x<margin.
fn shift_scene_positive(
    nodes: &mut [SceneNode],
    edges: &mut [SceneEdge],
    ports: &mut [crate::ScenePort],
) {
    let mut min_x = 24.0_f64;
    for n in nodes.iter() {
        min_x = min_x.min(n.x);
    }
    for e in edges.iter() {
        for p in &e.points {
            min_x = min_x.min(p.0);
        }
        if !e.label.is_empty() {
            min_x = min_x.min(e.label_x - 4.0);
        }
    }
    for p in ports.iter() {
        min_x = min_x.min(p.x);
    }
    if min_x >= 24.0 {
        return;
    }
    let dx = 24.0 - min_x;
    for n in nodes.iter_mut() {
        n.x += dx;
    }
    for e in edges.iter_mut() {
        for p in &mut e.points {
            p.0 += dx;
        }
        e.label_x += dx;
    }
    for p in ports.iter_mut() {
        p.x += dx;
    }
}

#[allow(clippy::only_used_in_recursion)]
fn build_cell(
    el: &Element,
    children_map: &HashMap<String, Vec<&Element>>,
    depth: u32,
    relationships: &[Relationship],
) -> Cell {
    let children_els = children_map.get(&el.id).cloned().unwrap_or_default();
    if children_els.is_empty() {
        let (w, h, members, stereotype) = sized_leaf(el);
        return Cell {
            id: el.id.clone(),
            kind: el.kind.as_str().into(),
            name: el.name.clone(),
            parent_id: el.parent_id.clone(),
            group: false,
            depth,
            x: 0.0,
            y: 0.0,
            w,
            h,
            children: vec![],
            members,
            stereotype,
            url: el.url.clone(),
        };
    }

    let children: Vec<Cell> = children_els
        .iter()
        .map(|c| build_cell(c, children_map, depth + 1, relationships))
        .collect();

    // Initial pack; apply_gap_boosts will re-pack with stack_ids + collision gaps.
    let stack_ids = HashSet::new();
    let mut cell = Cell {
        id: el.id.clone(),
        kind: el.kind.as_str().into(),
        name: el.name.clone(),
        parent_id: el.parent_id.clone(),
        group: true,
        depth,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        children,
        members: vec![],
        stereotype: None,
        url: el.url.clone(),
    };
    let (pad, base_gap, _, bottom_extra) = shell_metrics(el.kind);
    let header = 48.0;
    pack_children_into(
        &mut cell,
        el.kind,
        &HashMap::new(),
        base_gap.max(56.0),
        &stack_ids,
        pad,
        header,
        bottom_extra,
        &[],
    );
    cell
}

fn flatten_cell(c: &Cell, ox: f64, oy: f64, out: &mut Vec<SceneNode>) {
    let abs_x = ox + c.x;
    let abs_y = oy + c.y;
    // Children first depth-order for groups behind… actually push group then children
    out.push(SceneNode {
        id: c.id.clone(),
        kind: c.kind.clone(),
        layer: layer_of(&c.kind).into(),
        name: c.name.clone(),
        parent_id: c.parent_id.clone(),
        group: c.group,
        depth: c.depth,
        x: abs_x,
        y: abs_y,
        w: c.w,
        h: c.h,
        members: c.members.clone(),
        stereotype: c.stereotype.clone(),
        url: c.url.clone(),
    });
    for ch in &c.children {
        flatten_cell(ch, abs_x, abs_y, out);
    }
}

fn lca_id(a: &str, b: &str, parent_of: &HashMap<String, Option<String>>) -> Option<String> {
    let mut ancestors = HashSet::new();
    let mut cur = Some(a.to_string());
    while let Some(id) = cur {
        ancestors.insert(id.clone());
        cur = parent_of.get(&id).cloned().flatten();
    }
    let mut cur = Some(b.to_string());
    while let Some(id) = cur {
        if ancestors.contains(&id) {
            // Prefer parent of the deeper node if a/b are nested — LCA of nodes is first common
            return Some(id);
        }
        cur = parent_of.get(&id).cloned().flatten();
    }
    None
}

/// Direct child of LCA (or the node itself) that contains `id` in its subtree.
fn anchor_under_lca(
    id: &str,
    lca: &str,
    parent_of: &HashMap<String, Option<String>>,
    nodes: &HashMap<String, SceneNode>,
) -> Option<String> {
    if id == lca {
        return Some(id.to_string());
    }
    // Walk up until parent is lca
    let mut cur = id.to_string();
    loop {
        let p = parent_of.get(&cur).cloned().flatten()?;
        if p == lca {
            return if nodes.contains_key(&cur) {
                Some(cur)
            } else {
                None
            };
        }
        if !nodes.contains_key(&p) && p != lca {
            cur = p;
            continue;
        }
        cur = p;
        if cur == lca {
            return Some(id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ElementKind;

    fn el(id: &str, kind: ElementKind, name: &str, parent: Option<&str>) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind,
            parent_id: parent.map(str::to_string),
            name: name.into(),
            description: None,
            technology: None,
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn matryoshka_builds_nested_groups() {
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("c1", ElementKind::Component, "C1", Some("api")),
            el("code", ElementKind::Code, "Svc", Some("c1")),
        ];
        let g = build_matryoshka(&elements, &[], None);
        assert!(g.nodes.iter().any(|n| n.id == "sys" && n.group));
        assert!(g.nodes.iter().any(|n| n.id == "api" && n.group));
        let code = g.nodes.iter().find(|n| n.id == "code").unwrap();
        let c1 = g.nodes.iter().find(|n| n.id == "c1").unwrap();
        // code inside c1 bounds
        assert!(code.x >= c1.x - 0.5);
        assert!(code.y >= c1.y - 0.5);
        assert!(code.x + code.w <= c1.x + c1.w + 0.5);
    }

    #[test]
    fn long_code_name_gets_wide_box() {
        let elements = vec![
            el("c", ElementKind::Component, "Sec", None),
            el("bc", ElementKind::Code, "BcryptPasswordEncoder", Some("c")),
        ];
        let g = build_matryoshka(&elements, &[], None);
        let n = g.nodes.iter().find(|n| n.id == "bc").unwrap();
        assert!(
            n.w >= crate::labels::text_width("BcryptPasswordEncoder", 13.0) + 36.0,
            "box too narrow: {}",
            n.w
        );
    }

    #[test]
    fn wide_containers_use_compact_shape_pack() {
        // Even if each container is very wide, C4 container view keeps one L→R band.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("a", ElementKind::Container, "AAAA", Some("sys")),
            el("b", ElementKind::Container, "BBBB", Some("sys")),
            el("c", ElementKind::Container, "CCCC", Some("sys")),
            // fat code to widen containers
            el("a1", ElementKind::Code, "VeryLongClassNameAlpha", Some("a")),
            el("a2", ElementKind::Code, "VeryLongClassNameBeta", Some("a")),
            el("b1", ElementKind::Code, "VeryLongClassNameGamma", Some("b")),
            el("b2", ElementKind::Code, "VeryLongClassNameDelta", Some("b")),
            el(
                "c1",
                ElementKind::Code,
                "VeryLongClassNameEpsilon",
                Some("c"),
            ),
            el("c2", ElementKind::Code, "VeryLongClassNameZeta", Some("c")),
        ];
        let g = build_matryoshka(&elements, &[], None);
        let mut cons: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some("sys") && n.kind == "container")
            .collect();
        cons.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        assert_eq!(cons.len(), 3);
        // Compact shape: not all stacked at same x (pure column) unless chosen Col for n=3.
        let xs: Vec<_> = cons.iter().map(|c| c.x).collect();
        let ys: Vec<_> = cons.iter().map(|c| c.y).collect();
        let x_spread = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let y_spread = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - ys.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(x_spread > 1.0 || y_spread > 1.0);
    }

    #[test]
    fn containers_pack_in_compact_grid() {
        // Compact shape pack: 2D grid/cross/diamond — not one infinite line, not force column.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("cache", ElementKind::Container, "Cache", Some("sys")),
            el("h", ElementKind::Component, "Handler", Some("api")),
            el("s", ElementKind::Component, "Store", Some("db")),
        ];
        let rels = vec![Relationship {
            id: "r1".into(),
            workspace_id: "w".into(),
            from_id: "h".into(),
            to_id: "s".into(),
            description: Some("writes".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        let mut cons: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some("sys") && n.kind == "container")
            .collect();
        assert_eq!(cons.len(), 3);
        cons.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        let xs: Vec<f64> = cons.iter().map(|c| c.x).collect();
        let ys: Vec<f64> = cons.iter().map(|c| c.y).collect();
        let x_spread = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let y_spread = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - ys.iter().cloned().fold(f64::INFINITY, f64::min);
        // Must use both axes somehow across typical packs, OR a short row of 3.
        assert!(
            x_spread > 10.0 || y_spread > 10.0,
            "degenerate pack: all on one point"
        );
        // Not an absurd ribbon: width of parent sys shouldn't be ~sum of all widths only.
        let sys = g.nodes.iter().find(|n| n.id == "sys").unwrap();
        let sum_w: f64 = cons.iter().map(|c| c.w).sum();
        assert!(
            sys.w < sum_w * 0.95 || y_spread > 10.0,
            "should not force a single infinite L→R ribbon"
        );
    }

    #[test]
    fn sibling_containers_route_in_gap_not_through_boxes() {
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "api".into(),
            to_id: "db".into(),
            description: Some("stores".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        let api = g.nodes.iter().find(|n| n.id == "api").unwrap();
        let db = g.nodes.iter().find(|n| n.id == "db").unwrap();
        let side = api.x + api.w <= db.x + 2.0 || db.x + db.w <= api.x + 2.0;
        let stacked = api.y + api.h <= db.y + 2.0 || db.y + db.h <= api.y + 2.0;
        assert!(
            side || stacked,
            "containers adjacent in grid (row or column)"
        );
        let e = g.edges.iter().find(|e| e.id == "r").unwrap();
        assert!(e.points.len() >= 2);
        assert!(!e.label.is_empty());
        // Polyline must not stab either container interior (pad inward).
        for n in [api, db] {
            let box_ = (n.x + 4.0, n.y + 4.0, n.x + n.w - 4.0, n.y + n.h - 4.0);
            for w in e.points.windows(2) {
                // crude open-segment vs AABB
                let (p, q) = (w[0], w[1]);
                let mid = ((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
                assert!(
                    !(mid.0 > box_.0 && mid.0 < box_.2 && mid.1 > box_.1 && mid.1 < box_.3),
                    "edge mid {:?} inside {}",
                    mid,
                    n.id
                );
            }
        }
    }

    #[test]
    fn wasm_all_cross_container_keeps_note_and_local_class_edge() {
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("h", ElementKind::Component, "Handler", Some("api")),
            el("s", ElementKind::Component, "Store", Some("db")),
            el("c1", ElementKind::Code, "SvcA", Some("h")),
            el("c2", ElementKind::Code, "SvcB", Some("h")),
        ];
        let rels = vec![
            Relationship {
                id: "cross".into(),
                workspace_id: "w".into(),
                from_id: "h".into(),
                to_id: "s".into(),
                description: Some("writes across containers".into()),
                technology: None,
            },
            Relationship {
                id: "local".into(),
                workspace_id: "w".into(),
                from_id: "c1".into(),
                to_id: "c2".into(),
                description: Some("calls sibling".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        let cross = g.edges.iter().find(|e| e.id == "cross").unwrap();
        let local = g.edges.iter().find(|e| e.id == "local").unwrap();
        let api = g.nodes.iter().find(|n| n.id == "api").unwrap();
        let db = g.nodes.iter().find(|n| n.id == "db").unwrap();
        assert!(
            !cross.label.is_empty(),
            "cross-container keeps note, got empty"
        );
        assert!(
            !local.label.is_empty(),
            "same-parent class edge keeps note, got {:?}",
            local.label
        );
        // Containers L→R; cross wire should leave API toward DB (right of api or in gap).
        assert!(api.x + api.w <= db.x + 2.0, "api left of db in row");
        let max_x = cross
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_x > api.x + api.w - 1.0,
            "cross edge must reach past API toward DB: max_x={max_x} api.right={}",
            api.x + api.w
        );
    }

    #[test]
    fn affinity_pulls_linked_siblings_adjacent() {
        // Pack order without affinity: a, c, b. Link a↔b → after reorder they sit next to each other.
        let elements = vec![
            el("comp", ElementKind::Component, "C", None),
            el("a", ElementKind::Code, "A", Some("comp")),
            el("c", ElementKind::Code, "CIsolated", Some("comp")),
            el("b", ElementKind::Code, "B", Some("comp")),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "a".into(),
            to_id: "b".into(),
            description: Some("uses".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        let mut kids: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some("comp") && !n.group)
            .collect();
        let by: std::collections::HashMap<&str, &_> =
            kids.iter().map(|n| (n.id.as_str(), *n)).collect();
        let a = by["a"];
        let b = by["b"];
        let h_gap = if a.x + a.w <= b.x + 2.0 {
            b.x - (a.x + a.w)
        } else if b.x + b.w <= a.x + 2.0 {
            a.x - (b.x + b.w)
        } else {
            -1.0
        };
        let v_gap = if a.y + a.h <= b.y + 2.0 {
            b.y - (a.y + a.h)
        } else if b.y + b.h <= a.y + 2.0 {
            a.y - (b.y + b.h)
        } else {
            -1.0
        };
        let y_ov = a.y < b.y + b.h && b.y < a.y + a.h;
        let x_ov = a.x < b.x + b.w && b.x < a.x + a.w;
        assert!(
            (y_ov && h_gap >= 0.0 && h_gap < 500.0) || (x_ov && v_gap >= 0.0 && v_gap < 500.0),
            "linked a/b should be geometric neighbors"
        );
    }

    #[test]
    fn ports_follow_polyline_ends_not_stale_route_ports() {
        // Long rightward edges get south-exit rewrite; ◇ must sit on the new tip, not old east ports.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("c", ElementKind::Component, "C", Some("api")),
            el("s", ElementKind::Component, "S", Some("db")),
            el("logger", ElementKind::Code, "UsageLogger", Some("c")),
            el("batch", ElementKind::Code, "Batch", Some("s")),
            el("rados", ElementKind::Code, "Rados", Some("s")),
        ];
        let rels = vec![
            Relationship {
                id: "e1".into(),
                workspace_id: "w".into(),
                from_id: "logger".into(),
                to_id: "batch".into(),
                description: Some("flush".into()),
                technology: None,
            },
            Relationship {
                id: "e2".into(),
                workspace_id: "w".into(),
                from_id: "logger".into(),
                to_id: "rados".into(),
                description: Some("log".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        let logger = g.nodes.iter().find(|n| n.id == "logger").unwrap();
        let ports: Vec<_> = g.ports.iter().filter(|p| p.node_id == "logger").collect();
        assert!(!ports.is_empty(), "expected ports on logger");
        // Every logger port must lie on the logger border (not floating inside / far east ghosts).
        for p in &ports {
            let on_e = (p.x - (logger.x + logger.w)).abs() < 2.0
                && p.y >= logger.y - 1.0
                && p.y <= logger.y + logger.h + 1.0;
            let on_s = (p.y - (logger.y + logger.h)).abs() < 2.0
                && p.x >= logger.x - 1.0
                && p.x <= logger.x + logger.w + 1.0;
            let on_w = (p.x - logger.x).abs() < 2.0
                && p.y >= logger.y - 1.0
                && p.y <= logger.y + logger.h + 1.0;
            let on_n = (p.y - logger.y).abs() < 2.0
                && p.x >= logger.x - 1.0
                && p.x <= logger.x + logger.w + 1.0;
            assert!(
                on_e || on_s || on_w || on_n,
                "stale port off border: {:?} box=({},{} {}x{})",
                p,
                logger.x,
                logger.y,
                logger.w,
                logger.h
            );
        }
        // No more ports than unique tips (deduped) — fan-out south shouldn't leave 5 east ghosts.
        assert!(
            ports.len() <= 3,
            "too many logger ports (orphans?): {}",
            ports.len()
        );
    }

    #[test]
    fn multi_magistral_notes_do_not_overlap_chips() {
        // Fan-out from one class to several remote containers → ladder notes must clear.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("log", ElementKind::Container, "Log", Some("sys")),
            el("c", ElementKind::Component, "Acc", Some("api")),
            el("s", ElementKind::Component, "Store", Some("db")),
            el("l", ElementKind::Component, "LAcc", Some("log")),
            el("logger", ElementKind::Code, "UsageLogger", Some("c")),
            el("batch", ElementKind::Code, "RGWUsageBatch", Some("l")),
            el("rados", ElementKind::Code, "RGWRados", Some("s")),
            el("usage", ElementKind::Code, "RGWUsage", Some("l")),
        ];
        let rels = vec![
            Relationship {
                id: "e1".into(),
                workspace_id: "w".into(),
                from_id: "logger".into(),
                to_id: "batch".into(),
                description: Some("flush batches".into()),
                technology: None,
            },
            Relationship {
                id: "e2".into(),
                workspace_id: "w".into(),
                from_id: "logger".into(),
                to_id: "rados".into(),
                description: Some("flush to log_usage".into()),
                technology: None,
            },
            Relationship {
                id: "e3".into(),
                workspace_id: "w".into(),
                from_id: "logger".into(),
                to_id: "usage".into(),
                description: Some("batches by round_time".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        let outs: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.from == "logger" && !e.label.is_empty())
            .collect();
        assert!(
            outs.len() >= 3,
            "expected fan-out edges, got {}",
            outs.len()
        );
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                let a =
                    note_chip_aabb(outs[i].label_x, outs[i].label_y, &outs[i].label).inflate(2.0);
                let b =
                    note_chip_aabb(outs[j].label_x, outs[j].label_y, &outs[j].label).inflate(2.0);
                assert!(
                    !a.overlaps(&b),
                    "notes overlap: {} @({},{}) vs {} @({},{})",
                    outs[i].id,
                    outs[i].label_x,
                    outs[i].label_y,
                    outs[j].id,
                    outs[j].label_x,
                    outs[j].label_y
                );
            }
        }
    }

    #[test]
    fn atom_with_external_edge_moves_to_row_end() {
        // Inside comp: mid has no internal links but talks outside → end of row.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("comp", ElementKind::Component, "C", Some("api")),
            el("store", ElementKind::Component, "S", Some("db")),
            el("a", ElementKind::Code, "A", Some("comp")),
            el("hot", ElementKind::Code, "Hot", Some("comp")),
            el("c", ElementKind::Code, "CIsolated", Some("comp")),
            el("remote", ElementKind::Code, "Remote", Some("store")),
        ];
        let rels = vec![Relationship {
            id: "ext".into(),
            workspace_id: "w".into(),
            from_id: "hot".into(),
            to_id: "remote".into(),
            description: Some("calls".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        let mut kids: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some("comp") && !n.group)
            .collect();
        let hot = kids.iter().find(|n| n.id == "hot").unwrap();
        let min_x = kids.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let max_x = kids
            .iter()
            .map(|n| n.x + n.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = kids.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let max_y = kids
            .iter()
            .map(|n| n.y + n.h)
            .fold(f64::NEG_INFINITY, f64::max);
        let on_extreme = (hot.x - min_x).abs() < 2.0
            || ((hot.x + hot.w) - max_x).abs() < 2.0
            || (hot.y - min_y).abs() < 2.0
            || ((hot.y + hot.h) - max_y).abs() < 2.0;
        assert!(on_extreme, "external atom toward pack extreme");
    }

    #[test]
    fn nested_components_pack_left_to_right_by_minla() {
        // Olympiad PackComps: components in a container sit L→R; linked ones adjacent.
        let elements = vec![
            el("api", ElementKind::Container, "API", None),
            el("c1", ElementKind::Component, "Auth", Some("api")),
            el("c2", ElementKind::Component, "Billing", Some("api")),
            el("c3", ElementKind::Component, "Notify", Some("api")),
            el("k1", ElementKind::Code, "Jwt", Some("c1")),
            el("k2", ElementKind::Code, "Invoice", Some("c2")),
            el("k3", ElementKind::Code, "Mailer", Some("c3")),
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "k1".into(),
                to_id: "k2".into(),
                description: Some("uses".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "k2".into(),
                to_id: "k3".into(),
                description: Some("notifies".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        let comps: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some("api") && n.kind == "component")
            .collect();
        assert_eq!(comps.len(), 3);
        // MinLA keeps Auth—Billing—Notify chain: geometric neighbors (share side gap).
        let by: std::collections::HashMap<&str, &SceneNode> =
            comps.iter().map(|c| (c.id.as_str(), *c)).collect();
        let near = |a: &SceneNode, b: &SceneNode| {
            let h_gap = if a.x + a.w <= b.x + 2.0 {
                b.x - (a.x + a.w)
            } else if b.x + b.w <= a.x + 2.0 {
                a.x - (b.x + b.w)
            } else {
                -1.0
            };
            let v_gap = if a.y + a.h <= b.y + 2.0 {
                b.y - (a.y + a.h)
            } else if b.y + b.h <= a.y + 2.0 {
                a.y - (b.y + b.h)
            } else {
                -1.0
            };
            let y_ov = a.y < b.y + b.h && b.y < a.y + a.h;
            let x_ov = a.x < b.x + b.w && b.x < a.x + a.w;
            (y_ov && h_gap >= 0.0 && h_gap < 400.0) || (x_ov && v_gap >= 0.0 && v_gap < 400.0)
        };
        assert!(
            near(by["c1"], by["c2"]),
            "Auth should be grid-neighbor of Billing"
        );
        // Chain Auth-Billing-Notify: at least Auth-Billing neighbors; Notify near Billing or Auth.
        assert!(
            near(by["c1"], by["c2"]),
            "Auth should be grid-neighbor of Billing"
        );
        assert!(
            near(by["c2"], by["c3"]) || near(by["c1"], by["c3"]),
            "Notify near chain"
        );
    }

    #[test]
    fn atom_pulled_to_hot_edge_toward_neighbor_component() {
        let elements = vec![
            el("api", ElementKind::Container, "API", None),
            el("auth", ElementKind::Component, "Auth", Some("api")),
            el("bill", ElementKind::Component, "Billing", Some("api")),
            el("cold", ElementKind::Code, "Cold", Some("auth")),
            el("hot", ElementKind::Code, "Hot", Some("auth")),
            el("mid", ElementKind::Code, "Mid", Some("auth")),
            el("inv", ElementKind::Code, "Invoice", Some("bill")),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "hot".into(),
            to_id: "inv".into(),
            description: Some("uses".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        let bill = g.nodes.iter().find(|n| n.id == "bill").unwrap();
        let hot = g.nodes.iter().find(|n| n.id == "hot").unwrap();
        let cold = g.nodes.iter().find(|n| n.id == "cold").unwrap();
        let dist = |a: &SceneNode, b: &SceneNode| {
            let ax = a.x + a.w * 0.5;
            let ay = a.y + a.h * 0.5;
            let bx = b.x + b.w * 0.5;
            let by = b.y + b.h * 0.5;
            (ax - bx).abs() + (ay - by).abs()
        };
        assert!(
            dist(hot, bill) <= dist(cold, bill) + 1.0,
            "hot should be at least as close to Billing as cold"
        );
    }

    #[test]
    fn classes_inside_shell_use_classic_port_routing() {
        // Inside a component/container: Code↔Code must use old port→port routing
        // (not the left bus spine).
        let elements = vec![
            el("comp", ElementKind::Component, "OSD Service", None),
            el("am", ElementKind::Code, "AsyncMessenger", Some("comp")),
            el("osd", ElementKind::Code, "OSD", Some("comp")),
            el("svc", ElementKind::Code, "OSDService", Some("comp")),
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "osd".into(),
                to_id: "am".into(),
                description: Some("uses messenger for client IO".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "osd".into(),
                to_id: "svc".into(),
                description: Some("owns service lifecycle".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        assert_eq!(g.edges.len(), 2);
        for e in &g.edges {
            assert!(
                e.from_port.starts_with("osd:"),
                "expected osd leaf port, got {}",
                e.from_port
            );
            // Classic side-facing: not both ends on West (left-bus signature).
            assert!(
                !(e.from_port.contains(":w:") && e.to_port.contains(":w:")),
                "class edge still left-bus style: {} → {} path={:?}",
                e.from_port,
                e.to_port,
                e.points
            );
            assert!(e.points.len() >= 2);
        }
    }

    #[test]
    fn all_edge_polylines_orthogonal() {
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("a", ElementKind::Container, "A", Some("sys")),
            el("b", ElementKind::Container, "B", Some("sys")),
            el("c", ElementKind::Container, "C", Some("sys")),
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("uses".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "c".into(),
                description: Some("calls".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&elements, &rels, None);
        for e in &g.edges {
            for w in e.points.windows(2) {
                let (p, q) = (w[0], w[1]);
                assert!(
                    (p.0 - q.0).abs() < 0.05 || (p.1 - q.1).abs() < 0.05,
                    "diagonal on {}: {:?} -> {:?}",
                    e.id,
                    p,
                    q
                );
            }
        }
    }

    #[test]
    fn edge_routed_with_ports_not_empty_points() {
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("a", ElementKind::Container, "A", Some("sys")),
            el("b", ElementKind::Container, "B", Some("sys")),
        ];
        let rels = vec![Relationship {
            id: "r1".into(),
            workspace_id: "w".into(),
            from_id: "a".into(),
            to_id: "b".into(),
            description: Some("calls".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        assert_eq!(g.edges.len(), 1);
        assert!(g.edges[0].points.len() >= 2);
        assert!(!g.ports.is_empty());
        assert!(
            g.edges[0].label.contains('→') && g.edges[0].label.contains('\n'),
            "caption must be From→To\\nuses, got {:?}",
            g.edges[0].label
        );
        // ports on borders of a/b
        for p in &g.ports {
            let n = g.nodes.iter().find(|n| n.id == p.node_id).unwrap();
            let on_border = (p.x - n.x).abs() < 1.0
                || (p.x - (n.x + n.w)).abs() < 1.0
                || (p.y - n.y).abs() < 1.0
                || (p.y - (n.y + n.h)).abs() < 1.0;
            assert!(on_border, "port not on border {:?}", p);
        }
    }

    #[test]
    fn parent_child_edge_stays_on_parent_left_bus() {
        // rgw → rgw_usage_log must not Dijkstra through grandchild code boxes.
        let elements = vec![
            el("rgw", ElementKind::Container, "RGW", None),
            el("ulog", ElementKind::Component, "Usage Log", Some("rgw")),
            el("lu", ElementKind::Code, "log_usage", Some("ulog")),
        ];
        let rels = vec![Relationship {
            id: "r1".into(),
            workspace_id: "w".into(),
            from_id: "rgw".into(),
            to_id: "ulog".into(),
            description: Some("owns".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        assert_eq!(g.edges.len(), 1);
        let e = &g.edges[0];
        let lu = g.nodes.iter().find(|n| n.id == "lu").unwrap();
        for (a, b) in e.points.windows(2).map(|w| (w[0], w[1])) {
            // vertical segment must not pierce log_usage interior
            if (a.0 - b.0).abs() < 0.5 {
                let x = a.0;
                let y0 = a.1.min(b.1);
                let y1 = a.1.max(b.1);
                let through = x > lu.x + 2.0
                    && x < lu.x + lu.w - 2.0
                    && y0 < lu.y + 2.0
                    && y1 > lu.y + lu.h - 2.0;
                assert!(
                    !through,
                    "parent→child pierces log_usage at x={x} path={:?}",
                    e.points
                );
            }
        }
    }

    #[test]
    fn cross_container_edge_attaches_to_leaf_components() {
        // Comp links must leave the component pin, not only the container wall.
        let elements = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("db", ElementKind::Container, "DB", Some("sys")),
            el("handler", ElementKind::Component, "Handler", Some("api")),
            el("store", ElementKind::Component, "Store", Some("db")),
        ];
        let rels = vec![Relationship {
            id: "r1".into(),
            workspace_id: "w".into(),
            from_id: "handler".into(),
            to_id: "store".into(),
            description: Some("writes".into()),
            technology: None,
        }];
        let g = build_matryoshka(&elements, &rels, None);
        assert_eq!(g.edges.len(), 1);
        let e = &g.edges[0];
        assert!(
            e.from_port.starts_with("handler:"),
            "from_port must be leaf handler, got {}",
            e.from_port
        );
        assert!(
            e.to_port.starts_with("store:"),
            "to_port must be leaf store, got {}",
            e.to_port
        );
        let handler = g.nodes.iter().find(|n| n.id == "handler").unwrap();
        let store = g.nodes.iter().find(|n| n.id == "store").unwrap();
        let start = e.points[0];
        let end = *e.points.last().unwrap();
        let near = |p: (f64, f64), n: &SceneNode| {
            let on_x = (p.0 - n.x).abs() < 2.0 || (p.0 - (n.x + n.w)).abs() < 2.0;
            let on_y = (p.1 - n.y).abs() < 2.0 || (p.1 - (n.y + n.h)).abs() < 2.0;
            let in_y = p.1 >= n.y - 2.0 && p.1 <= n.y + n.h + 2.0;
            let in_x = p.0 >= n.x - 2.0 && p.0 <= n.x + n.w + 2.0;
            (on_x && in_y) || (on_y && in_x)
        };
        assert!(
            near(start, handler),
            "start {start:?} not on handler {:?}",
            handler
        );
        assert!(near(end, store), "end {end:?} not on store {:?}", store);
        // Prefer highway for distant shells; short path OK when pack places them adjacent.
        assert!(
            e.points.len() >= 2,
            "expected routed polyline, got {} pts",
            e.points.len()
        );
        if e.points.len() < 4 {
            // Adjacent packing: path must stay short (no spaghetti).
            let plen: f64 = e
                .points
                .windows(2)
                .map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs())
                .sum();
            assert!(
                plen < 800.0,
                "short cross-container path unexpectedly long: {plen} {:?}",
                e.points
            );
        }
    }

    #[test]
    fn worker_components_do_not_overlap_or_escape() {
        // Dramatiq regression: Actor Pipeline ∩ Worker Runtime under Worker.
        let mut els = vec![
            el("dramatiq", ElementKind::SoftwareSystem, "Dramatiq", None),
            el("worker", ElementKind::Container, "Worker", Some("dramatiq")),
            el(
                "actor_pipeline",
                ElementKind::Component,
                "Actor Pipeline",
                Some("worker"),
            ),
            el(
                "worker_runtime",
                ElementKind::Component,
                "Worker Runtime",
                Some("worker"),
            ),
            el("Actor", ElementKind::Code, "Actor", Some("actor_pipeline")),
            el(
                "Message",
                ElementKind::Code,
                "Message",
                Some("actor_pipeline"),
            ),
            el(
                "WorkerThread",
                ElementKind::Code,
                "WorkerThread",
                Some("worker_runtime"),
            ),
            el(
                "Consumer",
                ElementKind::Code,
                "Consumer",
                Some("worker_runtime"),
            ),
        ];
        // Fat UML members → tall boxes (prod-like).
        for e in &mut els {
            if e.kind == ElementKind::Code {
                e.members = vec![
                    architect_c4_domain::CodeMember {
                        kind: architect_c4_domain::CodeMemberKind::Method,
                        visibility: "+".into(),
                        name: "send".into(),
                        params: vec![architect_c4_domain::CodeParam {
                            name: "message".into(),
                            type_name: Some("Message".into()),
                            optional: false,
                        }],
                        return_type: Some("Message".into()),
                        type_name: None,
                    },
                    architect_c4_domain::CodeMember {
                        kind: architect_c4_domain::CodeMemberKind::Method,
                        visibility: "+".into(),
                        name: "process".into(),
                        params: vec![],
                        return_type: None,
                        type_name: None,
                    },
                ];
            }
        }
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "Actor".into(),
                to_id: "Message".into(),
                description: Some("uses".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "WorkerThread".into(),
                to_id: "Actor".into(),
                description: Some("invoke".into()),
                technology: None,
            },
        ];
        let g = build_matryoshka(&els, &rels, None);
        let by: std::collections::HashMap<&str, &SceneNode> =
            g.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let worker = by["worker"];
        let pipe = by["actor_pipeline"];
        let runtime = by["worker_runtime"];
        let contains = |p: &SceneNode, c: &SceneNode| {
            c.x + 0.5 >= p.x
                && c.y + 0.5 >= p.y
                && c.x + c.w <= p.x + p.w + 0.5
                && c.y + c.h <= p.y + p.h + 0.5
        };
        assert!(
            contains(worker, pipe),
            "pipeline escapes worker: w={:?} pipe={:?}",
            (worker.x, worker.y, worker.w, worker.h),
            (pipe.x, pipe.y, pipe.w, pipe.h)
        );
        assert!(
            contains(worker, runtime),
            "runtime escapes worker: w={:?} rt={:?}",
            (worker.x, worker.y, worker.w, worker.h),
            (runtime.x, runtime.y, runtime.w, runtime.h)
        );
        let overlap = pipe.x < runtime.x + runtime.w
            && pipe.x + pipe.w > runtime.x
            && pipe.y < runtime.y + runtime.h
            && pipe.y + pipe.h > runtime.y;
        assert!(
            !overlap,
            "components overlap: pipe={:?} runtime={:?}",
            (pipe.x, pipe.y, pipe.w, pipe.h),
            (runtime.x, runtime.y, runtime.w, runtime.h)
        );
    }

    #[test]
    fn heavy_containers_sit_adjacent() {
        // Six direct code↔code edges between A and D → containers must sit close.
        let mut els = vec![el("sys", ElementKind::SoftwareSystem, "S", None)];
        for (id, name) in [("c_a", "A"), ("c_b", "B"), ("c_c", "C"), ("c_d", "D")] {
            els.push(el(id, ElementKind::Container, name, Some("sys")));
            els.push(el(
                &format!("{id}_comp"),
                ElementKind::Component,
                &format!("{name}Comp"),
                Some(id),
            ));
            els.push(el(
                &format!("{id}_code"),
                ElementKind::Code,
                &format!("{name}Code"),
                Some(&format!("{id}_comp")),
            ));
        }
        let mut rels = Vec::new();
        for i in 0..6 {
            rels.push(Relationship {
                id: format!("r{i}"),
                workspace_id: "w".into(),
                from_id: "c_a_code".into(),
                to_id: "c_d_code".into(),
                description: Some(format!("link{i}")),
                technology: None,
            });
        }
        let g = build_matryoshka(&els, &rels, None);
        let a = g.nodes.iter().find(|n| n.id == "c_a").unwrap();
        let d = g.nodes.iter().find(|n| n.id == "c_d").unwrap();
        let b = g.nodes.iter().find(|n| n.id == "c_b").unwrap();
        let dist = (a.x + a.w * 0.5 - (d.x + d.w * 0.5)).abs()
            + (a.y + a.h * 0.5 - (d.y + d.h * 0.5)).abs();
        let far = (a.x + a.w * 0.5 - (b.x + b.w * 0.5)).abs()
            + (a.y + a.h * 0.5 - (b.y + b.h * 0.5)).abs();
        assert!(
            dist <= far + 1.0,
            "heavy A↔D should be as close as A↔B: dist={dist} far_to_B={far} a=({},{}) d=({},{})",
            a.x,
            a.y,
            d.x,
            d.y
        );
    }
}
