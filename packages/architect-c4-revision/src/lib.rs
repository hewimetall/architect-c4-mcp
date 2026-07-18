//! Append-only SQL revision mechanics (DRY for model + ADR).

use std::path::Path;
use std::sync::Arc;

use architect_c4_domain::ports::RevisionPort;
use architect_c4_domain::{ChangeKind, DomainError, EntityType, Revision};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use uuid::Uuid;

pub struct SqliteRevisionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRevisionStore {
    pub fn open(path: &Path) -> Result<Self, DomainError> {
        let conn = Connection::open(path).map_err(map_sql)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, DomainError> {
        let conn = Connection::open_in_memory().map_err(map_sql)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<(), DomainError> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS revisions (
              id            TEXT PRIMARY KEY,
              workspace_id  TEXT NOT NULL,
              entity_type   TEXT NOT NULL,
              entity_id     TEXT NOT NULL,
              rev_no        INTEGER NOT NULL,
              parent_rev_id TEXT,
              change_kind   TEXT NOT NULL,
              snapshot_json TEXT NOT NULL,
              git_commit_id TEXT,
              meta_json     TEXT,
              created_at    INTEGER NOT NULL,
              UNIQUE (workspace_id, entity_type, entity_id, rev_no)
            );
            CREATE TABLE IF NOT EXISTS revision_heads (
              workspace_id TEXT NOT NULL,
              entity_type  TEXT NOT NULL,
              entity_id    TEXT NOT NULL,
              head_rev_id  TEXT NOT NULL REFERENCES revisions(id),
              head_rev_no  INTEGER NOT NULL,
              PRIMARY KEY (workspace_id, entity_type, entity_id)
            );
            CREATE INDEX IF NOT EXISTS idx_revisions_entity
              ON revisions(workspace_id, entity_type, entity_id, rev_no);
            "#,
        )
        .map_err(map_sql)?;
        Ok(())
    }

    fn now_secs() -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

impl RevisionPort for SqliteRevisionStore {
    fn append(
        &self,
        workspace_id: &str,
        entity_type: EntityType,
        entity_id: &str,
        change_kind: ChangeKind,
        snapshot_json: &str,
        git_commit_id: Option<&str>,
    ) -> Result<Revision, DomainError> {
        if workspace_id.is_empty() || entity_id.is_empty() {
            return Err(DomainError::Validation(
                "workspace_id and entity_id required".into(),
            ));
        }
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(map_sql)?;

        let parent: Option<(String, i64)> = tx
            .query_row(
                r#"SELECT head_rev_id, head_rev_no FROM revision_heads
                   WHERE workspace_id=?1 AND entity_type=?2 AND entity_id=?3"#,
                params![workspace_id, entity_type.as_str(), entity_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional_mapped()?;

        let (parent_rev_id, rev_no) = match parent {
            Some((pid, n)) => (Some(pid), n + 1),
            None => (None, 1),
        };

        let id = Uuid::new_v4().to_string();
        let created_at = Self::now_secs();
        tx.execute(
            r#"INSERT INTO revisions
               (id, workspace_id, entity_type, entity_id, rev_no, parent_rev_id,
                change_kind, snapshot_json, git_commit_id, meta_json, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10)"#,
            params![
                id,
                workspace_id,
                entity_type.as_str(),
                entity_id,
                rev_no,
                parent_rev_id,
                change_kind.as_str(),
                snapshot_json,
                git_commit_id,
                created_at,
            ],
        )
        .map_err(map_sql)?;

        tx.execute(
            r#"INSERT INTO revision_heads
               (workspace_id, entity_type, entity_id, head_rev_id, head_rev_no)
               VALUES (?1,?2,?3,?4,?5)
               ON CONFLICT(workspace_id, entity_type, entity_id) DO UPDATE SET
                 head_rev_id=excluded.head_rev_id,
                 head_rev_no=excluded.head_rev_no"#,
            params![workspace_id, entity_type.as_str(), entity_id, id, rev_no],
        )
        .map_err(map_sql)?;

        tx.commit().map_err(map_sql)?;

        Ok(Revision {
            id,
            workspace_id: workspace_id.to_string(),
            entity_type,
            entity_id: entity_id.to_string(),
            rev_no,
            parent_rev_id,
            change_kind,
            snapshot_json: snapshot_json.to_string(),
            git_commit_id: git_commit_id.map(str::to_string),
            created_at,
        })
    }

    fn head(
        &self,
        workspace_id: &str,
        entity_type: EntityType,
        entity_id: &str,
    ) -> Result<Option<Revision>, DomainError> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                r#"SELECT r.id, r.workspace_id, r.entity_type, r.entity_id, r.rev_no,
                          r.parent_rev_id, r.change_kind, r.snapshot_json, r.git_commit_id,
                          r.created_at
                   FROM revision_heads h
                   JOIN revisions r ON r.id = h.head_rev_id
                   WHERE h.workspace_id=?1 AND h.entity_type=?2 AND h.entity_id=?3"#,
                params![workspace_id, entity_type.as_str(), entity_id],
                row_to_revision,
            )
            .optional_mapped()?;
        Ok(row)
    }

    fn history(
        &self,
        workspace_id: &str,
        entity_type: EntityType,
        entity_id: &str,
    ) -> Result<Vec<Revision>, DomainError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                r#"SELECT id, workspace_id, entity_type, entity_id, rev_no,
                          parent_rev_id, change_kind, snapshot_json, git_commit_id, created_at
                   FROM revisions
                   WHERE workspace_id=?1 AND entity_type=?2 AND entity_id=?3
                   ORDER BY rev_no ASC"#,
            )
            .map_err(map_sql)?;
        let rows = stmt
            .query_map(
                params![workspace_id, entity_type.as_str(), entity_id],
                row_to_revision,
            )
            .map_err(map_sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sql)?);
        }
        Ok(out)
    }
}

fn row_to_revision(r: &rusqlite::Row<'_>) -> rusqlite::Result<Revision> {
    let et: String = r.get(2)?;
    let ck: String = r.get(6)?;
    Ok(Revision {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        entity_type: EntityType::parse(&et).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?,
        entity_id: r.get(3)?,
        rev_no: r.get(4)?,
        parent_rev_id: r.get(5)?,
        change_kind: ChangeKind::parse(&ck).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?,
        snapshot_json: r.get(7)?,
        git_commit_id: r.get(8)?,
        created_at: r.get(9)?,
    })
}

fn map_sql(e: rusqlite::Error) -> DomainError {
    DomainError::Message(e.to_string())
}

trait OptionalMapped<T> {
    fn optional_mapped(self) -> Result<Option<T>, DomainError>;
}

impl<T> OptionalMapped<T> for Result<T, rusqlite::Error> {
    fn optional_mapped(self) -> Result<Option<T>, DomainError> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_sql(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::RevisionPort;

    #[test]
    fn append_creates_rev_one_and_head() {
        let store = SqliteRevisionStore::open_in_memory().unwrap();
        let rev = store
            .append(
                "ws1",
                EntityType::Decision,
                "001",
                ChangeKind::Create,
                r#"{"title":"t"}"#,
                Some("abc123"),
            )
            .unwrap();
        assert_eq!(rev.rev_no, 1);
        assert!(rev.parent_rev_id.is_none());
        assert_eq!(rev.git_commit_id.as_deref(), Some("abc123"));

        let head = store
            .head("ws1", EntityType::Decision, "001")
            .unwrap()
            .unwrap();
        assert_eq!(head.id, rev.id);
        assert_eq!(head.rev_no, 1);
    }

    #[test]
    fn append_chains_parent_and_increments() {
        let store = SqliteRevisionStore::open_in_memory().unwrap();
        let r1 = store
            .append(
                "ws1",
                EntityType::Element,
                "api",
                ChangeKind::Create,
                r#"{"name":"API"}"#,
                None,
            )
            .unwrap();
        let r2 = store
            .append(
                "ws1",
                EntityType::Element,
                "api",
                ChangeKind::Update,
                r#"{"name":"API v2"}"#,
                None,
            )
            .unwrap();
        assert_eq!(r2.rev_no, 2);
        assert_eq!(r2.parent_rev_id.as_deref(), Some(r1.id.as_str()));
        let hist = store.history("ws1", EntityType::Element, "api").unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].rev_no, 1);
        assert_eq!(hist[1].change_kind, ChangeKind::Update);
    }

    #[test]
    fn head_missing_is_none() {
        let store = SqliteRevisionStore::open_in_memory().unwrap();
        assert!(store
            .head("ws", EntityType::Relationship, "x")
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_empty_ids() {
        let store = SqliteRevisionStore::open_in_memory().unwrap();
        let err = store
            .append(
                "",
                EntityType::Decision,
                "1",
                ChangeKind::Create,
                "{}",
                None,
            )
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn open_path_and_migrate_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rev.db");
        let store = SqliteRevisionStore::open(&path).unwrap();
        store.migrate().unwrap();
        store
            .append(
                "w",
                EntityType::Decision,
                "1",
                ChangeKind::Import,
                "{}",
                Some("c0"),
            )
            .unwrap();
        let store2 = SqliteRevisionStore::open(&path).unwrap();
        assert_eq!(
            store2
                .history("w", EntityType::Decision, "1")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn delete_and_supersede_change_kinds_roundtrip() {
        let store = SqliteRevisionStore::open_in_memory().unwrap();
        store
            .append(
                "ws",
                EntityType::Relationship,
                "r1",
                ChangeKind::Create,
                "{}",
                None,
            )
            .unwrap();
        let del = store
            .append(
                "ws",
                EntityType::Relationship,
                "r1",
                ChangeKind::Delete,
                "null",
                None,
            )
            .unwrap();
        assert_eq!(del.change_kind, ChangeKind::Delete);
        let sup = store
            .append(
                "ws",
                EntityType::Decision,
                "d1",
                ChangeKind::Supersede,
                r#"{"status":"superseded"}"#,
                Some("deadbeef"),
            )
            .unwrap();
        assert_eq!(sup.change_kind, ChangeKind::Supersede);
        assert!(store
            .append(
                "ws",
                EntityType::Decision,
                "",
                ChangeKind::Create,
                "{}",
                None
            )
            .is_err());
        let hist = store.history("ws", EntityType::Relationship, "r1").unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(
            store
                .head("ws", EntityType::Relationship, "r1")
                .unwrap()
                .unwrap()
                .change_kind,
            ChangeKind::Delete
        );
    }
}
