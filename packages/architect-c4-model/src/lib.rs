//! In-memory C4 model store (HashMap). Durable truth is `docs/model.toml` via the app.

use std::collections::HashMap;
use std::sync::Arc;

use architect_c4_domain::ports::{ElementExistsPort, ModelPort};
use architect_c4_domain::{DomainError, Element, Relationship};
use parking_lot::Mutex;
use uuid::Uuid;

type Key = (String, String);

pub struct MemoryModelStore {
    elements: Mutex<HashMap<Key, Element>>,
    relationships: Mutex<HashMap<Key, Relationship>>,
}

impl MemoryModelStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            elements: Mutex::new(HashMap::new()),
            relationships: Mutex::new(HashMap::new()),
        })
    }

    /// Drop all elements/relationships for a workspace (sidecar rebind).
    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), DomainError> {
        self.elements
            .lock()
            .retain(|(ws, _), _| ws != workspace_id);
        self.relationships
            .lock()
            .retain(|(ws, _), _| ws != workspace_id);
        Ok(())
    }
}

impl Default for MemoryModelStore {
    fn default() -> Self {
        Self {
            elements: Mutex::new(HashMap::new()),
            relationships: Mutex::new(HashMap::new()),
        }
    }
}

impl ElementExistsPort for MemoryModelStore {
    fn element_exists(&self, workspace_id: &str, id: &str) -> Result<bool, DomainError> {
        Ok(self
            .elements
            .lock()
            .contains_key(&(workspace_id.to_string(), id.to_string())))
    }
}

impl ModelPort for MemoryModelStore {
    fn upsert_element(&self, element: Element) -> Result<Element, DomainError> {
        if element.id.is_empty() || element.workspace_id.is_empty() {
            return Err(DomainError::Validation(
                "element id and workspace_id required".into(),
            ));
        }
        let key = (element.workspace_id.clone(), element.id.clone());
        self.elements.lock().insert(key, element.clone());
        Ok(element)
    }

    fn get_element(&self, workspace_id: &str, id: &str) -> Result<Element, DomainError> {
        self.elements
            .lock()
            .get(&(workspace_id.to_string(), id.to_string()))
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("element {id}")))
    }

    fn list_elements(&self, workspace_id: &str) -> Result<Vec<Element>, DomainError> {
        let mut out: Vec<_> = self
            .elements
            .lock()
            .values()
            .filter(|e| e.workspace_id == workspace_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
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
        let key = (rel.workspace_id.clone(), rel.id.clone());
        self.relationships.lock().insert(key, rel.clone());
        Ok(rel)
    }

    fn delete_relationship(&self, workspace_id: &str, id: &str) -> Result<(), DomainError> {
        let removed = self
            .relationships
            .lock()
            .remove(&(workspace_id.to_string(), id.to_string()));
        if removed.is_none() {
            return Err(DomainError::NotFound(format!("relationship {id}")));
        }
        Ok(())
    }

    fn list_relationships(&self, workspace_id: &str) -> Result<Vec<Relationship>, DomainError> {
        let mut out: Vec<_> = self
            .relationships
            .lock()
            .values()
            .filter(|r| r.workspace_id == workspace_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::ports::ModelPort;
    use architect_c4_domain::ElementKind;

    fn store() -> Arc<MemoryModelStore> {
        MemoryModelStore::new()
    }

    #[test]
    fn upsert_element_roundtrip() {
        let model = store();
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
        let mut e2 = e;
        e2.name = "Customer".into();
        model.upsert_element(e2).unwrap();
        assert_eq!(model.get_element("w", "u").unwrap().name, "Customer");
    }

    #[test]
    fn relationship_roundtrip() {
        let model = store();
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
        let model = store();
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
        let err_to = model
            .upsert_relationship(Relationship {
                id: "r-missing-to".into(),
                workspace_id: "w".into(),
                from_id: "a".into(),
                to_id: "missing".into(),
                description: Some("Uses".into()),
                technology: None,
            })
            .unwrap_err();
        assert!(err_to.to_string().contains("to_id"));

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
    }

    #[test]
    fn members_json_roundtrip() {
        use architect_c4_domain::{CodeMember, CodeMemberKind, CodeParam};
        let model = store();
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
    fn delete_relationship_removes() {
        let model = store();
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
    }

    #[test]
    fn clear_workspace_drops_elements_and_relationships() {
        let model = store();
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
        model.clear_workspace("w").unwrap();
        assert!(model.list_elements("w").unwrap().is_empty());
        assert!(model.list_relationships("w").unwrap().is_empty());
    }
}
