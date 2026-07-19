//! Flow use-cases: TOML on disk + optional git commit + in-memory index.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use architect_c4_domain::ports::{ElementExistsPort, FlowPort, GitPort};
use architect_c4_domain::{DomainError, Flow};
use parking_lot::Mutex;

type Key = (String, String);

pub struct FlowService {
    flows: Mutex<HashMap<Key, Flow>>,
    git: Arc<dyn GitPort>,
    elements: Arc<dyn ElementExistsPort>,
    worktrees: Mutex<HashMap<String, PathBuf>>,
}

impl FlowService {
    pub fn new(git: Arc<dyn GitPort>, elements: Arc<dyn ElementExistsPort>) -> Self {
        Self {
            flows: Mutex::new(HashMap::new()),
            git,
            elements,
            worktrees: Mutex::new(HashMap::new()),
        }
    }

    pub fn bind_worktree(&self, workspace_id: &str, path: PathBuf) {
        self.worktrees.lock().insert(workspace_id.to_string(), path);
    }

    /// Drop in-memory flow index for a workspace (sidecar rebind). Does not touch disk.
    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), DomainError> {
        self.flows.lock().retain(|(ws, _), _| ws != workspace_id);
        Ok(())
    }

    /// Load flow already on disk into the in-memory index (no rewrite, no commit).
    pub fn import_from_disk(&self, flow: Flow) -> Result<Flow, DomainError> {
        self.validate_refs(&flow)?;
        let key = (flow.workspace_id.clone(), flow.id.clone());
        self.flows.lock().insert(key, flow.clone());
        Ok(flow)
    }

    fn worktree(&self, workspace_id: &str) -> Result<PathBuf, DomainError> {
        self.worktrees
            .lock()
            .get(workspace_id)
            .cloned()
            .ok_or_else(|| {
                DomainError::Validation(format!(
                    "workspace {workspace_id} has no bound worktree (checkout required for Flow)"
                ))
            })
    }

    fn validate_refs(&self, flow: &Flow) -> Result<(), DomainError> {
        flow.validate_shape()?;
        if let Some(scope) = flow.scope_element_id.as_deref() {
            if !scope.is_empty() && !self.elements.element_exists(&flow.workspace_id, scope)? {
                return Err(DomainError::Validation(format!(
                    "flow scope_element_id '{scope}' does not exist"
                )));
            }
        }
        for s in &flow.steps {
            for id in [&s.from_id, &s.to_id] {
                if !self.elements.element_exists(&flow.workspace_id, id)? {
                    return Err(DomainError::Validation(format!(
                        "flow step references missing element '{id}'"
                    )));
                }
            }
        }
        for a in &flow.anchors {
            if let Some(eid) = a.element_id.as_deref() {
                if !eid.is_empty() && !self.elements.element_exists(&flow.workspace_id, eid)? {
                    return Err(DomainError::Validation(format!(
                        "flow anchor element_id '{eid}' does not exist"
                    )));
                }
            }
        }
        Ok(())
    }

    fn persist(
        &self,
        mut flow: Flow,
        commit: bool,
    ) -> Result<(Flow, Option<String>), DomainError> {
        self.validate_refs(&flow)?;
        let wt = self.worktree(&flow.workspace_id)?;
        let rel = format!("docs/flows/{}.toml", flow.id);
        flow.path = rel.clone();
        let abs = wt.join(&rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| DomainError::Message(e.to_string()))?;
        }
        architect_c4_tomlio::write_flow_toml(&abs, &flow).map_err(DomainError::Message)?;

        let git_commit_id = if commit {
            Some(self.git.commit(
                &wt,
                &format!("flow: {} {}", flow.id, flow.title),
                std::slice::from_ref(&rel),
            )?)
        } else {
            None
        };
        flow.git_commit_id = git_commit_id.clone();
        let key = (flow.workspace_id.clone(), flow.id.clone());
        self.flows.lock().insert(key, flow.clone());
        Ok((flow, git_commit_id))
    }
}

impl FlowPort for FlowService {
    fn upsert_flow(&self, flow: Flow, commit: bool) -> Result<(Flow, Option<String>), DomainError> {
        self.persist(flow, commit)
    }

    fn get_flow(&self, workspace_id: &str, id: &str) -> Result<Flow, DomainError> {
        self.flows
            .lock()
            .get(&(workspace_id.to_string(), id.to_string()))
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("flow {id}")))
    }

    fn list_flows(&self, workspace_id: &str) -> Result<Vec<Flow>, DomainError> {
        let mut out: Vec<_> = self
            .flows
            .lock()
            .values()
            .filter(|f| f.workspace_id == workspace_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    fn delete_flow(&self, workspace_id: &str, id: &str, commit: bool) -> Result<(), DomainError> {
        let existing = self.get_flow(workspace_id, id)?;
        let wt = self.worktree(workspace_id)?;
        let rel = existing.path.clone();
        let abs = wt.join(&rel);
        if abs.is_file() {
            fs::remove_file(&abs).map_err(|e| DomainError::Message(e.to_string()))?;
        }
        if commit && !rel.is_empty() {
            let _ = self.git.commit(
                &wt,
                &format!("flow: delete {id}"),
                std::slice::from_ref(&rel),
            );
        }
        self.flows
            .lock()
            .remove(&(workspace_id.to_string(), id.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::{ElementExistsPort, FlowPort, GitPort};
    use architect_c4_domain::{FlowKind, FlowStep};
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

    fn sample() -> Flow {
        Flow {
            id: "usage-write".into(),
            workspace_id: "w".into(),
            title: "Write usage".into(),
            kind: FlowKind::C4Dynamic,
            usage_key: Some("rgw-usage".into()),
            scope_element_id: Some("rgw".into()),
            related_adrs: vec!["0002".into()],
            epoch: None,
            steps: vec![FlowStep {
                n: 1,
                from_id: "client".into(),
                to_id: "rgw".into(),
                label: Some("PUT".into()),
            }],
            body: None,
            anchors: vec![],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        }
    }

    fn setup() -> (tempfile::TempDir, FlowService) {
        let dir = tempdir().unwrap();
        let git = Arc::new(GixGitAdapter::new());
        let bare = git.init_bare(&dir.path().join("p.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("wt"), "main")
            .unwrap();
        let flow = FlowService::new(git, allow(&["rgw", "client"]));
        flow.bind_worktree("w", wt);
        (dir, flow)
    }

    #[test]
    fn upsert_writes_toml_and_lists() {
        let (_dir, svc) = setup();
        let (f, cid) = svc.upsert_flow(sample(), true).unwrap();
        assert!(cid.is_some());
        assert!(f.path.ends_with(".toml"));
        let wt = svc.worktree("w").unwrap();
        let raw = fs::read_to_string(wt.join(&f.path)).unwrap();
        assert!(raw.contains("c4_dynamic"));
        assert_eq!(svc.list_flows("w").unwrap().len(), 1);
        assert_eq!(
            svc.get_flow("w", "usage-write").unwrap().title,
            "Write usage"
        );
    }

    #[test]
    fn rejects_missing_step_element() {
        let (_dir, svc) = setup();
        let mut f = sample();
        f.steps[0].to_id = "missing".into();
        assert!(svc.upsert_flow(f, false).is_err());
    }

    #[test]
    fn sequence_body_ok() {
        let (_dir, svc) = setup();
        let f = Flow {
            id: "seq1".into(),
            workspace_id: "w".into(),
            title: "Seq".into(),
            kind: FlowKind::Sequence,
            usage_key: None,
            scope_element_id: None,
            related_adrs: vec![],
            epoch: None,
            steps: vec![],
            body: Some("sequenceDiagram\n  client->>rgw: hi\n".into()),
            anchors: vec![],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        };
        assert!(svc.upsert_flow(f, false).is_ok());
    }

    #[test]
    fn delete_removes_row() {
        let (_dir, svc) = setup();
        svc.upsert_flow(sample(), false).unwrap();
        svc.delete_flow("w", "usage-write", false).unwrap();
        assert!(svc.get_flow("w", "usage-write").is_err());
    }

    #[test]
    fn state_body_and_scope_and_update() {
        let (_dir, svc) = setup();
        let f = Flow {
            id: "st1".into(),
            workspace_id: "w".into(),
            title: "Window lifecycle".into(),
            kind: FlowKind::State,
            usage_key: Some("rgw-usage".into()),
            scope_element_id: Some("rgw".into()),
            related_adrs: vec![],
            epoch: Some(architect_c4_domain::FlowEpoch {
                kind: "phase".into(),
                phase: Some("enabled".into()),
                from: None,
                to: None,
                note: Some("usage log on".into()),
            }),
            steps: vec![],
            body: Some("stateDiagram-v2\n  [*] --> Open\n  Open --> Trimmed\n".into()),
            anchors: vec![architect_c4_domain::FlowAnchor {
                alias: "RGW".into(),
                element_id: Some("rgw".into()),
                adr_id: None,
            }],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        };
        assert!(svc.upsert_flow(f, false).is_ok());
        let mut f2 = sample();
        f2.title = "Write usage v2".into();
        assert!(svc.upsert_flow(f2, true).is_ok());
        let mut bad_scope = sample();
        bad_scope.id = "x2".into();
        bad_scope.scope_element_id = Some("nope".into());
        assert!(svc.upsert_flow(bad_scope, false).is_err());
    }

    #[test]
    fn requires_worktree() {
        let git: Arc<dyn GitPort> = Arc::new(GixGitAdapter::new());
        let svc = FlowService::new(git, allow(&["rgw", "client"]));
        assert!(svc.upsert_flow(sample(), false).is_err());
    }
}
