//! C4 model store with revision append on every mutate.

use std::path::Path;
use std::sync::Arc;

use architect_c4_domain::ports::{ElementExistsPort, ModelPort, RevisionPort};
use architect_c4_domain::{ChangeKind, DomainError, Element, EntityType, Relationship};
use architect_c4_revision::SqliteRevisionStore;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub struct SqliteModelStore {
    conn: Arc<Mutex<Connection>>,
    revisions: Arc<SqliteRevisionStore>,
}

impl SqliteModelStore {
    pub fn open(path: &Path, revisions: Arc<SqliteRevisionStore>) -> Result<Self, DomainError> {
        let conn = Connection::open(path).map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
            revisions,
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn open_in_memory(revisions: Arc<SqliteRevisionStore>) -> Result<Self, DomainError> {
        let conn = Connection::open_in_memory().map_err(map_sql)?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
            revisions,
        };
        s.migrate()?;
        Ok(s)
    }

    pub fn migrate(&self) -> Result<(), DomainError> {
        self.conn
            .lock()
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS elements (
                  id TEXT NOT NULL,
                  workspace_id TEXT NOT NULL,
                  kind TEXT NOT NULL,
                  parent_id TEXT,
                  name TEXT NOT NULL,
                  description TEXT,
                  technology TEXT,
                  url TEXT,
                  PRIMARY KEY (workspace_id, id)
                );
                CREATE TABLE IF NOT EXISTS relationships (
                  id TEXT NOT NULL,
                  workspace_id TEXT NOT NULL,
                  from_id TEXT NOT NULL,
                  to_id TEXT NOT NULL,
                  description TEXT,
                  technology TEXT,
                  PRIMARY KEY (workspace_id, id)
                );
                "#,
            )
            .map_err(map_sql)?;
        // Soft migration: structured UML members for kind=code.
        let _ = self.conn.lock().execute_batch(
            "ALTER TABLE elements ADD COLUMN members_json TEXT NOT NULL DEFAULT '[]';",
        );
        Ok(())
    }

    /// Drop all elements/relationships for a workspace (sidecar rebind).
    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), DomainError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM relationships WHERE workspace_id=?1",
            params![workspace_id],
        )
        .map_err(map_sql)?;
        conn.execute(
            "DELETE FROM elements WHERE workspace_id=?1",
            params![workspace_id],
        )
        .map_err(map_sql)?;
        Ok(())
    }
}

impl ElementExistsPort for SqliteModelStore {
    fn element_exists(&self, workspace_id: &str, id: &str) -> Result<bool, DomainError> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT 1 FROM elements WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, id],
                |_| Ok(true),
            )
            .optional()
            .map_err(map_sql)?
            .unwrap_or(false))
    }
}

impl ModelPort for SqliteModelStore {
    fn upsert_element(&self, element: Element) -> Result<Element, DomainError> {
        if element.id.is_empty() || element.workspace_id.is_empty() {
            return Err(DomainError::Validation(
                "element id and workspace_id required".into(),
            ));
        }
        let exists = self
            .conn
            .lock()
            .query_row(
                "SELECT 1 FROM elements WHERE workspace_id=?1 AND id=?2",
                params![element.workspace_id, element.id],
                |_| Ok(true),
            )
            .optional()
            .map_err(map_sql)?
            .unwrap_or(false);
        {
            let conn = self.conn.lock();
            conn.execute(
                r#"INSERT INTO elements
                   (id, workspace_id, kind, parent_id, name, description, technology, url, members_json)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                   ON CONFLICT(workspace_id, id) DO UPDATE SET
                     kind=excluded.kind, parent_id=excluded.parent_id, name=excluded.name,
                     description=excluded.description, technology=excluded.technology, url=excluded.url,
                     members_json=excluded.members_json"#,
                params![
                    element.id,
                    element.workspace_id,
                    element.kind.as_str(),
                    element.parent_id,
                    element.name,
                    element.description,
                    element.technology,
                    element.url,
                    serde_json::to_string(&element.members).unwrap_or_else(|_| "[]".into()),
                ],
            )
            .map_err(map_sql)?;
        }
        let snap =
            serde_json::to_string(&element).map_err(|e| DomainError::Message(e.to_string()))?;
        let kind = if exists {
            ChangeKind::Update
        } else {
            ChangeKind::Create
        };
        self.revisions.append(
            &element.workspace_id,
            EntityType::Element,
            &element.id,
            kind,
            &snap,
            None,
        )?;
        Ok(element)
    }

    fn get_element(&self, workspace_id: &str, id: &str) -> Result<Element, DomainError> {
        self.conn
            .lock()
            .query_row(
                "SELECT id, workspace_id, kind, parent_id, name, description, technology, url, COALESCE(members_json, '[]') FROM elements WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, id],
                element_row,
            )
            .optional()
            .map_err(map_sql)?
            .ok_or_else(|| DomainError::NotFound(format!("element {id}")))
    }

    fn list_elements(&self, workspace_id: &str) -> Result<Vec<Element>, DomainError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, workspace_id, kind, parent_id, name, description, technology, url, COALESCE(members_json, '[]') FROM elements WHERE workspace_id=?1",
            )
            .map_err(map_sql)?;
        let rows = stmt
            .query_map(params![workspace_id], element_row)
            .map_err(map_sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sql)?);
        }
        Ok(out)
    }

    fn upsert_relationship(&self, rel: Relationship) -> Result<Relationship, DomainError> {
        let id = if rel.id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            rel.id.clone()
        };
        let rel = Relationship {
            id: id.clone(),
            ..rel
        };
        // Incident fix: never persist edges to missing endpoints (dangling r1).
        if !self.element_exists(&rel.workspace_id, &rel.from_id)? {
            return Err(DomainError::Validation(format!(
                "relationship from_id '{}' does not exist in workspace '{}'",
                rel.from_id, rel.workspace_id
            )));
        }
        if !self.element_exists(&rel.workspace_id, &rel.to_id)? {
            return Err(DomainError::Validation(format!(
                "relationship to_id '{}' does not exist in workspace '{}'",
                rel.to_id, rel.workspace_id
            )));
        }
        let exists = self
            .conn
            .lock()
            .query_row(
                "SELECT 1 FROM relationships WHERE workspace_id=?1 AND id=?2",
                params![rel.workspace_id, rel.id],
                |_| Ok(true),
            )
            .optional()
            .map_err(map_sql)?
            .unwrap_or(false);
        {
            let conn = self.conn.lock();
            conn.execute(
                r#"INSERT INTO relationships
                   (id, workspace_id, from_id, to_id, description, technology)
                   VALUES (?1,?2,?3,?4,?5,?6)
                   ON CONFLICT(workspace_id, id) DO UPDATE SET
                     from_id=excluded.from_id, to_id=excluded.to_id,
                     description=excluded.description, technology=excluded.technology"#,
                params![
                    rel.id,
                    rel.workspace_id,
                    rel.from_id,
                    rel.to_id,
                    rel.description,
                    rel.technology
                ],
            )
            .map_err(map_sql)?;
        }
        let snap = serde_json::to_string(&rel).map_err(|e| DomainError::Message(e.to_string()))?;
        self.revisions.append(
            &rel.workspace_id,
            EntityType::Relationship,
            &rel.id,
            if exists {
                ChangeKind::Update
            } else {
                ChangeKind::Create
            },
            &snap,
            None,
        )?;
        Ok(rel)
    }

    fn delete_relationship(&self, workspace_id: &str, id: &str) -> Result<(), DomainError> {
        let n = self
            .conn
            .lock()
            .execute(
                "DELETE FROM relationships WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, id],
            )
            .map_err(map_sql)?;
        if n == 0 {
            return Err(DomainError::NotFound(format!("relationship {id}")));
        }
        // Tombstone revision for audit trail
        self.revisions.append(
            workspace_id,
            EntityType::Relationship,
            id,
            ChangeKind::Delete,
            "{}",
            None,
        )?;
        Ok(())
    }

    fn list_relationships(&self, workspace_id: &str) -> Result<Vec<Relationship>, DomainError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, workspace_id, from_id, to_id, description, technology FROM relationships WHERE workspace_id=?1",
            )
            .map_err(map_sql)?;
        let rows = stmt
            .query_map(params![workspace_id], |r| {
                Ok(Relationship {
                    id: r.get(0)?,
                    workspace_id: r.get(1)?,
                    from_id: r.get(2)?,
                    to_id: r.get(3)?,
                    description: r.get(4)?,
                    technology: r.get(5)?,
                })
            })
            .map_err(map_sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_sql)?);
        }
        Ok(out)
    }
}

fn element_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Element> {
    let kind: String = r.get(2)?;
    let members_raw: String = r.get(8)?;
    let members: Vec<architect_c4_domain::CodeMember> =
        serde_json::from_str(&members_raw).unwrap_or_default();
    Ok(Element {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        kind: architect_c4_domain::ElementKind::parse(&kind).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?,
        parent_id: r.get(3)?,
        name: r.get(4)?,
        description: r.get(5)?,
        technology: r.get(6)?,
        url: r.get(7)?,
        members,
    })
}

fn map_sql(e: rusqlite::Error) -> DomainError {
    DomainError::Message(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::{ModelPort, RevisionPort};
    use architect_c4_domain::ElementKind;
    use std::sync::Arc;

    fn stores() -> (Arc<SqliteRevisionStore>, SqliteModelStore) {
        let rev = Arc::new(SqliteRevisionStore::open_in_memory().unwrap());
        let model = SqliteModelStore::open_in_memory(rev.clone()).unwrap();
        (rev, model)
    }

    #[test]
    fn upsert_element_creates_revision() {
        let (rev, model) = stores();
        let e = Element {
            id: "u".into(),
            workspace_id: "w".into(),
            kind: ElementKind::Person,
            parent_id: None,
            name: "User".into(),
            description: Some("d".into()),
            technology: None,
            url: None,
            members: vec![],
        };
        model.upsert_element(e.clone()).unwrap();
        assert_eq!(model.get_element("w", "u").unwrap().name, "User");
        let h = rev.head("w", EntityType::Element, "u").unwrap().unwrap();
        assert_eq!(h.rev_no, 1);
        let mut e2 = e;
        e2.name = "Customer".into();
        model.upsert_element(e2).unwrap();
        assert_eq!(rev.history("w", EntityType::Element, "u").unwrap().len(), 2);
    }

    #[test]
    fn relationship_roundtrip() {
        let (_rev, model) = stores();
        model
            .upsert_element(Element {
                id: "a".into(),
                workspace_id: "w".into(),
                kind: ElementKind::Person,
                parent_id: None,
                name: "A".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            })
            .unwrap();
        model
            .upsert_element(Element {
                id: "b".into(),
                workspace_id: "w".into(),
                kind: ElementKind::SoftwareSystem,
                parent_id: None,
                name: "B".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            })
            .unwrap();
        model
            .upsert_relationship(Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("uses".into()),
                technology: None,
            })
            .unwrap();
        assert_eq!(model.list_relationships("w").unwrap().len(), 1);
        assert_eq!(model.list_elements("w").unwrap().len(), 2);
    }

    #[test]
    fn validation_and_missing() {
        let (_rev, model) = stores();
        assert!(model
            .upsert_element(Element {
                id: "".into(),
                workspace_id: "w".into(),
                kind: ElementKind::Person,
                parent_id: None,
                name: "x".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            })
            .is_err());
        assert!(model.get_element("w", "nope").is_err());
        // Incident: dangling endpoints must be rejected
        let err = model
            .upsert_relationship(Relationship {
                id: "r1".into(),
                workspace_id: "w".into(),
                from_id: "user".into(),
                to_id: "sys".into(),
                description: Some("Uses".into()),
                technology: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, DomainError::Validation(_)),
            "expected Validation, got {err:?}"
        );
        assert!(err.to_string().contains("from_id"));

        model
            .upsert_element(Element {
                id: "a".into(),
                workspace_id: "w".into(),
                kind: ElementKind::Person,
                parent_id: None,
                name: "A".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            })
            .unwrap();
        model
            .upsert_element(Element {
                id: "b".into(),
                workspace_id: "w".into(),
                kind: ElementKind::SoftwareSystem,
                parent_id: None,
                name: "B".into(),
                description: None,
                technology: None,
                url: None,
                members: vec![],
            })
            .unwrap();
        let r = model
            .upsert_relationship(Relationship {
                id: "".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: None,
                technology: None,
            })
            .unwrap();
        assert!(!r.id.is_empty());
        model
            .upsert_relationship(Relationship {
                id: r.id.clone(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("again".into()),
                technology: Some("http".into()),
            })
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rev2 = Arc::new(SqliteRevisionStore::open(&dir.path().join("r.db")).unwrap());
        let model2 = SqliteModelStore::open(&dir.path().join("m.db"), rev2).unwrap();
        model2.migrate().unwrap();
        assert!(model2.list_elements("w").unwrap().is_empty());
    }
    #[test]
    fn members_json_roundtrip() {
        use architect_c4_domain::{CodeMember, CodeMemberKind, CodeParam};
        let (_rev, model) = stores();
        let e = Element {
            id: "Actor".into(),
            workspace_id: "w".into(),
            kind: ElementKind::Code,
            parent_id: None,
            name: "Actor".into(),
            description: None,
            technology: Some("class".into()),
            url: None,
            members: vec![CodeMember {
                kind: CodeMemberKind::Method,
                visibility: "+".into(),
                name: "send".into(),
                params: vec![CodeParam {
                    name: "message".into(),
                    type_name: Some("Message".into()),
                    optional: false,
                }],
                return_type: Some("Message".into()),
                type_name: None,
            }],
        };
        model.upsert_element(e.clone()).unwrap();
        let got = model.get_element("w", "Actor").unwrap();
        assert_eq!(got.members.len(), 1);
        assert_eq!(
            got.members[0].to_uml_line(),
            "+send(message: Message) Message"
        );
    }

    #[test]
    fn delete_relationship_removes_and_revisions() {
        let (rev, model) = stores();
        for (id, kind, name) in [
            ("a", ElementKind::Person, "A"),
            ("b", ElementKind::SoftwareSystem, "B"),
        ] {
            model
                .upsert_element(Element {
                    id: id.into(),
                    workspace_id: "w".into(),
                    kind,
                    parent_id: None,
                    name: name.into(),
                    description: None,
                    technology: None,
                    url: None,
                    members: vec![],
                })
                .unwrap();
        }
        model
            .upsert_relationship(Relationship {
                id: "r-bad".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "b".into(),
                description: Some("x".into()),
                technology: None,
            })
            .unwrap();
        model.delete_relationship("w", "r-bad").unwrap();
        assert!(model.list_relationships("w").unwrap().is_empty());
        assert!(model.delete_relationship("w", "r-bad").is_err());
        let hist = rev.history("w", EntityType::Relationship, "r-bad").unwrap();
        assert!(hist.iter().any(|r| r.change_kind == ChangeKind::Delete));
    }
}
