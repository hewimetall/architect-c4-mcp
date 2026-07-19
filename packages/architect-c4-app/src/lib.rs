//! Composition root + thin PyO3 façade (Python stays slim).
//!
//! Persist on disk for the product repo: `docs/**/*.toml` only.
//! SQLite indexes are **in-memory** (never written into the product git tree).
//! Mutations that touch docs go through [`architect_c4_queue::WriteQueue`].

use std::path::PathBuf;
use std::sync::Arc;

use architect_c4_adr::AdrService;
use architect_c4_domain::ports::{
    AdrPort, ElementExistsPort, FlowPort, GitPort, ModelPort, SessionPort,
};
use architect_c4_domain::{
    project_relationships, C4Layer, Decision, DecisionStatus, Element, ElementKind, Flow,
    Relationship,
};
use architect_c4_flow::FlowService;
use architect_c4_git::GixGitAdapter;
use architect_c4_model::SqliteModelStore;
use architect_c4_policy::{blocks_write, check_parent, check_relationship, scan_model};
use architect_c4_queue::WriteQueue;
use architect_c4_render::{
    adr_detail_html, adrs_index_html, all_layers_mermaid, diagram_for_layer, flow_detail_html,
    flow_to_mermaid, flows_index_html, normalize_public_base, overview_mermaid,
    scene_json_for_view, view_html, view_links, DiagramInput,
};
use architect_c4_revision::SqliteRevisionStore;
use architect_c4_scene::ViewMode;
use architect_c4_session::SqliteSessionStore;
use architect_c4_tomlio::{
    ensure_docs_layout, read_adr_toml, read_flow_toml, read_model_toml, repo_root_from_docs,
    rewrite_legacy_adrs, rewrite_legacy_flows, write_model_toml, ModelFile,
};
use architect_c4_validate::{validate_model, ModelSnapshot};
use parking_lot::Mutex;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::json;

const WS: &str = "sidecar";

struct AppState {
    sessions: SqliteSessionStore,
    model: Arc<SqliteModelStore>,
    adr: AdrService,
    flows: FlowService,
    #[allow(dead_code)]
    revisions: Arc<SqliteRevisionStore>,
    queue: Arc<WriteQueue>,
    /// Absolute docs/ directory bound to the sidecar workspace.
    docs_dir: Mutex<Option<PathBuf>>,
}

static APP: Mutex<Option<Arc<AppState>>> = Mutex::new(None);

fn state() -> PyResult<Arc<AppState>> {
    APP.lock()
        .clone()
        .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("call init(data_dir) first"))
}

fn map_err(e: architect_c4_domain::DomainError) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(e.to_string())
}

fn persist_model_toml(s: &AppState) -> Result<(), architect_c4_domain::DomainError> {
    let docs = s.docs_dir.lock().clone().ok_or_else(|| {
        architect_c4_domain::DomainError::Message(
            "sidecar has no docs bind (call bind_docs)".into(),
        )
    })?;
    let model = ModelFile {
        workspace_id: WS.into(),
        elements: s.model.list_elements(WS)?,
        relationships: s.model.list_relationships(WS)?,
    };
    write_model_toml(&docs.join("model.toml"), &model)
        .map_err(architect_c4_domain::DomainError::Message)
}

#[pyfunction]
fn init(data_dir: &str) -> PyResult<()> {
    // data_dir kept for API/tests; indexes are in-memory — no *.db in product repo.
    let root = PathBuf::from(data_dir);
    let _ = std::fs::create_dir_all(&root);
    let rev = Arc::new(SqliteRevisionStore::open_in_memory().map_err(map_err)?);
    let sessions = SqliteSessionStore::open_in_memory().map_err(map_err)?;
    let model = Arc::new(SqliteModelStore::open_in_memory(rev.clone()).map_err(map_err)?);
    let git = Arc::new(GixGitAdapter::new());
    let git_port: Arc<dyn GitPort> = git.clone();
    let elements: Arc<dyn ElementExistsPort> = model.clone();
    let adr = AdrService::open_in_memory(rev.clone(), git_port.clone(), elements.clone())
        .map_err(map_err)?;
    let flows = FlowService::open_in_memory(rev.clone(), git_port, elements).map_err(map_err)?;
    let queue = Arc::new(WriteQueue::start());
    let app = Arc::new(AppState {
        sessions,
        model,
        adr,
        flows,
        revisions: rev,
        queue,
        docs_dir: Mutex::new(None),
    });
    *APP.lock() = Some(app.clone());

    // Sidecar auto-bind: ARCHITECT_C4_DOCS only.
    if let Ok(docs) = std::env::var("ARCHITECT_C4_DOCS") {
        let docs = PathBuf::from(docs);
        bind_docs_inner(&app, &docs).map_err(map_err)?;
    }
    Ok(())
}

fn bind_docs_inner(
    s: &AppState,
    docs_dir: &std::path::Path,
) -> Result<serde_json::Value, architect_c4_domain::DomainError> {
    let docs_dir = docs_dir
        .canonicalize()
        .unwrap_or_else(|_| docs_dir.to_path_buf());
    ensure_docs_layout(&docs_dir).map_err(architect_c4_domain::DomainError::Message)?;
    let n_adr =
        rewrite_legacy_adrs(&docs_dir).map_err(architect_c4_domain::DomainError::Message)?;
    let n_flow =
        rewrite_legacy_flows(&docs_dir).map_err(architect_c4_domain::DomainError::Message)?;
    let repo_root = repo_root_from_docs(&docs_dir);
    // Workspace row (path = repo root for gix commits of docs/…)
    if s.sessions.get_workspace(WS).is_err() {
        let _ = s.sessions.create_workspace(
            WS,
            "docs-sidecar",
            "main",
            &repo_root.to_string_lossy(),
        )?;
    }
    s.adr.bind_worktree(WS, repo_root.clone());
    s.flows.bind_worktree(WS, repo_root);
    *s.docs_dir.lock() = Some(docs_dir.clone());

    // Load model.toml into memory index
    let model_file = read_model_toml(&docs_dir.join("model.toml"))
        .map_err(architect_c4_domain::DomainError::Message)?;
    for mut el in model_file.elements {
        el.workspace_id = WS.into();
        let _ = s.model.upsert_element(el)?;
    }
    for mut rel in model_file.relationships {
        rel.workspace_id = WS.into();
        let _ = s.model.upsert_relationship(rel)?;
    }
    // Load ADR/flow toml into SQL index (commit=false — files already on disk)
    if docs_dir.join("adr").is_dir() {
        for entry in std::fs::read_dir(docs_dir.join("adr"))
            .map_err(|e| architect_c4_domain::DomainError::Message(e.to_string()))?
        {
            let entry =
                entry.map_err(|e| architect_c4_domain::DomainError::Message(e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let mut d = read_adr_toml(&path).map_err(architect_c4_domain::DomainError::Message)?;
            d.workspace_id = WS.into();
            let _ = s.adr.import_from_disk(d)?;
        }
    }
    if docs_dir.join("flows").is_dir() {
        for entry in std::fs::read_dir(docs_dir.join("flows"))
            .map_err(|e| architect_c4_domain::DomainError::Message(e.to_string()))?
        {
            let entry =
                entry.map_err(|e| architect_c4_domain::DomainError::Message(e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let mut f = read_flow_toml(&path).map_err(architect_c4_domain::DomainError::Message)?;
            f.workspace_id = WS.into();
            let _ = s.flows.upsert_flow(f, false)?;
        }
    }

    Ok(json!({
        "docs": docs_dir,
        "rewrote_adr_json": n_adr,
        "rewrote_flow_json": n_flow,
    }))
}

/// Bind workspace to a host `docs/` directory (sidecar happy path).
#[pyfunction]
fn bind_docs(docs_dir: &str) -> PyResult<String> {
    let s = state()?;
    let docs = PathBuf::from(docs_dir);
    let s2 = s.clone();
    let out = s
        .queue
        .submit(move || {
            let v = bind_docs_inner(&s2, &docs)?;
            Ok(v.to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn upsert_element(
    id: &str,
    kind: &str,
    name: &str,
    parent_id: Option<&str>,
    description: Option<&str>,
    technology: Option<&str>,
    url: Option<&str>,
    members_json: Option<&str>,
) -> PyResult<String> {
    let s = state()?;
    let members: Vec<architect_c4_domain::CodeMember> = match members_json {
        None | Some("") => Vec::new(),
        Some(raw) => serde_json::from_str(raw)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("members JSON: {e}")))?,
    };
    for m in &members {
        m.validate().map_err(map_err)?;
    }
    let kind_parsed = ElementKind::parse(kind).map_err(map_err)?;
    if !members.is_empty() && kind_parsed != ElementKind::Code {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "members only allowed for kind=code",
        ));
    }
    let el = Element {
        id: id.into(),
        workspace_id: WS.into(),
        kind: kind_parsed,
        parent_id: parent_id.map(str::to_string),
        name: name.into(),
        description: description.map(str::to_string),
        technology: technology.map(str::to_string),
        url: url.map(str::to_string),
        members,
    };
    if let Some(ref pid) = el.parent_id {
        let parent = s.model.get_element(WS, pid).ok();
        let problems = check_parent(&el, parent.as_ref());
        if !problems.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                problems
                    .iter()
                    .map(|p| p.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
    } else {
        let problems = check_parent(&el, None);
        if !problems.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                problems
                    .iter()
                    .map(|p| p.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
    }
    let s2 = s.clone();
    let out = s
        .queue
        .submit(move || {
            let saved = s2.model.upsert_element(el)?;
            let _ = persist_model_toml(&s2);
            Ok(serde_json::to_string(&saved).unwrap())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn upsert_relationship(
    id: &str,
    from_id: &str,
    to_id: &str,
    description: Option<&str>,
) -> PyResult<String> {
    let s = state()?;
    let from = s.model.get_element(WS, from_id).map_err(map_err)?;
    let to = s.model.get_element(WS, to_id).map_err(map_err)?;
    let adrs = s.adr.list_decisions(WS).map_err(map_err)?;
    let problems = check_relationship(&from, &to, id, &adrs);
    if blocks_write(&problems, &adrs) {
        let msg = problems
            .iter()
            .map(|p| {
                if let Some(a) = &p.adr_id {
                    format!("{} (adr={a})", p.message)
                } else {
                    p.message.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(pyo3::exceptions::PyValueError::new_err(msg));
    }
    let rel = Relationship {
        id: id.into(),
        workspace_id: WS.into(),
        from_id: from_id.into(),
        to_id: to_id.into(),
        description: description.map(str::to_string),
        technology: None,
    };
    let s2 = s.clone();
    let out = s
        .queue
        .submit(move || {
            let saved = s2.model.upsert_relationship(rel)?;
            let _ = persist_model_toml(&s2);
            Ok(serde_json::to_string(&saved).unwrap())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn delete_relationship(id: &str) -> PyResult<String> {
    let s = state()?;
    let s2 = s.clone();
    let rid = id.to_string();
    let out = s
        .queue
        .submit(move || {
            s2.model.delete_relationship(WS, &rid)?;
            let _ = persist_model_toml(&s2);
            Ok(json!({ "deleted": rid }).to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn get_model() -> PyResult<String> {
    let s = state()?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let relationships = s.model.list_relationships(WS).map_err(map_err)?;
    let decisions = s.adr.list_decisions(WS).map_err(map_err)?;
    Ok(
        json!({ "elements": elements, "relationships": relationships, "decisions": decisions })
            .to_string(),
    )
}

#[pyfunction]
fn validate_workspace() -> PyResult<String> {
    let s = state()?;
    let snap = ModelSnapshot {
        elements: s.model.list_elements(WS).map_err(map_err)?,
        relationships: s.model.list_relationships(WS).map_err(map_err)?,
        decisions: s.adr.list_decisions(WS).map_err(map_err)?,
    };
    let mut problems = validate_model(&snap);
    problems.extend(scan_model(
        &snap.elements,
        &snap.relationships,
        &snap.decisions,
    ));
    Ok(json!({ "ok": problems.iter().all(|p| p.severity != architect_c4_domain::Severity::Error), "problems": problems }).to_string())
}

#[pyfunction]
fn upsert_adr(adr_json: &str, commit: bool) -> PyResult<String> {
    let s = state()?;
    let mut d: Decision = serde_json::from_str(adr_json)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    d.workspace_id = WS.into();
    let s2 = s.clone();
    let out = s
        .queue
        .submit(move || {
            let (d, cid) = s2.adr.upsert_decision(d, commit)?;
            let base = normalize_public_base(
                std::env::var("ARCHITECT_C4_PUBLIC_BASE")
                    .unwrap_or_else(|_| "https://c4.example.com".into())
                    .trim_end_matches('/'),
            )
            .unwrap_or_else(|_| "https://c4.example.com".into());
            let view_url = format!("{base}/view/adrs/{}", d.id);
            Ok(json!({ "decision": d, "commit_id": cid, "view_url": view_url }).to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn set_adr_status(
    id: &str,
    status: &str,
    reason: Option<&str>,
    superseded_by_id: Option<&str>,
    commit: bool,
) -> PyResult<String> {
    // Process gate: require token unless unset (dev/local).
    if let Ok(expected) = std::env::var("ARCHITECT_C4_PROCESS_TOKEN") {
        if expected.is_empty() {
            // empty env = allow (tests)
        } else {
            let provided = std::env::var("ARCHITECT_C4_PROCESS_TOKEN_PRESENT")
                .ok()
                .or_else(|| std::env::var("ARCHITECT_C4_CALLER_PROCESS_TOKEN").ok());
            // FastMCP passes token via env ARCHITECT_C4_CALLER_PROCESS_TOKEN from tool arg in Python
            match provided {
                Some(p) if p == expected => {}
                _ => {
                    return Err(pyo3::exceptions::PyPermissionError::new_err(
                        "set_adr_status requires process token (ARCHITECT_C4_PROCESS_TOKEN)",
                    ));
                }
            }
        }
    }
    let s = state()?;
    let st = DecisionStatus::parse(status).map_err(map_err)?;
    let s2 = s.clone();
    let id = id.to_string();
    let reason = reason.map(str::to_string);
    let superseded_by_id = superseded_by_id.map(str::to_string);
    let out = s
        .queue
        .submit(move || {
            let (d, cid) = s2.adr.set_decision_status(
                WS,
                &id,
                st,
                reason.as_deref(),
                superseded_by_id.as_deref(),
                commit,
            )?;
            Ok(json!({ "decision": d, "commit_id": cid }).to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn get_adr(id: &str) -> PyResult<String> {
    let s = state()?;
    let d = s.adr.get_decision(WS, id).map_err(map_err)?;
    Ok(serde_json::to_string(&d).unwrap())
}

fn require_base(base_url: &str) -> PyResult<String> {
    normalize_public_base(base_url).map_err(pyo3::exceptions::PyValueError::new_err)
}

#[pyfunction]
fn get_overview_diagram(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let relationships = s.model.list_relationships(WS).map_err(map_err)?;
    // V3: Context view shows projected edges (atom→system/external).
    let projected = project_relationships(&elements, &relationships, C4Layer::Context);
    let mermaid = overview_mermaid(&DiagramInput {
        elements: &elements,
        relationships: &projected,
        base_url: &base,
    });
    let view_url = format!("{base}/view?layer=context");
    Ok(json!({
        "format": "mermaid",
        "layer": "context",
        "view_url": view_url,
        "content": mermaid
    })
    .to_string())
}

#[pyfunction]
fn get_layer_diagram(layer: &str, parent_id: Option<&str>, base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let layer = C4Layer::parse(layer).map_err(map_err)?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let relationships = s.model.list_relationships(WS).map_err(map_err)?;
    let projected = project_relationships(&elements, &relationships, layer);
    let mermaid = diagram_for_layer(
        &DiagramInput {
            elements: &elements,
            relationships: &projected,
            base_url: &base,
        },
        layer,
        parent_id,
    );
    let view_url = match parent_id {
        Some(p) => format!("{base}/view?layer={}&parent={p}", layer.as_str()),
        None => format!("{base}/view?layer={}", layer.as_str()),
    };
    Ok(json!({
        "format": "mermaid",
        "layer": layer.as_str(),
        "parent_id": parent_id,
        "view_url": view_url,
        "content": mermaid
    })
    .to_string())
}

#[pyfunction]
fn render_view_html(
    layer: &str,
    parent_id: Option<&str>,
    base_url: &str,
    mode: &str,
    renderer: &str,
) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let view_mode = ViewMode::parse(mode);
    let layer_parsed = if view_mode == ViewMode::All {
        C4Layer::Context
    } else {
        C4Layer::parse(layer).map_err(map_err)?
    };
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let relationships = s.model.list_relationships(WS).map_err(map_err)?;
    // Layer Mermaid uses V3 projections; All Mermaid keeps atom edges (WASM bundles trunks).
    let projected = project_relationships(&elements, &relationships, layer_parsed);
    let mermaid = if view_mode == ViewMode::All {
        all_layers_mermaid(
            &DiagramInput {
                elements: &elements,
                relationships: &relationships,
                base_url: &base,
            },
            parent_id,
        )
    } else {
        diagram_for_layer(
            &DiagramInput {
                elements: &elements,
                relationships: &projected,
                base_url: &base,
            },
            layer_parsed,
            parent_id,
        )
    };
    let scene = scene_json_for_view(
        &elements,
        &relationships,
        view_mode,
        if view_mode == ViewMode::All {
            None
        } else {
            Some(layer_parsed.as_str())
        },
        parent_id,
    );
    let up_parent = parent_id.and_then(|pid| {
        elements
            .iter()
            .find(|e| e.id == pid)
            .and_then(|e| e.parent_id.as_deref())
    });
    let adrs = s.adr.list_decisions(WS).map_err(map_err)?;
    let flows_n = s.flows.list_flows(WS).map_err(map_err)?.len();
    Ok(view_html(
        WS,
        layer_parsed,
        parent_id,
        &mermaid,
        &base,
        up_parent,
        &elements,
        adrs.len(),
        flows_n,
        view_mode.as_str(),
        renderer,
        &scene,
    ))
}

#[pyfunction]
fn get_scene(mode: &str, layer: Option<&str>, focus: Option<&str>) -> PyResult<String> {
    let s = state()?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let relationships = s.model.list_relationships(WS).map_err(map_err)?;
    Ok(scene_json_for_view(
        &elements,
        &relationships,
        ViewMode::parse(mode),
        layer,
        focus,
    ))
}

#[pyfunction]
fn render_adrs_html(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let adrs = s.adr.list_decisions(WS).map_err(map_err)?;
    Ok(adrs_index_html(WS, &base, &adrs))
}

#[pyfunction]
fn render_adr_html(adr_id: &str, base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let d = s.adr.get_decision(WS, adr_id).map_err(map_err)?;
    Ok(adr_detail_html(WS, &base, &d))
}

#[pyfunction]
fn list_adrs(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let adrs = s.adr.list_decisions(WS).map_err(map_err)?;
    let rows: Vec<_> = adrs
        .into_iter()
        .map(|d| {
            let view_url = format!("{base}/view/adrs/{}", d.id);
            json!({
                "id": d.id,
                "workspace_id": d.workspace_id,
                "scope_element_id": d.scope_element_id,
                "title": d.title,
                "status": d.status.as_str(),
                "decided_at": d.decided_at,
                "context": d.context,
                "decision": d.decision,
                "consequences": d.consequences,
                "policy": d.policy,
                "related_flows": d.related_flows,
                "refs": d.refs,
                "reason": d.reason,
                "superseded_by_id": d.superseded_by_id,
                "path": d.path,
                "git_commit_id": d.git_commit_id,
                "view_url": view_url,
            })
        })
        .collect();
    Ok(serde_json::to_string(&rows).unwrap())
}

#[pyfunction]
fn upsert_flow(flow_json: &str, commit: bool) -> PyResult<String> {
    let s = state()?;
    let mut f: Flow = serde_json::from_str(flow_json)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    f.workspace_id = WS.into();
    let s2 = s.clone();
    let out = s
        .queue
        .submit(move || {
            let (f, cid) = s2.flows.upsert_flow(f, commit)?;
            let base = normalize_public_base(
                std::env::var("ARCHITECT_C4_PUBLIC_BASE")
                    .unwrap_or_else(|_| "https://c4.example.com".into())
                    .trim_end_matches('/'),
            )
            .unwrap_or_else(|_| "https://c4.example.com".into());
            let view_url = format!("{base}/view/flows/{}", f.id);
            Ok(json!({ "flow": f, "commit_id": cid, "view_url": view_url }).to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn get_flow(id: &str) -> PyResult<String> {
    let s = state()?;
    Ok(serde_json::to_string(&s.flows.get_flow(WS, id).map_err(map_err)?).unwrap())
}

#[pyfunction]
fn list_flows(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let flows = s.flows.list_flows(WS).map_err(map_err)?;
    let rows: Vec<_> = flows
        .into_iter()
        .map(|f| {
            let view_url = format!("{base}/view/flows/{}", f.id);
            json!({
                "id": f.id,
                "title": f.title,
                "kind": f.kind.as_str(),
                "usage_key": f.usage_key,
                "scope_element_id": f.scope_element_id,
                "related_adrs": f.related_adrs,
                "epoch": f.epoch,
                "path": f.path,
                "git_commit_id": f.git_commit_id,
                "view_url": view_url,
            })
        })
        .collect();
    Ok(json!({ "flows": rows }).to_string())
}

#[pyfunction]
fn delete_flow(id: &str, commit: bool) -> PyResult<String> {
    let s = state()?;
    let s2 = s.clone();
    let id = id.to_string();
    let out = s
        .queue
        .submit(move || {
            s2.flows.delete_flow(WS, &id, commit)?;
            Ok(json!({ "deleted": id }).to_string())
        })
        .map_err(map_err)?;
    Ok(out)
}

#[pyfunction]
fn get_flow_diagram(id: &str, base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let f = s.flows.get_flow(WS, id).map_err(map_err)?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let content = flow_to_mermaid(&f, &elements);
    let view_url = format!("{base}/view/flows/{}", f.id);
    Ok(
        json!({ "format": "mermaid", "content": content, "view_url": view_url, "flow": f })
            .to_string(),
    )
}

#[pyfunction]
fn render_flows_html(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let flows = s.flows.list_flows(WS).map_err(map_err)?;
    let adrs_n = s.adr.list_decisions(WS).map_err(map_err)?.len();
    Ok(flows_index_html(WS, &base, &flows, adrs_n))
}

#[pyfunction]
fn render_flow_html(flow_id: &str, base_url: &str) -> PyResult<String> {
    let s = state()?;
    let base = require_base(base_url)?;
    let f = s.flows.get_flow(WS, flow_id).map_err(map_err)?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let adrs_n = s.adr.list_decisions(WS).map_err(map_err)?.len();
    let flows_n = s.flows.list_flows(WS).map_err(map_err)?.len();
    Ok(flow_detail_html(WS, &base, &f, &elements, adrs_n, flows_n))
}

#[pyfunction]
fn get_view_links(base_url: &str) -> PyResult<String> {
    let s = state()?;
    let elements = s.model.list_elements(WS).map_err(map_err)?;
    let adrs = s.adr.list_decisions(WS).map_err(map_err)?;
    let mut v = view_links(WS, base_url, &elements, &adrs)
        .map_err(pyo3::exceptions::PyValueError::new_err)?;
    let flows = s.flows.list_flows(WS).map_err(map_err)?;
    let base = require_base(base_url)?;
    let flow_rows: Vec<_> = flows
        .iter()
        .map(|f| {
            json!({
                "id": f.id,
                "title": f.title,
                "kind": f.kind.as_str(),
                "view_url": format!("{base}/view/flows/{}", f.id),
            })
        })
        .collect();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("flows".into(), json!(flow_rows));
    }
    Ok(v.to_string())
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(init, m)?)?;
    m.add_function(wrap_pyfunction!(bind_docs, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_element, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_relationship, m)?)?;
    m.add_function(wrap_pyfunction!(delete_relationship, m)?)?;
    m.add_function(wrap_pyfunction!(get_model, m)?)?;
    m.add_function(wrap_pyfunction!(validate_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_adr, m)?)?;
    m.add_function(wrap_pyfunction!(set_adr_status, m)?)?;
    m.add_function(wrap_pyfunction!(get_adr, m)?)?;
    m.add_function(wrap_pyfunction!(list_adrs, m)?)?;
    m.add_function(wrap_pyfunction!(get_overview_diagram, m)?)?;
    m.add_function(wrap_pyfunction!(get_layer_diagram, m)?)?;
    m.add_function(wrap_pyfunction!(get_view_links, m)?)?;
    m.add_function(wrap_pyfunction!(render_view_html, m)?)?;
    m.add_function(wrap_pyfunction!(get_scene, m)?)?;
    m.add_function(wrap_pyfunction!(render_adrs_html, m)?)?;
    m.add_function(wrap_pyfunction!(render_adr_html, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_flow, m)?)?;
    m.add_function(wrap_pyfunction!(get_flow, m)?)?;
    m.add_function(wrap_pyfunction!(list_flows, m)?)?;
    m.add_function(wrap_pyfunction!(delete_flow, m)?)?;
    m.add_function(wrap_pyfunction!(get_flow_diagram, m)?)?;
    m.add_function(wrap_pyfunction!(render_flows_html, m)?)?;
    m.add_function(wrap_pyfunction!(render_flow_html, m)?)?;
    Ok(())
}
