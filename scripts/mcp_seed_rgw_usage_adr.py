#!/usr/bin/env python3
"""Seed RGW usage-window ADR + flows via MCP tools/call (no DB hardcode).

Writes notes to /tmp/rgw-usage-mcp-notes.json — what worked / what failed.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

# Reuse client from sibling script
sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call  # noqa: E402

URL = "http://127.0.0.1:8766/mcp"
# Fresh workspace (re-bind after service restart requires checkout_workspace).
# Using dedicated id so we do not fight an existing ceph-rados-c4 worktree.
WS = "ws-rgw-usage"  # reused; checkout may fail if non-empty — OK if already bound
NOTES: list[dict[str, Any]] = []


def note(step: str, ok: bool, detail: Any = None) -> None:
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    tag = "OK" if ok else "FAIL"
    d = detail if not isinstance(detail, (dict, list)) else json.dumps(detail, ensure_ascii=False)[:500]
    print(f"[{tag}] {step}: {d}")


def main() -> int:
    c = McpClient(URL)
    try:
        init = c.call(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "rgw-usage-adr-seed", "version": "0.3.0"},
            },
        )
        note("initialize", True, {"server": init.get("serverInfo"), "session": c.session_id})
        c.notify("notifications/initialized", {})
    except Exception as e:
        note("initialize", False, str(e))
        _write()
        return 1

    # Ensure session/project so worktree is bound for ADR/flow git write
    try:
        sess = tool_call(c, "create_session", {"meta": "rgw-usage-adr-seed"})
        sid = sess.get("id") if isinstance(sess, dict) else None
        note("create_session", bool(sid), sess)
    except Exception as e:
        note("create_session", False, str(e))
        sid = None

    # Prefer existing ceph-rados-c4 workspace — checkout may fail if project missing
    wid = WS
    try:
        tool_call(c, "create_project", {"project_id": "ceph-rgw-usage"})
        note("create_project", True, "ceph-rgw-usage")
    except Exception as e:
        note("create_project", False, str(e))

    if sid:
        try:
            ws = tool_call(
                c,
                "checkout_workspace",
                {
                    "session_id": sid,
                    "project_id": "ceph-rgw-usage",
                    "ref_name": "main",
                    "workspace_id": wid,
                },
            )
            note("checkout_workspace", True, ws)
            if isinstance(ws, dict) and ws.get("id"):
                wid = ws["id"]
        except Exception as e:
            note(
                "checkout_workspace",
                False,
                f"{e} — will try writes on {wid} if already bound from prior seeds",
            )

    # C4 elements for RGW usage path (system must exist before containers)
    elements = [
        {
            "id": "ceph",
            "kind": "software_system",
            "name": "Ceph Storage Cluster",
            "description": "RADOS cluster hosting RGW and usage log pools",
            "url": "https://docs.ceph.com/en/latest/architecture/",
        },
        {
            "id": "s3_client",
            "kind": "person",
            "name": "S3 Client",
            "description": "Application using S3/Swift API",
        },
        {
            "id": "rgw",
            "kind": "container",
            "name": "RADOS Gateway (RGW)",
            "parent_id": "ceph",
            "description": "Object gateway; usage log aggregates ops into time windows",
            "technology": "C++ / radosgw",
            "url": "https://docs.ceph.com/en/latest/radosgw/",
        },
        {
            "id": "rgw_usage_log",
            "kind": "component",
            "name": "Usage Log",
            "parent_id": "rgw",
            "description": "In-memory counters flushed to .rgw.log; ops/bytes per window — not per-op timestamps",
            "technology": "rgw_enable_usage_log",
        },
        {
            "id": "rgw_log_pool",
            "kind": "container",
            "name": ".rgw.log pool",
            "parent_id": "ceph",
            "description": "RADOS pool for usage/ops log objects",
            "technology": "RADOS",
        },
        {
            "id": "rgw_admin",
            "kind": "person",
            "name": "RGW Admin",
            "description": "Operator using radosgw-admin / AdminOps",
        },
    ]

    for el in elements:
        args = {"workspace_id": wid, **el}
        try:
            tool_call(c, "upsert_element", args)
            note(f"upsert_element:{el['id']}", True, el["kind"])
        except Exception as e:
            note(f"upsert_element:{el['id']}", False, str(e))

    # Relationships (static)
    rels = [
        ("r_client_rgw", "s3_client", "rgw", "S3/Swift API"),
        ("r_rgw_usage", "rgw", "rgw_usage_log", "records usage"),
        ("r_usage_pool", "rgw_usage_log", "rgw_log_pool", "flush window counters"),
        ("r_admin_rgw", "rgw_admin", "rgw", "usage show / AdminOps"),
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
            note(f"upsert_relationship:{rid}", True, desc)
        except Exception as e:
            note(f"upsert_relationship:{rid}", False, str(e))

    adr = {
        "id": "0002-rgw-usage-window-not-op-time",
        "title": "RGW usage stats are window-aggregated, not per-op timed",
        "status": "proposed",
        "decided_at": "2026-07-18",
        "scope_element_id": "rgw",
        "context": (
            "Billing and analytics asked for arbitrary time-slice queries by exact "
            "operation wall-clock. Usage log was assumed to be a timeline of ops."
        ),
        "decision": (
            "Treat usage statistics as counters rolled up into fixed time windows only. "
            "Do not promise per-operation timestamps or sub-window slicing from usage show / "
            "AdminOps Get Usage. For per-request time use ops log (rgw_enable_ops_log) or "
            "an external pipeline — not the usage log."
        ),
        "consequences": (
            "usage show answers only within window granularity (start-date/end-date on windows). "
            "Cannot reconstruct exact op order/time from usage objects. Ops log is a different "
            "store with higher volume. Trim deletes whole windows, not individual ops."
        ),
        "related_flows": [
            "rgw-usage-record-on-request",
            "rgw-usage-read-admin",
        ],
        "refs": [
            {
                "title": "Ceph AdminOps — Get Usage",
                "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
            },
            {
                "title": "RGW ops and usage logging (overview)",
                "url": "https://docs.ceph.com/en/latest/radosgw/",
            },
        ],
    }
    try:
        out = tool_call(c, "upsert_adr", {"workspace_id": wid, "adr": adr, "commit": True})
        note("upsert_adr", True, out if not isinstance(out, dict) else {
            "id": out.get("decision", {}).get("id"),
            "view_url": out.get("view_url"),
            "path": out.get("decision", {}).get("path"),
        })
    except Exception as e:
        note("upsert_adr", False, str(e))

    try:
        accepted = tool_call(
            c,
            "set_adr_status",
            {
                "workspace_id": wid,
                "id": "0002-rgw-usage-window-not-op-time",
                "status": "accepted",
                "commit": True,
            },
        )
        note("set_adr_status:accepted", True, accepted.get("decision", {}).get("status") if isinstance(accepted, dict) else accepted)
    except Exception as e:
        note("set_adr_status:accepted", False, str(e))

    # Refresh accepted ADR (same status) to attach refs / related_flows updates.
    try:
        adr_refresh = dict(adr)
        adr_refresh["status"] = "accepted"
        out = tool_call(
            c, "upsert_adr", {"workspace_id": wid, "adr": adr_refresh, "commit": True}
        )
        refs = (out.get("decision") or {}).get("refs") if isinstance(out, dict) else None
        note("upsert_adr:refresh_refs", bool(refs), {"n_refs": len(refs or []), "view_url": out.get("view_url") if isinstance(out, dict) else None})
    except Exception as e:
        note("upsert_adr:refresh_refs", False, str(e))

    flow_record = {
        "id": "rgw-usage-record-on-request",
        "title": "RGW: accumulate S3 op into usage window",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "epoch": {
            "kind": "phase",
            "phase": "usage-log-enabled",
            "note": "Valid while rgw_enable_usage_log=true; counters flush by tick/threshold",
        },
        "steps": [
            {"n": 1, "from_id": "s3_client", "to_id": "rgw", "label": "S3/Swift request"},
            {"n": 2, "from_id": "rgw", "to_id": "rgw_usage_log", "label": "inc ops/bytes in current window"},
            {"n": 3, "from_id": "rgw_usage_log", "to_id": "rgw_log_pool", "label": "flush window object"},
        ],
        "refs": [
            {
                "title": "Ceph AdminOps — Get Usage",
                "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
            }
        ],
    }
    flow_read = {
        "id": "rgw-usage-read-admin",
        "title": "RGW: read usage by date window",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "epoch": {
            "kind": "retention_window",
            "note": "Filters apply to window boundaries, not individual op timestamps",
        },
        "steps": [
            {"n": 1, "from_id": "rgw_admin", "to_id": "rgw", "label": "usage show / AdminOps"},
            {"n": 2, "from_id": "rgw", "to_id": "rgw_log_pool", "label": "read usage objects in range"},
            {"n": 3, "from_id": "rgw", "to_id": "rgw_admin", "label": "ops / successful_ops / bytes"},
        ],
    }

    for flow in (flow_record, flow_read):
        try:
            out = tool_call(
                c, "upsert_flow", {"workspace_id": wid, "flow": flow, "commit": True}
            )
            note(
                f"upsert_flow:{flow['id']}",
                True,
                {
                    "view_url": out.get("view_url") if isinstance(out, dict) else None,
                    "path": (out.get("flow") or {}).get("path") if isinstance(out, dict) else None,
                },
            )
        except Exception as e:
            note(f"upsert_flow:{flow['id']}", False, str(e))

    for fid in ("rgw-usage-record-on-request", "rgw-usage-read-admin"):
        try:
            d = tool_call(c, "get_flow_diagram", {"workspace_id": wid, "id": fid})
            content = d.get("content", "") if isinstance(d, dict) else ""
            note(
                f"get_flow_diagram:{fid}",
                "sequenceDiagram" in content,
                {"view_url": d.get("view_url") if isinstance(d, dict) else None, "head": content[:200]},
            )
        except Exception as e:
            note(f"get_flow_diagram:{fid}", False, str(e))

    try:
        links = tool_call(c, "get_view_links", {"workspace_id": wid})
        flows = links.get("flows") if isinstance(links, dict) else None
        note("get_view_links:flows", isinstance(flows, list) and len(flows) >= 1, {"n": len(flows or []), "flows_url": links.get("flows_url") if isinstance(links, dict) else None})
    except Exception as e:
        note("get_view_links:flows", False, str(e))

    try:
        v = tool_call(c, "validate_model", {"workspace_id": wid})
        note("validate_model", True, {"ok": v.get("ok") if isinstance(v, dict) else None, "n_problems": len((v or {}).get("problems") or [])})
    except Exception as e:
        note("validate_model", False, str(e))

    _write(wid)
    ok_n = sum(1 for n in NOTES if n["ok"])
    fail_n = sum(1 for n in NOTES if not n["ok"])
    print(f"=== SUMMARY workspace={wid} ok={ok_n} fail={fail_n} ===")
    return 0 if fail_n == 0 else 2


def _write(wid: str | None = None) -> None:
    out = {
        "workspace_id": wid,
        "ok_count": sum(1 for n in NOTES if n["ok"]),
        "fail_count": sum(1 for n in NOTES if not n["ok"]),
        "notes": NOTES,
    }
    Path("/tmp/rgw-usage-mcp-notes.json").write_text(json.dumps(out, indent=2, ensure_ascii=False))
    print("wrote /tmp/rgw-usage-mcp-notes.json")


if __name__ == "__main__":
    raise SystemExit(main())
