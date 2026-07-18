//! C4 graph policy: baseline matrix + accepted ADR forbids.
//!
//! Atom-centric canon **V1 default** (opt out with `ARCHITECT_C4_ATOM_EDGES=0`):
//! - code ↔ code, code ↔ external, person ↔ software_system|external
//! - software_system ↔ software_system|external (context)
//!
//! Shells (container/component) are grouping-only; views project atom edges upward (V3).

use architect_c4_domain::{
    C4Layer, Decision, Element, ElementKind, PolicyMode, Problem, Relationship, Severity,
};

/// When true (default), container/component may not be relationship endpoints.
/// Set `ARCHITECT_C4_ATOM_EDGES=0|false|off` for legacy dual-mode.
pub fn atom_edges_strict() -> bool {
    match std::env::var("ARCHITECT_C4_ATOM_EDGES") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "0" | "false" | "no" | "off")
        }
        Err(_) => true,
    }
}

fn canon_rel_allowed(from: ElementKind, to: ElementKind) -> bool {
    use ElementKind::*;
    matches!(
        (from, to),
        (Code, Code)
            | (Code, External)
            | (External, Code)
            | (External, External)
            | (Person, SoftwareSystem)
            | (SoftwareSystem, Person)
            | (Person, External)
            | (External, Person)
            | (SoftwareSystem, SoftwareSystem)
            | (SoftwareSystem, External)
            | (External, SoftwareSystem)
    )
}

/// Structural allow-list (includes legacy shell links for existing models).
pub fn baseline_rel_allowed(from: ElementKind, to: ElementKind) -> bool {
    use ElementKind::*;
    if canon_rel_allowed(from, to) {
        return true;
    }
    // Legacy shells among themselves (not to/from code).
    match (from, to) {
        (Code, _) | (_, Code) => false,
        (
            Person | SoftwareSystem | Container | Component | External,
            Person | SoftwareSystem | Container | Component | External,
        ) => true,
    }
}

/// Write allow-list: V1 atom canon by default (opt out via env).
pub fn write_rel_allowed(from: ElementKind, to: ElementKind) -> bool {
    if atom_edges_strict() {
        canon_rel_allowed(from, to)
    } else {
        baseline_rel_allowed(from, to)
    }
}

pub fn baseline_parent_allowed(child: ElementKind, parent: Option<ElementKind>) -> bool {
    match (child, parent) {
        (ElementKind::Person, None) => true,
        (ElementKind::Person, Some(_)) => false,
        (ElementKind::External, None) => true,
        (ElementKind::External, Some(ElementKind::SoftwareSystem)) => true,
        (ElementKind::External, Some(_)) => false,
        (ElementKind::SoftwareSystem, None) => true,
        (ElementKind::SoftwareSystem, Some(_)) => false,
        (ElementKind::Container, Some(ElementKind::SoftwareSystem)) => true,
        (ElementKind::Component, Some(ElementKind::Container)) => true,
        (ElementKind::Code, Some(ElementKind::Component)) => true,
        (ElementKind::Container | ElementKind::Component | ElementKind::Code, None) => false,
        _ => false,
    }
}

fn problem(
    severity: Severity,
    layer: C4Layer,
    code: &str,
    element_id: Option<String>,
    message: String,
    adr_id: Option<String>,
) -> Problem {
    Problem {
        severity,
        layer,
        code: code.into(),
        element_id,
        message,
        adr_id,
    }
}

/// Check one relationship for **writes** (V1 strict by default).
pub fn check_relationship(
    from: &Element,
    to: &Element,
    rel_id: &str,
    adrs: &[Decision],
) -> Vec<Problem> {
    check_relationship_mode(from, to, rel_id, adrs, true)
}

/// Scan/validate mode: never hard-fail legacy shell edges (warning only).
pub fn check_relationship_scan(
    from: &Element,
    to: &Element,
    rel_id: &str,
    adrs: &[Decision],
) -> Vec<Problem> {
    check_relationship_mode(from, to, rel_id, adrs, false)
}

fn check_relationship_mode(
    from: &Element,
    to: &Element,
    rel_id: &str,
    adrs: &[Decision],
    for_write: bool,
) -> Vec<Problem> {
    let mut out = Vec::new();
    let allowed = if for_write {
        write_rel_allowed(from.kind, to.kind)
    } else {
        baseline_rel_allowed(from.kind, to.kind)
    };
    if !allowed {
        out.push(problem(
            Severity::Error,
            from.kind.layer(),
            "policy.baseline.illegal_kinds",
            Some(rel_id.into()),
            format!(
                "C4 baseline: {} → {} is not allowed{}",
                from.kind.as_str(),
                to.kind.as_str(),
                if for_write && atom_edges_strict() {
                    " (V1 atom canon: use code/external/person/system; ARCHITECT_C4_ATOM_EDGES=0 for legacy)"
                } else {
                    ""
                }
            ),
            None,
        ));
    } else if !canon_rel_allowed(from.kind, to.kind) {
        // Legacy shell edge: warn on scan and on legacy-mode writes.
        out.push(problem(
            Severity::Warning,
            from.kind.layer(),
            "policy.baseline.non_atom_endpoint",
            Some(rel_id.into()),
            format!(
                "Non-atom relationship {} → {} (prefer code/external); writes blocked unless ARCHITECT_C4_ATOM_EDGES=0",
                from.kind.as_str(),
                to.kind.as_str()
            ),
            None,
        ));
    }
    for d in adrs.iter().filter(|d| d.status.enforces_policy()) {
        let Some(pol) = d.policy.as_ref() else {
            continue;
        };
        for rule in &pol.forbid {
            if rule.from_kind == from.kind && rule.to_kind == to.kind {
                let sev = rule.severity;
                out.push(problem(
                    sev,
                    from.kind.layer(),
                    &format!("policy.{}", rule.code),
                    Some(rel_id.into()),
                    rule.message.clone(),
                    Some(d.id.clone()),
                ));
            }
        }
    }
    out
}

pub fn check_parent(child: &Element, parent: Option<&Element>) -> Vec<Problem> {
    let pk = parent.map(|p| p.kind);
    if baseline_parent_allowed(child.kind, pk) {
        return vec![];
    }
    vec![problem(
        Severity::Error,
        child.kind.layer(),
        "policy.baseline.illegal_parent",
        Some(child.id.clone()),
        match (child.kind, pk) {
            (ElementKind::Code, None) => {
                "C4 baseline: code elements must have a component parent".into()
            }
            (ElementKind::External, Some(p)) => format!(
                "C4 baseline: external may only sit at context or under software_system (not under {})",
                p.as_str()
            ),
            (k, None) => format!(
                "C4 baseline: {} must not have empty/invalid parent",
                k.as_str()
            ),
            (k, Some(p)) => format!(
                "C4 baseline: {} cannot be parent of {}",
                p.as_str(),
                k.as_str()
            ),
        },
        None,
    )]
}

/// Scan full model for policy violations (baseline + ADR forbids).
pub fn scan_model(
    elements: &[Element],
    relationships: &[Relationship],
    adrs: &[Decision],
) -> Vec<Problem> {
    let by_id: std::collections::HashMap<&str, &Element> =
        elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut out = Vec::new();
    for e in elements {
        let parent = e.parent_id.as_deref().and_then(|p| by_id.get(p).copied());
        out.extend(check_parent(e, parent));
    }
    for r in relationships {
        let Some(from) = by_id.get(r.from_id.as_str()) else {
            continue;
        };
        let Some(to) = by_id.get(r.to_id.as_str()) else {
            continue;
        };
        out.extend(check_relationship_scan(from, to, &r.id, adrs));
    }
    out
}

/// True if any problem is Error under enforce semantics (blocks write).
pub fn blocks_write(problems: &[Problem], adrs: &[Decision]) -> bool {
    problems.iter().any(|p| {
        if p.severity != Severity::Error {
            return false;
        }
        // baseline errors always block
        if p.adr_id.is_none() {
            return true;
        }
        // ADR-sourced: block if that ADR policy mode is enforce
        adrs.iter()
            .find(|d| Some(d.id.as_str()) == p.adr_id.as_deref())
            .and_then(|d| d.policy.as_ref())
            .map(|pol| matches!(pol.mode, PolicyMode::Enforce))
            .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::{AdrForbidRule, AdrPolicy, DecisionStatus};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn el(id: &str, kind: ElementKind, parent: Option<&str>) -> Element {
        Element {
            id: id.into(),
            workspace_id: "w".into(),
            kind,
            parent_id: parent.map(str::to_string),
            name: id.into(),
            description: Some("d".into()),
            technology: Some("t".into()),
            url: None,
            members: vec![],
        }
    }
    #[test]
    fn baseline_blocks_person_to_code() {
        assert!(!baseline_rel_allowed(
            ElementKind::Person,
            ElementKind::Code
        ));
        assert!(baseline_rel_allowed(ElementKind::Code, ElementKind::Code));
        assert!(baseline_rel_allowed(
            ElementKind::Person,
            ElementKind::SoftwareSystem
        ));
        assert!(baseline_rel_allowed(
            ElementKind::Code,
            ElementKind::External
        ));
        assert!(baseline_rel_allowed(
            ElementKind::Person,
            ElementKind::External
        ));
    }

    #[test]
    fn parent_code_requires_component() {
        assert!(!baseline_parent_allowed(ElementKind::Code, None));
        assert!(baseline_parent_allowed(
            ElementKind::Code,
            Some(ElementKind::Component)
        ));
        assert!(!baseline_parent_allowed(
            ElementKind::Code,
            Some(ElementKind::Container)
        ));
        assert!(baseline_parent_allowed(ElementKind::External, None));
        assert!(baseline_parent_allowed(
            ElementKind::External,
            Some(ElementKind::SoftwareSystem)
        ));
    }

    #[test]
    fn accepted_adr_forbid_emits_problem_with_adr_id() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARCHITECT_C4_ATOM_EDGES", "0");
        let from = el("u", ElementKind::Container, Some("s"));
        let to = el("c", ElementKind::Container, Some("s"));
        let adr = Decision {
            id: "0007".into(),
            workspace_id: "w".into(),
            scope_element_id: None,
            title: "No cross container".into(),
            status: DecisionStatus::Accepted,
            decided_at: "2026-07-17".into(),
            context: "ctx".into(),
            decision: "dec".into(),
            consequences: "con".into(),
            related_flows: vec![],
            refs: vec![],
            policy: Some(AdrPolicy {
                mode: PolicyMode::Enforce,
                forbid: vec![AdrForbidRule {
                    from_kind: ElementKind::Container,
                    to_kind: ElementKind::Container,
                    code: "no_container_links".into(),
                    severity: Severity::Error,
                    message: "no container to container".into(),
                }],
            }),
            reason: None,
            superseded_by_id: None,
            path: String::new(),
            git_commit_id: None,
        };
        let probs = check_relationship(&from, &to, "r1", &[adr]);
        std::env::remove_var("ARCHITECT_C4_ATOM_EDGES");
        let forbid = probs
            .iter()
            .find(|p| p.code == "policy.no_container_links")
            .expect("adr forbid");
        assert_eq!(forbid.adr_id.as_deref(), Some("0007"));
    }

    #[test]
    fn scan_and_blocks_write() {
        let els = vec![
            el("s", ElementKind::SoftwareSystem, None),
            el("u", ElementKind::Person, None),
            el("c", ElementKind::Code, None), // illegal parent
        ];
        let rels = vec![Relationship {
            id: "r".into(),
            workspace_id: "w".into(),
            from_id: "u".into(),
            to_id: "c".into(),
            description: Some("x".into()),
            technology: None,
        }];
        let probs = scan_model(&els, &rels, &[]);
        assert!(probs.iter().any(|p| p.code.contains("illegal")));
        assert!(blocks_write(&probs, &[]));
    }

    #[test]
    fn v1_default_rejects_shell_endpoints() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ARCHITECT_C4_ATOM_EDGES");
        let from = el("a", ElementKind::Container, Some("s"));
        let to = el("b", ElementKind::Container, Some("s"));
        let probs = check_relationship(&from, &to, "r1", &[]);
        assert!(
            probs
                .iter()
                .any(|p| p.code == "policy.baseline.illegal_kinds"),
            "{probs:?}"
        );
        assert!(blocks_write(&probs, &[]));
    }

    #[test]
    fn legacy_mode_warns_shell_endpoints() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARCHITECT_C4_ATOM_EDGES", "0");
        let from = el("a", ElementKind::Container, Some("s"));
        let to = el("b", ElementKind::Container, Some("s"));
        let probs = check_relationship(&from, &to, "r1", &[]);
        std::env::remove_var("ARCHITECT_C4_ATOM_EDGES");
        assert!(
            probs
                .iter()
                .any(|p| p.code == "policy.baseline.non_atom_endpoint"),
            "{probs:?}"
        );
        assert!(!blocks_write(&probs, &[]));
    }

    #[test]
    fn audit_mode_adr_error_does_not_block_write() {
        // Canon-allowed pair + ADR forbid in audit mode must not block writes.
        let from = el("a", ElementKind::Code, Some("c"));
        let to = el("b", ElementKind::External, None);
        let adr = Decision {
            id: "a1".into(),
            workspace_id: "w".into(),
            scope_element_id: None,
            title: "t".into(),
            status: DecisionStatus::Accepted,
            decided_at: "2026-07-17".into(),
            context: "c".into(),
            decision: "d".into(),
            consequences: "x".into(),
            related_flows: vec![],
            refs: vec![],
            policy: Some(AdrPolicy {
                mode: PolicyMode::Audit,
                forbid: vec![AdrForbidRule {
                    from_kind: ElementKind::Code,
                    to_kind: ElementKind::External,
                    code: "x".into(),
                    severity: Severity::Error,
                    message: "no".into(),
                }],
            }),
            reason: None,
            superseded_by_id: None,
            path: String::new(),
            git_commit_id: None,
        };
        let probs = check_relationship(&from, &to, "r", &[adr.clone()]);
        assert!(probs.iter().any(|p| p.code == "policy.x"));
        assert!(!blocks_write(&probs, &[adr]));
    }
}
