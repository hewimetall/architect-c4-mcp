//! ADR use-cases: TOML on disk + optional git commit + in-memory index.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use architect_c4_domain::ports::{AdrPort, ElementExistsPort, GitPort};
use architect_c4_domain::{
    validate_doc_refs, AdrForbidRule, Decision, DecisionStatus, DomainError,
};
use parking_lot::Mutex;

type Key = (String, String);

pub struct AdrService {
    decisions: Mutex<HashMap<Key, Decision>>,
    git: Arc<dyn GitPort>,
    elements: Arc<dyn ElementExistsPort>,
    worktrees: Mutex<HashMap<String, PathBuf>>,
}

impl AdrService {
    pub fn new(git: Arc<dyn GitPort>, elements: Arc<dyn ElementExistsPort>) -> Self {
        Self {
            decisions: Mutex::new(HashMap::new()),
            git,
            elements,
            worktrees: Mutex::new(HashMap::new()),
        }
    }

    pub fn bind_worktree(&self, workspace_id: &str, path: PathBuf) {
        self.worktrees.lock().insert(workspace_id.to_string(), path);
    }

    fn worktree(&self, workspace_id: &str) -> Result<PathBuf, DomainError> {
        self.worktrees
            .lock()
            .get(workspace_id)
            .cloned()
            .ok_or_else(|| {
                DomainError::Validation(format!(
                    "workspace {workspace_id} has no bound worktree (checkout required for ADR)"
                ))
            })
    }

    fn validate_document(d: &Decision, agent_upsert: bool) -> Result<(), DomainError> {
        if d.id.is_empty() || d.workspace_id.is_empty() {
            return Err(DomainError::Validation(
                "decision id and workspace_id required".into(),
            ));
        }
        if !d
            .id
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
            || !d
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
        {
            return Err(DomainError::Validation(
                "decision id must match [A-Za-z0-9][A-Za-z0-9_.-]*".into(),
            ));
        }
        if d.title.is_empty() || d.title.len() > 200 {
            return Err(DomainError::Validation(
                "title required and max 200 chars".into(),
            ));
        }
        validate_doc_refs(&d.refs)?;
        if agent_upsert && !d.status.agent_writable() {
            return Err(DomainError::Validation(format!(
                "agent may only set status draft|proposed, got '{}'",
                d.status.as_str()
            )));
        }
        if !valid_date(&d.decided_at) {
            return Err(DomainError::Validation(
                "decided_at must be YYYY-MM-DD".into(),
            ));
        }
        for (name, s) in [
            ("context", d.context.as_str()),
            ("decision", d.decision.as_str()),
            ("consequences", d.consequences.as_str()),
        ] {
            if s.trim().is_empty() || s.len() > 20_000 {
                return Err(DomainError::Validation(format!(
                    "{name} required and max 20000 chars"
                )));
            }
        }
        if d.status == DecisionStatus::Superseded
            && d.superseded_by_id
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(DomainError::Validation(
                "superseded_by_id required when status=superseded".into(),
            ));
        }
        if d.status == DecisionStatus::Rejected
            && d.reason
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(DomainError::Validation(
                "reason required when status=rejected".into(),
            ));
        }
        if let Some(pol) = &d.policy {
            if pol.forbid.len() > 32 {
                return Err(DomainError::Validation("policy.forbid max 32 rules".into()));
            }
            for rule in &pol.forbid {
                validate_forbid_rule(rule)?;
            }
        }
        Ok(())
    }

    fn persist(
        &self,
        mut decision: Decision,
        commit: bool,
    ) -> Result<(Decision, Option<String>), DomainError> {
        if let Some(scope) = decision.scope_element_id.as_deref() {
            if !scope.is_empty()
                && !self
                    .elements
                    .element_exists(&decision.workspace_id, scope)?
            {
                return Err(DomainError::Validation(format!(
                    "ADR scope_element_id '{scope}' does not exist in workspace '{}'",
                    decision.workspace_id
                )));
            }
        }
        let wt = self.worktree(&decision.workspace_id)?;
        let rel = format!("docs/adr/{}.toml", decision.id);
        decision.path = rel.clone();
        let abs = wt.join(&rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| DomainError::Message(e.to_string()))?;
        }
        architect_c4_tomlio::write_adr_toml(&abs, &decision).map_err(DomainError::Message)?;

        let git_commit_id = if commit {
            Some(self.git.commit(
                &wt,
                &format!("adr: {} {}", decision.id, decision.title),
                std::slice::from_ref(&rel),
            )?)
        } else {
            None
        };
        decision.git_commit_id = git_commit_id.clone();
        let key = (decision.workspace_id.clone(), decision.id.clone());
        self.decisions.lock().insert(key, decision.clone());
        Ok((decision, git_commit_id))
    }

    /// Load ADR already on disk into the in-memory index (no rewrite, no commit).
    pub fn import_from_disk(
        &self,
        decision: Decision,
    ) -> Result<(Decision, Option<String>), DomainError> {
        Self::validate_document(&decision, false)?;
        let key = (decision.workspace_id.clone(), decision.id.clone());
        self.decisions.lock().insert(key, decision.clone());
        Ok((decision, None))
    }

    /// Drop in-memory ADR index for a workspace (sidecar rebind). Does not touch disk.
    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), DomainError> {
        self.decisions
            .lock()
            .retain(|(ws, _), _| ws != workspace_id);
        Ok(())
    }
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

fn validate_forbid_rule(rule: &AdrForbidRule) -> Result<(), DomainError> {
    if rule.code.is_empty()
        || !rule.code.chars().next().unwrap().is_ascii_lowercase()
        || !rule
            .code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(DomainError::Validation(
            "forbid.code must match [a-z][a-z0-9_]*".into(),
        ));
    }
    if rule.message.trim().is_empty() || rule.message.len() > 2000 {
        return Err(DomainError::Validation(
            "forbid.message required max 2000".into(),
        ));
    }
    let _ = (rule.from_kind, rule.to_kind, rule.severity);
    Ok(())
}

impl AdrPort for AdrService {
    fn upsert_decision(
        &self,
        decision: Decision,
        commit: bool,
    ) -> Result<(Decision, Option<String>), DomainError> {
        let agent_rules = match self.get_decision(&decision.workspace_id, &decision.id) {
            Ok(existing) if !existing.status.agent_writable() => {
                if decision.status != existing.status {
                    return Err(DomainError::Validation(format!(
                        "ADR '{}' is in process status '{}'; use set_adr_status to change status (cannot set '{}')",
                        decision.id,
                        existing.status.as_str(),
                        decision.status.as_str()
                    )));
                }
                false
            }
            _ => true,
        };
        Self::validate_document(&decision, agent_rules)?;
        self.persist(decision, commit)
    }

    fn set_decision_status(
        &self,
        workspace_id: &str,
        id: &str,
        status: DecisionStatus,
        reason: Option<&str>,
        superseded_by_id: Option<&str>,
        commit: bool,
    ) -> Result<(Decision, Option<String>), DomainError> {
        if status.agent_writable() {
            return Err(DomainError::Validation(
                "set_adr_status is for process statuses only (accepted|rejected|deprecated|superseded)"
                    .into(),
            ));
        }
        let mut d = self.get_decision(workspace_id, id)?;
        d.status = status;
        match status {
            DecisionStatus::Rejected => {
                let r = reason.map(str::trim).filter(|s| !s.is_empty());
                let Some(r) = r else {
                    return Err(DomainError::Validation(
                        "reason required when status=rejected".into(),
                    ));
                };
                if r.len() > 2000 {
                    return Err(DomainError::Validation("reason max 2000 chars".into()));
                }
                d.reason = Some(r.to_string());
            }
            DecisionStatus::Superseded => {
                let sid = superseded_by_id.map(str::trim).filter(|s| !s.is_empty());
                let Some(sid) = sid else {
                    return Err(DomainError::Validation(
                        "superseded_by_id required when status=superseded".into(),
                    ));
                };
                d.superseded_by_id = Some(sid.to_string());
            }
            _ => {
                d.reason = None;
            }
        }
        Self::validate_document(&d, false)?;
        self.persist(d, commit)
    }

    fn get_decision(&self, workspace_id: &str, id: &str) -> Result<Decision, DomainError> {
        self.decisions
            .lock()
            .get(&(workspace_id.to_string(), id.to_string()))
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("decision {id}")))
    }

    fn list_decisions(&self, workspace_id: &str) -> Result<Vec<Decision>, DomainError> {
        let mut out: Vec<_> = self
            .decisions
            .lock()
            .values()
            .filter(|d| d.workspace_id == workspace_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::{AdrPort, ElementExistsPort, GitPort};
    use architect_c4_domain::{AdrPolicy, ElementKind, PolicyMode, Severity};
    use architect_c4_git::GixGitAdapter;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct AllowElements {
        ids: HashSet<String>,
    }

    impl ElementExistsPort for AllowElements {
        fn element_exists(&self, _workspace_id: &str, id: &str) -> Result<bool, DomainError> {
            Ok(self.ids.is_empty() || self.ids.contains(id))
        }
    }

    fn allow(ids: &[&str]) -> Arc<dyn ElementExistsPort> {
        Arc::new(AllowElements {
            ids: ids.iter().map(|s| (*s).to_string()).collect(),
        })
    }

    fn sample(status: DecisionStatus) -> Decision {
        Decision {
            id: "0001-use-toml".into(),
            workspace_id: "w".into(),
            scope_element_id: Some("billing".into()),
            title: "Use TOML on disk".into(),
            status,
            decided_at: "2026-07-16".into(),
            context: "Need durable architecture docs.".into(),
            decision: "Store ADR as TOML under docs/adr/.".into(),
            consequences: "Git history is the audit trail.".into(),
            policy: None,
            related_flows: vec![],
            refs: vec![],
            reason: None,
            superseded_by_id: None,
            path: String::new(),
            git_commit_id: None,
        }
    }

    fn setup() -> (tempfile::TempDir, AdrService) {
        let dir = tempdir().unwrap();
        let git = Arc::new(GixGitAdapter::new());
        let bare = git.init_bare(&dir.path().join("p.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("wt"), "main")
            .unwrap();
        let adr = AdrService::new(git, allow(&["billing"]));
        adr.bind_worktree("w", wt);
        (dir, adr)
    }

    #[test]
    fn upsert_writes_toml_and_commits() {
        let (_dir, adr) = setup();
        let (d, cid) = adr
            .upsert_decision(sample(DecisionStatus::Proposed), true)
            .unwrap();
        assert!(cid.as_ref().unwrap().len() >= 7);
        assert!(d.path.ends_with(".toml"));
        let wt = adr.worktree("w").unwrap();
        let raw = fs::read_to_string(wt.join(&d.path)).unwrap();
        assert!(raw.contains("context = '''") || raw.contains("context ="));
        assert!(!raw.contains("content_md"));
        assert_eq!(d.git_commit_id, cid);
    }

    #[test]
    fn agent_cannot_upsert_accepted_status() {
        let (_dir, adr) = setup();
        let err = adr
            .upsert_decision(sample(DecisionStatus::Accepted), false)
            .unwrap_err();
        assert!(err.to_string().contains("draft|proposed"));
    }

    #[test]
    fn process_reject_requires_reason() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Proposed), false)
            .unwrap();
        let err = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Rejected,
                None,
                None,
                false,
            )
            .unwrap_err();
        assert!(err.to_string().contains("reason"));
        let (d, _) = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Rejected,
                Some("Not durable enough"),
                None,
                false,
            )
            .unwrap();
        assert_eq!(d.status, DecisionStatus::Rejected);
        assert_eq!(d.reason.as_deref(), Some("Not durable enough"));
    }

    #[test]
    fn process_accept_then_agent_cannot_edit() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Draft), false)
            .unwrap();
        adr.set_decision_status(
            "w",
            "0001-use-toml",
            DecisionStatus::Accepted,
            None,
            None,
            false,
        )
        .unwrap();
        let err = adr
            .upsert_decision(sample(DecisionStatus::Proposed), false)
            .unwrap_err();
        assert!(err.to_string().contains("process status"));
    }

    #[test]
    fn rejects_missing_scope_element() {
        let (_dir, adr) = setup();
        let mut d = sample(DecisionStatus::Proposed);
        d.scope_element_id = Some("sys".into());
        let err = adr.upsert_decision(d, true).unwrap_err();
        assert!(err.to_string().contains("scope_element_id"));
    }

    #[test]
    fn policy_forbid_validated() {
        let (_dir, adr) = setup();
        let mut d = sample(DecisionStatus::Draft);
        d.policy = Some(AdrPolicy {
            mode: PolicyMode::Enforce,
            forbid: vec![AdrForbidRule {
                from_kind: ElementKind::Person,
                to_kind: ElementKind::Code,
                code: "person_to_code".into(),
                severity: Severity::Error,
                message: "no".into(),
            }],
        });
        assert!(adr.upsert_decision(d, false).is_ok());
    }

    #[test]
    fn rejects_bad_date_and_empty_context() {
        let (_dir, adr) = setup();
        let mut d = sample(DecisionStatus::Draft);
        d.decided_at = "16-07-2026".into();
        assert!(adr.upsert_decision(d.clone(), false).is_err());
        d.decided_at = "2026-07-16".into();
        d.context = " ".into();
        assert!(adr.upsert_decision(d, false).is_err());
    }

    #[test]
    fn rejects_bad_id_and_forbid_code() {
        let (_dir, adr) = setup();
        let mut d = sample(DecisionStatus::Draft);
        d.id = "-bad".into();
        assert!(adr.upsert_decision(d.clone(), false).is_err());
        d.id = "ok1".into();
        d.policy = Some(AdrPolicy {
            mode: PolicyMode::Enforce,
            forbid: vec![AdrForbidRule {
                from_kind: ElementKind::Person,
                to_kind: ElementKind::Code,
                code: "BadCode".into(),
                severity: Severity::Error,
                message: "m".into(),
            }],
        });
        assert!(adr.upsert_decision(d, false).is_err());
    }

    #[test]
    fn superseded_requires_id() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Proposed), false)
            .unwrap();
        let err = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Superseded,
                None,
                None,
                false,
            )
            .unwrap_err();
        assert!(err.to_string().contains("superseded_by_id"));
        let (d, _) = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Superseded,
                None,
                Some("0002-next"),
                false,
            )
            .unwrap();
        assert_eq!(d.superseded_by_id.as_deref(), Some("0002-next"));
    }

    #[test]
    fn list_and_get_roundtrip() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Draft), false)
            .unwrap();
        let mut d2 = sample(DecisionStatus::Draft);
        d2.id = "0002-other".into();
        d2.scope_element_id = None;
        adr.upsert_decision(d2, false).unwrap();
        assert_eq!(adr.list_decisions("w").unwrap().len(), 2);
        assert_eq!(
            adr.get_decision("w", "0002-other").unwrap().title,
            "Use TOML on disk"
        );
    }

    #[test]
    fn set_status_rejects_agent_statuses() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Draft), false)
            .unwrap();
        let err = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Proposed,
                None,
                None,
                false,
            )
            .unwrap_err();
        assert!(err.to_string().contains("process statuses"));
    }

    #[test]
    fn clear_workspace_and_import_from_disk() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Draft), false)
            .unwrap();
        assert_eq!(adr.list_decisions("w").unwrap().len(), 1);
        adr.clear_workspace("w").unwrap();
        assert!(adr.list_decisions("w").unwrap().is_empty());

        let (d, cid) = adr
            .import_from_disk(sample(DecisionStatus::Accepted))
            .unwrap();
        assert_eq!(d.status, DecisionStatus::Accepted);
        assert!(cid.is_none());
        assert_eq!(
            adr.get_decision("w", "0001-use-toml").unwrap().title,
            "Use TOML on disk"
        );
    }

    #[test]
    fn validate_document_edge_cases() {
        let (_dir, adr) = setup();
        let mut d = sample(DecisionStatus::Draft);
        d.workspace_id.clear();
        assert!(adr.upsert_decision(d.clone(), false).is_err());

        d = sample(DecisionStatus::Draft);
        d.title = "t".repeat(201);
        assert!(adr.upsert_decision(d.clone(), false).is_err());

        d = sample(DecisionStatus::Draft);
        d.policy = Some(AdrPolicy {
            mode: PolicyMode::Enforce,
            forbid: (0..33)
                .map(|i| AdrForbidRule {
                    from_kind: ElementKind::Person,
                    to_kind: ElementKind::Code,
                    code: format!("rule_{i}"),
                    severity: Severity::Error,
                    message: "m".into(),
                })
                .collect(),
        });
        assert!(adr.upsert_decision(d.clone(), false).is_err());

        d = sample(DecisionStatus::Draft);
        d.policy = Some(AdrPolicy {
            mode: PolicyMode::Enforce,
            forbid: vec![AdrForbidRule {
                from_kind: ElementKind::Person,
                to_kind: ElementKind::Code,
                code: "ok_rule".into(),
                severity: Severity::Error,
                message: " ".into(),
            }],
        });
        assert!(adr.upsert_decision(d, false).is_err());

        adr.upsert_decision(sample(DecisionStatus::Proposed), false)
            .unwrap();
        let err = adr
            .set_decision_status(
                "w",
                "0001-use-toml",
                DecisionStatus::Rejected,
                Some(&"x".repeat(2001)),
                None,
                false,
            )
            .unwrap_err();
        assert!(err.to_string().contains("2000"));

        assert!(adr.worktree("missing-ws").is_err());
    }

    #[test]
    fn accepted_refresh_keeps_status_without_agent_gate() {
        let (_dir, adr) = setup();
        adr.upsert_decision(sample(DecisionStatus::Proposed), false)
            .unwrap();
        adr.set_decision_status(
            "w",
            "0001-use-toml",
            DecisionStatus::Accepted,
            None,
            None,
            false,
        )
        .unwrap();
        let mut d = sample(DecisionStatus::Accepted);
        d.consequences = "Updated consequences.".into();
        let (out, _) = adr.upsert_decision(d, false).unwrap();
        assert_eq!(out.status, DecisionStatus::Accepted);
        assert_eq!(out.consequences, "Updated consequences.");
    }
}
