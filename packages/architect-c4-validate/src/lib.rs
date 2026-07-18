//! Pure validation rules over domain snapshots (no IO).

use std::collections::{HashMap, HashSet};

use architect_c4_domain::{
    C4Layer, Decision, Element, ElementKind, Problem, Relationship, Severity,
};

#[derive(Debug, Clone, Default)]
pub struct ModelSnapshot {
    pub elements: Vec<Element>,
    pub relationships: Vec<Relationship>,
    pub decisions: Vec<Decision>,
}

pub fn validate_model(snap: &ModelSnapshot) -> Vec<Problem> {
    let mut problems = Vec::new();
    if snap.elements.is_empty() {
        problems.push(Problem {
            severity: Severity::Error,
            layer: C4Layer::Landscape,
            code: "model.empty".into(),
            element_id: None,
            message: "LANDSCAPE layer: model has no elements".into(),
            adr_id: None,
        });
        return problems;
    }

    let ids: HashSet<&str> = snap.elements.iter().map(|e| e.id.as_str()).collect();
    let mut children: HashMap<&str, Vec<&Element>> = HashMap::new();
    for e in &snap.elements {
        if let Some(p) = &e.parent_id {
            children.entry(p.as_str()).or_default().push(e);
        }
        if e.description
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            problems.push(Problem {
                severity: Severity::Warning,
                layer: e.kind.layer(),
                code: "element.missing_description".into(),
                element_id: Some(e.id.clone()),
                message: format!(
                    "{} layer: {} \"{}\" missing description",
                    e.kind.layer().as_str().to_ascii_uppercase(),
                    e.kind.as_str(),
                    e.name
                ),
                adr_id: None,
            });
        }
        if matches!(
            e.kind,
            ElementKind::Container
                | ElementKind::Component
                | ElementKind::Code
                | ElementKind::External
        ) && e
            .technology
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            problems.push(Problem {
                severity: Severity::Warning,
                layer: e.kind.layer(),
                code: "element.missing_technology".into(),
                element_id: Some(e.id.clone()),
                message: format!(
                    "{} layer: {} \"{}\" missing technology",
                    e.kind.layer().as_str().to_ascii_uppercase(),
                    e.kind.as_str(),
                    e.name
                ),
                adr_id: None,
            });
        }
        if let Some(p) = &e.parent_id {
            if !ids.contains(p.as_str()) {
                problems.push(Problem {
                    severity: Severity::Error,
                    layer: e.kind.layer(),
                    code: "element.dangling_parent".into(),
                    element_id: Some(e.id.clone()),
                    message: format!(
                        "{} layer: element \"{}\" parent \"{}\" not found",
                        e.kind.layer().as_str().to_ascii_uppercase(),
                        e.id,
                        p
                    ),
                    adr_id: None,
                });
            }
        }
    }

    let connected: HashSet<&str> = snap
        .relationships
        .iter()
        .flat_map(|r| [r.from_id.as_str(), r.to_id.as_str()])
        .collect();

    for e in &snap.elements {
        if !connected.contains(e.id.as_str()) && snap.elements.len() > 1 {
            problems.push(Problem {
                severity: Severity::Warning,
                layer: e.kind.layer(),
                code: "element.disconnected".into(),
                element_id: Some(e.id.clone()),
                message: format!(
                    "{} layer: {} \"{}\" is disconnected",
                    e.kind.layer().as_str().to_ascii_uppercase(),
                    e.kind.as_str(),
                    e.name
                ),
                adr_id: None,
            });
        }
    }

    for r in &snap.relationships {
        if !ids.contains(r.from_id.as_str()) || !ids.contains(r.to_id.as_str()) {
            problems.push(Problem {
                severity: Severity::Error,
                layer: C4Layer::Context,
                code: "relationship.dangling_endpoint".into(),
                element_id: Some(r.id.clone()),
                message: format!(
                    "CONTEXT layer: relationship \"{}\" references missing element",
                    r.id
                ),
                adr_id: None,
            });
        }
        if r.description
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            problems.push(Problem {
                severity: Severity::Warning,
                layer: C4Layer::Context,
                code: "relationship.missing_description".into(),
                element_id: Some(r.id.clone()),
                message: format!(
                    "CONTEXT layer: relationship \"{}\" missing description",
                    r.id
                ),
                adr_id: None,
            });
        }
    }

    // Structurizr-like: software system with containers but no scoped ADRs
    for e in &snap.elements {
        if e.kind != ElementKind::SoftwareSystem {
            continue;
        }
        let has_containers = children
            .get(e.id.as_str())
            .map(|c| c.iter().any(|x| x.kind == ElementKind::Container))
            .unwrap_or(false);
        if !has_containers {
            continue;
        }
        let has_adr = snap
            .decisions
            .iter()
            .any(|d| d.scope_element_id.as_deref() == Some(e.id.as_str()));
        if !has_adr {
            problems.push(Problem {
                severity: Severity::Warning,
                layer: C4Layer::Adr,
                code: "system.missing_decisions".into(),
                element_id: Some(e.id.clone()),
                message: format!(
                    "ADR layer: software system \"{}\" has containers but no ADRs scoped to it",
                    e.name
                ),
                adr_id: None,
            });
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(id: &str, kind: ElementKind, parent: Option<&str>, name: &str) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind,
            parent_id: parent.map(str::to_string),
            name: name.into(),
            description: Some("d".into()),
            technology: Some("t".into()),
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn empty_model_is_error() {
        let p = validate_model(&ModelSnapshot::default());
        assert_eq!(p[0].code, "model.empty");
    }

    #[test]
    fn missing_description_and_technology() {
        let mut e = el("c1", ElementKind::Container, Some("s1"), "API");
        e.description = None;
        e.technology = None;
        let snap = ModelSnapshot {
            elements: vec![el("s1", ElementKind::SoftwareSystem, None, "Sys"), e],
            relationships: vec![],
            decisions: vec![],
        };
        let problems = validate_model(&snap);
        let codes: Vec<_> = problems.iter().map(|p| p.code.as_str()).collect();
        assert!(codes.contains(&"element.missing_description"));
        assert!(codes.contains(&"element.missing_technology"));
    }

    #[test]
    fn system_with_containers_needs_adr() {
        let snap = ModelSnapshot {
            elements: vec![
                el("s1", ElementKind::SoftwareSystem, None, "Billing"),
                el("c1", ElementKind::Container, Some("s1"), "API"),
            ],
            relationships: vec![Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "s1".into(),
                to_id: "c1".into(),
                description: Some("owns".into()),
                technology: None,
            }],
            decisions: vec![],
        };
        let p = validate_model(&snap);
        assert!(p.iter().any(|x| x.code == "system.missing_decisions"));
        assert!(p.iter().any(|x| x.layer == C4Layer::Adr));
    }

    #[test]
    fn adr_scoped_clears_missing_decisions() {
        let snap = ModelSnapshot {
            elements: vec![
                el("s1", ElementKind::SoftwareSystem, None, "Billing"),
                el("c1", ElementKind::Container, Some("s1"), "API"),
            ],
            relationships: vec![Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "s1".into(),
                to_id: "c1".into(),
                description: Some("owns".into()),
                technology: None,
            }],
            decisions: vec![Decision {
                id: "1".into(),
                workspace_id: "w".into(),
                scope_element_id: Some("s1".into()),
                title: "Use Postgres".into(),
                status: architect_c4_domain::DecisionStatus::Accepted,
                decided_at: "2026-07-16".into(),
                context: "Need durable store.".into(),
                decision: "Use Postgres.".into(),
                consequences: "Ops must run migrations.".into(),
                policy: None,
                related_flows: vec![],
                refs: vec![],
                reason: None,
                superseded_by_id: None,
                path: "docs/adr/0001-use-postgres.toml".into(),
                git_commit_id: Some("deadbeef".into()),
            }],
        };
        assert!(!validate_model(&snap)
            .iter()
            .any(|x| x.code == "system.missing_decisions"));
    }

    #[test]
    fn dangling_relationship_is_error() {
        let snap = ModelSnapshot {
            elements: vec![el("a", ElementKind::Person, None, "User")],
            relationships: vec![Relationship {
                id: "r".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "missing".into(),
                description: Some("uses".into()),
                technology: None,
            }],
            decisions: vec![],
        };
        assert!(validate_model(&snap)
            .iter()
            .any(|p| p.code == "relationship.dangling_endpoint"));
    }

    #[test]
    fn dangling_parent_disconnected_and_rel_desc() {
        let mut orphan = el("c", ElementKind::Component, Some("missing"), "Comp");
        orphan.description = Some(" ".into());
        let snap = ModelSnapshot {
            elements: vec![
                el("a", ElementKind::Person, None, "User"),
                el("b", ElementKind::SoftwareSystem, None, "Sys"),
                orphan,
            ],
            relationships: vec![Relationship {
                id: "r".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: None,
                technology: None,
            }],
            decisions: vec![],
        };
        let codes: Vec<_> = validate_model(&snap)
            .iter()
            .map(|p| p.code.clone())
            .collect();
        assert!(codes.iter().any(|c| c == "element.dangling_parent"));
        assert!(codes.iter().any(|c| c == "element.disconnected"));
        assert!(codes
            .iter()
            .any(|c| c == "relationship.missing_description"));
        assert!(codes.iter().any(|c| c == "element.missing_description"));
    }

    #[test]
    fn single_element_not_flagged_disconnected() {
        let snap = ModelSnapshot {
            elements: vec![el("a", ElementKind::Person, None, "Solo")],
            relationships: vec![],
            decisions: vec![],
        };
        assert!(!validate_model(&snap)
            .iter()
            .any(|p| p.code == "element.disconnected"));
    }
}
