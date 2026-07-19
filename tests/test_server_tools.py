"""Sidecar MCP: bind docs + tools without workspace_id."""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from architect_c4 import native, server


def _git_init(repo: Path) -> None:
    import subprocess

    def run(*args: str) -> None:
        subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)

    run("init")
    run("config", "user.email", "t@t")
    run("config", "user.name", "t")


@pytest.fixture()
def docs(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "data"))
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    product = tmp_path / "product"
    d = product / "docs"
    d.mkdir(parents=True)
    _git_init(product)
    native.init(str(tmp_path / "data"))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    bound = server.bind_docs(str(d))
    assert "docs" in bound
    return d


def test_native_bind_and_adr_flow(docs: Path):
    native.upsert_element("user", "person", "User", None, "A user", None, None, None)
    native.upsert_element(
        "sys", "software_system", "Billing", None, "Billing system", None, None, None
    )
    native.upsert_element("api", "container", "API", "sys", "HTTP API", "Python", None, None)
    native.upsert_relationship("r1", "user", "sys", "Uses")
    v = json.loads(native.validate_workspace())
    assert any(p["code"] == "system.missing_decisions" for p in v["problems"])
    adr = json.loads(
        native.upsert_adr(
            json.dumps(
                {
                    "id": "0001-use-toml",
                    "title": "Use TOML",
                    "status": "proposed",
                    "decided_at": "2026-07-16",
                    "scope_element_id": "sys",
                    "context": "Need files in docs/.",
                    "decision": "Persist as TOML.",
                    "consequences": "Git is history.",
                }
            ),
            True,
        )
    )
    assert adr["decision"]["path"].endswith(".toml")
    native.set_adr_status("0001-use-toml", "accepted", None, None, True, None)
    assert (docs / "adr" / "0001-use-toml.toml").is_file()
    assert (docs / "model.toml").is_file()
    v2 = json.loads(native.validate_workspace())
    assert not any(p["code"] == "system.missing_decisions" for p in v2["problems"])
    diagram = json.loads(native.get_overview_diagram("https://ex.com"))
    assert "C4Context" in diagram["content"]
    assert len(json.loads(native.list_adrs("https://c4.example.com"))) == 1


def test_server_tools_cover_facade(docs: Path):
    server.upsert_element("u", "person", "User", description="d")
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_relationship("r", "u", "s", "Uses")
    model = server.get_model()
    assert len(model["elements"]) == 2
    assert "problems" in server.validate_model()
    out = server.upsert_adr(
        {
            "id": "0001-record-adrs",
            "title": "Record ADRs",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "s",
            "context": "Need decision log.",
            "decision": "Record ADRs as TOML.",
            "consequences": "Agents use upsert_adr.",
        },
        commit=True,
    )
    server.set_adr_status("0001-record-adrs", "accepted", commit=True)
    assert out["decision"]["path"].endswith(".toml")
    assert server.list_adrs()["adrs"]
    assert "mermaid" in server.get_overview_diagram()["format"]
    assert (docs / "model.toml").is_file()


def test_server_module_main_and_mcp_name():
    assert server.mcp.name == "architect-c4"
    assert callable(server.main)


def test_ensure_init_reads_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "d"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    server._ensure_init()
    assert (tmp_path / "d").is_dir()
    server._ensure_init()


def test_main_invokes_mcp_run(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    called = {"n": 0}

    def fake_run(**_kwargs):
        called["n"] += 1

    monkeypatch.setattr(server.mcp, "run", fake_run)
    server.main([])
    assert called["n"] == 1


def test_cli_docs_arg_sets_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    docs = tmp_path / "product" / "docs"
    docs.mkdir(parents=True)
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    monkeypatch.delenv("ARCHITECT_C4_TRANSPORT", raising=False)
    args = server._apply_cli_env(
        [
            "--docs",
            str(docs),
            "--transport",
            "http",
            "--host",
            "0.0.0.0",
            "--port",
            "8766",
            "--public-base",
            "https://c4.example.com",
        ]
    )
    assert args.docs == str(docs)
    assert os.environ["ARCHITECT_C4_DOCS"] == str(docs.resolve())
    assert os.environ["ARCHITECT_C4_TRANSPORT"] == "http"
    assert os.environ["ARCHITECT_C4_HOST"] == "0.0.0.0"
    assert os.environ["ARCHITECT_C4_PORT"] == "8766"
    assert os.environ["ARCHITECT_C4_PUBLIC_BASE"] == "https://c4.example.com"


def test_main_docs_arg_before_init(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    docs = tmp_path / "docs"
    docs.mkdir()
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "data"))
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    seen: dict = {}

    def fake_run(**_kwargs):
        seen["docs"] = os.environ.get("ARCHITECT_C4_DOCS")

    monkeypatch.setattr(server.mcp, "run", fake_run)
    server.main(["--docs", str(docs)])
    assert seen["docs"] == str(docs.resolve())


def test_reject_dangling_relationship_and_delete(docs: Path):
    server.upsert_element("a", "person", "A", description="d")
    with pytest.raises(ValueError):
        server.upsert_relationship("r-bad", "a", "missing", "x")
    server.upsert_element("b", "software_system", "B", description="d")
    server.upsert_relationship("r-ok", "a", "b", "uses")
    assert server.delete_relationship("r-ok")["deleted"] == "r-ok"
    with pytest.raises(ValueError):
        server.upsert_adr(
            {
                "id": "0009-bad",
                "title": "Bad",
                "status": "proposed",
                "decided_at": "2026-07-16",
                "scope_element_id": "nope",
                "context": "Invalid scope.",
                "decision": "Should fail.",
                "consequences": "Rejected.",
            },
            commit=False,
        )


def test_policy_blocks_person_to_code_and_adr_reject_reason(docs: Path):
    server.upsert_element("u", "person", "User", description="d")
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element(
        "api", "container", "API", parent_id="s", description="d", technology="Go"
    )
    server.upsert_element(
        "cmp", "component", "Cmp", parent_id="api", description="d", technology="Go"
    )
    server.upsert_element(
        "cls", "code", "Cls", parent_id="cmp", description="+m()", technology="Go"
    )
    with pytest.raises(ValueError) as ei:
        server.upsert_relationship("bad", "u", "cls", "hack")
    assert "code" in str(ei.value).lower() or "atom" in str(ei.value).lower() or "person" in str(
        ei.value
    ).lower()
    server.upsert_adr(
        {
            "id": "0001-ok",
            "title": "Ok",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "s",
            "context": "c",
            "decision": "d",
            "consequences": "q",
        },
        commit=True,
    )
    server.set_adr_status("0001-ok", "accepted", commit=True)
    with pytest.raises(ValueError):
        server.set_adr_status("0001-ok", "rejected", reason=None, commit=True)
    server.set_adr_status("0001-ok", "rejected", reason="no longer", commit=True)


def test_base_url_https_only():
    with pytest.raises(ValueError):
        server._base_url("http://evil.example")
    with pytest.raises(ValueError):
        server._base_url("javascript:alert(1)")
    assert server._base_url("https://c4.example.com/") == "https://c4.example.com"


def test_main_http_transport(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    monkeypatch.setenv("ARCHITECT_C4_TRANSPORT", "http")
    monkeypatch.setenv("ARCHITECT_C4_HOST", "127.0.0.1")
    monkeypatch.setenv("ARCHITECT_C4_PORT", "8766")
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    seen = {}

    def fake_run(**kwargs):
        seen.update(kwargs)

    monkeypatch.setattr(server.mcp, "run", fake_run)
    server.main([])
    assert seen.get("transport") == "http"
    assert seen.get("port") == 8766


def test_layer_diagrams_all_c4_levels(docs: Path):
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element("api", "container", "API", parent_id="s", description="d")
    server.upsert_element("cmp", "component", "Cmp", parent_id="api", description="d")
    server.upsert_element(
        "cls", "code", "Cls", parent_id="cmp", description="+m()", technology="class"
    )
    ctx = server.get_layer_diagram("context")
    assert "mermaid" in ctx["format"]
    cont = server.get_layer_diagram("container", parent_id="s")
    assert "content" in cont
    links = server.get_view_links()
    assert links["view_url"].endswith("/")
    assert "workspace_id" not in links


def test_upsert_flow_c4_dynamic_and_diagram(docs: Path):
    server.upsert_element("u", "person", "User", description="d")
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element(
        "api", "container", "API", parent_id="s", description="d", technology="Go"
    )
    out = server.upsert_flow(
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
    assert (docs / "flows" / "login-happy.toml").is_file()
    diagram = server.get_flow_diagram("login-happy")
    assert "sequenceDiagram" in diagram["content"]
    assert "/flows/login-happy" in diagram["view_url"]
    listed = server.list_flows()
    assert len(listed["flows"]) == 1
    html = native.render_flows_html("https://c4.example.com")
    assert "Flows" in html and "login-happy" in html
    detail = native.render_flow_html("login-happy", "https://c4.example.com")
    assert "sequenceDiagram" in detail or "mermaid" in detail
    with pytest.raises(ValueError):
        server.upsert_flow(
            {
                "id": "bad",
                "title": "Bad",
                "kind": "c4_dynamic",
                "steps": [{"n": 1, "from_id": "u", "to_id": "nope", "label": "x"}],
            },
            commit=False,
        )


def test_upsert_element_structured_members(tmp_path, monkeypatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "data"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    product = tmp_path / "product"
    docs = product / "docs"
    docs.mkdir(parents=True)
    _git_init(product)
    native.init(str(tmp_path / "data"))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    server.bind_docs(str(docs))
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element("api", "container", "API", parent_id="s", description="d")
    server.upsert_element("cmp", "component", "Cmp", parent_id="api", description="d")
    server.upsert_element(
        "cls",
        "code",
        "Cls",
        parent_id="cmp",
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
    model = server.get_model()
    el = next(e for e in model["elements"] if e["id"] == "cls")
    assert el.get("members") or "send" in json.dumps(el)


def test_bind_docs_sidecar_toml_only(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "data"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    product = tmp_path / "product"
    docs = product / "docs"
    (docs / "adr").mkdir(parents=True)
    _git_init(product)
    (docs / "adr" / "legacy.json").write_text(
        json.dumps(
            {
                "id": "legacy",
                "title": "L",
                "status": "draft",
                "decided_at": "2026-07-16",
                "context": "c",
                "decision": "d",
                "consequences": "k",
            }
        ),
        encoding="utf-8",
    )
    native.init(str(tmp_path / "data"))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    bound = server.bind_docs(str(docs))
    assert bound["rewrote_adr_json"] == 1
    assert not (docs / "adr" / "legacy.json").exists()
    assert (docs / "adr" / "legacy.toml").is_file()
    server.upsert_element("sys", "software_system", "Sys", description="d")
    assert (docs / "model.toml").is_file()
    assert list(docs.rglob("*.db")) == []
    assert list(docs.rglob("*.json")) == []


def test_bind_docs_requires_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    native.init(str(tmp_path))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    with pytest.raises(ValueError, match="docs_dir"):
        server.bind_docs(None)


def test_cli_accepts_glued_docs_flag(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    server._apply_cli_env(["architect-c4-mcp", "--docs /tmp/glued-docs"])
    assert os.environ["ARCHITECT_C4_DOCS"].endswith("glued-docs")


def test_bind_docs_rejects_windows_path_on_unix(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    native.init(str(tmp_path))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    with pytest.raises(ValueError, match="Windows path"):
        server.bind_docs(r"C:\Users\derty\AppData\Local\Temp\local-architect-smoke-dramatiq")


def test_upsert_without_bind_fails_persist(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path))
    monkeypatch.delenv("ARCHITECT_C4_DOCS", raising=False)
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    native.init(str(tmp_path))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    with pytest.raises(ValueError, match="no docs bind"):
        server.upsert_element("x", "person", "X", description="d")


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
    onboard = captured["sidecar_onboard"]("Billing", "sys")
    assert "bind_docs" in onboard
    assert "workspace_id" not in onboard
    assert "docs/model.toml" in captured["model_c4"]("container", "sys")
    assert "docs/adr/a1.toml" in captured["write_adr"]("a1", "Title", "draft")
    assert "docs/flows/f1.toml" in captured["write_flow"]("f1", "Flow", "sequence")
    assert "validate_model" in captured["validate_architecture"]()


def test_flow_crud_and_scene(docs: Path):
    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element("a", "container", "API", parent_id="s", description="d")
    server.upsert_element("b", "container", "DB", parent_id="s", description="d")
    server.upsert_flow(
        {
            "id": "f-crud",
            "title": "CRUD",
            "kind": "c4_dynamic",
            "steps": [{"n": 1, "from_id": "a", "to_id": "b", "label": "query"}],
        },
        commit=True,
    )
    assert server.get_flow("f-crud")["id"] == "f-crud"
    assert server.list_flows()["flows"]
    assert isinstance(server.get_scene(mode="all"), dict)
    server.delete_flow("f-crud", commit=True)


def test_view_routes(docs: Path):
    from starlette.testclient import TestClient

    server.upsert_element("s", "software_system", "Sys", description="d")
    server.upsert_element("a", "container", "API", parent_id="s", description="d")
    server.upsert_element("b", "container", "DB", parent_id="s", description="d")
    server.upsert_flow(
        {
            "id": "f-view",
            "title": "View",
            "kind": "c4_dynamic",
            "steps": [{"n": 1, "from_id": "a", "to_id": "b", "label": "x"}],
        },
        commit=True,
    )
    server.upsert_adr(
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
    # Relative chrome links (no default jump to c4.example.com)
    home = client.get("/")
    assert home.status_code == 200
    assert "c4.example.com" not in home.text
    assert 'href="/?layer=context"' in home.text or 'href="/?layer=context&' in home.text
    assert 'href="/flows"' in home.text
    assert 'href="/adrs"' in home.text
    assert client.get("/flows").status_code == 200
    assert client.get("/flows/f-view").status_code == 200
    assert client.get("/adrs").status_code == 200
    assert client.get("/adrs/0001-v").status_code == 200
    assert client.get("/?mode=all").status_code == 200
    assert client.get("/?layer=context&renderer=wasm").status_code == 200
    # Optional absolute override still works
    abs_page = client.get("/?base_url=https://c4.example.com")
    assert abs_page.status_code == 200
    assert "https://c4.example.com" in abs_page.text
    # legacy /view → 308
    assert client.get("/view/", follow_redirects=False).status_code == 308
    assert client.get("/wasm/missing.js").status_code == 404


def test_no_workspace_public_api():
    assert not hasattr(server, "create_project")
    assert not hasattr(server, "checkout_workspace")
    assert not hasattr(server, "list_workspaces")
    assert not hasattr(server, "create_session")
    assert "workspace-id" not in server._apply_cli_env.__doc__


def test_rebind_clears_stale_model(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("ARCHITECT_C4_DATA", str(tmp_path / "data"))
    if hasattr(server._ensure_init, "_done"):
        delattr(server._ensure_init, "_done")
    a = tmp_path / "a" / "docs"
    b = tmp_path / "b" / "docs"
    a.mkdir(parents=True)
    b.mkdir(parents=True)
    _git_init(tmp_path / "a")
    _git_init(tmp_path / "b")
    native.init(str(tmp_path / "data"))
    server._ensure_init._done = True  # type: ignore[attr-defined]
    server.bind_docs(str(a))
    server.upsert_element("only-in-a", "software_system", "A", description="d")
    assert any(e["id"] == "only-in-a" for e in server.get_model()["elements"])
    server.bind_docs(str(b))
    ids = {e["id"] for e in server.get_model()["elements"]}
    assert "only-in-a" not in ids
