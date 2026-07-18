//! C4 Code layer as UML class diagram (parity with Mermaid `classDiagram`, ADR 0004).
//! WASM draws the same scene; Mermaid keeps its own DSL generator in render.

use crate::collision::Aabb;
use crate::labels::{place_edge_labels, text_width};
use crate::ports::{allocate_side_ports, pick_port, side_facing, Port};
use crate::router::route_port_to_port;
use crate::{SceneEdge, SceneGraph, SceneNode, ScenePort, Side, ViewMode};
use architect_c4_domain::{Element, ElementKind, Relationship};
use std::collections::{HashMap, HashSet};

/// UML member lines for a code element (structured `members` preferred).
pub fn class_members_for(el: &Element) -> Vec<String> {
    architect_c4_domain::element_uml_members(el)
        .into_iter()
        .map(sanitize_member_line)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Legacy: UML-like members from `description` only (`+foo()`, `-bar`, …).
pub fn class_members(description: Option<&str>) -> Vec<String> {
    let el = Element {
        id: "_".into(),
        workspace_id: "_".into(),
        kind: ElementKind::Code,
        parent_id: None,
        name: "_".into(),
        description: description.map(str::to_string),
        technology: None,
        url: None,
        members: vec![],
    };
    class_members_for(&el)
}

/// Keep typed signatures; camelCase only the method/field identifier, not params.
fn sanitize_member_line(s: String) -> String {
    let t = s.trim();
    let (vis, rest) = if let Some(r) = t.strip_prefix('+') {
        ("+", r)
    } else if let Some(r) = t.strip_prefix('-') {
        ("-", r)
    } else if let Some(r) = t.strip_prefix('#') {
        ("#", r)
    } else if let Some(r) = t.strip_prefix('~') {
        ("~", r)
    } else {
        return String::new();
    };
    let rest = rest.trim();
    if let Some(paren) = rest.find('(') {
        let ident = rest[..paren].trim();
        let after = &rest[paren..];
        let name = snake_ident_to_camel(ident);
        return format!("{vis}{name}{after}");
    }
    if let Some(colon) = rest.find(':') {
        let ident = rest[..colon].trim();
        let ty = rest[colon + 1..].trim();
        let name = snake_ident_to_camel(ident);
        if ty.is_empty() {
            return format!("{vis}{name}");
        }
        return format!("{vis}{name}: {ty}");
    }
    format!("{vis}{}", snake_ident_to_camel(rest))
}

fn snake_ident_to_camel(s: &str) -> String {
    let mut parts = s.split('_').filter(|p| !p.is_empty());
    let mut name = parts.next().unwrap_or("").to_string();
    for p in parts {
        let mut chs = p.chars();
        if let Some(f) = chs.next() {
            name.extend(f.to_uppercase());
            name.push_str(chs.as_str());
        }
    }
    name
}

pub fn stereotype_of(
    id: &str,
    technology: Option<&str>,
    interfaces: &HashSet<&str>,
    bases: &HashSet<&str>,
) -> Option<String> {
    if interfaces.contains(id) {
        return Some("Interface".into());
    }
    if bases.contains(id) {
        return Some("Base".into());
    }
    let tech = technology.unwrap_or("").trim();
    if tech.is_empty() {
        return None;
    }
    let low = tech.to_ascii_lowercase();
    match low.as_str() {
        "class" | "cls" => None,
        "interface" => Some("Interface".into()),
        "enum" => Some("Enum".into()),
        "base" | "abstract" => Some("Base".into()),
        _ => {
            // Language hint (cpp, rust) is not a UML stereotype — skip.
            if tech.len() <= 12 && tech.chars().all(|c| c.is_ascii_alphanumeric()) {
                None
            } else {
                Some(tech.into())
            }
        }
    }
}

/// Size a UML class box from name + members (compartments).
pub fn class_box_size(name: &str, stereotype: Option<&str>, members: &[String]) -> (f64, f64) {
    let title_w = text_width(name, 13.0);
    let stereo_w = stereotype
        .map(|s| text_width(&format!("«{s}»"), 11.0))
        .unwrap_or(0.0);
    let mem_w = members
        .iter()
        .map(|m| text_width(m, 11.0))
        .fold(0.0_f64, f64::max);
    let w = title_w.max(stereo_w).max(mem_w).max(140.0) + 40.0;
    let header = if stereotype.is_some() { 40.0 } else { 32.0 };
    let body_lines = members.len().max(1) as f64;
    // Name + divider + member rows (16px) + bottom pad — taller than plain C4 leaf.
    let h = header + 14.0 + body_lines * 16.0 + 18.0;
    (w, h.max(88.0))
}

fn edge_kind_of(desc: &str) -> &'static str {
    let d = desc.to_ascii_lowercase();
    if d.contains("extends") || d.contains("inherit") {
        "extends"
    } else if d.contains("implements") {
        "implements"
    } else {
        "assoc"
    }
}

/// Build WASM/Mermaid-parity UML scene for `layer=code`.
pub fn build_code_uml(
    elements: &[Element],
    relationships: &[Relationship],
    parent_id: Option<&str>,
) -> SceneGraph {
    let Some(parent) = parent_id else {
        return empty_code("pick a component parent");
    };
    let els: Vec<&Element> = elements
        .iter()
        .filter(|e| e.kind == ElementKind::Code && e.parent_id.as_deref() == Some(parent))
        .collect();
    if els.is_empty() {
        return empty_code("no code elements yet");
    }

    let ids: HashSet<&str> = els.iter().map(|e| e.id.as_str()).collect();
    let mut interfaces = HashSet::new();
    let mut bases = HashSet::new();
    let mut has_inherit = false;
    for r in relationships {
        if !(ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str())) {
            continue;
        }
        let kind = edge_kind_of(r.description.as_deref().unwrap_or(""));
        match kind {
            "implements" => {
                interfaces.insert(r.to_id.as_str());
                has_inherit = true;
            }
            "extends" => {
                bases.insert(r.to_id.as_str());
                has_inherit = true;
            }
            _ => {}
        }
    }

    // Parent namespace frame (like Mermaid `namespace Parent { … }`).
    let parent_el = elements.iter().find(|e| e.id == parent);
    let parent_name = parent_el.map(|e| e.name.as_str()).unwrap_or(parent);

    let mut classes: Vec<SceneNode> = Vec::new();
    for e in &els {
        let members = class_members_for(e);
        let stereo = stereotype_of(e.id.as_str(), e.technology.as_deref(), &interfaces, &bases);
        let (w, h) = class_box_size(&e.name, stereo.as_deref(), &members);
        classes.push(SceneNode {
            id: e.id.clone(),
            kind: "code".into(),
            layer: "code".into(),
            name: e.name.clone(),
            parent_id: e.parent_id.clone(),
            group: false,
            depth: 1,
            x: 0.0,
            y: 0.0,
            w,
            h,
            members,
            stereotype: stereo,
            url: e.url.clone(),
        });
    }

    // Place: TB for inheritance, LR for flat deps — same rule as Mermaid.
    let pad = 48.0;
    let gap = 160.0; // room for Rel chips between classes
    let header = 40.0;
    if has_inherit {
        // Bases / interfaces on top row, children below.
        let mut top: Vec<usize> = Vec::new();
        let mut bot: Vec<usize> = Vec::new();
        for (i, n) in classes.iter().enumerate() {
            if n.stereotype.as_deref() == Some("Interface")
                || n.stereotype.as_deref() == Some("Base")
                || bases.contains(n.id.as_str())
                || interfaces.contains(n.id.as_str())
            {
                top.push(i);
            } else {
                bot.push(i);
            }
        }
        if top.is_empty() {
            let all: Vec<usize> = (0..classes.len()).collect();
            place_row(&mut classes, &all, pad, header + pad, gap);
        } else {
            place_row(&mut classes, &top, pad, header + pad, gap);
            let top_h = top.iter().map(|&i| classes[i].h).fold(0.0_f64, f64::max);
            place_row(&mut classes, &bot, pad, header + pad + top_h + gap, gap);
        }
    } else {
        let all: Vec<usize> = (0..classes.len()).collect();
        place_row(&mut classes, &all, pad, header + pad, gap);
    }

    let inner_r = classes.iter().map(|n| n.x + n.w).fold(pad, f64::max);
    let inner_b = classes
        .iter()
        .map(|n| n.y + n.h)
        .fold(header + pad, f64::max);
    let frame_w = (inner_r + pad).max(text_width(parent_name, 13.0) + 64.0);
    let frame_h = inner_b + pad;

    let mut nodes = vec![SceneNode {
        id: parent.to_string(),
        kind: "component".into(),
        layer: "component".into(),
        name: parent_name.to_string(),
        parent_id: parent_el.and_then(|e| e.parent_id.clone()),
        group: true,
        depth: 0,
        x: 0.0,
        y: 0.0,
        w: frame_w,
        h: frame_h,
        members: vec![],
        stereotype: None,
        url: None,
    }];
    // Shift classes into frame coords (already relative to pad).
    nodes.extend(classes);

    // Iterative gap expand for label chips (same idea as matryoshka).
    let mut gap_extra = 0.0_f64;
    let mut scene_edges = Vec::new();
    let mut used_ports = Vec::new();
    for _ in 0..4 {
        // Re-spread code leaves horizontally with gap_extra.
        respread_code_leaves(&mut nodes, pad, header + pad, gap + gap_extra, has_inherit);
        // Resize frame
        let r = nodes
            .iter()
            .filter(|n| n.id != parent)
            .map(|n| n.x + n.w)
            .fold(pad, f64::max);
        let b = nodes
            .iter()
            .filter(|n| n.id != parent)
            .map(|n| n.y + n.h)
            .fold(header + pad, f64::max);
        if let Some(frame) = nodes.iter_mut().find(|n| n.id == parent) {
            frame.w = (r + pad).max(text_width(parent_name, 13.0) + 64.0);
            frame.h = b + pad;
        }

        let (edges, ports) = route_code_edges(&nodes, relationships, &ids);
        scene_edges = edges;
        used_ports = ports;
        place_edge_labels(&mut scene_edges, &nodes, &used_ports);

        let mut need = 0.0_f64;
        for e in &scene_edges {
            let lw = text_width(e.label.lines().next().unwrap_or(""), 10.0) + 64.0;
            need = need.max(lw);
            let chip = crate::labels::text_aabb(e.label_x, e.label_y, &e.label, 10.0).inflate(6.0);
            for n in nodes.iter().filter(|n| !n.group) {
                if chip.overlaps(&Aabb::from_node(n, 0.0)) {
                    need = need.max(lw + 40.0);
                }
            }
        }
        if need > gap + gap_extra + 1.0 {
            gap_extra = need - gap;
        } else {
            break;
        }
    }

    let width = nodes.iter().map(|n| n.x + n.w).fold(480.0, f64::max) + 48.0;
    let height = nodes.iter().map(|n| n.y + n.h).fold(360.0, f64::max) + 48.0;
    SceneGraph {
        mode: ViewMode::Layer.as_str().into(),
        focus: Some(parent.to_string()),
        width,
        height,
        nodes,
        edges: scene_edges,
        ports: used_ports,
    }
}

fn empty_code(msg: &str) -> SceneGraph {
    SceneGraph {
        mode: ViewMode::Layer.as_str().into(),
        focus: None,
        width: 420.0,
        height: 240.0,
        nodes: vec![SceneNode {
            id: "Empty".into(),
            kind: "code".into(),
            layer: "code".into(),
            name: "Empty".into(),
            parent_id: None,
            group: false,
            depth: 0,
            x: 80.0,
            y: 60.0,
            w: 220.0,
            h: 100.0,
            members: vec![msg.into()],
            stereotype: None,
            url: None,
        }],
        edges: vec![],
        ports: vec![],
    }
}

fn place_row(classes: &mut [SceneNode], idxs: &[usize], x0: f64, y: f64, gap: f64) {
    let mut x = x0;
    for &i in idxs {
        classes[i].x = x;
        classes[i].y = y;
        x += classes[i].w + gap;
    }
}

fn respread_code_leaves(nodes: &mut [SceneNode], x0: f64, y0: f64, gap: f64, has_inherit: bool) {
    let parent_id = nodes
        .iter()
        .find(|n| n.group)
        .map(|n| n.id.clone())
        .unwrap_or_default();
    let mut idxs: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.group && n.kind == "code")
        .map(|(i, _)| i)
        .collect();
    if idxs.is_empty() {
        return;
    }
    if has_inherit {
        let mut top = Vec::new();
        let mut bot = Vec::new();
        for &i in &idxs {
            let st = nodes[i].stereotype.as_deref();
            if st == Some("Interface") || st == Some("Base") {
                top.push(i);
            } else {
                bot.push(i);
            }
        }
        if top.is_empty() {
            top = idxs;
            bot.clear();
        }
        let mut x = x0;
        let mut top_h = 0.0_f64;
        for &i in &top {
            nodes[i].x = x;
            nodes[i].y = y0;
            top_h = top_h.max(nodes[i].h);
            x += nodes[i].w + gap;
        }
        if !bot.is_empty() {
            x = x0;
            let y = y0 + top_h + gap;
            for &i in &bot {
                nodes[i].x = x;
                nodes[i].y = y;
                x += nodes[i].w + gap;
            }
        }
    } else {
        idxs.sort_by(|a, b| nodes[*a].x.partial_cmp(&nodes[*b].x).unwrap());
        let mut x = x0;
        for i in idxs {
            nodes[i].x = x;
            nodes[i].y = y0;
            x += nodes[i].w + gap;
        }
    }
    let _ = parent_id;
}

fn route_code_edges(
    nodes: &[SceneNode],
    relationships: &[Relationship],
    ids: &HashSet<&str>,
) -> (Vec<SceneEdge>, Vec<ScenePort>) {
    let by: HashMap<&str, &SceneNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut port_bag: HashMap<String, Vec<Port>> = HashMap::new();
    for n in nodes.iter().filter(|n| !n.group) {
        let mut ports = Vec::new();
        for side in [Side::N, Side::E, Side::S, Side::W] {
            ports.extend(allocate_side_ports(&n.id, side, n.x, n.y, n.w, n.h, 3));
        }
        port_bag.insert(n.id.clone(), ports);
    }
    let mut edges = Vec::new();
    let mut used_ports = Vec::new();
    let mut used_ids = HashSet::new();
    let obstacles: Vec<Aabb> = nodes
        .iter()
        .filter(|n| !n.group)
        .map(|n| Aabb::from_node(n, 4.0))
        .collect();

    for r in relationships {
        if !(ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str())) {
            continue;
        }
        let Some(from) = by.get(r.from_id.as_str()) else {
            continue;
        };
        let Some(to) = by.get(r.to_id.as_str()) else {
            continue;
        };
        let ek = edge_kind_of(r.description.as_deref().unwrap_or(""));
        // UML: inheritance arrow points to base (to); assoc from→to.
        let (src, dst) = match ek {
            "extends" | "implements" => (from, to), // polyline from child to base; head is UML hollow △ at base
            _ => (from, to),
        };
        let toward_dst = (dst.x + dst.w / 2.0, dst.y + dst.h / 2.0);
        let toward_src = (src.x + src.w / 2.0, src.y + src.h / 2.0);
        let side_a = side_facing(src.x, src.y, src.w, src.h, toward_dst);
        let side_b = side_facing(dst.x, dst.y, dst.w, dst.h, toward_src);
        let prefer_a = match side_a {
            Side::N | Side::S => toward_dst.0,
            _ => toward_dst.1,
        };
        let prefer_b = match side_b {
            Side::N | Side::S => toward_src.0,
            _ => toward_src.1,
        };
        let pa = {
            let bag = port_bag.get(&src.id).unwrap();
            pick_port(bag, side_a, prefer_a)
                .cloned()
                .unwrap_or_else(|| bag[0].clone())
        };
        let pb = {
            let bag = port_bag.get(&dst.id).unwrap();
            pick_port(bag, side_b, prefer_b)
                .cloned()
                .unwrap_or_else(|| bag[0].clone())
        };
        let obs: Vec<Aabb> = nodes
            .iter()
            .filter(|n| !n.group && n.id != src.id && n.id != dst.id)
            .map(|n| Aabb::from_node(n, 4.0))
            .collect();
        let _ = obstacles;
        let points = route_port_to_port(&pa, &pb, &obs, 6.0);
        let label = match ek {
            "extends" => "extends".into(),
            "implements" => "implements".into(),
            _ => truncate(r.description.as_deref().unwrap_or("uses"), 28),
        };
        edges.push(SceneEdge {
            id: r.id.clone(),
            from: r.from_id.clone(),
            to: r.to_id.clone(),
            label,
            points,
            from_port: pa.id.clone(),
            to_port: pb.id.clone(),
            label_x: 0.0,
            label_y: 0.0,
            edge_kind: ek.into(),
        });
        for p in [pa, pb] {
            if used_ids.insert(p.id.clone()) {
                used_ports.push(ScenePort {
                    id: p.id,
                    node_id: p.node_id,
                    x: p.x,
                    y: p.y,
                });
            }
        }
    }
    (edges, used_ports)
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ElementKind;

    fn el(id: &str, name: &str, parent: &str, desc: &str, tech: &str) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind: ElementKind::Code,
            parent_id: Some(parent.into()),
            name: name.into(),
            description: Some(desc.into()),
            technology: Some(tech.into()),
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn code_uml_builds_class_compartments() {
        let elements = vec![
            Element {
                id: "svc".into(),
                workspace_id: "w".into(),
                kind: ElementKind::Component,
                parent_id: Some("osd".into()),
                name: "OSD Service".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            },
            el(
                "am",
                "AsyncMessenger",
                "svc",
                "+start(); +send_message()",
                "class",
            ),
            el("osd", "OSD", "svc", "+handle(); -tick()", "class"),
            el("osvc", "OSDService", "svc", "+lookup()", "class"),
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "osd".into(),
                to_id: "am".into(),
                description: Some("uses".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "osd".into(),
                to_id: "osvc".into(),
                description: Some("owns".into()),
                technology: None,
            },
        ];
        let g = build_code_uml(&elements, &rels, Some("svc"));
        let osd = g.nodes.iter().find(|n| n.id == "osd").unwrap();
        assert!(
            osd.members.iter().any(|m| m.contains("handle")),
            "members={:?}",
            osd.members
        );
        assert!(osd.h > 90.0, "UML box should grow for members: {}", osd.h);
        let am = g.nodes.iter().find(|n| n.id == "am").unwrap();
        let gap = osd.x - (am.x + am.w);
        assert!(
            gap >= 140.0,
            "classes must push apart for labels, gap={gap}"
        );
        assert!(g.edges.iter().all(|e| e.points.len() >= 2));
    }

    #[test]
    fn implements_gets_stereotype_and_edge_kind() {
        let elements = vec![
            Element {
                id: "os".into(),
                workspace_id: "w".into(),
                kind: ElementKind::Component,
                parent_id: None,
                name: "ObjectStore".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            },
            el(
                "iface",
                "ObjectStore",
                "os",
                "+read(); +write()",
                "interface",
            ),
            el("bs", "BlueStore", "os", "+read(); +write()", "class"),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "bs".into(),
            to_id: "iface".into(),
            description: Some("implements".into()),
            technology: None,
        }];
        let g = build_code_uml(&elements, &rels, Some("os"));
        let iface = g.nodes.iter().find(|n| n.id == "iface").unwrap();
        assert_eq!(iface.stereotype.as_deref(), Some("Interface"));
        assert_eq!(g.edges[0].edge_kind, "implements");
    }
}
