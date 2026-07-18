#!/usr/bin/env python3
"""Streamable-HTTP MCP client — exercise architect-c4 via tools/call (no hardcoded DB seed)."""
from __future__ import annotations

import json
import sys
import urllib.request
from pathlib import Path
from typing import Any

URL = "http://127.0.0.1:8766/mcp"


class McpClient:
    def __init__(self, url: str = URL):
        self.url = url
        self.session_id: str | None = None
        self._id = 0

    def _next(self) -> int:
        self._id += 1
        return self._id

    def _post(self, payload: dict) -> tuple[dict | None, dict]:
        data = json.dumps(payload).encode()
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id
        req = urllib.request.Request(self.url, data=data, headers=headers, method="POST")
        with urllib.request.urlopen(req, timeout=60) as r:
            sid = r.headers.get("Mcp-Session-Id") or r.headers.get("mcp-session-id")
            if sid:
                self.session_id = sid
            raw = r.read().decode()
            meta = {
                "status": r.status,
                "content_type": r.headers.get("content-type"),
                "session": self.session_id,
            }
        msg = None
        if "text/event-stream" in (meta["content_type"] or ""):
            for line in raw.splitlines():
                if line.startswith("data:"):
                    chunk = line[5:].strip()
                    if not chunk:
                        continue
                    try:
                        obj = json.loads(chunk)
                    except json.JSONDecodeError:
                        continue
                    if obj.get("id") == payload.get("id") or "result" in obj or "error" in obj:
                        msg = obj
        else:
            msg = json.loads(raw)
        return msg, meta

    def call(self, method: str, params: dict | None = None) -> Any:
        payload: dict = {"jsonrpc": "2.0", "id": self._next(), "method": method}
        if params is not None:
            payload["params"] = params
        msg, meta = self._post(payload)
        if msg is None:
            raise RuntimeError(f"no response for {method} meta={meta}")
        if "error" in msg:
            raise RuntimeError(f"{method} error: {msg['error']}")
        return msg.get("result")

    def notify(self, method: str, params: dict | None = None) -> None:
        payload: dict = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        try:
            self._post(payload)
        except Exception:
            pass


def tool_call(client: McpClient, name: str, arguments: dict) -> Any:
    result = client.call("tools/call", {"name": name, "arguments": arguments})
    if isinstance(result, dict) and result.get("isError"):
        texts = [
            c.get("text", "")
            for c in (result.get("content") or [])
            if isinstance(c, dict) and c.get("type") == "text"
        ]
        raise RuntimeError(texts[0] if texts else f"tools/call {name} isError")
    if isinstance(result, dict) and "content" in result:
        texts = [c.get("text") for c in result["content"] if c.get("type") == "text"]
        # FastMCP sometimes returns errors as text without isError
        if texts and isinstance(texts[0], str) and texts[0].startswith("Error calling tool"):
            raise RuntimeError(texts[0])
        if len(texts) == 1:
            try:
                return json.loads(texts[0])
            except Exception:
                return texts[0]
        return result
    return result


def main() -> None:
    notes: list[dict] = []
    c = McpClient()

    def note(step: str, ok: bool, detail: Any = None):
        notes.append({"step": step, "ok": ok, "detail": detail})
        status = "OK" if ok else "FAIL"
        d = detail if not isinstance(detail, (dict, list)) else json.dumps(detail)[:400]
        print(f"[{status}] {step}: {d}")

    try:
        init = c.call(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "ceph-c4-mcp-test", "version": "0.1"},
            },
        )
        note("initialize", True, {"server": init.get("serverInfo"), "session": c.session_id})
        c.notify("notifications/initialized", {})
        note("notifications/initialized", True, "sent")
    except Exception as e:
        note("initialize", False, str(e))
        Path("/tmp/ceph-mcp-notes.json").write_text(json.dumps({"notes": notes}, indent=2))
        sys.exit(1)

    try:
        tools = c.call("tools/list", {})
        names = [t["name"] for t in tools.get("tools", [])]
        note("tools/list", True, names)
    except Exception as e:
        note("tools/list", False, str(e))
        names = []

    for n in [
        "create_session",
        "create_project",
        "checkout_workspace",
        "upsert_element",
        "upsert_relationship",
        "validate_model",
        "get_view_links",
        "get_layer_diagram",
        "upsert_adr",
        "set_adr_status",
        "get_adr",
    ]:
        note(f"tool_present:{n}", n in names)

    wid = "ceph-mcp-live"
    sid = None
    try:
        sess = tool_call(c, "create_session", {"meta": "mcp-live-ceph-c4"})
        sid = sess.get("id") if isinstance(sess, dict) else None
        note("create_session", bool(sid), sess)
    except Exception as e:
        note("create_session", False, str(e))

    try:
        proj = tool_call(c, "create_project", {"project_id": "ceph-mcp"})
        note("create_project", True, proj)
    except Exception as e:
        note("create_project", False, str(e))

    if sid:
        try:
            ws = tool_call(
                c,
                "checkout_workspace",
                {
                    "session_id": sid,
                    "project_id": "ceph-mcp",
                    "ref_name": "main",
                    "workspace_id": wid,
                },
            )
            note("checkout_workspace", True, ws)
            if isinstance(ws, dict) and ws.get("id"):
                wid = ws["id"]
        except Exception as e:
            note("checkout_workspace", False, str(e))

    elements = [
        ("admin", "person", "Storage Admin", None, "Operates Ceph", None, None),
        ("ceph", "software_system", "Ceph Storage Cluster", None, "RADOS-based storage", None, None),
        (
            "client_app",
            "software_system",
            "Client Application",
            None,
            "external Uses Ceph APIs",
            None,
            None,
        ),
        ("librados", "container", "librados", "ceph", "Client library", "C++ library", None),
        ("mon", "container", "Ceph Monitor", "ceph", "Quorum and maps", "ceph-mon", None),
        ("mgr", "container", "Ceph Manager", "ceph", "Management and metrics", "ceph-mgr", None),
        ("osd", "container", "Ceph OSD", "ceph", "Object storage daemon", "ceph-osd", None),
        ("osd_svc", "component", "OSD Service", "osd", "Daemon loop and messengers", "C++", None),
        ("pg", "component", "Placement Group", "osd", "Peering and recovery", "C++", None),
        ("objectstore", "component", "ObjectStore", "osd", "Local object backend", "BlueStore", None),
        (
            "OSD",
            "code",
            "OSD",
            "osd_svc",
            "+tick(); +_dispatch(); +create_logger()",
            "class",
            "https://github.com/ceph/ceph/blob/main/src/osd/OSD.h",
        ),
        (
            "PrimaryLogPG",
            "code",
            "PrimaryLogPG",
            "pg",
            "+on_local_recover(); +get_pgbackend()",
            "class",
            "https://github.com/ceph/ceph/blob/main/src/osd/PrimaryLogPG.h",
        ),
        (
            "PG",
            "code",
            "PG",
            "pg",
            "+is_active(); +is_peered()",
            "class",
            "https://github.com/ceph/ceph/blob/main/src/osd/PG.h",
        ),
        (
            "ObjectStore",
            "code",
            "ObjectStore",
            "objectstore",
            "+mount(); +umount(); +read()",
            "class",
            "https://github.com/ceph/ceph/blob/main/src/os/ObjectStore.h",
        ),
        (
            "BlueStore",
            "code",
            "BlueStore",
            "objectstore",
            "+mount(); +umount(); +aio_finish()",
            "class",
            "https://github.com/ceph/ceph/blob/main/src/os/bluestore/BlueStore.h",
        ),
    ]
    for id_, kind, name, parent, desc, tech, url in elements:
        try:
            args: dict = {
                "workspace_id": wid,
                "id": id_,
                "kind": kind,
                "name": name,
                "description": desc,
            }
            if parent:
                args["parent_id"] = parent
            if tech:
                args["technology"] = tech
            if url:
                args["url"] = url
            tool_call(c, "upsert_element", args)
            note(f"upsert_element:{id_}", True, {"kind": kind, "parent": parent})
        except Exception as e:
            note(f"upsert_element:{id_}", False, str(e))

    rels = [
        ("r1", "admin", "ceph", "Operates"),
        ("r2", "client_app", "ceph", "Stores data"),
        ("r3", "client_app", "librados", "Uses"),
        ("r4", "librados", "mon", "Fetches maps"),
        ("r5", "librados", "osd", "Read/write objects"),
        ("r6", "osd", "mon", "Reports status"),
        ("r7", "mgr", "mon", "Shares state"),
        ("r8", "mgr", "osd", "Metrics"),
        ("r9", "osd_svc", "pg", "Dispatches ops"),
        ("r10", "pg", "objectstore", "Persists objects"),
        ("r11", "PrimaryLogPG", "PG", "extends"),
        ("r12", "BlueStore", "ObjectStore", "implements"),
        ("r13", "OSD", "PrimaryLogPG", "uses"),
        ("r14", "PrimaryLogPG", "ObjectStore", "uses"),
    ]
    for rid, frm, to, desc in rels:
        try:
            tool_call(
                c,
                "upsert_relationship",
                {
                    "workspace_id": wid,
                    "id": rid,
                    "from_id": frm,
                    "to_id": to,
                    "description": desc,
                },
            )
            note(f"upsert_relationship:{rid}", True, f"{frm}->{to}")
        except Exception as e:
            note(f"upsert_relationship:{rid}", False, str(e))

    try:
        v = tool_call(c, "validate_model", {"workspace_id": wid})
        note("validate_model", bool(isinstance(v, dict) and v.get("ok")), v)
    except Exception as e:
        note("validate_model", False, str(e))

    for layer, parent in [
        ("context", None),
        ("container", "ceph"),
        ("component", "osd"),
        ("code", "objectstore"),
        ("code", "osd_svc"),
        ("code", "pg"),
    ]:
        args = {
            "workspace_id": wid,
            "layer": layer,
            "base_url": "https://c4.example.com",
        }
        if parent:
            args["parent_id"] = parent
        try:
            d = tool_call(c, "get_layer_diagram", args)
            content = d.get("content", "") if isinstance(d, dict) else str(d)
            ok = ("C4" in content or "classDiagram" in content) and "srcos" not in content
            note(
                f"get_layer_diagram:{layer}:{parent or '-'}",
                ok,
                {
                    "view_url": d.get("view_url") if isinstance(d, dict) else None,
                    "len": len(content),
                    "head": content[:140].replace("\n", " | "),
                    "has_members": "+mount" in content or "+tick" in content,
                    "garbage_path": "srcos" in content,
                },
            )
        except Exception as e:
            note(f"get_layer_diagram:{layer}:{parent or '-'}", False, str(e))

    try:
        links = tool_call(
            c,
            "get_view_links",
            {"workspace_id": wid, "base_url": "https://c4.example.com"},
        )
        note("get_view_links", True, links)
    except Exception as e:
        note("get_view_links", False, str(e))

    try:
        adr = tool_call(
            c,
            "upsert_adr",
            {
                "workspace_id": wid,
                "adr": {
                    "id": "0001-bluestore-via-mcp",
                    "title": "BlueStore via MCP",
                    "status": "proposed",
                    "decided_at": "2026-07-16",
                    "scope_element_id": "objectstore",
                    "context": "Need ObjectStore implementation for OSD diagrams.",
                    "decision": "Model BlueStore as the primary Code implementation.",
                    "consequences": "Code views show BlueStore implementing ObjectStore.",
                },
                "commit": True,
            },
        )
        note("upsert_adr", True, adr)
        try:
            accepted = tool_call(
                c,
                "set_adr_status",
                {
                    "workspace_id": wid,
                    "id": "0001-bluestore-via-mcp",
                    "status": "accepted",
                    "commit": True,
                },
            )
            note("set_adr_status", True, accepted)
        except Exception as e2:
            note("set_adr_status", False, str(e2))
    except Exception as e:
        note("upsert_adr", False, str(e))

    out = {
        "workspace_id": wid,
        "ok_count": sum(1 for n in notes if n["ok"]),
        "fail_count": sum(1 for n in notes if not n["ok"]),
        "notes": notes,
    }
    Path("/tmp/ceph-mcp-notes.json").write_text(json.dumps(out, indent=2))
    print("=== SUMMARY ===")
    print("ok", out["ok_count"], "fail", out["fail_count"], "workspace", wid)
    for n in notes:
        if not n["ok"]:
            print("FAIL:", n["step"], "->", n["detail"])


if __name__ == "__main__":
    main()
