//! Flow use-cases: rigid JSON in worktree + git commit fixation + SQL index.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use architect_c4_domain::ports::{ElementExistsPort, FlowPort, GitPort, RevisionPort};
use architect_c4_domain::{ChangeKind, DomainError, EntityType, Flow};
use architect_c4_revision::SqliteRevisionStore;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

pub struct FlowService {
    conn: Arc<Mutex<Connection>>,
    revisions: Arc<SqliteRevisionStore>,
    git: Arc<dyn GitPort>,
    elements: Arc<dyn ElementExistsPort>,
    worktrees: Mutex<std::collections::HashMap<String, PathBuf>>,
}

impl FlowService {
    pub fn open(
        path: &Path,
        revisions: Arc<SqliteRevisionStore>,
        git: Arc<dyn GitPort>,
        elements: Arc<dyn ElementExistsPort>,
    ) -> Result<Self, DomainError> {
        let conn = Connection::open(path).map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
            revisions,
            git,
            elements,
            worktrees: Mutex::new(std::collections::HashMap::new()),
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn open_in_memory(
        revisions: Arc<SqliteRevisionStore>,
        git: Arc<dyn GitPort>,
        elements: Arc<dyn ElementExistsPort>,
    ) -> Result<Self, DomainError> {
        let conn = Connection::open_in_memory().map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
            revisions,
            git,
            elements,
            worktrees: Mutex::new(std::collections::HashMap::new()),
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn bind_worktree(&self, workspace_id: &str, path: PathBuf) {
        self.worktrees.lock().insert(workspace_id.to_string(), path);
    }

    pub fn migrate(&self) -> Result<(), DomainError> {
        self.conn
            .lock()
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS flows (
                  id TEXT NOT NULL,
                  workspace_id TEXT NOT NULL,
                  title TEXT NOT NULL,
                  kind TEXT NOT NULL,
                  body_json TEXT NOT NULL,
                  path TEXT NOT NULL,
                  git_commit_id TEXT,
                  PRIMARY KEY (workspace_id, id)
                );
                "#,
            )
            .map_err(map_sql)?;
        Ok(())
    }

    /// Drop in-memory flow index for a workspace (sidecar rebind). Does not touch disk.
    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), DomainError> {
        self.conn
            .lock()
            .execute(
                "DELETE FROM flows WHERE workspace_id=?1",
                params![workspace_id],
            )
            .map_err(map_sql)?;
        Ok(())
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
        change: ChangeKind,
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

        let exists = self
            .conn
            .lock()
            .query_row(
                "SELECT 1 FROM flows WHERE workspace_id=?1 AND id=?2",
                params![flow.workspace_id, flow.id],
                |_| Ok(true),
            )
            .optional()
            .map_err(map_sql)?
            .unwrap_or(false);

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
        let body = serde_json::to_string(&flow).map_err(|e| DomainError::Message(e.to_string()))?;

        {
            let conn = self.conn.lock();
            conn.execute(
                r#"INSERT INTO flows
                   (id, workspace_id, title, kind, body_json, path, git_commit_id)
                   VALUES (?1,?2,?3,?4,?5,?6,?7)
                   ON CONFLICT(workspace_id, id) DO UPDATE SET
                     title=excluded.title, kind=excluded.kind,
                     body_json=excluded.body_json, path=excluded.path,
                     git_commit_id=excluded.git_commit_id"#,
                params![
                    flow.id,
                    flow.workspace_id,
                    flow.title,
                    flow.kind.as_str(),
                    body,
                    flow.path,
                    flow.git_commit_id,
                ],
            )
            .map_err(map_sql)?;
        }

        let kind = if exists { ChangeKind::Update } else { change };
        self.revisions.append(
            &flow.workspace_id,
            EntityType::Flow,
            &flow.id,
            kind,
            &body,
            git_commit_id.as_deref(),
        )?;
        Ok((flow, git_commit_id))
    }
}

impl FlowPort for FlowService {
    fn upsert_flow(&self, flow: Flow, commit: bool) -> Result<(Flow, Option<String>), DomainError> {
        self.persist(flow, commit, ChangeKind::Create)
    }

    fn get_flow(&self, workspace_id: &str, id: &str) -> Result<Flow, DomainError> {
        let body: String = self
            .conn
            .lock()
            .query_row(
                "SELECT body_json FROM flows WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_sql)?
            .ok_or_else(|| DomainError::NotFound(format!("flow {id}")))?;
        serde_json::from_str(&body).map_err(|e| DomainError::Message(e.to_string()))
    }

    fn list_flows(&self, workspace_id: &str) -> Result<Vec<Flow>, DomainError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT body_json FROM flows WHERE workspace_id=?1 ORDER BY id")
            .map_err(map_sql)?;
        let rows = stmt
            .query_map(params![workspace_id], |r| r.get::<_, String>(0))
            .map_err(map_sql)?;
        let mut out = Vec::new();
        for r in rows {
            let body = r.map_err(map_sql)?;
            out.push(serde_json::from_str(&body).map_err(|e| DomainError::Message(e.to_string()))?);
        }
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
        self.conn
            .lock()
            .execute(
                "DELETE FROM flows WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, id],
            )
            .map_err(map_sql)?;
        let snap =
            serde_json::to_string(&existing).map_err(|e| DomainError::Message(e.to_string()))?;
        self.revisions.append(
            workspace_id,
            EntityType::Flow,
            id,
            ChangeKind::Delete,
            &snap,
            None,
        )?;
        Ok(())
    }
}

fn map_sql(e: rusqlite::Error) -> DomainError {
    DomainError::Message(e.to_string())
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
        let rev = Arc::new(SqliteRevisionStore::open_in_memory().unwrap());
        let flow = FlowService::open_in_memory(rev, git, allow(&["rgw", "client"])).unwrap();
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
        let rev = Arc::new(SqliteRevisionStore::open_in_memory().unwrap());
        let git: Arc<dyn GitPort> = Arc::new(GixGitAdapter::new());
        let svc = FlowService::open_in_memory(rev, git, allow(&["rgw", "client"])).unwrap();
        assert!(svc.upsert_flow(sample(), false).is_err());
    }
}
