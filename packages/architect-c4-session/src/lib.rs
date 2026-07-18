//! Session/workspace persistence (hex adapter over rusqlite).

use std::path::Path;
use std::sync::Arc;

use architect_c4_domain::ports::SessionPort;
use architect_c4_domain::{DomainError, Session, Workspace};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub struct SqliteSessionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSessionStore {
    pub fn open(path: &Path) -> Result<Self, DomainError> {
        let conn = Connection::open(path).map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn open_in_memory() -> Result<Self, DomainError> {
        let conn = Connection::open_in_memory().map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn migrate(&self) -> Result<(), DomainError> {
        self.conn
            .lock()
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS sessions (
                  id TEXT PRIMARY KEY,
                  meta TEXT NOT NULL DEFAULT '',
                  active_workspace_id TEXT,
                  status TEXT NOT NULL DEFAULT 'open',
                  created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS workspaces (
                  id TEXT PRIMARY KEY,
                  project_id TEXT NOT NULL,
                  ref_name TEXT NOT NULL,
                  path TEXT NOT NULL,
                  status TEXT NOT NULL DEFAULT 'active',
                  created_at INTEGER NOT NULL
                );
                "#,
            )
            .map_err(map_sql)?;
        Ok(())
    }

    fn now() -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

impl SessionPort for SqliteSessionStore {
    fn create_session(&self, meta: &str) -> Result<Session, DomainError> {
        let id = Uuid::new_v4().to_string();
        let created_at = Self::now();
        self.conn
            .lock()
            .execute(
                "INSERT INTO sessions (id, meta, created_at) VALUES (?1,?2,?3)",
                params![id, meta, created_at],
            )
            .map_err(map_sql)?;
        Ok(Session {
            id,
            meta: meta.to_string(),
            active_workspace_id: None,
            created_at,
        })
    }

    fn get_session(&self, id: &str) -> Result<Session, DomainError> {
        self.conn
            .lock()
            .query_row(
                "SELECT id, meta, active_workspace_id, created_at FROM sessions WHERE id=?1 AND status='open'",
                params![id],
                |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        meta: r.get(1)?,
                        active_workspace_id: r.get(2)?,
                        created_at: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(map_sql)?
            .ok_or_else(|| DomainError::NotFound(format!("session {id}")))
    }

    fn list_sessions(&self) -> Result<Vec<Session>, DomainError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, meta, active_workspace_id, created_at FROM sessions WHERE status='open' ORDER BY created_at",
            )
            .map_err(map_sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    meta: r.get(1)?,
                    active_workspace_id: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(map_sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sql)?);
        }
        Ok(out)
    }

    fn close_session(&self, id: &str) -> Result<(), DomainError> {
        let n = self
            .conn
            .lock()
            .execute(
                "UPDATE sessions SET status='closed' WHERE id=?1 AND status='open'",
                params![id],
            )
            .map_err(map_sql)?;
        if n == 0 {
            return Err(DomainError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    fn create_workspace(
        &self,
        id: &str,
        project_id: &str,
        ref_name: &str,
        path: &str,
    ) -> Result<Workspace, DomainError> {
        if id.is_empty() || project_id.is_empty() || path.is_empty() {
            return Err(DomainError::Validation(
                "id, project_id, path required".into(),
            ));
        }
        let created_at = Self::now();
        self.conn
            .lock()
            .execute(
                "INSERT INTO workspaces (id, project_id, ref_name, path, created_at) VALUES (?1,?2,?3,?4,?5)",
                params![id, project_id, ref_name, path, created_at],
            )
            .map_err(map_sql)?;
        Ok(Workspace {
            id: id.to_string(),
            project_id: project_id.to_string(),
            ref_name: ref_name.to_string(),
            path: path.to_string(),
            status: "active".into(),
            created_at,
        })
    }

    fn get_workspace(&self, id: &str) -> Result<Workspace, DomainError> {
        self.conn
            .lock()
            .query_row(
                "SELECT id, project_id, ref_name, path, status, created_at FROM workspaces WHERE id=?1",
                params![id],
                |r| {
                    Ok(Workspace {
                        id: r.get(0)?,
                        project_id: r.get(1)?,
                        ref_name: r.get(2)?,
                        path: r.get(3)?,
                        status: r.get(4)?,
                        created_at: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(map_sql)?
            .ok_or_else(|| DomainError::NotFound(format!("workspace {id}")))
    }

    fn list_workspaces(&self, project_id: Option<&str>) -> Result<Vec<Workspace>, DomainError> {
        let conn = self.conn.lock();
        let mut out = Vec::new();
        if let Some(pid) = project_id {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, ref_name, path, status, created_at FROM workspaces WHERE project_id=?1 AND status='active'",
                )
                .map_err(map_sql)?;
            for r in stmt
                .query_map(params![pid], workspace_row)
                .map_err(map_sql)?
            {
                out.push(r.map_err(map_sql)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, ref_name, path, status, created_at FROM workspaces WHERE status='active'",
                )
                .map_err(map_sql)?;
            for r in stmt.query_map([], workspace_row).map_err(map_sql)? {
                out.push(r.map_err(map_sql)?);
            }
        }
        Ok(out)
    }

    fn set_active_workspace(
        &self,
        session_id: &str,
        workspace_id: &str,
    ) -> Result<(), DomainError> {
        let conn = self.conn.lock();
        let ws_ok: bool = conn
            .query_row(
                "SELECT 1 FROM workspaces WHERE id=?1 AND status='active'",
                params![workspace_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(map_sql)?
            .unwrap_or(false);
        if !ws_ok {
            return Err(DomainError::NotFound(format!("workspace {workspace_id}")));
        }
        let n = conn
            .execute(
                "UPDATE sessions SET active_workspace_id=?1 WHERE id=?2 AND status='open'",
                params![workspace_id, session_id],
            )
            .map_err(map_sql)?;
        if n == 0 {
            return Err(DomainError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }
}

fn workspace_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: r.get(0)?,
        project_id: r.get(1)?,
        ref_name: r.get(2)?,
        path: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get(5)?,
    })
}

fn map_sql(e: rusqlite::Error) -> DomainError {
    DomainError::Message(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::SessionPort;

    #[test]
    fn session_lifecycle() {
        let s = SqliteSessionStore::open_in_memory().unwrap();
        let sess = s.create_session("meta").unwrap();
        assert_eq!(s.get_session(&sess.id).unwrap().meta, "meta");
        assert_eq!(s.list_sessions().unwrap().len(), 1);
        s.close_session(&sess.id).unwrap();
        assert!(s.get_session(&sess.id).is_err());
    }

    #[test]
    fn workspace_and_active_binding() {
        let s = SqliteSessionStore::open_in_memory().unwrap();
        let sess = s.create_session("").unwrap();
        let ws = s
            .create_workspace("ws1", "proj", "main", "/tmp/ws1")
            .unwrap();
        s.set_active_workspace(&sess.id, &ws.id).unwrap();
        assert_eq!(
            s.get_session(&sess.id)
                .unwrap()
                .active_workspace_id
                .as_deref(),
            Some("ws1")
        );
        assert_eq!(s.list_workspaces(Some("proj")).unwrap().len(), 1);
    }

    #[test]
    fn set_active_unknown_fails() {
        let s = SqliteSessionStore::open_in_memory().unwrap();
        let sess = s.create_session("").unwrap();
        assert!(s.set_active_workspace(&sess.id, "nope").is_err());
    }

    #[test]
    fn validation_and_not_found_paths() {
        let dir = tempfile::tempdir().unwrap();
        let s = SqliteSessionStore::open(&dir.path().join("s.db")).unwrap();
        s.migrate().unwrap();
        assert!(s
            .create_workspace("", "p", "main", "/x")
            .unwrap_err()
            .to_string()
            .contains("required"));
        assert!(s.get_workspace("missing").is_err());
        assert!(s.close_session("missing").is_err());
        let sess = s.create_session("m").unwrap();
        s.create_workspace("w1", "p", "main", "/tmp/w1").unwrap();
        s.create_workspace("w2", "q", "dev", "/tmp/w2").unwrap();
        assert_eq!(s.list_workspaces(None).unwrap().len(), 2);
        assert!(s.set_active_workspace("no-sess", "w1").is_err());
        let _ = sess;
    }
}
