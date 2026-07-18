#!/usr/bin/env python3
"""Seed C4 Code level for RGW usage/ops log (via MCP tools/call)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call

URL = "http://127.0.0.1:8766/mcp"
WS = "ceph-rados-c4"
GH = "https://github.com/ceph/ceph/blob/main/"
NOTES: list[dict] = []


def note(step: str, ok: bool, detail=None):
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    print(("OK" if ok else "FAIL"), step, str(detail)[:300])


def main() -> int:
    c = McpClient(URL)
    c.call(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "rgw-usage-code", "version": "0.3"},
        },
    )
    c.notify("notifications/initialized", {})
    sess = tool_call(c, "create_session", {"meta": "rgw-usage-code"})
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

    # Code under RGW accumulator component
    codes = [
        {
            "id": "UsageLogger",
            "parent_id": "rgw_usage_log",
            "name": "UsageLogger",
            "description": "+insert();+flush();tick interval / threshold flush to RADOS",
            "technology": "class",
            "url": GH + "src/rgw/rgw_log.h",
        },
        {
            "id": "log_usage",
            "parent_id": "rgw_usage_log",
            "name": "log_usage()",
            "description": "+log_usage();builds rgw_usage_log_entry from req_state; ops/successful_ops",
            "technology": "function",
            "url": GH + "src/rgw/rgw_log.cc",
        },
        {
            "id": "rgw_usage_log_entry",
            "parent_id": "rgw_usage_objects",
            "name": "rgw_usage_log_entry",
            "description": "+add_usage();+aggregate();owner;bucket;categories;ops;bytes",
            "technology": "struct",
            "url": GH + "src/rgw/rgw_log.h",
        },
        {
            "id": "RGWUsageBatch",
            "parent_id": "rgw_usage_objects",
            "name": "RGWUsageBatch",
            "description": "+insert();map of real_time → rgw_usage_log_entry; hourly resolution buckets",
            "technology": "struct",
            "url": GH + "src/rgw/driver/rados/rgw_rados.h",
        },
        {
            "id": "RGWUsage",
            "parent_id": "rgw_usage_objects",
            "name": "RGWUsage",
            "description": "+show();+trim();formats usage show / AdminOps Get Usage over epoch windows",
            "technology": "class",
            "url": GH + "src/rgw/rgw_usage.cc",
        },
        {
            "id": "RGWRados_usage",
            "parent_id": "rgw_usage_objects",
            "name": "RGWRados (usage I/O)",
            "description": "+log_usage();+read_usage();+trim_usage();usage_log_hash; shards",
            "technology": "class",
            "url": GH + "src/rgw/driver/rados/rgw_rados.h",
        },
        {
            "id": "rgw_log_entry",
            "parent_id": "rgw_ops_log_objects",
            "name": "rgw_log_entry",
            "description": "+fields for per-request ops log; HTTP status; bucket; object; timestamp",
            "technology": "struct",
            "url": GH + "src/rgw/rgw_log.h",
        },
        {
            "id": "OpsLogSink",
            "parent_id": "rgw_ops_log_objects",
            "name": "OpsLog / logging path",
            "description": "+log();writes per-request ops log when rgw_enable_ops_log; not window aggregates",
            "technology": "module",
            "url": GH + "src/rgw/rgw_log.cc",
        },
    ]

    for el in codes:
        try:
            tool_call(
                c,
                "upsert_element",
                {
                    "workspace_id": WS,
                    "kind": "code",
                    **el,
                },
            )
            note(f"code:{el['id']}", True, el["parent_id"])
        except Exception as e:
            note(f"code:{el['id']}", False, str(e))

    rels = [
        ("r_log_usage_logger", "log_usage", "UsageLogger", "insert entry"),
        ("r_logger_batch", "UsageLogger", "RGWUsageBatch", "flush batches"),
        ("r_batch_entry", "RGWUsageBatch", "rgw_usage_log_entry", "aggregates"),
        ("r_logger_rados", "UsageLogger", "RGWRados_usage", "log_usage to shards"),
        ("r_usage_show", "RGWUsage", "RGWRados_usage", "read_usage / trim_usage"),
        ("r_usage_entry_read", "RGWUsage", "rgw_usage_log_entry", "formats entries"),
        ("r_ops_entry", "OpsLogSink", "rgw_log_entry", "writes"),
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

    # Extend record flow with code-level step optionally — keep container/component flow;
    # add a code-focused flow
    flow_code = {
        "id": "rgw-usage-code-path",
        "title": "RGW usage: log_usage → UsageLogger → RGWRados shards",
        "kind": "c4_dynamic",
        "usage_key": "rgw-bucket-usage",
        "scope_element_id": "rgw_usage_log",
        "related_adrs": ["0002-rgw-usage-window-not-op-time"],
        "refs": [
            {
                "title": "rgw_log.cc (usage logger)",
                "url": GH + "src/rgw/rgw_log.cc",
            },
            {
                "title": "radosgw man — usage log",
                "url": "https://docs.ceph.com/en/latest/man/8/radosgw/",
            },
        ],
        "epoch": {
            "kind": "phase",
            "phase": "usage-log-enabled",
            "note": "Hourly resolution under bucket owner; tick/threshold flush",
        },
        "steps": [
            {"n": 1, "from_id": "log_usage", "to_id": "UsageLogger", "label": "insert(ts, entry)"},
            {"n": 2, "from_id": "UsageLogger", "to_id": "RGWUsageBatch", "label": "batch by hour"},
            {
                "n": 3,
                "from_id": "UsageLogger",
                "to_id": "RGWRados_usage",
                "label": "log_usage → sharded objects",
            },
            {
                "n": 4,
                "from_id": "RGWRados_usage",
                "to_id": "rgw_usage_log_entry",
                "label": "persist entries",
            },
        ],
    }
    try:
        out = tool_call(c, "upsert_flow", {"workspace_id": WS, "flow": flow_code, "commit": True})
        note("flow:rgw-usage-code-path", True, out.get("view_url") if isinstance(out, dict) else out)
    except Exception as e:
        note("flow:rgw-usage-code-path", False, str(e))

    # Attach new flow to ADR
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
            "Treat usage statistics as counters rolled up into fixed time windows only "
            "(see UsageLogger / RGWUsageBatch hourly buckets). Do not promise per-operation "
            "timestamps from usage show. Per-request time belongs to ops log (rgw_log_entry), "
            "not rgw_usage_log_entry aggregates."
        ),
        "consequences": (
            "Code drill: rgw_usage_log → UsageLogger/log_usage; "
            "rgw_usage_objects → RGWUsage/RGWRados/rgw_usage_log_entry; "
            "rgw_ops_log_objects → OpsLogSink/rgw_log_entry."
        ),
        "related_flows": [
            "rgw-usage-record-on-request",
            "rgw-usage-read-admin",
            "rgw-usage-code-path",
        ],
        "refs": [
            {
                "title": "Ceph AdminOps — Get Usage",
                "url": "https://docs.ceph.com/en/latest/radosgw/adminops/#get-usage",
            },
            {
                "title": "radosgw(8) — usage log",
                "url": "https://docs.ceph.com/en/latest/man/8/radosgw/",
            },
            {
                "title": "src/rgw/rgw_log.cc",
                "url": GH + "src/rgw/rgw_log.cc",
            },
        ],
    }
    try:
        tool_call(c, "upsert_adr", {"workspace_id": WS, "adr": adr, "commit": True})
        note("adr_refresh", True)
    except Exception as e:
        note("adr_refresh", False, str(e))

    Path("/tmp/rgw-usage-code-notes.json").write_text(
        json.dumps({"notes": NOTES}, indent=2, ensure_ascii=False)
    )
    fails = sum(1 for n in NOTES if not n["ok"])
    print("SUMMARY fail", fails)
    return 0 if fails == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
