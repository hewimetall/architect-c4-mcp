"""TDD: slim FastMCP wiring against real Rust native + server tools."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from architect_c4 import native, server


@pytest.fixture()
def data_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    # reset init flag
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    native.init(str(tmp_path))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    return tmp_path


def test_native_session_project_workspace_adr_flow(data_dir: Path):
    sess = json.loads(native.create_session("t"))
    assert "id" in sess
    native.create_project("demo")
    ws = json.loads(native.checkout_workspace(sess["id"], "demo", "main", "ws1"))
    assert ws["id"] == "ws1"
    native.upsert_element("ws1", "user", "person", "User", None, "A user", None, None, None)
    native.upsert_element(
        "ws1", "sys", "software_system", "Billing", None, "Billing system", None, None, None
    )
    native.upsert_element("ws1", "api", "container", "API", "sys", "HTTP API", "Python", None, None)
    native.upsert_relationship("ws1", "r1", "user", "sys", "Uses")
    v = json.loads(native.validate_workspace("ws1"))
    assert any(p["code"] == "system.missing_decisions" for p in v["problems"])
    adr = json.loads(
        native.upsert_adr(
            "ws1",
            json.dumps({
                "id": "0001-use-sqlite",
                "title": "Use SQLite",
                "status": "proposed",
                "decided_at": "2026-07-16",
                "scope_element_id": "sys",
                "context": "Need embedded persistence for the model.",
                "decision": "Use SQLite for model storage.",
                "consequences": "Ops must back up workspace databases.",
            }),
            True,
        )
    )
    assert adr.get("commit_id") or adr["decision"]["path"].endswith(".toml")
    native.set_adr_status("ws1", "0001-use-sqlite", "accepted", None, None, True)
    assert (data_dir / "workspaces" / "ws1" / "docs" / "adr" / "0001-use-sqlite.toml").is_file()
    assert (data_dir / "workspaces" / "ws1" / "docs" / "model.toml").is_file()
    v2 = json.loads(native.validate_workspace("ws1"))
    assert not any(p["code"] == "system.missing_decisions" for p in v2["problems"])
    diagram = json.loads(native.get_overview_diagram("ws1", "https://ex.com"))
    assert "C4Context" in diagram["content"]
    assert "Person(" in diagram["content"] or "System(" in diagram["content"]
    assert len(json.loads(native.list_adrs("ws1", "https://c4.example.com"))) == 1


def test_server_tools_cover_facade(data_dir: Path):
    sess = server.create_session("meta")
    assert sess["id"]
    assert server.get_session(sess["id"])["id"] == sess["id"]
    assert "sessions" in server.list_sessions()
    assert server.create_project("p1")["project_id"] == "p1"
    ws = server.checkout_workspace(sess["id"], "p1", "main", "wsA")
    wid = ws["id"]
    server.upsert_element(wid, "u", "person", "User", description="d")
    server.upsert_element(wid, "s", "software_system", "Sys", description="d")
    server.upsert_relationship(wid, "r", "u", "s", "Uses")
    model = server.get_model(wid)
    assert len(model["elements"]) == 2
    problems = server.validate_model(wid)
    assert "problems" in problems
    out = server.upsert_adr(
        wid,
        {
            "id": "0001-record-adrs",
            "title": "Record ADRs",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "s",
            "context": "Need decision log.",
            "decision": "Record ADRs as structured JSON.",
            "consequences": "Agents use upsert_adr.",
        },
        commit=True,
    )
    server.set_adr_status(wid, "0001-record-adrs", "accepted", commit=True)
    assert out["decision"]["path"].endswith(".toml")
    assert server.list_adrs(wid)["adrs"]
    assert "mermaid" in server.get_overview_diagram(wid)["format"]


def test_server_module_main_and_mcp_name():
    assert server.mcp.name == "architect-c4"
    assert callable(server.main)


def test_ensure_init_reads_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "d"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    server._ensure_init()
    assert (tmp_path / "d").is_dir()
    server._ensure_init()  # idempotent


def test_main_invokes_mcp_run(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    called = {"n": 0}

    def fake_run():
        called["n"] += 1

    monkeypatch.setattr(server.mcp, "run", fake_run)
    server.main()
    assert called["n"] == 1


def test_reject_dangling_relationship_and_delete(data_dir: Path):
    sess = server.create_session("inc")
    server.create_project("inc-p")
    ws = server.checkout_workspace(sess["id"], "inc-p", "main", "ws-inc")
    wid = ws["id"]
    server.upsert_element(wid, "a", "person", "A", description="d")
    with pytest.raises(ValueError) as ei:
        server.upsert_relationship(wid, "r-bad", "a", "missing", "x")
    assert (
        "to_id" in str(ei.value)
        or "does not exist" in str(ei.value)
        or "not found" in str(ei.value).lower()
    )
    server.upsert_element(wid, "b", "software_system", "B", description="d")
    server.upsert_relationship(wid, "r-ok", "a", "b", "uses")
    assert server.delete_relationship(wid, "r-ok")["deleted"] == "r-ok"
    with pytest.raises(ValueError):
        server.upsert_adr(
            wid,
            {
                "id": "0009-bad",
                "title": "Bad",
                "status": "proposed",
                "decided_at": "2026-07-16",
                "scope_element_id": "nope",
                "context": "Invalid scope.",
                "decision": "Should fail.",
                "consequences": "Rejected by validation.",
            },
            commit=False,
        )


def test_policy_blocks_person_to_code_and_adr_reject_reason(data_dir: Path):
    sess = server.create_session("pol")
    server.create_project("pol-p")
    ws = server.checkout_workspace(sess["id"], "pol-p", "main", "ws-pol")
    wid = ws["id"]
    server.upsert_element(wid, "u", "person", "User", description="d")
    server.upsert_element(wid, "s", "software_system", "Sys", description="d")
    server.upsert_element(
        wid, "api", "container", "API", parent_id="s", description="d", technology="Go"
    )
    server.upsert_element(
        wid, "cmp", "component", "Cmp", parent_id="api", description="d", technology="Go"
    )
    server.upsert_element(
        wid, "cls", "code", "Cls", parent_id="cmp", description="+m()", technology="Go"
    )
    with pytest.raises(ValueError) as ei:
        server.upsert_relationship(wid, "bad", "u", "cls", "hack")
    assert "not allowed" in str(ei.value).lower() or "baseline" in str(ei.value).lower()

    server.upsert_adr(
        wid,
        {
            "id": "pol-1",
            "title": "No container links",
            "status": "proposed",
            "decided_at": "2026-07-17",
            "context": "Cross-container coupling.",
            "decision": "Forbid container→container.",
            "consequences": "Use system APIs.",
            "policy": {
                "mode": "enforce",
                "forbid": [
                    {
                        "from_kind": "container",
                        "to_kind": "container",
                        "code": "no_container_links",
                        "severity": "error",
                        "message": "no container to container",
                    }
                ],
            },
        },
        commit=False,
    )
    server.set_adr_status(wid, "pol-1", "accepted", commit=False)
    server.upsert_element(
        wid, "api2", "container", "API2", parent_id="s", description="d", technology="Go"
    )
    with pytest.raises(ValueError) as ei2:
        server.upsert_relationship(wid, "c2c", "api", "api2", "talks")
    assert "no container" in str(ei2.value).lower() or "adr=" in str(ei2.value).lower()

    with pytest.raises(ValueError):
        server.set_adr_status(wid, "pol-1", "rejected", reason=None, commit=False)

    # new draft for reject path
    server.upsert_adr(
        wid,
        {
            "id": "pol-2",
            "title": "Reject me",
            "status": "draft",
            "decided_at": "2026-07-17",
            "context": "c",
            "decision": "d",
            "consequences": "x",
        },
        commit=False,
    )
    out = server.set_adr_status(
        wid, "pol-2", "rejected", reason="Not durable enough", commit=False
    )
    assert out["decision"]["reason"] == "Not durable enough"
    assert server.get_adr(wid, "pol-2")["status"] == "rejected"


def test_base_url_https_only():
    assert server._base_url(None).startswith("https://")
    assert server._base_url("https://c4.example.com/").endswith("c4.example.com")
    with pytest.raises(ValueError):
        server._base_url("http://insecure.example")
    with pytest.raises(ValueError):
        server._base_url("javascript:alert(1)")
    with pytest.raises(ValueError):
        server._base_url("https://user@evil")


def test_http_view_routes(data_dir: Path):
    import asyncio

    sess = server.create_session("http")
    server.create_project("httpp")
    ws = server.checkout_workspace(sess["id"], "httpp", "main", "ws-http")
    wid = ws["id"]
    server.upsert_element(wid, "sys", "software_system", "Sys", description="d")
    server.upsert_element(
        wid, "api", "container", "API", parent_id="sys", description="d", technology="Go"
    )
    server.upsert_adr(
        wid,
        {
            "id": "adr1",
            "title": "Pick Go",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "sys",
            "context": "Need API language.",
            "decision": "Use Go.",
            "consequences": "Hire Go engineers.",
        },
        commit=True,
    )
    server.set_adr_status(wid, "adr1", "accepted", commit=True)

    class Req:
        def __init__(self, path_params, query_params=None):
            self.path_params = path_params
            self.query_params = query_params or {}

    def body_text(resp) -> str:
        raw = resp.body
        if isinstance(raw, memoryview):
            raw = raw.tobytes()
        if isinstance(raw, (bytes, bytearray)):
            return raw.decode()
        return str(raw)

    async def _run():
        health = await server.health(Req({}))
        assert health.media_type == "text/plain"
        assert body_text(health) == "ok"

        view = await server.c4_view(
            Req({"workspace_id": wid}, {"layer": "component", "parent": "api"})
        )
        assert view.status_code == 200
        assert "C4Component" in body_text(view) or "No components yet" in body_text(view)
        assert view.headers.get("cache-control", "").startswith("no-store")
        assert "top-tabs" in body_text(view)
        assert "app-shell" in body_text(view)

        adrs = await server.c4_adrs_index(Req({"workspace_id": wid}))
        assert adrs.status_code == 200
        assert "Pick Go" in body_text(adrs)
        assert adrs.headers.get("cache-control", "").startswith("no-store")
        assert "top-tabs" in body_text(adrs)
        assert "legend-panel" in body_text(adrs)
        detail = await server.c4_adr_detail(Req({"workspace_id": wid, "adr_id": "adr1"}))
        assert detail.status_code == 200
        missing = await server.c4_adr_detail(Req({"workspace_id": wid, "adr_id": "nope"}))
        assert missing.status_code == 404

        bad = await server.c4_view(Req({"workspace_id": wid}, {"layer": "not-a-layer"}))
        assert bad.status_code == 400

        all_view = await server.c4_view(
            Req({"workspace_id": wid}, {"mode": "all", "renderer": "mermaid"})
        )
        assert all_view.status_code == 200
        assert "All" in body_text(all_view)
        assert all_view.headers.get("cache-control", "").startswith("no-store")

        wasm_view = await server.c4_view(
            Req({"workspace_id": wid}, {"mode": "all", "renderer": "wasm"})
        )
        assert wasm_view.status_code == 200
        assert "c4-canvas" in body_text(wasm_view)
        assert "/wasm/architect_c4_wasm.js" in body_text(wasm_view)

        # static wasm asset
        class WasmReq:
            def __init__(self):
                self.path_params = {"path": "architect_c4_wasm.js"}

        asset = await server.wasm_static(WasmReq())
        assert asset.status_code == 200

        class BadWasmReq:
            def __init__(self):
                self.path_params = {"path": "../x"}

        bad_asset = await server.wasm_static(BadWasmReq())
        assert bad_asset.status_code == 400

    asyncio.run(_run())


def test_main_http_transport(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    monkeypatch.setenv("ARCHITECT_C4_TRANSPORT", "http")
    monkeypatch.setenv("ARCHITECT_C4_HOST", "127.0.0.1")
    monkeypatch.setenv("ARCHITECT_C4_PORT", "18765")
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    called = {}

    def fake_run(**kwargs):
        called.update(kwargs)

    monkeypatch.setattr(server.mcp, "run", fake_run)
    server.main()
    assert called.get("transport") == "http"
    assert called.get("port") == 18765


def test_layer_diagrams_all_c4_levels(data_dir: Path):
    sess = server.create_session("c4")
    server.create_project("c4p")
    ws = server.checkout_workspace(sess["id"], "c4p", "main", "wsc4")
    wid = ws["id"]
    server.upsert_element(wid, "u", "person", "User", description="d")
    server.upsert_element(wid, "sys", "software_system", "Sys", description="d")
    server.upsert_element(
        wid, "api", "container", "API", parent_id="sys", description="d", technology="Go"
    )
    server.upsert_element(
        wid,
        "handler",
        "component",
        "Handler",
        parent_id="api",
        description="d",
        technology="pkg",
    )
    server.upsert_element(
        wid,
        "fn",
        "code",
        "Handle()",
        parent_id="handler",
        description="d",
        technology="func",
    )
    ov = server.get_overview_diagram(wid)
    assert ov["layer"] == "context"
    assert "C4Context" in ov["content"] or "Person(" in ov["content"] or "System(" in ov["content"]
    cont = server.get_layer_diagram(wid, "container", parent_id="sys")
    assert "API" in cont["content"]
    assert "C4Container" in cont["content"] or "Container(" in cont["content"]
    comp = server.get_layer_diagram(wid, "component", parent_id="api")
    assert "C4Component" in comp["content"] or "Handler" in comp["content"]
    code = server.get_layer_diagram(wid, "code", parent_id="handler")
    assert code["content"].startswith("classDiagram")
    assert "class fn" in code["content"] or "Handle" in code["content"]
    assert "view_url" in code and "layer=code" in code["view_url"]
    html = native.render_view_html(
        wid, "container", "sys", "https://c4.example.com", "layer", "mermaid"
    )
    assert "mermaid" in html or "app-shell" in html
    all_html = native.render_view_html(
        wid, "context", None, "https://c4.example.com", "all", "mermaid"
    )
    assert "All layers" in all_html or "mode=all" in all_html
    scene = json.loads(native.get_scene(wid, "all", None, None))
    assert scene["mode"] == "all"
    assert len(scene["nodes"]) >= 3
    links = server.get_view_links(wid)
    assert links["context_url"].endswith("?layer=context")
    assert links["adrs_url"].endswith("/adrs")
    assert any(c["id"] == "api" for c in links["containers"])
    empty = server.get_layer_diagram(wid, "component", parent_id="missing-parent")
    assert "No components yet" in empty["content"]
    assert ", \"\", \"\")" not in empty["content"]
    with pytest.raises(ValueError):
        server.get_view_links(wid, base_url="javascript:alert(1)")
    adrs = server.list_adrs(wid)["adrs"]
    # may be empty; when present must include view_url
    for a in adrs:
        assert a["view_url"].startswith("https://")


def test_upsert_flow_c4_dynamic_and_diagram(data_dir: Path):
    sess = server.create_session("flow")
    server.create_project("flow-p")
    ws = server.checkout_workspace(sess["id"], "flow-p", "main", "ws-flow")
    wid = ws["id"]
    server.upsert_element(wid, "u", "person", "User", description="d")
    server.upsert_element(wid, "s", "software_system", "Sys", description="d")
    server.upsert_element(
        wid, "api", "container", "API", parent_id="s", description="d", technology="Go"
    )
    out = server.upsert_flow(
        wid,
        {
            "id": "login-happy",
            "title": "Login happy path",
            "kind": "c4_dynamic",
            "usage_key": "login",
            "related_adrs": [],
            "steps": [
                {"n": 1, "from_id": "u", "to_id": "api", "label": "POST /login"},
                {"n": 2, "from_id": "api", "to_id": "s", "label": "auth"},
            ],
        },
        commit=True,
    )
    assert out["flow"]["path"].endswith(".toml")
    assert (data_dir / "workspaces" / wid / "docs" / "flows" / "login-happy.toml").is_file()
    diagram = server.get_flow_diagram(wid, "login-happy")
    assert "sequenceDiagram" in diagram["content"]
    assert "login-happy" in diagram["view_url"]
    listed = server.list_flows(wid)
    assert len(listed["flows"]) == 1
    html = native.render_flows_html(wid, "https://c4.example.com")
    assert "Flows" in html and "login-happy" in html
    detail = native.render_flow_html(wid, "login-happy", "https://c4.example.com")
    assert "sequenceDiagram" in detail or "mermaid" in detail
    with pytest.raises(ValueError):
        server.upsert_flow(
            wid,
            {
                "id": "bad",
                "title": "Bad",
                "kind": "c4_dynamic",
                "steps": [{"n": 1, "from_id": "u", "to_id": "nope", "label": "x"}],
            },
            commit=False,
        )


def test_upsert_element_structured_members(tmp_path, monkeypatch):
    import json

    from architect_c4 import server

    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    server._ensure_init()
    from architect_c4 import _native as native

    s = json.loads(native.create_session("members"))
    native.create_project("p-members")
    ws = json.loads(native.checkout_workspace(s["id"], "p-members", "main", "ws-members"))
    wid = ws["id"]
    server.upsert_element(wid, "sys", "software_system", "Sys")
    server.upsert_element(wid, "api", "container", "API", parent_id="sys")
    server.upsert_element(wid, "pipe", "component", "Pipeline", parent_id="api")
    out = server.upsert_element(
        wid,
        "Actor",
        "code",
        "Actor",
        parent_id="pipe",
        technology="class",
        members=[
            {
                "kind": "method",
                "visibility": "+",
                "name": "send",
                "params": [{"name": "message", "type": "Message"}],
                "return_type": "Message",
            }
        ],
    )
    assert out["id"] == "Actor"
    assert out["members"][0]["name"] == "send"
    model = server.get_model(wid)
    actor = next(e for e in model["elements"] if e["id"] == "Actor")
    assert actor["members"][0]["params"][0]["type"] == "Message"


def test_bind_docs_sidecar_toml_only(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Product docs/ gets only *.toml; no sqlite files inside docs/."""
    repo = tmp_path / "product"
    docs = repo / "docs"
    docs.mkdir(parents=True)
    (docs / "adr").mkdir()
    (docs / "adr" / "legacy.json").write_text(
        json.dumps(
            {
                "id": "legacy",
                "title": "Legacy",
                "status": "draft",
                "decided_at": "2026-07-16",
                "context": "c",
                "decision": "d",
                "consequences": "k",
            }
        ),
        encoding="utf-8",
    )
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "sidecar-data"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    native.init(str(tmp_path / "sidecar-data"))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    bound = server.bind_docs("default", str(docs))
    assert bound["rewrote_adr_json"] == 1
    assert not (docs / "adr" / "legacy.json").exists()
    assert (docs / "adr" / "legacy.toml").is_file()
    server.upsert_element("default", "sys", "software_system", "Sys", description="d")
    assert (docs / "model.toml").is_file()
    assert list(docs.rglob("*.db")) == []
    assert list(docs.rglob("*.json")) == []


def test_prompts_registered():
    import asyncio

    from architect_c4 import prompts as prompts_mod

    assert callable(prompts_mod.register_prompts)
    prompts = asyncio.run(server.mcp.list_prompts())
    names = {p.name for p in prompts}
    assert {
        "sidecar_onboard",
        "model_c4",
        "write_adr",
        "write_flow",
        "validate_architecture",
    } <= names

    captured: dict = {}

    class _FakeMcp:
        def prompt(self, **kwargs):
            def deco(fn):
                captured[kwargs["name"]] = fn
                return fn

            return deco

    prompts_mod.register_prompts(_FakeMcp())
    assert "bind_docs" in captured["sidecar_onboard"]("Billing", "sys")
    assert "docs/model.toml" in captured["model_c4"]("container", "sys")
    assert "docs/adr/a1.toml" in captured["write_adr"]("a1", "Title", "draft")
    assert "docs/flows/f1.toml" in captured["write_flow"]("f1", "Flow", "sequence")
    assert "validate_model" in captured["validate_architecture"]()


def test_list_workspaces_and_flow_crud(data_dir: Path):
    sess = server.create_session("meta")
    server.create_project("p1")
    wid = server.checkout_workspace(sess["id"], "p1", "main", "wsB")["id"]
    server.upsert_element(wid, "s", "software_system", "Sys", description="d")
    server.upsert_element(wid, "a", "container", "API", parent_id="s", description="d")
    server.upsert_element(wid, "b", "container", "DB", parent_id="s", description="d")
    out = server.list_workspaces()
    assert any(w["id"] == wid for w in out["workspaces"])
    assert out["workspaces"][0]["view_url"].startswith("https://")
    flow = {
        "id": "f-crud",
        "title": "CRUD",
        "kind": "c4_dynamic",
        "steps": [
            {"n": 1, "from_id": "a", "to_id": "b", "label": "query"},
        ],
    }
    server.upsert_flow(wid, flow, commit=True)
    assert server.get_flow(wid, "f-crud")["id"] == "f-crud"
    listed = server.list_flows(wid)
    assert listed.get("flows") or isinstance(listed, (list, dict))
    scene = server.get_scene(wid, mode="all")
    assert isinstance(scene, dict)
    server.delete_flow(wid, "f-crud", commit=True)


def test_bind_docs_requires_path(data_dir: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    with pytest.raises(ValueError, match="docs_dir"):
        server.bind_docs("default", None)


def test_view_index_and_flow_pages(data_dir: Path):
    from starlette.testclient import TestClient

    sess = server.create_session("v")
    server.create_project("pv")
    wid = server.checkout_workspace(sess["id"], "pv", "main", "wsV")["id"]
    server.upsert_element(wid, "s", "software_system", "Sys", description="d")
    server.upsert_element(wid, "a", "container", "API", parent_id="s", description="d")
    server.upsert_element(wid, "b", "container", "DB", parent_id="s", description="d")
    server.upsert_flow(
        wid,
        {
            "id": "f-view",
            "title": "View",
            "kind": "c4_dynamic",
            "steps": [{"n": 1, "from_id": "a", "to_id": "b", "label": "x"}],
        },
        commit=True,
    )
    server.upsert_adr(
        wid,
        {
            "id": "0001-v",
            "title": "V",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "s",
            "context": "c",
            "decision": "d",
            "consequences": "q",
        },
        commit=True,
    )
    client = TestClient(server.mcp.http_app())
    assert client.get("/view/?base_url=https://c4.example.com").status_code == 200
    assert client.get(f"/view/{wid}/flows?base_url=https://c4.example.com").status_code == 200
    assert (
        client.get(f"/view/{wid}/flows/f-view?base_url=https://c4.example.com").status_code
        == 200
    )
    assert client.get(f"/view/{wid}/adrs?base_url=https://c4.example.com").status_code == 200
    assert (
        client.get(f"/view/{wid}/adrs/0001-v?base_url=https://c4.example.com").status_code
        == 200
    )
    assert client.get("/wasm/missing.js").status_code == 404
    assert client.get("/wasm/%2e%2e/secret").status_code in (400, 404)
