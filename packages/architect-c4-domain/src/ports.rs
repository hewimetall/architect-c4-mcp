//! Application ports (hexagonal). Adapters live in sibling crates.

use crate::{Decision, DomainError, Element, Relationship};

/// Lookup whether a C4 element id exists (used by ADR scope checks).
pub trait ElementExistsPort: Send + Sync {
    fn element_exists(&self, workspace_id: &str, id: &str) -> Result<bool, DomainError>;
}

pub trait ModelPort: ElementExistsPort + Send + Sync {
    fn upsert_element(&self, element: Element) -> Result<Element, DomainError>;
    fn get_element(&self, workspace_id: &str, id: &str) -> Result<Element, DomainError>;
    fn list_elements(&self, workspace_id: &str) -> Result<Vec<Element>, DomainError>;
    fn upsert_relationship(&self, rel: Relationship) -> Result<Relationship, DomainError>;
    fn delete_relationship(&self, workspace_id: &str, id: &str) -> Result<(), DomainError>;
    fn list_relationships(&self, workspace_id: &str) -> Result<Vec<Relationship>, DomainError>;
}

pub trait GitPort: Send + Sync {
    fn init_bare(&self, path: &std::path::Path) -> Result<std::path::PathBuf, DomainError>;
    fn add_worktree(
        &self,
        bare: &std::path::Path,
        worktree_path: &std::path::Path,
        ref_name: &str,
    ) -> Result<std::path::PathBuf, DomainError>;
    fn commit(
        &self,
        worktree_path: &std::path::Path,
        message: &str,
        paths: &[String],
    ) -> Result<String, DomainError>;
}

pub trait AdrPort: Send + Sync {
    /// Agent upsert: status must be draft|proposed; rigid JSON fields validated.
    fn upsert_decision(
        &self,
        decision: Decision,
        commit: bool,
    ) -> Result<(Decision, Option<String>), DomainError>;
    /// Process-only status transition (accepted|rejected|deprecated|superseded).
    fn set_decision_status(
        &self,
        workspace_id: &str,
        id: &str,
        status: crate::DecisionStatus,
        reason: Option<&str>,
        superseded_by_id: Option<&str>,
        commit: bool,
    ) -> Result<(Decision, Option<String>), DomainError>;
    fn get_decision(&self, workspace_id: &str, id: &str) -> Result<Decision, DomainError>;
    fn list_decisions(&self, workspace_id: &str) -> Result<Vec<Decision>, DomainError>;
}

pub trait FlowPort: Send + Sync {
    fn upsert_flow(
        &self,
        flow: crate::Flow,
        commit: bool,
    ) -> Result<(crate::Flow, Option<String>), DomainError>;
    fn get_flow(&self, workspace_id: &str, id: &str) -> Result<crate::Flow, DomainError>;
    fn list_flows(&self, workspace_id: &str) -> Result<Vec<crate::Flow>, DomainError>;
    fn delete_flow(&self, workspace_id: &str, id: &str, commit: bool) -> Result<(), DomainError>;
}
