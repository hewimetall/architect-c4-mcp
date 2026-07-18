# 0008 — Flow kinds v1 (c4_dynamic, sequence, state)

## Status

Accepted

## Context

We need behavior views (usage scenarios, protocols, lifecycles) linked to C4 elements and ADRs, without adopting BPMN or a Mermaid zoo that agents will abuse.

## Decision

Support three Flow kinds in v1:

1. **`c4_dynamic`** — ordered steps `{from_id,to_id}` against existing C4 elements (Structurizr Dynamic). Agent default.
2. **`sequence`** — Mermaid `sequenceDiagram` body + optional anchors.
3. **`state`** — Mermaid `stateDiagram*` body for lifecycles / window epochs.

Store rigid JSON under `docs/flows/{id}.json` (SQLite index + git commit), schema `schemas/flow.json`. Link via `related_adrs` / ADR `related_flows`.

Defer: flowchart, timeline. Reject as core: BPMN, journey, gantt.

## Consequences

- Viewer tab **Flows**; MCP `upsert_flow` / `list_flows` / `get_flow_diagram`.
- One ADR can illustrate many flows (e.g. RGW usage window policy).
- Agents must not invent element ids in `c4_dynamic` steps.
