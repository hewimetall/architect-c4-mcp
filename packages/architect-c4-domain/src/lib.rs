//! Pure domain: entities, errors, ports. No IO (hexagonal / SOLID).

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod ports;
pub mod project;

pub use project::{project_endpoint_id, project_relationships};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Element,
    Relationship,
    Decision,
    Flow,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Element => "element",
            Self::Relationship => "relationship",
            Self::Decision => "decision",
            Self::Flow => "flow",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "element" => Ok(Self::Element),
            "relationship" => Ok(Self::Relationship),
            "decision" => Ok(Self::Decision),
            "flow" => Ok(Self::Flow),
            other => Err(DomainError::InvalidEntityType(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Create,
    Update,
    Delete,
    Supersede,
    Import,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Supersede => "supersede",
            Self::Import => "import",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            "supersede" => Ok(Self::Supersede),
            "import" => Ok(Self::Import),
            other => Err(DomainError::InvalidChangeKind(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Person,
    SoftwareSystem,
    Container,
    Component,
    /// C4 level 4 — classes/functions/modules inside a component (atoms).
    Code,
    /// Outside the codebase: datastore, queue, SaaS, identity provider, etc.
    External,
}

/// Stereotype for [`ElementKind::Code`] atoms (usually carried in `technology`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomStereotype {
    Class,
    Interface,
    Function,
}

impl AtomStereotype {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Function => "function",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "class" | "struct" | "type" => Some(Self::Class),
            "interface" | "trait" | "protocol" => Some(Self::Interface),
            "function" | "fn" | "method" | "free_function" => Some(Self::Function),
            _ => None,
        }
    }
}

/// Role hint for [`ElementKind::External`] (usually in `technology` prefix or description).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalRole {
    Datastore,
    Queue,
    Saas,
    Identity,
    Other,
}

impl ExternalRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Datastore => "datastore",
            Self::Queue => "queue",
            Self::Saas => "saas",
            Self::Identity => "identity",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "datastore" | "database" | "db" | "storage" => Some(Self::Datastore),
            "queue" | "broker" | "mq" => Some(Self::Queue),
            "saas" | "api" | "service" => Some(Self::Saas),
            "identity" | "idp" | "auth" => Some(Self::Identity),
            "other" | "external" => Some(Self::Other),
            _ => None,
        }
    }
}

impl ElementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::SoftwareSystem => "software_system",
            Self::Container => "container",
            Self::Component => "component",
            Self::Code => "code",
            Self::External => "external",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "person" => Ok(Self::Person),
            "software_system" => Ok(Self::SoftwareSystem),
            "container" => Ok(Self::Container),
            "component" => Ok(Self::Component),
            "code" => Ok(Self::Code),
            "external" => Ok(Self::External),
            other => Err(DomainError::InvalidElementKind(other.to_string())),
        }
    }

    pub fn layer(self) -> C4Layer {
        match self {
            Self::Person | Self::SoftwareSystem | Self::External => C4Layer::Context,
            Self::Container => C4Layer::Container,
            Self::Component => C4Layer::Component,
            Self::Code => C4Layer::Code,
        }
    }

    /// True for code atoms (class / interface / function) — not shells, not person/external.
    pub fn is_code_atom(self) -> bool {
        matches!(self, Self::Code)
    }

    /// Endpoints allowed in the atom-centric relationship canon (strict mode).
    pub fn is_canon_rel_endpoint(self) -> bool {
        matches!(
            self,
            Self::Code | Self::External | Self::Person | Self::SoftwareSystem
        )
    }

    /// Child kinds shown when drilling into this element.
    pub fn child_kinds(self) -> &'static [ElementKind] {
        match self {
            Self::SoftwareSystem => &[Self::Container],
            Self::Container => &[Self::Component],
            Self::Component => &[Self::Code],
            Self::Person | Self::Code | Self::External => &[],
        }
    }

    pub fn drill_layer(self) -> Option<C4Layer> {
        match self {
            Self::SoftwareSystem => Some(C4Layer::Container),
            Self::Container => Some(C4Layer::Component),
            Self::Component => Some(C4Layer::Code),
            Self::Person | Self::Code | Self::External => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum C4Layer {
    Landscape,
    Context,
    Container,
    Component,
    Code,
    Adr,
}

impl C4Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Landscape => "landscape",
            Self::Context => "context",
            Self::Container => "container",
            Self::Component => "component",
            Self::Code => "code",
            Self::Adr => "adr",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "landscape" => Ok(Self::Landscape),
            "context" => Ok(Self::Context),
            "container" => Ok(Self::Container),
            "component" => Ok(Self::Component),
            "code" => Ok(Self::Code),
            "adr" => Ok(Self::Adr),
            other => Err(DomainError::Message(format!("invalid c4 layer: {other}"))),
        }
    }

    pub fn element_kinds(self) -> &'static [ElementKind] {
        match self {
            Self::Landscape | Self::Context => &[
                ElementKind::Person,
                ElementKind::SoftwareSystem,
                ElementKind::External,
            ],
            Self::Container => &[ElementKind::Container],
            Self::Component => &[ElementKind::Component],
            Self::Code => &[ElementKind::Code],
            Self::Adr => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    /// Agent-writable draft.
    Draft,
    /// Agent-writable; awaiting process.
    Proposed,
    /// Process-only; policy enforce applies.
    Accepted,
    /// Process-only; requires `reason`.
    Rejected,
    /// Process-only.
    Deprecated,
    /// Process-only; requires `superseded_by_id`.
    Superseded,
}

impl DecisionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Deprecated => "deprecated",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s.to_ascii_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "deprecated" => Ok(Self::Deprecated),
            "superseded" => Ok(Self::Superseded),
            other => Err(DomainError::InvalidDecisionStatus(other.to_string())),
        }
    }

    /// Statuses an agent may set via `upsert_adr`.
    pub fn agent_writable(self) -> bool {
        matches!(self, Self::Draft | Self::Proposed)
    }

    /// Statuses that activate `policy.forbid` enforcement.
    pub fn enforces_policy(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    Enforce,
    Audit,
}

impl PolicyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Audit => "audit",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s.to_ascii_lowercase().as_str() {
            "enforce" => Ok(Self::Enforce),
            "audit" => Ok(Self::Audit),
            other => Err(DomainError::Validation(format!(
                "invalid policy.mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdrForbidRule {
    pub from_kind: ElementKind,
    pub to_kind: ElementKind,
    pub code: String,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdrPolicy {
    #[serde(default = "default_policy_mode")]
    pub mode: PolicyMode,
    #[serde(default)]
    pub forbid: Vec<AdrForbidRule>,
}

fn default_policy_mode() -> PolicyMode {
    PolicyMode::Enforce
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub severity: Severity,
    pub layer: C4Layer,
    pub code: String,
    pub element_id: Option<String>,
    pub message: String,
    /// Set when the problem comes from an accepted ADR policy rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_id: Option<String>,
}

impl Problem {
    pub fn new(
        severity: Severity,
        layer: C4Layer,
        code: impl Into<String>,
        element_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            layer,
            code: code.into(),
            element_id,
            message: message.into(),
            adr_id: None,
        }
    }
}

/// Parameter of a code method (UML signature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeParam {
    pub name: String,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeMemberKind {
    Method,
    Field,
}

impl CodeMemberKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Field => "field",
        }
    }
}

/// Structured UML class member for `kind=code` (preferred over freeform description lines).
///
/// Example method:
/// `{ "kind": "method", "visibility": "+", "name": "send",
///    "params": [{"name": "message", "type": "Message"}],
///    "return_type": "Message" }`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeMember {
    pub kind: CodeMemberKind,
    /// One of `+` `-` `#` `~` (UML visibility).
    pub visibility: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<CodeParam>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Field type (`kind=field`). Serde name: `type`.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
}

impl CodeMember {
    pub fn validate(&self) -> Result<(), DomainError> {
        let vis = self.visibility.trim();
        if !matches!(vis, "+" | "-" | "#" | "~") {
            return Err(DomainError::Validation(
                "member.visibility must be one of + - # ~".into(),
            ));
        }
        let name = self.name.trim();
        if name.is_empty() || name.len() > 120 {
            return Err(DomainError::Validation(
                "member.name required (max 120)".into(),
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(DomainError::Validation(
                "member.name must be ascii alphanumeric/_/-".into(),
            ));
        }
        match self.kind {
            CodeMemberKind::Method => {
                if self.type_name.is_some() {
                    return Err(DomainError::Validation(
                        "method must not set type (use return_type / params)".into(),
                    ));
                }
                for p in &self.params {
                    let pn = p.name.trim();
                    if pn.is_empty() || pn.len() > 80 {
                        return Err(DomainError::Validation(
                            "param.name required (max 80)".into(),
                        ));
                    }
                    if let Some(ref ty) = p.type_name {
                        if ty.trim().is_empty() || ty.len() > 120 {
                            return Err(DomainError::Validation("param.type max 120".into()));
                        }
                    }
                }
                if let Some(ref rt) = self.return_type {
                    if rt.trim().is_empty() || rt.len() > 120 {
                        return Err(DomainError::Validation("return_type max 120".into()));
                    }
                }
            }
            CodeMemberKind::Field => {
                if !self.params.is_empty() || self.return_type.is_some() {
                    return Err(DomainError::Validation(
                        "field must not set params/return_type".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// UML classDiagram line, e.g. `+send(message: Message) Message`.
    pub fn to_uml_line(&self) -> String {
        let vis = self.visibility.trim();
        let name = self.name.trim();
        match self.kind {
            CodeMemberKind::Field => {
                if let Some(ref ty) = self.type_name {
                    let ty = ty.trim();
                    if !ty.is_empty() {
                        return format!("{vis}{name}: {ty}");
                    }
                }
                format!("{vis}{name}")
            }
            CodeMemberKind::Method => {
                let mut args = Vec::with_capacity(self.params.len());
                for p in &self.params {
                    let mut s = p.name.trim().to_string();
                    if let Some(ref ty) = p.type_name {
                        let ty = ty.trim();
                        if !ty.is_empty() {
                            s.push_str(": ");
                            s.push_str(ty);
                        }
                    }
                    if p.optional {
                        s.push('?');
                    }
                    args.push(s);
                }
                let mut line = format!("{vis}{name}({})", args.join(", "));
                if let Some(ref rt) = self.return_type {
                    let rt = rt.trim();
                    if !rt.is_empty() {
                        line.push(' ');
                        line.push_str(rt);
                    }
                }
                line
            }
        }
    }
}

/// Prefer structured `members`; fallback to UML-ish lines in `description`.
pub fn element_uml_members(el: &Element) -> Vec<String> {
    if !el.members.is_empty() {
        return el.members.iter().map(CodeMember::to_uml_line).collect();
    }
    parse_description_members(el.description.as_deref())
}

fn parse_description_members(description: Option<&str>) -> Vec<String> {
    let Some(raw) = description.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    raw.split([';', '\n'])
        .map(str::trim)
        .filter(|s| {
            s.starts_with('+') || s.starts_with('-') || s.starts_with('#') || s.starts_with('~')
        })
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub workspace_id: String,
    pub kind: ElementKind,
    pub parent_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub technology: Option<String>,
    pub url: Option<String>,
    /// Structured UML members for `kind=code` (methods/fields with typed params).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<CodeMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: String,
    pub workspace_id: String,
    pub from_id: String,
    pub to_id: String,
    pub description: Option<String>,
    pub technology: Option<String>,
}

/// Rigid ADR document (Nygard core + optional executable `policy`).
/// Agent upsert payload uses the same fields minus storage metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub scope_element_id: Option<String>,
    pub title: String,
    pub status: DecisionStatus,
    pub decided_at: String,
    pub context: String,
    pub decision: String,
    pub consequences: String,
    #[serde(default)]
    pub policy: Option<AdrPolicy>,
    /// Optional links to Flow documents (`docs/flows/{id}.json`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_flows: Vec<String>,
    /// External documentation URLs (https only), e.g. upstream Ceph docs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<DocRef>,
    /// Set only via process `set_adr_status(rejected, reason=…)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by_id: Option<String>,
    /// Storage: relative path in worktree (`docs/adr/{id}.json`).
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit_id: Option<String>,
}

/// External documentation / source reference (https URL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocRef {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl DocRef {
    pub fn validate(&self) -> Result<(), DomainError> {
        let u = self.url.trim();
        if u.is_empty() || u.len() > 2000 {
            return Err(DomainError::Validation(
                "ref.url required and max 2000 chars".into(),
            ));
        }
        let lower = u.to_ascii_lowercase();
        if !lower.starts_with("https://") {
            return Err(DomainError::Validation(
                "ref.url must start with https://".into(),
            ));
        }
        if lower.contains("javascript:") || lower.contains("data:") {
            return Err(DomainError::Validation("ref.url scheme not allowed".into()));
        }
        if let Some(t) = &self.title {
            if t.len() > 200 {
                return Err(DomainError::Validation("ref.title max 200 chars".into()));
            }
        }
        Ok(())
    }
}

pub fn validate_doc_refs(refs: &[DocRef]) -> Result<(), DomainError> {
    if refs.len() > 32 {
        return Err(DomainError::Validation("refs max 32".into()));
    }
    for r in refs {
        r.validate()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    C4Dynamic,
    Sequence,
    State,
}

impl FlowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::C4Dynamic => "c4_dynamic",
            Self::Sequence => "sequence",
            Self::State => "state",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "c4_dynamic" => Ok(Self::C4Dynamic),
            "sequence" => Ok(Self::Sequence),
            "state" => Ok(Self::State),
            other => Err(DomainError::Validation(format!(
                "invalid flow.kind: {other} (v1: c4_dynamic|sequence|state)"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowStep {
    pub n: u32,
    pub from_id: String,
    pub to_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowEpoch {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowAnchor {
    pub alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_id: Option<String>,
}

/// Rigid Flow document (behavior view linked to C4 + ADR).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    #[serde(default)]
    pub workspace_id: String,
    pub title: String,
    pub kind: FlowKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_element_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_adrs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<FlowEpoch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<FlowStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<FlowAnchor>,
    /// External documentation URLs (https only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<DocRef>,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit_id: Option<String>,
}

impl Flow {
    /// Kind-specific structural rules (element existence checked by flow crate).
    pub fn validate_shape(&self) -> Result<(), DomainError> {
        validate_doc_refs(&self.refs)?;
        if self.id.is_empty() || self.title.is_empty() {
            return Err(DomainError::Validation("flow id and title required".into()));
        }
        if !self
            .id
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
        {
            return Err(DomainError::Validation(
                "flow id must match [A-Za-z0-9][A-Za-z0-9_.-]*".into(),
            ));
        }
        if self.title.len() > 200 {
            return Err(DomainError::Validation("flow title max 200 chars".into()));
        }
        match self.kind {
            FlowKind::C4Dynamic => {
                if self.steps.is_empty() || self.steps.len() > 64 {
                    return Err(DomainError::Validation(
                        "c4_dynamic requires 1..=64 steps".into(),
                    ));
                }
                if self.body.as_ref().is_some_and(|b| !b.trim().is_empty()) {
                    return Err(DomainError::Validation(
                        "c4_dynamic must not set body (use steps)".into(),
                    ));
                }
                for s in &self.steps {
                    if s.from_id.is_empty() || s.to_id.is_empty() {
                        return Err(DomainError::Validation(
                            "step from_id and to_id required".into(),
                        ));
                    }
                }
            }
            FlowKind::Sequence | FlowKind::State => {
                let body = self.body.as_deref().map(str::trim).unwrap_or("");
                if body.is_empty() || body.len() > 50_000 {
                    return Err(DomainError::Validation(
                        "sequence/state require non-empty body (max 50000)".into(),
                    ));
                }
                if !self.steps.is_empty() {
                    return Err(DomainError::Validation(
                        "sequence/state must not set steps (use body)".into(),
                    ));
                }
                let expect = if self.kind == FlowKind::Sequence {
                    "sequenceDiagram"
                } else {
                    "stateDiagram"
                };
                if !body.starts_with(expect) {
                    return Err(DomainError::Validation(format!(
                        "{} body must start with {expect}",
                        self.kind.as_str()
                    )));
                }
            }
        }
        for a in &self.anchors {
            if a.alias.is_empty() {
                return Err(DomainError::Validation("anchor.alias required".into()));
            }
            if a.element_id.is_none() && a.adr_id.is_none() {
                return Err(DomainError::Validation(
                    "anchor needs element_id or adr_id".into(),
                ));
            }
        }
        if self.related_adrs.len() > 32 {
            return Err(DomainError::Validation("related_adrs max 32".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub workspace_id: String,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub rev_no: i64,
    pub parent_rev_id: Option<String>,
    pub change_kind: ChangeKind,
    pub snapshot_json: String,
    pub git_commit_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub meta: String,
    pub active_workspace_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub project_id: String,
    pub ref_name: String,
    pub path: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid entity type: {0}")]
    InvalidEntityType(String),
    #[error("invalid change kind: {0}")]
    InvalidChangeKind(String),
    #[error("invalid element kind: {0}")]
    InvalidElementKind(String),
    #[error("invalid decision status: {0}")]
    InvalidDecisionStatus(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("{0}")]
    Message(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_member_to_uml_line() {
        let m = CodeMember {
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
        };
        m.validate().unwrap();
        assert_eq!(m.to_uml_line(), "+send(message: Message) Message");
    }

    #[test]
    fn flow_kind_and_shape_rules() {
        assert_eq!(FlowKind::parse("c4_dynamic").unwrap(), FlowKind::C4Dynamic);
        assert!(FlowKind::parse("bpmn").is_err());
        let mut f = Flow {
            id: "f1".into(),
            workspace_id: "w".into(),
            title: "T".into(),
            kind: FlowKind::C4Dynamic,
            usage_key: None,
            scope_element_id: None,
            related_adrs: vec![],
            epoch: None,
            steps: vec![FlowStep {
                n: 1,
                from_id: "a".into(),
                to_id: "b".into(),
                label: Some("x".into()),
            }],
            body: None,
            anchors: vec![],
            refs: vec![],
            path: String::new(),
            git_commit_id: None,
        };
        assert!(f.validate_shape().is_ok());
        f.body = Some("sequenceDiagram\n".into());
        assert!(f.validate_shape().is_err());
        f.body = None;
        f.kind = FlowKind::Sequence;
        f.steps.clear();
        f.body = Some("sequenceDiagram\n  A->>B: hi\n".into());
        assert!(f.validate_shape().is_ok());
        f.body = Some("flowchart TD\n  A-->B\n".into());
        assert!(f.validate_shape().is_err());
        f.kind = FlowKind::State;
        f.body = Some("stateDiagram-v2\n  [*] --> A\n".into());
        assert!(f.validate_shape().is_ok());
        f.anchors = vec![FlowAnchor {
            alias: String::new(),
            element_id: None,
            adr_id: None,
        }];
        assert!(f.validate_shape().is_err());
        assert_eq!(FlowKind::State.as_str(), "state");
        assert_eq!(EntityType::Flow.as_str(), "flow");
    }

    #[test]
    fn entity_type_roundtrip() {
        for t in [
            EntityType::Element,
            EntityType::Relationship,
            EntityType::Decision,
            EntityType::Flow,
        ] {
            assert_eq!(EntityType::parse(t.as_str()).unwrap(), t);
        }
        assert!(EntityType::parse("nope").is_err());
    }

    #[test]
    fn change_kind_roundtrip() {
        for k in [
            ChangeKind::Create,
            ChangeKind::Update,
            ChangeKind::Delete,
            ChangeKind::Supersede,
            ChangeKind::Import,
        ] {
            assert_eq!(ChangeKind::parse(k.as_str()).unwrap(), k);
        }
    }

    #[test]
    fn element_kind_maps_to_layer() {
        assert_eq!(ElementKind::Person.layer(), C4Layer::Context);
        assert_eq!(ElementKind::SoftwareSystem.layer(), C4Layer::Context);
        assert_eq!(ElementKind::Container.layer(), C4Layer::Container);
        assert_eq!(ElementKind::Component.layer(), C4Layer::Component);
        assert_eq!(ElementKind::Code.layer(), C4Layer::Code);
        assert_eq!(
            ElementKind::SoftwareSystem.drill_layer(),
            Some(C4Layer::Container)
        );
        assert_eq!(
            ElementKind::Container.drill_layer(),
            Some(C4Layer::Component)
        );
        assert_eq!(ElementKind::Component.drill_layer(), Some(C4Layer::Code));
        assert_eq!(ElementKind::Code.drill_layer(), None);
        assert_eq!(ElementKind::Person.drill_layer(), None);
        assert_eq!(
            ElementKind::SoftwareSystem.child_kinds(),
            &[ElementKind::Container]
        );
        assert_eq!(
            ElementKind::Container.child_kinds(),
            &[ElementKind::Component]
        );
        assert_eq!(ElementKind::Component.child_kinds(), &[ElementKind::Code]);
        assert!(ElementKind::Person.child_kinds().is_empty());
        assert!(ElementKind::Code.child_kinds().is_empty());
        assert_eq!(
            C4Layer::Context.element_kinds(),
            &[
                ElementKind::Person,
                ElementKind::SoftwareSystem,
                ElementKind::External
            ]
        );
        assert_eq!(
            C4Layer::Landscape.element_kinds(),
            &[
                ElementKind::Person,
                ElementKind::SoftwareSystem,
                ElementKind::External
            ]
        );
        assert_eq!(ElementKind::External.layer(), C4Layer::Context);
        assert!(ElementKind::Code.is_code_atom());
        assert!(ElementKind::External.is_canon_rel_endpoint());
        assert!(!ElementKind::Container.is_canon_rel_endpoint());
        assert_eq!(
            AtomStereotype::parse("Interface"),
            Some(AtomStereotype::Interface)
        );
        assert_eq!(
            ExternalRole::parse("datastore"),
            Some(ExternalRole::Datastore)
        );
        assert_eq!(
            C4Layer::Container.element_kinds(),
            &[ElementKind::Container]
        );
        assert_eq!(
            C4Layer::Component.element_kinds(),
            &[ElementKind::Component]
        );
        assert_eq!(C4Layer::Code.element_kinds(), &[ElementKind::Code]);
        assert!(C4Layer::Adr.element_kinds().is_empty());
        assert!(C4Layer::parse("nope").is_err());
    }

    #[test]
    fn decision_status_case_insensitive() {
        assert_eq!(
            DecisionStatus::parse("Accepted").unwrap(),
            DecisionStatus::Accepted
        );
        assert!(DecisionStatus::parse("maybe").is_err());
    }

    #[test]
    fn adr_json_denies_unknown_fields() {
        let raw = r#"{
          "id": "a1",
          "title": "T",
          "status": "draft",
          "decided_at": "2026-07-17",
          "context": "c",
          "decision": "d",
          "consequences": "x",
          "content_md": "nope"
        }"#;
        let err = serde_json::from_str::<Decision>(raw).unwrap_err();
        assert!(
            err.to_string().contains("unknown field") || err.to_string().contains("content_md")
        );
    }

    #[test]
    fn adr_policy_roundtrip() {
        let d = Decision {
            id: "a1".into(),
            workspace_id: "w".into(),
            scope_element_id: None,
            title: "T".into(),
            status: DecisionStatus::Draft,
            decided_at: "2026-07-17".into(),
            context: "c".into(),
            decision: "d".into(),
            consequences: "x".into(),
            related_flows: vec![],
            refs: vec![],
            policy: Some(AdrPolicy {
                mode: PolicyMode::Audit,
                forbid: vec![AdrForbidRule {
                    from_kind: ElementKind::Person,
                    to_kind: ElementKind::Code,
                    code: "person_to_code".into(),
                    severity: Severity::Error,
                    message: "no".into(),
                }],
            }),
            reason: None,
            superseded_by_id: None,
            path: String::new(),
            git_commit_id: None,
        };
        let v = serde_json::to_string(&d).unwrap();
        let back: Decision = serde_json::from_str(&v).unwrap();
        assert_eq!(back.policy.as_ref().unwrap().mode, PolicyMode::Audit);
        assert_eq!(PolicyMode::parse("enforce").unwrap(), PolicyMode::Enforce);
        assert_eq!(PolicyMode::Audit.as_str(), "audit");
        assert!(PolicyMode::parse("x").is_err());
        let p = Problem::new(Severity::Error, C4Layer::Context, "c", None, "m");
        assert!(p.adr_id.is_none());
    }

    #[test]
    fn problem_serializes() {
        let p = Problem {
            severity: Severity::Warning,
            layer: C4Layer::Adr,
            code: "system.missing_decisions".into(),
            element_id: Some("billing".into()),
            message: "missing ADRs".into(),
            adr_id: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["severity"], "warning");
        assert_eq!(v["layer"], "adr");
    }

    #[test]
    fn all_as_str_and_parse_errors() {
        for k in [
            ElementKind::Person,
            ElementKind::SoftwareSystem,
            ElementKind::Container,
            ElementKind::Component,
            ElementKind::Code,
        ] {
            assert_eq!(ElementKind::parse(k.as_str()).unwrap(), k);
        }
        assert!(ElementKind::parse("x").is_err());
        assert!(ChangeKind::parse("x").is_err());
        for s in [Severity::Error, Severity::Warning, Severity::Info] {
            assert!(!s.as_str().is_empty());
        }
        for l in [
            C4Layer::Landscape,
            C4Layer::Context,
            C4Layer::Container,
            C4Layer::Component,
            C4Layer::Code,
            C4Layer::Adr,
        ] {
            assert_eq!(C4Layer::parse(l.as_str()).unwrap(), l);
        }
        for st in [
            DecisionStatus::Draft,
            DecisionStatus::Proposed,
            DecisionStatus::Accepted,
            DecisionStatus::Rejected,
            DecisionStatus::Deprecated,
            DecisionStatus::Superseded,
        ] {
            assert_eq!(DecisionStatus::parse(st.as_str()).unwrap(), st);
        }
        assert!(DecisionStatus::Draft.agent_writable());
        assert!(!DecisionStatus::Accepted.agent_writable());
        assert!(DecisionStatus::Accepted.enforces_policy());
        let err = DomainError::Conflict("c".into());
        assert!(err.to_string().contains("conflict"));
        let _ = DomainError::Message("m".into());
        let _ = DomainError::NotFound("n".into());
        let _ = DomainError::Validation("v".into());
    }
}
