# 3. Full C4 levels: context → container → component → code

## Status

Accepted

## Context

Product requirement: not only C1/C2. All C4 structural levels must be first-class
in model + diagram generation + browser drill-down.

## Decision

- `ElementKind` includes `code` (level 4).
- MCP: `get_overview_diagram` (context) + `get_layer_diagram(layer, parent_id?)`.
- HTTP: `GET /view/{workspace_id}?layer=&parent=` serves Mermaid HTML with click
  drill-down; Caddy routes `/view/*` to architect-c4.
- Default public base: `https://c4.example.com`.

## Consequences

Agents can model and visualize all four levels. Existing workspaces without
components/code still validate; empty layers render an empty-state diagram
**inside** the C4 boundary (never empty `""` Mermaid args — that breaks
Mermaid 11). Code level rendering is defined in ADR 0004 (`classDiagram`).
