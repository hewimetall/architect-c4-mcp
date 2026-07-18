#!/usr/bin/env python3
"""Enrich ceph-rados-c4 Code layer via architect-c4 MCP tools/call (no DB hardcode)."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

# Allow import sibling client helpers
sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call  # type: ignore

WS = "ceph-rados-c4"
NOTES: list[dict] = []


def note(step: str, ok: bool, detail: Any = None) -> None:
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    print(("OK" if ok else "FAIL"), step, detail if detail is not None else "")


def upsert_el(c: McpClient, **kw: Any) -> None:
    try:
        tool_call(c, "upsert_element", {"workspace_id": WS, **kw})
        note(f"upsert_element:{kw['id']}", True)
    except Exception as e:
        note(f"upsert_element:{kw['id']}", False, str(e))


def upsert_rel(c: McpClient, **kw: Any) -> None:
    try:
        tool_call(c, "upsert_relationship", {"workspace_id": WS, **kw})
        note(f"upsert_relationship:{kw['id']}", True)
    except Exception as e:
        note(f"upsert_relationship:{kw['id']}", False, str(e))


def main() -> int:
    c = McpClient("http://127.0.0.1:8766/mcp")
    c.call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "enrich-ceph-code", "version": "0.1"},
    })
    c.notify("notifications/initialized")

    # --- richer members on existing classes (LSP-sampled) ---
    upsert_el(
        c,
        id="OSD",
        kind="code",
        name="OSD",
        parent_id="osd_svc",
        description=(
            "+tick(); +_dispatch(); +ms_dispatch(); +handle_osd_map(); "
            "+create_logger(); +check_osdmap_features(); +get_osdmap(); +shutdown()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/OSD.h",
    )
    upsert_el(
        c,
        id="OSDService",
        kind="code",
        name="OSDService",
        parent_id="osd_svc",
        description=(
            "+whoami; +store; +meta_ch; +get_osdmap(); +publish_stats_to_osd(); "
            "+send_message_osd_cluster(); +lookup_session()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/OSD.h",
    )
    upsert_el(
        c,
        id="AsyncMessenger",
        kind="code",
        name="AsyncMessenger",
        parent_id="osd_svc",
        description=(
            "+start(); +shutdown(); +bind(); +bindv(); +send_to(); +connect_to(); "
            "+mark_down_all(); +get_dispatch_queue_len()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/msg/async/AsyncMessenger.h",
    )
    upsert_el(
        c,
        id="ObjectStore",
        kind="code",
        name="ObjectStore",
        parent_id="objectstore",
        description=(
            "+mount(); +umount(); +mkfs(); +queue_transactions(); +read(); "
            "+statfs(); +exists(); +omap_get(); +collection_list(); +fsck()"
        ),
        technology="interface",
        url="https://github.com/ceph/ceph/blob/main/src/os/ObjectStore.h",
    )
    upsert_el(
        c,
        id="BlueStore",
        kind="code",
        name="BlueStore",
        parent_id="objectstore",
        description=(
            "+mount(); +umount(); +queue_transactions(); +aio_finish(); "
            "+is_empty(); +compact(); +fsck(); +collect_metadata()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/os/bluestore/BlueStore.h",
    )
    upsert_el(
        c,
        id="PG",
        kind="code",
        name="PG",
        parent_id="pg",
        description=(
            "+get_pgid(); +is_active(); +is_peered(); +is_primary(); +do_peering_event(); "
            "+start_recovery_ops(); +do_request(); +get_osdmap(); +on_activate()"
        ),
        technology="base",
        url="https://github.com/ceph/ceph/blob/main/src/osd/PG.h",
    )
    upsert_el(
        c,
        id="PrimaryLogPG",
        kind="code",
        name="PrimaryLogPG",
        parent_id="pg",
        description=(
            "+do_request(); +on_local_recover(); +on_global_recover(); "
            "+get_pgbackend(); +begin_peer_recover(); +on_activate(); +snap_trimmer()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/PrimaryLogPG.h",
    )
    upsert_el(
        c,
        id="OSDMap",
        kind="code",
        name="OSDMap",
        parent_id="osd_crush",
        description=(
            "+get_epoch(); +calc_pg_upmap(); +crush; +get_pg_acting(); "
            "+pg_to_up_acting_osds(); +get_primary()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/OSDMap.h",
    )

    # --- new code hotspots ---
    upsert_el(
        c,
        id="Messenger",
        kind="code",
        name="Messenger",
        parent_id="osd_svc",
        description="+start(); +shutdown(); +bind(); +send_message(); +connect_to()",
        technology="interface",
        url="https://github.com/ceph/ceph/blob/main/src/msg/Messenger.h",
    )
    upsert_el(
        c,
        id="OSDShard",
        kind="code",
        name="OSDShard",
        parent_id="osd_svc",
        description="+_process(); +register_and_wake_context(); +get_pg()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/OSD.h",
    )
    upsert_el(
        c,
        id="OpRequest",
        kind="code",
        name="OpRequest",
        parent_id="osd_svc",
        description="+get_req(); +mark_queued_for_pg(); +mark_started(); +mark_event()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/osd/OpRequest.h",
    )
    upsert_el(
        c,
        id="PGBackend",
        kind="code",
        name="PGBackend",
        parent_id="pg",
        description="+submit_transaction(); +recover_object(); +handle_message()",
        technology="base",
        url="https://github.com/ceph/ceph/blob/main/src/osd/PGBackend.h",
    )
    upsert_el(
        c,
        id="CrushWrapper",
        kind="code",
        name="CrushWrapper",
        parent_id="osd_crush",
        description="+do_rule(); +insert_item(); +get_item_id(); +get_type_id()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/crush/CrushWrapper.h",
    )

    # MON code (was empty)
    upsert_el(
        c,
        id="Monitor",
        kind="code",
        name="Monitor",
        parent_id="mon_core",
        description=(
            "+preinit(); +init(); +bootstrap(); +tick(); +ms_dispatch(); "
            "+win_election(); +lose_election(); +handle_command(); +shutdown()"
        ),
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/mon/Monitor.h",
    )
    upsert_el(
        c,
        id="Paxos",
        kind="code",
        name="Paxos",
        parent_id="mon_core",
        description="+is_active(); +is_updating(); +is_readable(); +is_lease_valid(); +trigger_propose()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/mon/Paxos.h",
    )
    upsert_el(
        c,
        id="MonMap",
        kind="code",
        name="MonMap",
        parent_id="mon_maps",
        description="+get_epoch(); +get_addrs(); +contains(); +add(); +remove()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/mon/MonMap.h",
    )
    upsert_el(
        c,
        id="OSDMonitor",
        kind="code",
        name="OSDMonitor",
        parent_id="mon_maps",
        description="+update_from_paxos(); +create_pending(); +encode_pending(); +check_osdmap_subs()",
        technology="class",
        url="https://github.com/ceph/ceph/blob/main/src/mon/OSDMonitor.h",
    )

    # relationships
    upsert_rel(
        c,
        id="r_am_msgr",
        from_id="AsyncMessenger",
        to_id="Messenger",
        description="implements",
    )
    upsert_rel(
        c,
        id="r_osd_shard",
        from_id="OSD",
        to_id="OSDShard",
        description="owns shards",
    )
    upsert_rel(
        c,
        id="r_osd_op",
        from_id="OSD",
        to_id="OpRequest",
        description="queues",
    )
    upsert_rel(
        c,
        id="r_pg_backend",
        from_id="PrimaryLogPG",
        to_id="PGBackend",
        description="uses",
    )
    upsert_rel(
        c,
        id="r_map_crush",
        from_id="OSDMap",
        to_id="CrushWrapper",
        description="uses",
    )
    upsert_rel(
        c,
        id="r_mon_paxos",
        from_id="Monitor",
        to_id="Paxos",
        description="owns",
    )
    upsert_rel(
        c,
        id="r_mon_monmap",
        from_id="Monitor",
        to_id="MonMap",
        description="uses",
    )
    upsert_rel(
        c,
        id="r_osdmon_osdmap",
        from_id="OSDMonitor",
        to_id="OSDMap",
        description="authors",
    )
    upsert_rel(
        c,
        id="r_mon_osdmon",
        from_id="Monitor",
        to_id="OSDMonitor",
        description="owns service",
    )

    try:
        v = tool_call(c, "validate_model", {"workspace_id": WS})
        note("validate_model", True, v)
    except Exception as e:
        note("validate_model", False, str(e))

    try:
        links = tool_call(c, "get_view_links", {"workspace_id": WS})
        note("get_view_links", True, links)
    except Exception as e:
        note("get_view_links", False, str(e))

    out = Path(__file__).resolve().parents[1] / "docs" / "research" / "ceph-c4-enrich-notes.md"
    ok_n = sum(1 for n in NOTES if n["ok"])
    fail_n = len(NOTES) - ok_n
    lines = [
        "# Ceph C4 enrich via MCP",
        "",
        f"Workspace `{WS}`. Steps OK={ok_n} FAIL={fail_n}.",
        "",
        "| Step | OK | Detail |",
        "|------|----|--------|",
    ]
    for n in NOTES:
        d = json.dumps(n["detail"], ensure_ascii=False)[:120] if n["detail"] is not None else ""
        lines.append(f"| `{n['step']}` | {n['ok']} | {d} |")
    out.write_text("\n".join(lines) + "\n")
    print("wrote", out)
    return 0 if fail_n == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
