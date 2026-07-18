//! Atomic TOML persist for sidecar `docs/` (model / adr / flows).
//!
//! ADR prose fields use TOML literal strings `'''…'''` (GFM, no raw HTML).

use std::fs;
use std::path::{Path, PathBuf};

use architect_c4_domain::{Decision, Element, Flow, Relationship};
use serde::{Deserialize, Serialize};

pub fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|s| s.to_str()).unwrap_or("toml")
    ));
    fs::write(&tmp, contents.as_bytes()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn escape_literal(s: &str) -> String {
    // TOML literal ''' cannot contain ''' — break if needed.
    s.replace("'''", "''\\'''")
}

fn lit(s: &str) -> String {
    format!("'''\n{}'''", escape_literal(s))
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Write ADR as TOML with `'''` prose fields. Path = docs/adr/{id}.toml relative form.
pub fn write_adr_toml(path: &Path, d: &Decision) -> Result<(), String> {
    let mut out = String::new();
    out.push_str(&format!("id = {}\n", q(&d.id)));
    if !d.workspace_id.is_empty() {
        out.push_str(&format!("workspace_id = {}\n", q(&d.workspace_id)));
    }
    out.push_str(&format!("title = {}\n", q(&d.title)));
    out.push_str(&format!("status = {}\n", q(d.status.as_str())));
    out.push_str(&format!("decided_at = {}\n", q(&d.decided_at)));
    if let Some(scope) = &d.scope_element_id {
        out.push_str(&format!("scope_element_id = {}\n", q(scope)));
    }
    out.push_str(&format!("context = {}\n", lit(&d.context)));
    out.push_str(&format!("decision = {}\n", lit(&d.decision)));
    out.push_str(&format!("consequences = {}\n", lit(&d.consequences)));
    if let Some(reason) = &d.reason {
        out.push_str(&format!("reason = {}\n", lit(reason)));
    }
    if let Some(sid) = &d.superseded_by_id {
        out.push_str(&format!("superseded_by_id = {}\n", q(sid)));
    }
    if !d.related_flows.is_empty() {
        out.push_str("related_flows = [");
        out.push_str(
            &d.related_flows
                .iter()
                .map(|x| q(x))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]\n");
    }
    if !d.path.is_empty() {
        out.push_str(&format!("path = {}\n", q(&d.path)));
    }
    if let Some(gid) = &d.git_commit_id {
        out.push_str(&format!("git_commit_id = {}\n", q(gid)));
    }
    if let Some(pol) = &d.policy {
        let raw = toml::to_string(pol).map_err(|e| e.to_string())?;
        out.push_str("\n[policy]\n");
        out.push_str(&raw);
    }
    for (i, r) in d.refs.iter().enumerate() {
        out.push_str("\n[[refs]]\n");
        out.push_str(&format!("url = {}\n", q(&r.url)));
        if let Some(t) = &r.title {
            out.push_str(&format!("title = {}\n", q(t)));
        }
        let _ = i;
    }
    atomic_write(path, &out)
}

pub fn read_adr_toml(path: &Path) -> Result<Decision, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&raw).map_err(|e| e.to_string())
}

pub fn write_flow_toml(path: &Path, flow: &Flow) -> Result<(), String> {
    let raw = toml::to_string_pretty(flow).map_err(|e| e.to_string())?;
    atomic_write(path, &raw)
}

pub fn read_flow_toml(path: &Path) -> Result<Flow, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&raw).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelFile {
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub elements: Vec<Element>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

pub fn write_model_toml(path: &Path, model: &ModelFile) -> Result<(), String> {
    let raw = toml::to_string_pretty(model).map_err(|e| e.to_string())?;
    atomic_write(path, &raw)
}

pub fn read_model_toml(path: &Path) -> Result<ModelFile, String> {
    if !path.is_file() {
        return Ok(ModelFile::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(ModelFile::default());
    }
    toml::from_str(&raw).map_err(|e| e.to_string())
}

/// One-shot: `docs/adr/*.json` → `.toml`, remove JSON. Returns count rewritten.
pub fn rewrite_legacy_adrs(docs_dir: &Path) -> Result<usize, String> {
    let adr_dir = docs_dir.join("adr");
    if !adr_dir.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in fs::read_dir(&adr_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut d: Decision = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let toml_path = path.with_extension("toml");
        d.path = format!("docs/adr/{}.toml", d.id);
        write_adr_toml(&toml_path, &d)?;
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// One-shot: `docs/flows/*.json` → `.toml`.
pub fn rewrite_legacy_flows(docs_dir: &Path) -> Result<usize, String> {
    let dir = docs_dir.join("flows");
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut f: Flow = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let toml_path = path.with_extension("toml");
        f.path = format!("docs/flows/{}.toml", f.id);
        write_flow_toml(&toml_path, &f)?;
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

pub fn ensure_docs_layout(docs_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(docs_dir.join("adr")).map_err(|e| e.to_string())?;
    fs::create_dir_all(docs_dir.join("flows")).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn repo_root_from_docs(docs_dir: &Path) -> PathBuf {
    docs_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| docs_dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect_c4_domain::DecisionStatus;
    use tempfile::tempdir;

    #[test]
    fn adr_roundtrip_uses_literal_prose() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("0001.toml");
        let d = Decision {
            id: "0001".into(),
            workspace_id: "ws".into(),
            scope_element_id: Some("sys".into()),
            title: "T".into(),
            status: DecisionStatus::Draft,
            decided_at: "2026-07-16".into(),
            context: "line1\n\n| a | b |\n|---|---|\n| 1 | 2 |".into(),
            decision: "use toml".into(),
            consequences: "ok".into(),
            policy: None,
            related_flows: vec![],
            refs: vec![],
            reason: None,
            superseded_by_id: None,
            path: "docs/adr/0001.toml".into(),
            git_commit_id: None,
        };
        write_adr_toml(&path, &d).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("context = '''"));
        assert!(raw.contains("| a | b |"));
        let back = read_adr_toml(&path).unwrap();
        assert_eq!(back.context, d.context);
        assert_eq!(back.decision, d.decision);
    }

    #[test]
    fn rewrite_json_adr() {
        let dir = tempdir().unwrap();
        let docs = dir.path().join("docs");
        fs::create_dir_all(docs.join("adr")).unwrap();
        let json_path = docs.join("adr/x.json");
        fs::write(
            &json_path,
            r#"{
              "id":"x","title":"X","status":"draft","decided_at":"2026-07-16",
              "context":"c","decision":"d","consequences":"k"
            }"#,
        )
        .unwrap();
        assert_eq!(rewrite_legacy_adrs(&docs).unwrap(), 1);
        assert!(!json_path.exists());
        assert!(docs.join("adr/x.toml").is_file());
    }

    #[test]
    fn model_and_flow_roundtrip_and_layout() {
        let dir = tempdir().unwrap();
        let docs = dir.path().join("docs");
        ensure_docs_layout(&docs).unwrap();
        assert!(docs.join("adr").is_dir());
        assert!(docs.join("flows").is_dir());
        assert_eq!(repo_root_from_docs(&docs), dir.path());

        let model_path = docs.join("model.toml");
        let model = ModelFile {
            workspace_id: "w".into(),
            elements: vec![],
            relationships: vec![],
        };
        write_model_toml(&model_path, &model).unwrap();
        let back = read_model_toml(&model_path).unwrap();
        assert!(back.elements.is_empty());

        let flow = Flow {
            id: "f1".into(),
            workspace_id: "w".into(),
            title: "T".into(),
            kind: architect_c4_domain::FlowKind::Sequence,
            usage_key: None,
            scope_element_id: None,
            related_adrs: vec![],
            epoch: None,
            steps: vec![],
            body: Some("sequenceDiagram\nA->>B: hi".into()),
            anchors: vec![],
            refs: vec![],
            path: "docs/flows/f1.toml".into(),
            git_commit_id: None,
        };
        let flow_path = docs.join("flows/f1.toml");
        write_flow_toml(&flow_path, &flow).unwrap();
        assert_eq!(read_flow_toml(&flow_path).unwrap().title, "T");

        let json_path = docs.join("flows/legacy.json");
        fs::write(
            &json_path,
            r#"{"id":"legacy","workspace_id":"w","title":"L","kind":"sequence","steps":[],"body":"sequenceDiagram\n"}"#,
        )
        .unwrap();
        assert_eq!(rewrite_legacy_flows(&docs).unwrap(), 1);
        assert!(!json_path.exists());
        assert!(docs.join("flows/legacy.toml").is_file());
    }
}
