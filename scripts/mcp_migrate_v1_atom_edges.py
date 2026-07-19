#!/usr/bin/env python3
"""Migrate MCP workspaces to V1 atom/external relationship canon.

- Deletes relationships with container/component endpoints
- Recreates as code↔code, code↔external, or person/system↔system|external
- Notes OK/FAIL per step
"""
from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_architect_client import McpClient, tool_call  # noqa: E402

WORKSPACES = [
    "ceph-rados-c4",
    "demo",
    "ceph-mcp-live",
    "ws-rgw-usage",
]

SHELL = {"container", "component"}
ATOM = {"code", "external"}
CONTEXT = {"person", "software_system", "external"}


def enc_system(eid: str, by: dict[str, dict]) -> str | None:
    cur: str | None = eid
    while cur:
        e = by.get(cur)
        if not e:
            return None
        if e["kind"] == "software_system":
            return cur
        if e["kind"] == "external":
            return cur
        cur = e.get("parent_id")
    return None


def children_of(pid: str, by: dict[str, dict]) -> list[dict]:
    return [e for e in by.values() if e.get("parent_id") == pid]


def descendants(pid: str, by: dict[str, dict]) -> list[dict]:
    out: list[dict] = []
    stack = [pid]
    seen: set[str] = set()
    while stack:
        cur = stack.pop()
        if cur in seen:
            continue
        seen.add(cur)
        for ch in children_of(cur, by):
            out.append(ch)
            stack.append(ch["id"])
    return out


def pick_code_under(shell_id: str, by: dict[str, dict]) -> str | None:
    codes = [e for e in descendants(shell_id, by) if e["kind"] == "code"]
    if not codes:
        return None
    # Prefer non-stub names; stable sort
    codes.sort(key=lambda e: (len(e["name"]), e["id"]))
    return codes[0]["id"]


def ensure_code_stub(
    c: McpClient, ws: str, shell_id: str, by: dict[str, dict], notes: list
) -> str | None:
    """Ensure a component+code under container, or code under component."""
    shell = by.get(shell_id)
    if not shell:
        return None
    existing = pick_code_under(shell_id, by)
    if existing:
        return existing

    if shell["kind"] == "component":
        cid = f"{shell_id}__atom"
        try:
            tool_call(
                c,
                "upsert_element",
                {
                    "workspace_id": ws,
                    "id": cid,
                    "kind": "code",
                    "name": f"{shell['name']} (api)",
                    "parent_id": shell_id,
                    "description": "+entry()\nV1 atom stub for shell relationship migration",
                    "technology": "class",
                },
            )
            notes.append({"step": f"stub_code:{cid}", "ok": True})
            by[cid] = {
                "id": cid,
                "kind": "code",
                "parent_id": shell_id,
                "name": f"{shell['name']} (api)",
            }
            return cid
        except Exception as e:
            notes.append({"step": f"stub_code:{cid}", "ok": False, "detail": str(e)[:200]})
            return None

    if shell["kind"] == "container":
        # need component then code
        comps = [e for e in children_of(shell_id, by) if e["kind"] == "component"]
        if comps:
            return ensure_code_stub(c, ws, comps[0]["id"], by, notes)
        comp_id = f"{shell_id}__comp"
        code_id = f"{shell_id}__atom"
        try:
            tool_call(
                c,
                "upsert_element",
                {
                    "workspace_id": ws,
                    "id": comp_id,
                    "kind": "component",
                    "name": f"{shell['name']} Core",
                    "parent_id": shell_id,
                    "description": "V1 migration component shell",
                    "technology": "internal",
                },
            )
            by[comp_id] = {
                "id": comp_id,
                "kind": "component",
                "parent_id": shell_id,
                "name": f"{shell['name']} Core",
            }
            tool_call(
                c,
                "upsert_element",
                {
                    "workspace_id": ws,
                    "id": code_id,
                    "kind": "code",
                    "name": f"{shell['name']}Api",
                    "parent_id": comp_id,
                    "description": "+entry()\nV1 atom stub",
                    "technology": "class",
                },
            )
            by[code_id] = {
                "id": code_id,
                "kind": "code",
                "parent_id": comp_id,
                "name": f"{shell['name']}Api",
            }
            notes.append({"step": f"stub_comp_code:{code_id}", "ok": True})
            return code_id
        except Exception as e:
            notes.append({"step": f"stub_comp_code:{shell_id}", "ok": False, "detail": str(e)[:200]})
            return None
    return None


def migrate_ws(c: McpClient, ws: str) -> dict[str, Any]:
    notes: list[dict] = []
    model = tool_call(c, "get_model", {"workspace_id": ws})
    by = {e["id"]: e for e in model.get("elements", [])}
    rels = list(model.get("relationships", []))
    notes.append(
        {
            "step": "audit",
            "ok": True,
            "detail": {
                "elements": len(by),
                "relationships": len(rels),
                "kinds": {k: sum(1 for e in by.values() if e["kind"] == k) for k in sorted({e["kind"] for e in by.values()})},
            },
        }
    )

    shell_rels = []
    for r in rels:
        fk = by.get(r["from_id"], {}).get("kind", "?")
        tk = by.get(r["to_id"], {}).get("kind", "?")
        if fk in SHELL or tk in SHELL:
            shell_rels.append((r, fk, tk))

    created = 0
    deleted = 0
    skipped = 0

    for r, fk, tk in shell_rels:
        rid = r["id"]
        frm, to = r["from_id"], r["to_id"]
        desc = r.get("description") or "uses"
        new_from = None
        new_to = None
        new_id = f"v1:{rid}"

        # person → shell ⇒ person → enclosing system
        if fk == "person" and tk in SHELL:
            new_from = frm
            new_to = enc_system(to, by)
        elif tk == "person" and fk in SHELL:
            new_from = enc_system(frm, by)
            new_to = to
        # system → shell ⇒ system → enclosing system (or skip if same)
        elif fk == "software_system" and tk in SHELL:
            new_from = frm
            new_to = enc_system(to, by)
        elif tk == "software_system" and fk in SHELL:
            new_from = enc_system(frm, by)
            new_to = to
        # shell → shell ⇒ code stubs under each
        elif fk in SHELL and tk in SHELL:
            new_from = pick_code_under(frm, by) or ensure_code_stub(c, ws, frm, by, notes)
            new_to = pick_code_under(to, by) or ensure_code_stub(c, ws, to, by, notes)
        else:
            skipped += 1
            notes.append(
                {
                    "step": f"skip:{rid}",
                    "ok": False,
                    "detail": f"unhandled {fk}→{tk}",
                }
            )
            continue

        if not new_from or not new_to:
            skipped += 1
            notes.append(
                {
                    "step": f"skip:{rid}",
                    "ok": False,
                    "detail": f"no atom mapping for {frm}→{to}",
                }
            )
            continue
        if new_from == new_to:
            # delete shell edge only
            try:
                tool_call(c, "delete_relationship", {"workspace_id": ws, "id": rid})
                deleted += 1
                notes.append({"step": f"delete_only:{rid}", "ok": True, "detail": "same projected end"})
            except Exception as e:
                notes.append({"step": f"delete_only:{rid}", "ok": False, "detail": str(e)[:200]})
            continue

        try:
            tool_call(
                c,
                "upsert_relationship",
                {
                    "workspace_id": ws,
                    "id": new_id,
                    "from_id": new_from,
                    "to_id": new_to,
                    "description": desc,
                },
            )
            created += 1
            tool_call(c, "delete_relationship", {"workspace_id": ws, "id": rid})
            deleted += 1
            notes.append(
                {
                    "step": f"migrate:{rid}",
                    "ok": True,
                    "detail": f"{frm}→{to} => {new_from}→{new_to}",
                }
            )
        except Exception as e:
            notes.append(
                {
                    "step": f"migrate:{rid}",
                    "ok": False,
                    "detail": str(e)[:300],
                }
            )

    # Tag code without technology as class
    for e in list(by.values()):
        if e["kind"] == "code" and not (e.get("technology") or "").strip():
            try:
                tool_call(
                    c,
                    "upsert_element",
                    {
                        "workspace_id": ws,
                        "id": e["id"],
                        "kind": "code",
                        "name": e["name"],
                        "parent_id": e.get("parent_id"),
                        "description": e.get("description") or "+member()",
                        "technology": "class",
                    },
                )
                notes.append({"step": f"tag_class:{e['id']}", "ok": True})
            except Exception as ex:
                notes.append({"step": f"tag_class:{e['id']}", "ok": False, "detail": str(ex)[:160]})

    try:
        v = tool_call(c, "validate_model", {"workspace_id": ws})
        notes.append({"step": "validate_model", "ok": bool(v.get("ok", True)), "detail": {
            "ok": v.get("ok"),
            "problems": len(v.get("problems") or []),
            "errors": sum(1 for p in (v.get("problems") or []) if p.get("severity") == "error"),
            "non_atom_warns": sum(
                1
                for p in (v.get("problems") or [])
                if p.get("code") == "policy.baseline.non_atom_endpoint"
            ),
        }})
    except Exception as e:
        notes.append({"step": "validate_model", "ok": False, "detail": str(e)[:200]})

    try:
        m2 = tool_call(c, "get_model", {"workspace_id": ws})
        by2 = {e["id"]: e for e in m2.get("elements", [])}
        shell_left = 0
        for r in m2.get("relationships", []):
            fk = by2.get(r["from_id"], {}).get("kind")
            tk = by2.get(r["to_id"], {}).get("kind")
            if fk in SHELL or tk in SHELL:
                shell_left += 1
        notes.append(
            {
                "step": "post_audit",
                "ok": shell_left == 0,
                "detail": {
                    "relationships": len(m2.get("relationships", [])),
                    "shell_endpoint_rels_left": shell_left,
                    "created": created,
                    "deleted": deleted,
                    "skipped": skipped,
                },
            }
        )
    except Exception as e:
        notes.append({"step": "post_audit", "ok": False, "detail": str(e)[:200]})

    return {"workspace_id": ws, "notes": notes}


def main() -> int:
    c = McpClient("http://127.0.0.1:8766/mcp")
    c.call(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "migrate-v1-atoms", "version": "1"},
        },
    )
    c.notify("notifications/initialized", {})

    report = []
    for ws in WORKSPACES:
        print(f"\n===== {ws} =====")
        try:
            r = migrate_ws(c, ws)
            report.append(r)
            for n in r["notes"]:
                tag = "OK" if n["ok"] else "FAIL"
                d = n.get("detail")
                if isinstance(d, (dict, list)):
                    d = json.dumps(d, ensure_ascii=False)[:220]
                print(f"  [{tag}] {n['step']}: {d}")
        except Exception as e:
            print(f"  [FAIL] workspace: {e}")
            report.append({"workspace_id": ws, "notes": [{"step": "workspace", "ok": False, "detail": str(e)}]})

    Path("/tmp/mcp-migrate-v1-notes.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False)
    )
    print("\nWrote /tmp/mcp-migrate-v1-notes.json")
    fails = sum(1 for w in report for n in w["notes"] if not n["ok"] and n["step"].startswith("migrate"))
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
