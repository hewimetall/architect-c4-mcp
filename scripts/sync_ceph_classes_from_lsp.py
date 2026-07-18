#!/usr/bin/env python3
"""
Sync ALL top-level C++ classes from Ceph LSP symbol dumps into architect-c4 via MCP.

Input: directory of JSON files from agent-lsp list_symbols
  { "file_path": "...", "symbols": [ {name, kind, line, character}, ... ] }

kind 5 = Class/Struct (LSP SymbolKind.Class / Struct often both 5 here)
Top-level only: name without '.'  (nested types skipped for diagram noise)
Methods (kind 6) named Class.method → UML members (+method())
"""
from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call  # type: ignore

WS = "ceph-rados-c4"
KIND_CLASS = 5
KIND_METHOD = 6
KIND_CTOR = 9

# file prefix / exact → parent component
FILE_PARENT: list[tuple[str, str]] = [
    ("src/osd/PrimaryLogPG", "pg"),
    ("src/osd/PGBackend", "pg"),
    ("src/osd/ECBackend", "pg"),
    ("src/osd/ReplicatedBackend", "pg"),
    ("src/osd/PeeringState", "pg"),
    ("src/osd/PG.h", "pg"),
    ("src/osd/OSDMap", "osd_crush"),
    ("src/crush/", "osd_crush"),
    ("src/os/bluestore/", "objectstore"),
    ("src/os/ObjectStore", "objectstore"),
    ("src/msg/", "osd_svc"),
    ("src/osd/OpRequest", "osd_svc"),
    ("src/osd/Session", "osd_svc"),
    ("src/osd/OSD.h", "osd_svc"),
    ("src/mon/OSDMonitor", "mon_maps"),
    ("src/mon/MonMap", "mon_maps"),
    ("src/mon/MonmapMonitor", "mon_maps"),
    ("src/mon/PaxosService", "mon_core"),
    ("src/mon/Paxos", "mon_core"),
    ("src/mon/Monitor", "mon_core"),
]

SKIP_NAMES = {
    "ceph",
    "Scrub",
    "CrushTreeDumper",
    "encode",
    "decode",
    "operator<<",
    "print_osd_utilization",
}
SKIP_SUFFIX = ("Ref", "_t")  # keep some _t that are important? skip pure typedefs ending Ref
KEEP_T = {
    "osd_info_t",
    "osd_xinfo_t",
    "mon_info_t",
    "failure_info_t",
    "osdmap_manifest_t",
}

NOTES: list[dict] = []


def note(step: str, ok: bool, detail: Any = None) -> None:
    NOTES.append({"step": step, "ok": ok, "detail": detail})
    print(("OK" if ok else "FAIL"), step, "" if detail is None else str(detail)[:160])


def parent_for(file_path: str) -> str | None:
    for prefix, parent in FILE_PARENT:
        if file_path.startswith(prefix) or file_path == prefix or file_path.endswith(prefix):
            return parent
    return None


def sanitize_id(name: str) -> str:
    s = re.sub(r"[^A-Za-z0-9_]", "_", name)
    if s and s[0].isdigit():
        s = "C_" + s
    return s[:80]


def snake_member(method: str) -> str:
    # drop Class. prefix already handled; keep snake as-is for sanitize in server
    m = method.strip()
    if m.startswith("~") or m in {"operator=", "operator()", "operator<<"}:
        return ""
    if m.startswith("_") and m not in {"_dispatch"}:
        # keep private-ish as -method
        return f"-{m}()"
    return f"+{m}()"


def load_dump(path: Path) -> dict:
    raw = path.read_text()
    # agent-tools sometimes one-line JSON; sometimes wrapped
    data = json.loads(raw)
    if "result" in data and isinstance(data["result"], dict):
        data = data["result"]
    # content text wrapper from MCP tools/call
    if "content" in data and isinstance(data["content"], list):
        for c in data["content"]:
            if c.get("type") == "text":
                return json.loads(c["text"])
    return data


def extract_classes(dump: dict) -> list[dict]:
    file_path = dump.get("file_path") or dump.get("file") or ""
    parent = parent_for(file_path)
    if not parent:
        return []
    symbols = dump.get("symbols") or []
    # top-level classes
    classes: dict[str, dict] = {}
    methods: dict[str, list[str]] = defaultdict(list)
    for s in symbols:
        name = s.get("name") or ""
        kind = s.get("kind")
        if kind == KIND_CLASS:
            if "." in name:
                continue
            if name in SKIP_NAMES or name.startswith("("):
                continue
            if name.endswith("Ref"):
                continue
            if name.endswith("_t") and name not in KEEP_T:
                continue
            # Always store absolute GitHub blob URLs (viewer / Mermaid click need https://).
            path = str(file_path).lstrip("./")
            if path.startswith("ceph/"):
                path = path[len("ceph/") :]
            url = (
                path
                if path.startswith("https://")
                else f"https://github.com/ceph/ceph/blob/main/{path}"
            )
            classes[name] = {
                "id": sanitize_id(name),
                "name": name,
                "parent_id": parent,
                "url": url,
                "line": s.get("line"),
            }
        elif kind in (KIND_METHOD, KIND_CTOR) and "." in name:
            cls, _, meth = name.partition(".")
            if cls and meth and "." not in meth:
                mem = snake_member(meth)
                if mem and mem not in methods[cls]:
                    methods[cls].append(mem)

    out = []
    for name, meta in classes.items():
        mems = methods.get(name, [])[:12]
        if not mems:
            mems = ["+…()"]
        tech = "interface" if name in {"ObjectStore", "Messenger", "PGBackend"} else "class"
        if name in {"PG", "PaxosService"}:
            tech = "base"
        meta["description"] = "; ".join(mems)
        meta["technology"] = tech
        out.append(meta)
    return out


def main() -> int:
    dump_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/ceph-lsp-syms")
    if not dump_dir.is_dir():
        print("usage: sync_ceph_classes_from_lsp.py <dump_dir>", file=sys.stderr)
        return 2

    all_classes: dict[str, dict] = {}
    for path in sorted(dump_dir.glob("*.json")):
        try:
            dump = load_dump(path)
        except Exception as e:
            note(f"load:{path.name}", False, str(e))
            continue
        extracted = extract_classes(dump)
        note(f"parse:{path.name}", True, f"{len(extracted)} classes from {dump.get('file_path')}")
        for c in extracted:
            # Prefer richer members; never let a forward-decl (+…()) steal parent.
            prev = all_classes.get(c["id"])
            if prev:
                prev_n = prev["description"].count("+")
                new_n = c["description"].count("+")
                prev_stub = prev["description"].strip() in {"+…()", "+...()"}
                new_stub = c["description"].strip() in {"+…()", "+...()"}
                if new_stub and not prev_stub:
                    continue
                if not prev_stub and new_n <= prev_n:
                    continue
            all_classes[c["id"]] = c

    print(f"TOTAL unique top-level classes: {len(all_classes)}")

    c = McpClient("http://127.0.0.1:8766/mcp")
    c.call(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sync-ceph-classes", "version": "0.2"},
        },
    )
    c.notify("notifications/initialized")

    for meta in sorted(all_classes.values(), key=lambda x: (x["parent_id"], x["name"])):
        try:
            tool_call(
                c,
                "upsert_element",
                {
                    "workspace_id": WS,
                    "id": meta["id"],
                    "kind": "code",
                    "name": meta["name"],
                    "parent_id": meta["parent_id"],
                    "description": meta["description"],
                    "technology": meta["technology"],
                    "url": meta.get("url"),
                },
            )
            note(f"upsert:{meta['id']}", True, meta["parent_id"])
        except Exception as e:
            note(f"upsert:{meta['id']}", False, str(e))

    # key inheritance / implements
    rels = [
        ("r_bs_os", "BlueStore", "ObjectStore", "implements"),
        ("r_am_msgr", "AsyncMessenger", "Messenger", "implements"),
        ("r_plpg_pg", "PrimaryLogPG", "PG", "extends"),
        ("r_ec_pgbe", "ECBackend", "PGBackend", "extends"),
        ("r_rep_pgbe", "ReplicatedBackend", "PGBackend", "extends"),
        ("r_osdmon_ps", "OSDMonitor", "PaxosService", "extends"),
        ("r_pg_backend", "PrimaryLogPG", "PGBackend", "uses"),
        ("r_map_crush", "OSDMap", "CrushWrapper", "uses"),
        ("r_mon_paxos", "Monitor", "Paxos", "owns"),
        ("r_mon_osdmon", "Monitor", "OSDMonitor", "owns service"),
        ("r_osd_msgr", "OSD", "AsyncMessenger", "uses"),
        ("r_osd_svc", "OSD", "OSDService", "owns"),
    ]
    for rid, frm, to, desc in rels:
        if frm not in all_classes and frm not in {"BlueStore", "ObjectStore", "OSD", "AsyncMessenger"}:
            # still try — may exist from earlier seed
            pass
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
            note(f"rel:{rid}", True)
        except Exception as e:
            note(f"rel:{rid}", False, str(e))

    try:
        v = tool_call(c, "validate_model", {"workspace_id": WS})
        note("validate_model", True, v)
    except Exception as e:
        note("validate_model", False, str(e))

    out = Path(__file__).resolve().parents[1] / "docs" / "research" / "ceph-c4-all-classes-notes.md"
    ok_n = sum(1 for n in NOTES if n["ok"])
    lines = [
        "# Ceph — ALL top-level classes synced from LSP",
        "",
        f"Workspace `{WS}`. Classes upserted≈{len(all_classes)}. Steps OK={ok_n}/{len(NOTES)}.",
        "",
        "## Classes by parent",
        "",
    ]
    by_p: dict[str, list[str]] = defaultdict(list)
    for meta in all_classes.values():
        by_p[meta["parent_id"]].append(meta["name"])
    for p, names in sorted(by_p.items()):
        lines.append(f"### `{p}` ({len(names)})")
        lines.append(", ".join(f"`{n}`" for n in sorted(names)))
        lines.append("")
    lines += ["## Steps", "", "| Step | OK | Detail |", "|------|----|--------|"]
    for n in NOTES:
        d = json.dumps(n["detail"], ensure_ascii=False)[:100] if n["detail"] is not None else ""
        lines.append(f"| `{n['step']}` | {n['ok']} | {d} |")
    out.write_text("\n".join(lines) + "\n")
    print("wrote", out)
    return 0 if ok_n == len(NOTES) else 1


if __name__ == "__main__":
    raise SystemExit(main())
