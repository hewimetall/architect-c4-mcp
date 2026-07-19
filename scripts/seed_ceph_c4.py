#!/usr/bin/env python3
"""DEPRECATED for live process — use MCP tools/call via scripts/mcp_architect_client.py.

This file is an offline fallback only. Do not hardcode model data into the PR as the workflow.
"""

## Status
Accepted

## Context
Ceph OSD needs a local object backend. FileStore on XFS was legacy; BlueStore stores objects on raw devices with its own allocator and RocksDB metadata.

## Decision
Model ObjectStore as the component boundary and BlueStore as the primary Code implementation for RADOS OSD diagrams.

## Consequences
Code-level views show BlueStore implementing ObjectStore; FileStore can be added later as an alternate implementation.
"""
    print(
        "adr",
        j(
            n.upsert_adr(
                wid,
                __import__("json").dumps(
                    {
                        "id": "0001-bluestore-default",
                        "title": "BlueStore as default ObjectStore",
                        "status": "proposed",
                        "decided_at": "2026-07-16",
                        "scope_element_id": "objectstore",
                        "context": "Need ObjectStore for OSD diagrams.",
                        "decision": "BlueStore is the primary Code implementation.",
                        "consequences": "Code views show BlueStore implementing ObjectStore.",
                    }
                ),
                True,
            )
        ),
    )
    print(
        "adr_status",
        j(n.set_adr_status(wid, "0001-bluestore-default", "accepted", None, None, True)),
    )

    v = j(n.validate_workspace(wid))
    print("validate", json.dumps(v, indent=2)[:800])

    links = j(n.get_view_links(wid, BASE))
    print("view_links", json.dumps(links, indent=2)[:1200])

    model = j(n.get_model(wid))
    print(
        "counts",
        len(model.get("elements", [])),
        "elements",
        len(model.get("relationships", [])),
        "rels",
        len(model.get("decisions", [])),
        "adrs",
    )
    print("DONE", wid)


if __name__ == "__main__":
    main()
