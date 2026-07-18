//! View projections (V3): lift atom/external edges to the layer being drawn.

use crate::{C4Layer, Element, ElementKind, Relationship};
use std::collections::HashMap;

fn by_id(elements: &[Element]) -> HashMap<&str, &Element> {
    elements.iter().map(|e| (e.id.as_str(), e)).collect()
}

/// Walk parent chain until `kind` is in `stop` (inclusive).
fn project_up<'a>(
    id: &str,
    by: &HashMap<&str, &'a Element>,
    stop: &[ElementKind],
) -> Option<&'a Element> {
    let mut cur = *by.get(id)?;
    for _ in 0..64 {
        if stop.contains(&cur.kind) {
            return Some(cur);
        }
        let pid = cur.parent_id.as_deref()?;
        cur = *by.get(pid)?;
    }
    None
}

fn stop_kinds(layer: C4Layer) -> &'static [ElementKind] {
    match layer {
        C4Layer::Landscape | C4Layer::Context => &[
            ElementKind::Person,
            ElementKind::SoftwareSystem,
            ElementKind::External,
        ],
        C4Layer::Container => &[ElementKind::Container, ElementKind::External],
        C4Layer::Component => &[ElementKind::Component, ElementKind::External],
        C4Layer::Code => &[ElementKind::Code, ElementKind::External],
        C4Layer::Adr => &[],
    }
}

/// Project a single endpoint id into the visible kind set for `layer`.
pub fn project_endpoint_id(id: &str, elements: &[Element], layer: C4Layer) -> Option<String> {
    let by = by_id(elements);
    project_up(id, &by, stop_kinds(layer)).map(|e| e.id.clone())
}

/// Collapse atom-level relationships into unique projected edges for a view layer.
///
/// Duplicate (from,to) pairs merge: description becomes `N× first` when N>1.
pub fn project_relationships(
    elements: &[Element],
    relationships: &[Relationship],
    layer: C4Layer,
) -> Vec<Relationship> {
    if matches!(layer, C4Layer::Code | C4Layer::Adr) {
        return relationships.to_vec();
    }
    let by = by_id(elements);
    let stop = stop_kinds(layer);
    let mut acc: HashMap<(String, String), (Relationship, usize)> = HashMap::new();

    for r in relationships {
        let Some(from_el) = project_up(&r.from_id, &by, stop) else {
            continue;
        };
        let Some(to_el) = project_up(&r.to_id, &by, stop) else {
            continue;
        };
        if from_el.id == to_el.id {
            continue; // internal to same projected box
        }
        let key = (from_el.id.clone(), to_el.id.clone());
        acc.entry(key)
            .and_modify(|(_rel, n)| *n += 1)
            .or_insert_with(|| {
                (
                    Relationship {
                        id: format!("proj:{}:{}:{}", layer.as_str(), from_el.id, to_el.id),
                        workspace_id: r.workspace_id.clone(),
                        from_id: from_el.id.clone(),
                        to_id: to_el.id.clone(),
                        description: r.description.clone(),
                        technology: r.technology.clone(),
                    },
                    1usize,
                )
            });
    }

    let mut out: Vec<Relationship> = acc
        .into_values()
        .map(|(mut rel, n)| {
            if n > 1 {
                let base = rel.description.unwrap_or_else(|| "uses".into());
                rel.description = Some(format!("{n}× {base}"));
            }
            rel
        })
        .collect();
    out.sort_by(|a, b| a.from_id.cmp(&b.from_id).then(a.to_id.cmp(&b.to_id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementKind;

    fn el(id: &str, kind: ElementKind, parent: Option<&str>) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind,
            parent_id: parent.map(str::to_string),
            name: id.into(),
            description: Some("d".into()),
            technology: Some("class".into()),
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn projects_code_edge_to_containers() {
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, None),
            el("api", ElementKind::Container, Some("sys")),
            el("db", ElementKind::Container, Some("sys")),
            el("c1", ElementKind::Component, Some("api")),
            el("c2", ElementKind::Component, Some("db")),
            el("a", ElementKind::Code, Some("c1")),
            el("b", ElementKind::Code, Some("c2")),
        ];
        let rels = vec![
            Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("writes".into()),
                technology: None,
            },
            Relationship {
                id: "r2".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("reads".into()),
                technology: None,
            },
        ];
        let proj = project_relationships(&els, &rels, C4Layer::Container);
        assert_eq!(proj.len(), 1);
        assert_eq!(proj[0].from_id, "api");
        assert_eq!(proj[0].to_id, "db");
        assert!(
            proj[0]
                .description
                .as_deref()
                .unwrap_or("")
                .starts_with("2×"),
            "{:?}",
            proj[0].description
        );
    }

    #[test]
    fn projects_to_system_on_context() {
        let els = vec![
            el("sys", ElementKind::SoftwareSystem, None),
            el("ext", ElementKind::External, None),
            el("api", ElementKind::Container, Some("sys")),
            el("c", ElementKind::Component, Some("api")),
            el("a", ElementKind::Code, Some("c")),
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "a".into(),
            to_id: "ext".into(),
            description: Some("sql".into()),
            technology: None,
        }];
        let proj = project_relationships(&els, &rels, C4Layer::Context);
        assert_eq!(proj.len(), 1);
        assert_eq!(proj[0].from_id, "sys");
        assert_eq!(proj[0].to_id, "ext");
    }
}
