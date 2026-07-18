#!/usr/bin/env python3
"""Seed C4 model of architect-c4 itself via MCP tools/call (upstream :8766)."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call  # noqa: E402

WS = "architect-c4-self"
PROJECT = "architect-c4-self"
BASE = "https://architecture.runmcp.ru"
NOTES: list[dict[str, Any]] = []


def note(step: str, ok: bool, detail: Any = None) -> None:
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    tag = "OK" if ok else "FAIL"
    d = detail if not isinstance(detail, (dict, list)) else json.dumps(detail, ensure_ascii=False)[:500]
    print(f"[{tag}] {step}: {d}")


def upsert_el(c: McpClient, **kw: Any) -> None:
    tool_call(c, "upsert_element", {"workspace_id": WS, **kw})


def upsert_rel(c: McpClient, id_: str, frm: str, to: str, desc: str) -> None:
    tool_call(
        c,
        "upsert_relationship",
        {
            "workspace_id": WS,
            "id": id_,
            "from_id": frm,
            "to_id": to,
            "description": desc,
        },
    )


def main() -> int:
    c = McpClient("http://127.0.0.1:8766/mcp")
    try:
        c.call(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "seed-architect-c4-self", "version": "1"},
            },
        )
        c.notify("notifications/initialized", {})
        note("initialize", True, c.session_id)
    except Exception as e:
        note("initialize", False, str(e))
        return 1

    # Session + workspace (idempotent-ish)
    try:
        sess = tool_call(c, "create_session", {"meta": "architect-c4-self-model"})
        sid = sess["id"] if isinstance(sess, dict) else sess
        note("create_session", True, sid)
    except Exception as e:
        note("create_session", False, str(e))
        return 1

    try:
        tool_call(c, "create_project", {"project_id": PROJECT})
        note("create_project", True, PROJECT)
    except Exception as e:
        note("create_project", False, str(e))  # may already exist

    try:
        ws = tool_call(
            c,
            "checkout_workspace",
            {
                "session_id": sid,
                "project_id": PROJECT,
                "workspace_id": WS,
                "ref_name": "main",
            },
        )
        note("checkout_workspace", True, ws)
    except Exception as e:
        # re-bind attempt: empty workspace_id auto
        try:
            ws = tool_call(
                c,
                "checkout_workspace",
                {"session_id": sid, "project_id": PROJECT, "workspace_id": WS},
            )
            note("checkout_workspace_retry", True, ws)
        except Exception as e2:
            note("checkout_workspace", False, f"{e} / {e2}")
            return 1

    # --- Context ---
    try:
        upsert_el(
            c,
            id="agent",
            kind="person",
            name="AI / human agent",
            description="Models C4 via MCP tools (Cursor / clients)",
            parent_id=None,
        )
        upsert_el(
            c,
            id="browser_user",
            kind="person",
            name="Browser user",
            description="Reads diagrams, ADRs, Flows in HTTPS viewer",
            parent_id=None,
        )
        upsert_el(
            c,
            id="architect_c4",
            kind="software_system",
            name="architect-c4",
            description="C4 modeling MCP: FastMCP + hexagonal Rust, SQLite, git fixation, Mermaid/WASM viewer",
            parent_id=None,
            url=f"{BASE}/view/{WS}?layer=context",
        )
        upsert_el(
            c,
            id="vmcp",
            kind="software_system",
            name="vmcp gateway",
            description="Aggregates MCP upstreams; GraphQL /mcp and tool proxy /mcp-proxy; bearer auth",
            parent_id=None,
            technology="external",
        )
        upsert_el(
            c,
            id="caddy",
            kind="software_system",
            name="Caddy",
            description="TLS termination and reverse proxy to vmcp + viewer",
            parent_id=None,
            technology="external",
        )
        note("context_elements", True, 5)
    except Exception as e:
        note("context_elements", False, str(e))
        return 1

    for rid, frm, to, desc in [
        ("r-agent-caddy", "agent", "caddy", "HTTPS MCP tools/call"),
        ("r-browser-caddy", "browser_user", "caddy", "HTTPS viewer /view /wasm"),
        ("r-caddy-vmcp", "caddy", "vmcp", "proxies /mcp*"),
        ("r-caddy-arch", "caddy", "architect_c4", "proxies /view* /wasm*"),
        ("r-vmcp-arch", "vmcp", "architect_c4", "forwards architect-c4 tools"),
        ("r-agent-arch", "agent", "architect_c4", "models via MCP (through gateway)"),
    ]:
        try:
            upsert_rel(c, rid, frm, to, desc)
        except Exception as e:
            note(f"rel {rid}", False, str(e))

    # --- Containers ---
    containers = [
        ("fastmcp", "FastMCP server", "Python FastMCP tools + Starlette /view routes", "Python"),
        ("rust_app", "Rust app façade", "PyO3 composition root (architect-c4-app)", "Rust/PyO3"),
        ("sqlite_db", "SQLite store", "Elements, relationships, ADR/Flow index, revisions", "SQLite"),
        ("git_store", "Git worktrees", "ADR/Flow JSON fixation via gix commits", "gix"),
        ("viewer_assets", "Viewer + WASM", "HTML chrome, Mermaid, architect-c4-wasm Canvas2D", "HTML/WASM"),
    ]
    try:
        for cid, name, desc, tech in containers:
            upsert_el(
                c,
                id=cid,
                kind="container",
                name=name,
                description=desc,
                parent_id="architect_c4",
                technology=tech,
            )
        note("containers", True, [x[0] for x in containers])
    except Exception as e:
        note("containers", False, str(e))
        return 1

    for rid, frm, to, desc in [
        ("r-fm-app", "fastmcp", "rust_app", "calls PyO3 façade"),
        ("r-app-sql", "rust_app", "sqlite_db", "persists model + index"),
        ("r-app-git", "rust_app", "git_store", "commits ADR/Flow JSON"),
        ("r-fm-view", "fastmcp", "viewer_assets", "serves /view HTML + /wasm"),
        ("r-app-view", "rust_app", "viewer_assets", "render_view_html / scene JSON"),
    ]:
        try:
            upsert_rel(c, rid, frm, to, desc)
        except Exception as e:
            note(f"rel {rid}", False, str(e))

    # --- Components ---
    comps = [
        ("fastmcp", "mcp_tools", "MCP tool handlers", "list/create session, upsert_*, validate, diagrams"),
        ("fastmcp", "http_routes", "HTTP view routes", "/view/{ws} diagrams ADRs Flows + wasm assets"),
        ("rust_app", "session_svc", "Session service", "sessions + workspaces (architect-c4-session)"),
        ("rust_app", "model_svc", "Model service", "elements + relationships (architect-c4-model)"),
        ("rust_app", "adr_svc", "ADR service", "structured ADR JSON + git (architect-c4-adr)"),
        ("rust_app", "flow_svc", "Flow service", "flow JSON + git (architect-c4-flow)"),
        ("rust_app", "scene_svc", "Scene service", "matryoshka layout + highway/bus routing"),
        ("rust_app", "render_svc", "Render service", "Mermaid DSL + viewer HTML chrome"),
        ("rust_app", "validate_svc", "Validate + policy", "layer problems + ADR policy forbid rules"),
        ("rust_app", "revision_svc", "Revision ledger", "append-only revisions / revision_heads"),
        ("viewer_assets", "mermaid_board", "Mermaid board", "C4*/classDiagram + pan/zoom fit"),
        ("viewer_assets", "wasm_board", "WASM board", "Canvas2D redraw-on-zoom scene graph"),
    ]
    try:
        for parent, cid, name, desc in comps:
            upsert_el(
                c,
                id=cid,
                kind="component",
                name=name,
                description=desc,
                parent_id=parent,
            )
        note("components", True, len(comps))
    except Exception as e:
        note("components", False, str(e))

    for rid, frm, to, desc in [
        ("r-tools-session", "mcp_tools", "session_svc", "session lifecycle"),
        ("r-tools-model", "mcp_tools", "model_svc", "upsert element/relationship"),
        ("r-tools-adr", "mcp_tools", "adr_svc", "upsert_adr / set_adr_status"),
        ("r-tools-flow", "mcp_tools", "flow_svc", "upsert_flow / list_flows"),
        ("r-tools-val", "mcp_tools", "validate_svc", "validate_model"),
        ("r-tools-render", "mcp_tools", "render_svc", "get_*_diagram HTML"),
        ("r-tools-scene", "mcp_tools", "scene_svc", "get_scene All/WASM"),
        ("r-routes-render", "http_routes", "render_svc", "render_view_html"),
        ("r-routes-scene", "http_routes", "scene_svc", "embed scene JSON"),
        ("r-model-rev", "model_svc", "revision_svc", "append revisions"),
        ("r-adr-rev", "adr_svc", "revision_svc", "append revisions"),
        ("r-flow-rev", "flow_svc", "revision_svc", "append revisions"),
        ("r-scene-render", "scene_svc", "render_svc", "SceneGraph → Mermaid/WASM HTML"),
        ("r-val-policy", "validate_svc", "adr_svc", "reads accepted ADR policies"),
        ("r-render-mermaid", "render_svc", "mermaid_board", "inject Mermaid DSL"),
        ("r-render-wasm", "render_svc", "wasm_board", "inject scene + wasm boot"),
    ]:
        try:
            upsert_rel(c, rid, frm, to, desc)
        except Exception as e:
            note(f"rel {rid}", False, str(e))

    # --- Code (key crates as UML-ish) ---
    codes = [
        ("scene_svc", "matryoshka", "matryoshka", "+build_matryoshka()\n+inside-out layout", "crate"),
        ("scene_svc", "highway", "highway", "+route_all_highway()\n+LCA channel", "crate"),
        ("scene_svc", "bus_rails", "bus", "+allocate_left_buses()\n+route_cross_on_rails()", "crate"),
        ("scene_svc", "router", "router", "+route_port_to_port()\northogonal Dijkstra", "crate"),
        ("adr_svc", "decision_doc", "Decision", "+validate_document()\n+related_flows\n+policy", "struct"),
        ("flow_svc", "flow_doc", "Flow", "+validate_shape()\n+c4_dynamic|sequence|state", "struct"),
        ("validate_svc", "policy_engine", "Policy", "+forbid rules from accepted ADRs", "crate"),
        ("render_svc", "view_html", "view_html", "+view_html()\n+flows_index_html()", "fn"),
    ]
    try:
        for parent, cid, name, desc, tech in codes:
            upsert_el(
                c,
                id=cid,
                kind="code",
                name=name,
                description=desc,
                parent_id=parent,
                technology=tech,
            )
        note("code", True, len(codes))
    except Exception as e:
        note("code", False, str(e))

    for rid, frm, to, desc in [
        ("r-mat-hwy", "matryoshka", "highway", "calls route_all_in_scene"),
        ("r-hwy-bus", "highway", "bus_rails", "cross-container left rails"),
        ("r-hwy-router", "highway", "router", "intra-shell orthogonal"),
        ("r-mat-router", "matryoshka", "router", "sibling classic routes"),
    ]:
        try:
            upsert_rel(c, rid, frm, to, desc)
        except Exception as e:
            note(f"rel {rid}", False, str(e))

    # --- Flows first (so ADRs can related_flows) ---
    flows = [
        {
            "id": "mcp-upsert-element",
            "title": "MCP: upsert element → SQLite revision",
            "kind": "c4_dynamic",
            "usage_key": "mcp-write-path",
            "scope_element_id": "architect_c4",
            "related_adrs": ["0001-hex-solid-revision"],
            "steps": [
                {"n": 1, "from_id": "agent", "to_id": "caddy", "label": "HTTPS tools/call"},
                {"n": 2, "from_id": "caddy", "to_id": "vmcp", "label": "proxy /mcp"},
                {"n": 3, "from_id": "vmcp", "to_id": "fastmcp", "label": "architect-c4 tool"},
                {"n": 4, "from_id": "mcp_tools", "to_id": "model_svc", "label": "upsert_element"},
                {"n": 5, "from_id": "model_svc", "to_id": "sqlite_db", "label": "write row"},
                {"n": 6, "from_id": "model_svc", "to_id": "revision_svc", "label": "append revision"},
            ],
            "refs": [
                {
                    "url": "https://github.com/hewimetall/architect-c4-mcp",
                    "title": "architect-c4-mcp repo",
                }
            ],
        },
        {
            "id": "viewer-all-wasm",
            "title": "Viewer: All mode WASM scene",
            "kind": "c4_dynamic",
            "usage_key": "viewer-render",
            "scope_element_id": "viewer_assets",
            "related_adrs": ["0005-wasm-canvas-viewer-prototype", "0006-matryoshka-port-routing"],
            "steps": [
                {"n": 1, "from_id": "browser_user", "to_id": "caddy", "label": "GET /view?mode=all&renderer=wasm"},
                {"n": 2, "from_id": "caddy", "to_id": "http_routes", "label": "route"},
                {"n": 3, "from_id": "http_routes", "to_id": "scene_svc", "label": "build_matryoshka"},
                {"n": 4, "from_id": "scene_svc", "to_id": "render_svc", "label": "embed scene JSON"},
                {"n": 5, "from_id": "render_svc", "to_id": "wasm_board", "label": "Canvas2D redraw"},
            ],
            "refs": [],
        },
        {
            "id": "adr-git-fixation",
            "title": "ADR upsert with git fixation",
            "kind": "c4_dynamic",
            "usage_key": "adr-write",
            "scope_element_id": "adr_svc",
            "related_adrs": ["0001-hex-solid-revision", "0007-structured-adr-json"],
            "steps": [
                {"n": 1, "from_id": "agent", "to_id": "mcp_tools", "label": "upsert_adr"},
                {"n": 2, "from_id": "mcp_tools", "to_id": "adr_svc", "label": "validate rigid JSON"},
                {"n": 3, "from_id": "adr_svc", "to_id": "sqlite_db", "label": "index body_json"},
                {"n": 4, "from_id": "adr_svc", "to_id": "git_store", "label": "commit docs/adr/{id}.json"},
                {"n": 5, "from_id": "adr_svc", "to_id": "revision_svc", "label": "append revision"},
            ],
            "refs": [],
        },
        {
            "id": "flow-kinds-lifecycle",
            "title": "Flow kinds: agent picks c4_dynamic",
            "kind": "state",
            "usage_key": "flow-kinds",
            "scope_element_id": "flow_svc",
            "related_adrs": ["0008-flow-kinds-v1"],
            "body": """stateDiagram-v2
    [*] --> Draft: upsert_flow
    Draft --> Validated: validate_shape
    Validated --> Indexed: SQLite + git
    Indexed --> Rendered: get_flow_diagram
    Rendered --> [*]
    Validated --> Rejected: bad kind/body
    Rejected --> Draft: fix
""",
            "steps": [],
            "refs": [],
        },
    ]

    for fl in flows:
        try:
            tool_call(c, "upsert_flow", {"workspace_id": WS, "flow": fl, "commit": True})
            note(f"flow:{fl['id']}", True, fl["kind"])
        except Exception as e:
            note(f"flow:{fl['id']}", False, str(e))

    # --- ADRs (structured) ---
    adrs = [
        {
            "id": "0001-hex-solid-revision",
            "title": "Hexagonal multi-crate + SQL revisions",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "rust_app",
            "context": "Need SOLID/DRY architecture, slim Python, no monolith Rust crate, ADR git fixation, TDD coverage ≥93%.",
            "decision": "Hex ports in architect-c4-domain; adapters in small crates; append-only SQL revisions; ADR/Flow in git worktree; Python FastMCP only calls architect-c4-app PyO3 façade.",
            "consequences": "Clear testability per crate; higher coverage; history via git log + SQL rev_no.",
            "related_flows": ["mcp-upsert-element", "adr-git-fixation"],
            "refs": [
                {
                    "url": "https://github.com/hewimetall/architect-c4-mcp/blob/main/docs/adr/0001-hex-solid-revision.md",
                    "title": "ADR 0001",
                }
            ],
        },
        {
            "id": "0002-reject-dangling-relationships",
            "title": "Reject dangling relationships and invalid ADR scopes",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "model_svc",
            "context": "Smoke incident: upsert_relationship succeeded with missing endpoints; no delete tool; ADR scoped to missing element.",
            "decision": "Validate both relationship endpoints exist; add delete_relationship; reject ADR scope_element_id that is not an element.",
            "consequences": "Immediate validation errors; ops can remove bad edges; ADR scopes stay consistent.",
            "related_flows": ["mcp-upsert-element"],
            "refs": [],
            "policy": {
                "mode": "enforce",
                "forbid": [],
            },
        },
        {
            "id": "0003-full-c4-layers",
            "title": "Full C4 layers including Code",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "architect_c4",
            "context": "Only context/container were usable; component/code missing from model API.",
            "decision": "Support person|software_system|container|component|code with layer diagrams and All mode nesting.",
            "consequences": "Complete C4 drill-down; code uses UML-ish members in description.",
            "related_flows": ["viewer-all-wasm"],
            "refs": [],
        },
        {
            "id": "0004-code-level-mermaid-classdiagram",
            "title": "Code level via Mermaid classDiagram",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "render_svc",
            "context": "Need readable code diagrams without Structurizr Java stack.",
            "decision": "Render Code layer as Mermaid classDiagram with sanitized members and stereotypes; WASM draws UML compartments.",
            "consequences": "Agents put +method() lines in description; paths/prose filtered.",
            "related_flows": ["viewer-all-wasm"],
            "refs": [],
        },
        {
            "id": "0005-wasm-canvas-viewer-prototype",
            "title": "WASM Canvas2D viewer prototype",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "wasm_board",
            "context": "Mermaid alone is weak for All-layers nested groups; WebGPU not universal.",
            "decision": "Ship Canvas2D WASM with pan/zoom redraw; optional WebGPU detect later; renderer toggle in header.",
            "consequences": "SceneGraph JSON owned by Rust; WASM only draws; mobile pinch/fit.",
            "related_flows": ["viewer-all-wasm"],
            "refs": [],
        },
        {
            "id": "0006-matryoshka-port-routing",
            "title": "Matryoshka layout + hierarchical highways / left buses",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "scene_svc",
            "context": "Flat All-mode routing produced spaghetti and center-derived arrows.",
            "decision": "Inside-out matryoshka layout; inside container classic leaf ports; between containers left bus rails + mid-gap highway; parent→child on parent rail.",
            "consequences": "Classes connect old way inside shells; no pierce through grandchildren; research in docs/research/schematic-left-bus-rails.md.",
            "related_flows": ["viewer-all-wasm"],
            "refs": [
                {
                    "url": "https://www.altium.com/documentation/altium-designer/schematic/multi-sheet-hierarchical-designs",
                    "title": "Altium hierarchical designs",
                }
            ],
        },
        {
            "id": "0007-structured-adr-json",
            "title": "Rigid structured ADR JSON",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "adr_svc",
            "context": "Freeform markdown ADRs let agents hallucinate; policies need machine fields.",
            "decision": "ADR is deny_unknown_fields JSON (schemas/adr.json); agent status draft|proposed; process sets accepted|rejected|…; optional policy.forbid; related_flows + refs.",
            "consequences": "Hot-reloadable policies without rebuild; reject requires reason via set_adr_status.",
            "related_flows": ["adr-git-fixation"],
            "refs": [],
        },
        {
            "id": "0008-flow-kinds-v1",
            "title": "Flow kinds v1 (c4_dynamic, sequence, state)",
            "status": "proposed",
            "decided_at": "2026-07-16",
            "scope_element_id": "flow_svc",
            "context": "Need behavior views linked to C4/ADR without BPMN or Mermaid zoo.",
            "decision": "Support c4_dynamic (default), sequence, state; store docs/flows/{id}.json; defer flowchart/timeline.",
            "consequences": "Viewer Flows tab; one ADR can link many flows; agents must not invent element ids in steps.",
            "related_flows": ["flow-kinds-lifecycle", "mcp-upsert-element"],
            "refs": [],
        },
    ]

    for adr in adrs:
        try:
            tool_call(c, "upsert_adr", {"workspace_id": WS, "adr": adr, "commit": True})
            note(f"adr:{adr['id']}", True, adr["status"])
        except Exception as e:
            note(f"adr:{adr['id']}", False, str(e))

    # Accept ADRs via process tool if available
    for adr in adrs:
        try:
            tool_call(
                c,
                "set_adr_status",
                {
                    "workspace_id": WS,
                    "id": adr["id"],
                    "status": "accepted",
                },
            )
            note(f"accept:{adr['id']}", True, "accepted")
        except Exception as e:
            note(f"accept:{adr['id']}", False, str(e))

    try:
        v = tool_call(c, "validate_model", {"workspace_id": WS})
        note("validate_model", True, v)
    except Exception as e:
        note("validate_model", False, str(e))

    try:
        links = tool_call(c, "get_view_links", {"workspace_id": WS, "base_url": BASE})
        note("get_view_links", True, links)
    except Exception as e:
        note("get_view_links", False, str(e))
        links = {}

    try:
        model = tool_call(c, "get_model", {"workspace_id": WS})
        note(
            "get_model",
            True,
            {
                "elements": len(model.get("elements", [])),
                "relationships": len(model.get("relationships", [])),
                "decisions": len(model.get("decisions", [])),
            },
        )
    except Exception as e:
        note("get_model", False, str(e))

    try:
        fl = tool_call(c, "list_flows", {"workspace_id": WS, "base_url": BASE})
        note("list_flows", True, fl)
    except Exception as e:
        note("list_flows", False, str(e))

    out = {
        "workspace_id": WS,
        "viewer": f"{BASE}/view/{WS}?mode=all&renderer=wasm",
        "adrs": f"{BASE}/view/{WS}/adrs",
        "flows": f"{BASE}/view/{WS}/flows",
        "notes": NOTES,
    }
    Path("/tmp/architect-c4-self-notes.json").write_text(json.dumps(out, indent=2, ensure_ascii=False))
    print(json.dumps({k: out[k] for k in ("workspace_id", "viewer", "adrs", "flows")}, indent=2))
    fails = sum(1 for n in NOTES if not n["ok"])
    return 1 if fails and fails > 3 else 0


if __name__ == "__main__":
    raise SystemExit(main())
