//! C4 model → scene graph + nested container grouping (Structurizr-style).

mod bundle;
mod bus;
mod collision;
mod collision_pass;
mod highway;
mod labels;
mod matryoshka;
mod minla;
mod patterns;
mod ports;
mod router;
mod routing;
mod shapes;
mod uml;

pub use collision::{segment_hits_aabb, Aabb, SpatialHash};
pub use matryoshka::build_matryoshka;
pub use ports::Port;
pub use routing::{
    collect_viewpoints, is_inter_component, route_all_edges, route_orthogonal, EdgeClass,
    EdgeRoute, Side, Viewpoint,
};
pub use uml::{build_code_uml, class_box_size, class_members, class_members_for};

use architect_c4_domain::{Element, ElementKind, Relationship};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    Layer,
    /// Nested Context → Container → Component → Code in one scene.
    All,
}

impl ViewMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" | "landscape_all" | "full" => Self::All,
            _ => Self::Layer,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Layer => "layer",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNode {
    pub id: String,
    pub kind: String,
    pub layer: String,
    pub name: String,
    pub parent_id: Option<String>,
    /// True when this node is a grouping boundary (system/container/component with children).
    pub group: bool,
    pub depth: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// UML class body members (`+foo()`) for code-layer boxes.
    #[serde(default)]
    pub members: Vec<String>,
    /// UML stereotype (`Interface`, `Base`, …).
    #[serde(default)]
    pub stereotype: Option<String>,
    /// Optional external source link (e.g. GitHub blob URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
    /// Polyline in world coords (matryoshka router). Empty ⇒ legacy runtime routing.
    #[serde(default)]
    pub points: Vec<(f64, f64)>,
    #[serde(default)]
    pub from_port: String,
    #[serde(default)]
    pub to_port: String,
    /// Baseline position for edge label (after text collision resolve).
    #[serde(default)]
    pub label_x: f64,
    #[serde(default)]
    pub label_y: f64,
    /// `assoc` | `extends` | `implements` — WASM draws UML arrowheads.
    #[serde(default)]
    pub edge_kind: String,
}

/// Border viewpoint (◇) for WASM draw — filled by matryoshka.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScenePort {
    pub id: String,
    pub node_id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneGraph {
    pub mode: String,
    pub focus: Option<String>,
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    #[serde(default)]
    pub ports: Vec<ScenePort>,
}

#[derive(Debug, Clone)]
pub struct SceneInput<'a> {
    pub elements: &'a [Element],
    pub relationships: &'a [Relationship],
    pub mode: ViewMode,
    pub layer: Option<&'a str>,
    pub focus: Option<&'a str>,
}

pub fn build_scene(input: &SceneInput<'_>) -> SceneGraph {
    match input.mode {
        ViewMode::All => build_all_layers(input),
        ViewMode::Layer => build_layer(input),
    }
}

fn build_layer(input: &SceneInput<'_>) -> SceneGraph {
    let layer = input.layer.unwrap_or("context");
    // Code layer = UML class diagram (ADR 0004) — same semantics as Mermaid classDiagram.
    if layer == "code" {
        return uml::build_code_uml(input.elements, input.relationships, input.focus);
    }
    let kinds: &[ElementKind] = match layer {
        "container" => &[ElementKind::Container],
        "component" => &[ElementKind::Component],
        _ => &[ElementKind::Person, ElementKind::SoftwareSystem],
    };
    let focus = input.focus;
    let mut nodes: Vec<&Element> = input
        .elements
        .iter()
        .filter(|e| kinds.contains(&e.kind))
        .filter(|e| match (focus, e.kind) {
            (Some(f), ElementKind::Container | ElementKind::Component | ElementKind::Code) => {
                e.parent_id.as_deref() == Some(f)
                    || e.id == f
                    || is_descendant(input.elements, &e.id, f)
            }
            _ => true,
        })
        .collect();
    if layer == "context" || layer == "landscape" {
        nodes = input
            .elements
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ElementKind::Person | ElementKind::SoftwareSystem | ElementKind::External
                )
            })
            .collect();
    }
    layout_flat(&nodes, input.relationships, ViewMode::Layer, focus)
}

fn build_all_layers(input: &SceneInput<'_>) -> SceneGraph {
    // Matryoshka inside-out + LCA-shell routing (ADR 0006).
    build_matryoshka(input.elements, input.relationships, input.focus)
}

fn is_descendant(elements: &[Element], id: &str, ancestor: &str) -> bool {
    let mut cur = elements.iter().find(|e| e.id == id);
    while let Some(e) = cur {
        if e.parent_id.as_deref() == Some(ancestor) {
            return true;
        }
        cur = e
            .parent_id
            .as_deref()
            .and_then(|p| elements.iter().find(|x| x.id == p));
    }
    false
}

fn layer_name(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::Person | ElementKind::SoftwareSystem => "context",
        ElementKind::External => "external",
        ElementKind::Container => "container",
        ElementKind::Component => "component",
        ElementKind::Code => "code",
    }
}

fn layout_flat(
    elements: &[&Element],
    relationships: &[Relationship],
    mode: ViewMode,
    focus: Option<&str>,
) -> SceneGraph {
    let cols = 3usize;
    let nw = 200.0;
    let nh = 88.0;
    let gap_x = 40.0;
    let gap_y = 36.0;
    let pad = 40.0;
    let mut nodes = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        nodes.push(SceneNode {
            id: e.id.clone(),
            kind: e.kind.as_str().into(),
            layer: layer_name(e.kind).into(),
            name: e.name.clone(),
            parent_id: e.parent_id.clone(),
            group: false,
            depth: 0,
            x: pad + col as f64 * (nw + gap_x),
            y: pad + row as f64 * (nh + gap_y),
            w: nw,
            h: nh,

            members: vec![],
            stereotype: None,
            url: e.url.clone(),
        });
    }
    let edges = edges_for(elements, relationships);
    let width = pad * 2.0
        + (cols.min(elements.len().max(1)) as f64) * nw
        + (cols.saturating_sub(1).min(elements.len().saturating_sub(1)) as f64) * gap_x;
    let rows = elements.len().div_ceil(cols).max(1) as f64;
    let height = pad * 2.0 + rows * nh + (rows - 1.0).max(0.0) * gap_y;
    SceneGraph {
        mode: mode.as_str().into(),
        focus: focus.map(str::to_string),
        width: width.max(400.0),
        height: height.max(300.0),
        nodes,
        edges,
        ports: vec![],
    }
}

fn edges_for(elements: &[&Element], relationships: &[Relationship]) -> Vec<SceneEdge> {
    let ids: std::collections::HashSet<&str> = elements.iter().map(|e| e.id.as_str()).collect();
    relationships
        .iter()
        .filter(|r| ids.contains(r.from_id.as_str()) && ids.contains(r.to_id.as_str()))
        .map(|r| SceneEdge {
            id: r.id.clone(),
            from: r.from_id.clone(),
            to: r.to_id.clone(),
            label: truncate_label(r.description.as_deref().unwrap_or("uses"), 28),
            points: vec![],
            from_port: String::new(),
            to_port: String::new(),
            label_x: 0.0,
            label_y: 0.0,

            edge_kind: String::new(),
        })
        .collect()
}

fn truncate_label(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

pub fn scene_to_json(scene: &SceneGraph) -> String {
    serde_json::to_string(scene).expect("scene json")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn all_mode_groups_inside_container_bounds() {
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("h", ElementKind::Component, "Handler", Some("api")),
            el("fn", ElementKind::Code, "Handle", Some("h")),
        ];
        let g = build_scene(&SceneInput {
            elements: &els,
            relationships: &[],
            mode: ViewMode::All,
            layer: None,
            focus: None,
        });
        let sys = g.nodes.iter().find(|n| n.id == "sys").unwrap();
        let api = g.nodes.iter().find(|n| n.id == "api").unwrap();
        let h = g.nodes.iter().find(|n| n.id == "h").unwrap();
        assert!(sys.group);
        assert!(api.group);
        // children geometrically inside parent boundary
        assert!(api.x >= sys.x && api.x + api.w <= sys.x + sys.w + 0.1);
        assert!(api.y >= sys.y && api.y + api.h <= sys.y + sys.h + 0.1);
        assert!(h.x >= api.x && h.x + h.w <= api.x + api.w + 0.1);
    }

    #[test]
    fn focus_container_scopes_subtree_and_ancestors() {
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("other", ElementKind::Container, "Other", Some("sys")),
            el("h", ElementKind::Component, "Handler", Some("api")),
        ];
        let g = build_scene(&SceneInput {
            elements: &els,
            relationships: &[],
            mode: ViewMode::All,
            layer: None,
            focus: Some("api"),
        });
        let ids: Vec<_> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"sys"));
        assert!(ids.contains(&"api"));
        assert!(ids.contains(&"h"));
        assert!(!ids.contains(&"other"));
    }

    #[test]
    fn view_mode_parse() {
        assert_eq!(ViewMode::parse("all"), ViewMode::All);
        assert_eq!(ViewMode::parse("context"), ViewMode::Layer);
        assert_eq!(ViewMode::parse("full").as_str(), "all");
        assert_eq!(ViewMode::Layer.as_str(), "layer");
    }

    #[test]
    fn layer_mode_filters_kinds() {
        let els = vec![
            el("u", ElementKind::Person, "User", None),
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
        ];
        let g = build_scene(&SceneInput {
            elements: &els,
            relationships: &[],
            mode: ViewMode::Layer,
            layer: Some("container"),
            focus: Some("sys"),
        });
        assert!(g.nodes.iter().all(|n| n.kind == "container"));
        assert_eq!(g.nodes.len(), 1);
    }

    #[test]
    fn all_mode_includes_nested_layers() {
        let els = vec![
            el("u", ElementKind::Person, "User", None),
            el("sys", ElementKind::SoftwareSystem, "Sys", None),
            el("api", ElementKind::Container, "API", Some("sys")),
            el("h", ElementKind::Component, "Handler", Some("api")),
            el("fn", ElementKind::Code, "Handle", Some("h")),
        ];
        let rels = vec![Relationship {
            id: "r1".into(),
            workspace_id: "w".into(),
            from_id: "u".into(),
            to_id: "sys".into(),
            description: Some("Uses".into()),
            technology: None,
        }];
        let g = build_scene(&SceneInput {
            elements: &els,
            relationships: &rels,
            mode: ViewMode::All,
            layer: None,
            focus: None,
        });
        assert_eq!(g.mode, "all");
        assert_eq!(g.nodes.len(), 5);
        assert!(g.nodes.iter().any(|n| n.layer == "code" && !n.group));
    }
}
