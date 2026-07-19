#!/usr/bin/env python3
"""Upsert RGW usage Code elements from agent-lsp symbol dump via architect-c4 MCP."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call

URL = "http://127.0.0.1:8766/mcp"
WS = "ceph-rados-c4"
GH = "https://github.com/ceph/ceph/blob/main/"
DUMP = Path(__file__).resolve().parents[1] / "fixtures/rgw-usage-lsp-symbols.json"
NOTES: list[dict] = []


def note(step: str, ok: bool, detail=None):
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    print(("OK" if ok else "FAIL"), step, str(detail)[:280])


def sanitize_id(name: str) -> str:
    s = re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("_")
    if not s or s[0].isdigit():
        s = "n" + s
    return s[:80]


def methods_desc(methods: list[str], fields: list[str] | None = None) -> str:
    parts = []
    for m in methods[:12]:
        if m.startswith("~") or m in ("encode", "decode", "dump", "generate_test_instances"):
            continue
        parts.append(f"+{m}()")
    if fields:
        for f in fields[:6]:
            parts.append(f"+{f}")
    return ";".join(parts) if parts else "+…()"


def main() -> int:
    dump = json.loads(DUMP.read_text())
    mapping = dump["c4_mapping"]
    files = dump["files"]

    # Flatten class records with parent from mapping
    by_name: dict[str, dict] = {}
    for fpath, meta in files.items():
        for cls in meta.get("classes") or []:
            by_name[cls["name"]] = {**cls, "file": fpath}
        for fn in meta.get("functions") or []:
            # free functions as code under usage accumulator
            by_name[fn] = {
                "name": fn,
                "kind": "function",
                "methods": [fn],
                "file": fpath,
            }

    c = McpClient(URL)
    c.call(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "rgw-code-from-lsp", "version": "0.3"},
        },
    )
    c.notify("notifications/initialized", {})
    sess = tool_call(c, "create_session", {"meta": "rgw-code-from-lsp"})
    tool_call(
        c,
        "checkout_workspace",
        {
            "session_id": sess["id"],
            "project_id": "ceph",
            "ref_name": "main",
            "workspace_id": WS,
        },
    )
    note("checkout", True, WS)
    note("lsp_dump", True, {"session": dump["session_id"], "classes": len(by_name)})

    created = []
    for parent, names in mapping.items():
        for name in names:
            meta = by_name.get(name)
            if not meta:
                note(f"missing_in_dump:{name}", False, parent)
                continue
            eid = sanitize_id(name)
            tech = meta.get("kind") or "class"
            if tech == "struct":
                tech = "struct"
            desc = methods_desc(meta.get("methods") or [name], meta.get("fields"))
            url = GH + meta["file"]
            try:
                tool_call(
                    c,
                    "upsert_element",
                    {
                        "workspace_id": WS,
                        "id": eid,
                        "kind": "code",
                        "name": name,
                        "parent_id": parent,
                        "description": desc,
                        "technology": tech,
                        "url": url,
                    },
                )
                note(f"code:{eid}", True, {"parent": parent, "file": meta["file"]})
                created.append(eid)
            except Exception as e:
                note(f"code:{eid}", False, str(e))

    # Relationships from LSP-derived call path
    rels = [
        ("r_lsp_log_usage_logger", "log_usage", "UsageLogger", "insert"),
        ("r_lsp_logger_flush", "UsageLogger", "RGWRados", "flush → log_usage"),
        ("r_lsp_batch", "UsageLogger", "RGWUsageBatch", "batches by round_timestamp"),
        ("r_lsp_usage_show", "RGWUsage", "RGWRados", "read_usage / trim_usage"),
        ("r_lsp_ops_manifold", "OpsLogManifold", "OpsLogSink", "implements"),
        ("r_lsp_ops_rados", "OpsLogRados", "OpsLogSink", "implements"),
        ("r_lsp_ops_entry", "OpsLogRados", "rgw_log_entry", "log"),
        ("r_lsp_json_sink", "JsonOpsLogSink", "OpsLogSink", "implements"),
        ("r_lsp_file_sink", "OpsLogFile", "OpsLogSink", "implements"),
    ]
    for rid, frm, to, desc in rels:
        try:
            tool_call(
                c,
                "upsert_relationship",
                {
                    "workspace_id": WS,
                    "id": rid,
                    "from_id": frm,
                    "to_id": to,
                    "description": desc,
                },
            )
            note(f"rel:{rid}", True, desc)
        except Exception as e:
            note(f"rel:{rid}", False, str(e))

    flow = {
        "id": "rgw-usage-code-path",
        "title": "RGW usage code path (from agent-lsp symbols)",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw_usage_log",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "refs": [
            {"title": "rgw_log.cc", "url": GH + "src/rgw/rgw_log.cc"},
            {"title": "rgw_usage.h", "url": GH + "src/rgw/rgw_usage.h"},
            {
                "title": "radosgw(8) usage log",
                "url": "https://docs.ceph.com/en/latest/man/8/radosgw/",
            },
        ],
        "epoch": {
            "kind": "phase",
            "phase": "usage-log-enabled",
            "note": "LSP: UsageLogger.round_timestamp → hourly buckets",
        },
        "steps": [
            {"n": 1, "from_id": "log_usage", "to_id": "UsageLogger", "label": "insert"},
            {"n": 2, "from_id": "UsageLogger", "to_id": "RGWUsageBatch", "label": "batch"},
            {"n": 3, "from_id": "UsageLogger", "to_id": "RGWRados", "label": "flush log_usage"},
            {"n": 4, "from_id": "RGWUsage", "to_id": "RGWRados", "label": "show/trim read_usage"},
        ],
    }
    try:
        out = tool_call(c, "upsert_flow", {"workspace_id": WS, "flow": flow, "commit": True})
        note("flow", True, out.get("view_url") if isinstance(out, dict) else out)
    except Exception as e:
        note("flow", False, str(e))

    Path("/tmp/rgw-lsp-code-notes.json").write_text(
        json.dumps(
            {
                "workspace_id": WS,
                "lsp_session": dump["session_id"],
                "created": created,
                "notes": NOTES,
            },
            indent=2,
            ensure_ascii=False,
        )
    )
    fails = sum(1 for n in NOTES if not n["ok"])
    print("SUMMARY created", len(created), "fail", fails)
    return 0 if fails == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
