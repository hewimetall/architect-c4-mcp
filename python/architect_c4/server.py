"""Slim FastMCP server: tools delegate to Rust composition root.

Sidecar: mount product ``docs/`` via ``--docs`` / ``ARCHITECT_C4_DOCS``.
Persist = TOML only. Writes go through an in-process Rust queue.
SQLite indexes stay in-memory.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from fastmcp import FastMCP
from starlette.requests import Request
from starlette.responses import HTMLResponse, JSONResponse, Response

from architect_c4 import native
from architect_c4.prompts import register_prompts

mcp = FastMCP("architect-c4")
register_prompts(mcp)

DEFAULT_PUBLIC_BASE = os.environ.get("ARCHITECT_C4_PUBLIC_BASE", "https://c4.example.com")


def _apply_cli_env(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse sidecar CLI flags into env (before ``native.init`` / auto-bind).

    ``--docs`` wins over ``ARCHITECT_C4_DOCS``. Remaining argv is left in
    ``sys.argv`` for FastMCP if it ever consumes it.
    """
    parser = argparse.ArgumentParser(
        prog="architect-c4",
        description="MCP sidecar: C4/ADR/Flow → product docs/*.toml",
    )
    parser.add_argument(
        "--docs",
        "-d",
        metavar="DIR",
        help="Absolute path to product docs/ (sets ARCHITECT_C4_DOCS)",
    )
    parser.add_argument(
        "--transport",
        choices=("stdio", "http", "streamable-http", "sse"),
        help="MCP transport (default: ARCHITECT_C4_TRANSPORT or stdio)",
    )
    parser.add_argument(
        "--host",
        help="HTTP bind host (default: ARCHITECT_C4_HOST or 127.0.0.1)",
    )
    parser.add_argument(
        "--port",
        type=int,
        help="HTTP port (default: ARCHITECT_C4_PORT or 8765)",
    )
    parser.add_argument(
        "--public-base",
        metavar="URL",
        help="HTTPS base for viewer links (ARCHITECT_C4_PUBLIC_BASE)",
    )
    args, rest = parser.parse_known_args(argv)
    if argv is None:
        sys.argv = [sys.argv[0], *rest]

    if args.docs:
        os.environ["ARCHITECT_C4_DOCS"] = os.path.abspath(args.docs)
    if args.transport:
        os.environ["ARCHITECT_C4_TRANSPORT"] = args.transport
    if args.host:
        os.environ["ARCHITECT_C4_HOST"] = args.host
    if args.port is not None:
        os.environ["ARCHITECT_C4_PORT"] = str(args.port)
    if args.public_base:
        os.environ["ARCHITECT_C4_PUBLIC_BASE"] = args.public_base
    return args


def _ensure_init() -> None:
    # Ephemeral sidecar state dir (NOT the product repo). Indexes are in-memory.
    data = os.environ.get("ARCHITECT_C4_DATA", os.path.join(os.getcwd(), ".data"))
    if not getattr(_ensure_init, "_done", False):
        native.init(data)
        _ensure_init._done = True  # type: ignore[attr-defined]


def _j(s: str) -> Any:
    return json.loads(s)


def _base_url(explicit: str | None = None) -> str:
    """Resolve public HTTPS base for viewer links (rejects javascript:/http:)."""
    if explicit and explicit.strip() and explicit.strip() != "https://localhost":
        candidate = explicit.strip().rstrip("/")
    else:
        candidate = DEFAULT_PUBLIC_BASE.rstrip("/")
    lower = candidate.lower()
    if not lower.startswith("https://"):
        raise ValueError("base_url must start with https://")
    if any(ch in candidate for ch in ("@", "\\", "\n", "\r", "<")):
        raise ValueError("base_url contains forbidden characters")
    if "javascript:" in lower or "data:" in lower:
        raise ValueError("base_url scheme not allowed")
    return candidate


@mcp.tool()
def bind_docs(docs_dir: str | None = None) -> dict:
    """Bind to a host ``docs/`` directory (sidecar happy path).

    Rewrites legacy ``*.json`` ADR/Flow → ``*.toml``. Loads ``model.toml``.
    Env default: ``ARCHITECT_C4_DOCS``.
    """
    _ensure_init()
    path = (docs_dir or os.environ.get("ARCHITECT_C4_DOCS") or "").strip()
    if not path:
        raise ValueError("docs_dir required (or set ARCHITECT_C4_DOCS)")
    return _j(native.bind_docs(path))


@mcp.tool()
def upsert_element(
    id: str,
    kind: str,
    name: str,
    parent_id: str | None = None,
    description: str | None = None,
    technology: str | None = None,
    url: str | None = None,
    members: list[dict] | None = None,
) -> dict:
    """Upsert C4 element (person|software_system|container|component|code|external).

    Code atoms: set ``technology`` to ``class``|``interface``|``function``.
    For code methods/fields prefer structured ``members`` (see schemas/code_member.json), e.g.
    ``{"kind":"method","visibility":"+","name":"send","params":[{"name":"message","type":"Message"}],"return_type":"Message"}``.
    Legacy: UML lines in ``description`` (`+foo()`, `+bar(x: T) R`).
    External (DB/SaaS/queue): kind=external, technology e.g. ``datastore``|``queue``|``saas``.
    """
    import json as _json

    _ensure_init()
    members_json = None if members is None else _json.dumps(members)
    return _j(
        native.upsert_element(
            id,
            kind,
            name,
            parent_id,
            description,
            technology,
            url,
            members_json,
        )
    )


@mcp.tool()
def upsert_relationship(
    id: str,
    from_id: str,
    to_id: str,
    description: str | None = None,
) -> dict:
    """Upsert relationship; enforces C4 baseline + accepted ADR policy.forbid.

    V1 atom canon (default): code↔code, code↔external, person↔system|external.
    Shell endpoints rejected unless ARCHITECT_C4_ATOM_EDGES=0 (legacy).
    """
    _ensure_init()
    return _j(native.upsert_relationship(id, from_id, to_id, description))


@mcp.tool()
def delete_relationship(id: str) -> dict:
    """Delete a relationship (revision recorded)."""
    _ensure_init()
    return _j(native.delete_relationship(id))


@mcp.tool()
def get_model() -> dict:
    """Return elements, relationships, decisions."""
    _ensure_init()
    return _j(native.get_model())


@mcp.tool()
def validate_model() -> dict:
    """Validate C4 + ADR layers + policy; agent-facing problems with layer/code/message."""
    _ensure_init()
    return _j(native.validate_workspace())


@mcp.tool()
def upsert_adr(adr: dict, commit: bool = True) -> dict:
    """Upsert rigid ADR JSON (Nygard fields + optional policy).

    Agent may only set status to ``draft`` or ``proposed``. Unknown fields rejected.
    See ``schemas/adr.json``.
    """
    _ensure_init()
    payload = dict(adr)
    return _j(native.upsert_adr(json.dumps(payload), commit))


@mcp.tool()
def set_adr_status(
    id: str,
    status: str,
    reason: str | None = None,
    superseded_by_id: str | None = None,
    commit: bool = True,
    process_token: str | None = None,
) -> dict:
    """Process-only ADR status transition.

    ``rejected`` requires non-empty ``reason``.
    ``superseded`` requires ``superseded_by_id``.
    When ``ARCHITECT_C4_PROCESS_TOKEN`` is set, ``process_token`` must match.
    """
    _ensure_init()
    if process_token is not None:
        os.environ["ARCHITECT_C4_CALLER_PROCESS_TOKEN"] = process_token
    try:
        return _j(native.set_adr_status(id, status, reason, superseded_by_id, commit))
    finally:
        os.environ.pop("ARCHITECT_C4_CALLER_PROCESS_TOKEN", None)


@mcp.tool()
def get_adr(id: str) -> dict:
    """Get one ADR as rigid JSON."""
    _ensure_init()
    return _j(native.get_adr(id))


@mcp.tool()
def list_adrs(base_url: str = "https://c4.example.com") -> dict:
    """List ADR index rows (each includes view_url)."""
    _ensure_init()
    return {"adrs": _j(native.list_adrs(_base_url(base_url)))}


@mcp.tool()
def upsert_flow(flow: dict, commit: bool = True) -> dict:
    """Upsert rigid Flow JSON (see ``schemas/flow.json``).

    Prefer ``kind=c4_dynamic`` with ``steps`` referencing existing C4 element ids.
    ``sequence`` / ``state`` use Mermaid ``body``.
    """
    _ensure_init()
    payload = dict(flow)
    return _j(native.upsert_flow(json.dumps(payload), commit))


@mcp.tool()
def get_flow(id: str) -> dict:
    """Get one Flow as rigid JSON."""
    _ensure_init()
    return _j(native.get_flow(id))


@mcp.tool()
def list_flows(base_url: str = "https://c4.example.com") -> dict:
    """List flows (each includes view_url)."""
    _ensure_init()
    return _j(native.list_flows(_base_url(base_url)))


@mcp.tool()
def delete_flow(id: str, commit: bool = True) -> dict:
    """Delete a flow document (revision recorded)."""
    _ensure_init()
    return _j(native.delete_flow(id, commit))


@mcp.tool()
def get_flow_diagram(id: str, base_url: str = "https://c4.example.com") -> dict:
    """Mermaid for a flow + view_url."""
    _ensure_init()
    return _j(native.get_flow_diagram(id, _base_url(base_url)))


@mcp.tool()
def get_overview_diagram(base_url: str = "https://c4.example.com") -> dict:
    """C4 Context (level 1) Mermaid + view_url for the browser viewer."""
    _ensure_init()
    return _j(native.get_overview_diagram(_base_url(base_url)))


@mcp.tool()
def get_layer_diagram(
    layer: str,
    parent_id: str | None = None,
    base_url: str = "https://c4.example.com",
) -> dict:
    """C4 layer diagram: context|container|component|code. Includes view_url."""
    _ensure_init()
    return _j(native.get_layer_diagram(layer, parent_id, _base_url(base_url)))


@mcp.tool()
def get_view_links(base_url: str = "https://c4.example.com") -> dict:
    """Absolute viewer URLs for context/containers/components/code/ADRs (for agents)."""
    _ensure_init()
    return _j(native.get_view_links(_base_url(base_url)))


@mcp.tool()
def get_scene(
    mode: str = "all",
    layer: str | None = None,
    focus: str | None = None,
) -> dict:
    """Scene graph JSON for WASM/canvas (and All-layers mode)."""
    _ensure_init()
    return _j(native.get_scene(mode, layer, focus))


def _html(html: str, status_code: int = 200) -> HTMLResponse:
    """HTML responses must not be cached — UI chrome changes often during design iteration."""
    resp = HTMLResponse(html, status_code=status_code)
    resp.headers["Cache-Control"] = "no-store, max-age=0"
    resp.headers["Pragma"] = "no-cache"
    return resp


@mcp.custom_route("/view/adrs/{adr_id}", methods=["GET"])
async def c4_adr_detail(request: Request) -> Response:
    """Single ADR page."""
    _ensure_init()
    adr_id = request.path_params["adr_id"]
    base = _base_url(request.query_params.get("base_url"))
    try:
        html = native.render_adr_html(adr_id, base)
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=404)
    return _html(html)


@mcp.custom_route("/view/adrs", methods=["GET"])
async def c4_adrs_index(request: Request) -> Response:
    """ADR index."""
    _ensure_init()
    base = _base_url(request.query_params.get("base_url"))
    try:
        html = native.render_adrs_html(base)
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=400)
    return _html(html)


@mcp.custom_route("/view/flows/{flow_id}", methods=["GET"])
async def c4_flow_detail(request: Request) -> Response:
    """Single Flow page (Mermaid)."""
    _ensure_init()
    flow_id = request.path_params["flow_id"]
    base = _base_url(request.query_params.get("base_url"))
    try:
        html = native.render_flow_html(flow_id, base)
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=404)
    return _html(html)


@mcp.custom_route("/view/flows", methods=["GET"])
async def c4_flows_index(request: Request) -> Response:
    """Flow index."""
    _ensure_init()
    base = _base_url(request.query_params.get("base_url"))
    try:
        html = native.render_flows_html(base)
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=400)
    return _html(html)


@mcp.custom_route("/view", methods=["GET"])
@mcp.custom_route("/view/", methods=["GET"])
async def c4_view(request: Request) -> Response:
    """Browser C4 viewer. Query: layer, parent, mode=all, renderer=mermaid|wasm|auto."""
    _ensure_init()
    layer = request.query_params.get("layer") or "context"
    parent_id = request.query_params.get("parent") or request.query_params.get("focus") or None
    mode = request.query_params.get("mode") or "layer"
    renderer = request.query_params.get("renderer") or "mermaid"
    base = _base_url(request.query_params.get("base_url"))
    try:
        html = native.render_view_html(layer, parent_id, base, mode, renderer)
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=400)
    return _html(html)


@mcp.custom_route("/wasm/{path:path}", methods=["GET"])
async def wasm_static(request: Request) -> Response:
    """Serve prebuilt wasm-pack artifacts for the canvas viewer."""
    from pathlib import Path

    rel = request.path_params["path"]
    if ".." in rel or rel.startswith("/"):
        return JSONResponse({"error": "invalid path"}, status_code=400)
    root = Path(__file__).resolve().parent / "static" / "wasm"
    file_path = (root / rel).resolve()
    if not str(file_path).startswith(str(root)) or not file_path.is_file():
        return JSONResponse({"error": "not found"}, status_code=404)
    data = file_path.read_bytes()
    media = "application/wasm" if file_path.suffix == ".wasm" else "text/javascript"
    if file_path.suffix == ".json":
        media = "application/json"
    return Response(
        data,
        media_type=media,
        headers={
            "Cache-Control": "no-store, max-age=0",
            "Pragma": "no-cache",
        },
    )


@mcp.custom_route("/health", methods=["GET"])
async def health(_request: Request) -> Response:
    return Response("ok", media_type="text/plain")


def main(argv: list[str] | None = None) -> None:
    _apply_cli_env(argv)
    _ensure_init()
    transport = os.environ.get("ARCHITECT_C4_TRANSPORT", "stdio").strip().lower()
    if transport in {"http", "streamable-http", "sse"}:
        host = os.environ.get("ARCHITECT_C4_HOST", "127.0.0.1")
        port = int(os.environ.get("ARCHITECT_C4_PORT", "8765"))
        path = os.environ.get("ARCHITECT_C4_PATH", "/mcp")
        mcp.run(transport=transport, host=host, port=port, path=path)
        return
    mcp.run()


if __name__ == "__main__":  # pragma: no cover
    main()
