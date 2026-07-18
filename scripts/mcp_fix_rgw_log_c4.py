#!/usr/bin/env python3
"""Make .rgw.log a proper C4 data-store container with components (via MCP)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call

URL = "http://127.0.0.1:8766/mcp"
WS = "ws-rgw-usage"
NOTES: list[dict] = []


def note(step: str, ok: bool, detail=None):
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    print(("OK" if ok else "FAIL"), step, str(detail)[:300])


def main() -> int:
    c = McpClient(URL)
    try:
        c.call(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "rgw-log-c4-fix", "version": "0.3"},
            },
        )
        c.notify("notifications/initialized", {})
        note("initialize", True)
    except Exception as e:
        note("initialize", False, str(e))
        return 1

    sess = tool_call(c, "create_session", {"meta": "rgw-log-c4-fix"})
    sid = sess["id"]
    try:
        tool_call(c, "create_project", {"project_id": "ceph-rgw-usage"})
    except Exception as e:
        note("create_project", False, str(e))
    ws = tool_call(
        c,
        "checkout_workspace",
        {
            "session_id": sid,
            "project_id": "ceph-rgw-usage",
            "ref_name": "main",
            "workspace_id": WS,
        },
    )
    note("checkout", True, ws.get("id"))

    # Ensure system exists
    for el in [
        {
            "id": "ceph",
            "kind": "software_system",
            "name": "Ceph Storage Cluster",
            "description": "RADOS cluster; hosts RGW and RGW metadata/log pools",
            "url": "https://docs.ceph.com/en/latest/architecture/",
        },
        {
            "id": "rgw",
            "kind": "container",
            "name": "RADOS Gateway (RGW)",
            "parent_id": "ceph",
            "description": "Object gateway process (radosgw). Writes usage/ops into the log pool.",
            "technology": "radosgw / C++",
            "url": "https://docs.ceph.com/en/latest/radosgw/",
        },
        # C4 Container = data store (RADOS pool), not an application process
        {
            "id": "rgw_log_pool",
            "kind": "container",
            "name": "RGW log pool (.rgw.log)",
            "parent_id": "ceph",
            "description": (
                "C4 container: RADOS data store for RGW usage and ops logs "
                "(typically .default.rgw.log). Separate operational lifecycle "
                "(shards, trim, growth) from RGW process and bucket data pools."
            ),
            "technology": "RADOS pool",
            "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
        },
        # Components inside the log-pool container (C3)
        {
            "id": "rgw_usage_objects",
            "kind": "component",
            "name": "Usage log objects",
            "parent_id": "rgw_log_pool",
            "description": (
                "Window-aggregated counters (ops / successful_ops / bytes) "
                "per user·bucket·category. No per-operation timestamps."
            ),
            "technology": "RADOS objects / usage namespace",
        },
        {
            "id": "rgw_ops_log_objects",
            "kind": "component",
            "name": "Ops log objects",
            "parent_id": "rgw_log_pool",
            "description": (
                "Per-request HTTP/S3 audit records when rgw_enable_ops_log=true. "
                "Carries request timing; higher volume than usage windows."
            ),
            "technology": "RADOS objects / ops log",
        },
        # Keep Usage Log as component of RGW (logic), not of the pool
        {
            "id": "rgw_usage_log",
            "kind": "component",
            "name": "Usage Log accumulator",
            "parent_id": "rgw",
            "description": (
                "In-RGW component: increments in-memory window counters and flushes "
                "to Usage log objects in the RGW log pool."
            ),
            "technology": "rgw_enable_usage_log",
        },
        {
            "id": "s3_client",
            "kind": "person",
            "name": "S3 Client",
            "description": "Application using S3/Swift API",
        },
        {
            "id": "rgw_admin",
            "kind": "person",
            "name": "RGW Admin",
            "description": "Operator using radosgw-admin / AdminOps",
        },
    ]:
        try:
            tool_call(c, "upsert_element", {"workspace_id": WS, **el})
            note(f"upsert_element:{el['id']}", True, el["kind"])
        except Exception as e:
            note(f"upsert_element:{el['id']}", False, str(e))

    # Containment is parent_id (C4 nesting). Relationships = runtime communication only.
    rels = [
        ("r_client_rgw", "s3_client", "rgw", "S3/Swift API"),
        ("r_rgw_usage_acc", "rgw", "rgw_usage_log", "records usage"),
        ("r_usage_flush", "rgw_usage_log", "rgw_usage_objects", "flush window counters"),
        ("r_admin_rgw", "rgw_admin", "rgw", "usage show / AdminOps"),
        ("r_rgw_read_usage", "rgw", "rgw_usage_objects", "read usage windows"),
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

    # Update flows to talk to usage objects (store) after accumulator
    flow_record = {
        "id": "rgw-usage-record-on-request",
        "title": "RGW: accumulate S3 op into usage window",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "refs": [
            {
                "title": "Ceph AdminOps — Get Usage",
                "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
            }
        ],
        "epoch": {
            "kind": "phase",
            "phase": "usage-log-enabled",
            "note": "Counters flush into Usage log objects inside RGW log pool",
        },
        "steps": [
            {"n": 1, "from_id": "s3_client", "to_id": "rgw", "label": "S3/Swift request"},
            {
                "n": 2,
                "from_id": "rgw",
                "to_id": "rgw_usage_log",
                "label": "inc ops/bytes in current window",
            },
            {
                "n": 3,
                "from_id": "rgw_usage_log",
                "to_id": "rgw_usage_objects",
                "label": "flush window object",
            },
        ],
    }
    flow_read = {
        "id": "rgw-usage-read-admin",
        "title": "RGW: read usage by date window",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "refs": [
            {
                "title": "Ceph AdminOps — Get Usage",
                "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
            }
        ],
        "epoch": {
            "kind": "retention_window",
            "note": "Filters apply to window boundaries in Usage log objects",
        },
        "steps": [
            {"n": 1, "from_id": "rgw_admin", "to_id": "rgw", "label": "usage show / AdminOps"},
            {
                "n": 2,
                "from_id": "rgw",
                "to_id": "rgw_usage_objects",
                "label": "read usage objects in range",
            },
            {
                "n": 3,
                "from_id": "rgw",
                "to_id": "rgw_admin",
                "label": "ops / successful_ops / bytes",
            },
        ],
    }
    for flow in (flow_record, flow_read):
        try:
            out = tool_call(c, "upsert_flow", {"workspace_id": WS, "flow": flow, "commit": True})
            note(f"flow:{flow['id']}", True, out.get("view_url") if isinstance(out, dict) else out)
        except Exception as e:
            note(f"flow:{flow['id']}", False, str(e))

    # ADR refresh with same accepted status + note that log pool is C4 data-store container
    adr = {
        "id": "0002-rgw-usage-window-not-op-time",
        "title": "RGW usage stats are window-aggregated, not per-op timed",
        "status": "accepted",
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
            "an external pipeline — not the usage log. "
            "In C4, model `.rgw.log` as a data-store container under Ceph with components "
            "Usage log objects and Ops log objects; RGW holds the accumulator component."
        ),
        "consequences": (
            "usage show answers only within window granularity. "
            "C4 drill: Ceph → RGW log pool (.rgw.log) → Usage log objects. "
            "Trim deletes whole windows in the log pool, not individual ops."
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
                "title": "RGW documentation",
                "url": "https://docs.ceph.com/en/latest/radosgw/",
            },
        ],
    }
    try:
        out = tool_call(c, "upsert_adr", {"workspace_id": WS, "adr": adr, "commit": True})
        note("adr_refresh", True, out.get("view_url") if isinstance(out, dict) else out)
    except Exception as e:
        note("adr_refresh", False, str(e))

    Path("/tmp/rgw-log-c4-notes.json").write_text(
        json.dumps({"workspace_id": WS, "notes": NOTES}, indent=2, ensure_ascii=False)
    )
    fails = sum(1 for n in NOTES if not n["ok"])
    print("SUMMARY fail", fails)
    return 0 if fails == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
