# ADR 0006: Matryoshka inside-out layout + per-shell port routing

## Status

Accepted (2026-07-17)

## Context

Flat All-layers calculation (pack roots → global elbows → collision patches) produced spaghetti: arrows through labels, center-derived “viewpoints”, and inconsistent compound obstacles.

## Decision

1. **Matryoshka layout** — size/place **inside-out** (code → component → container → system → context).
2. **Leaf pins** — relationship endpoints always attach to border ports on the real `from_id` / `to_id` (◇), never only to a parent wall and never to node centers.
3. **Hierarchical highways** (schematic style) — cross-shell nets are multi-segment:
   - ascend: leaf → sheet-entry ports on each enclosing shell
   - highway: trunk in the LCA channel between the two direct child blocks (магистраль), with parallel **tracks** when several nets share the same (A,B) pair
   - descend: sheet-entry → leaf
4. **Same-parent shortcut** — siblings under one shell keep a single orthogonal route in that shell.
5. **Scene owns geometry** — polylines + ports live on `SceneGraph`; WASM only draws.

## Consequences

- Reject “anchor only at LCA child” (that made component links look like container links).
- Reject global `route_orthogonal` + greedy `resolve_polyline` as the All-mode path (legacy may remain behind tests).
- Implementation: `architect-c4-scene/src/highway.rs` + research note `docs/research/schematic-highway-routing.md`.
- Mermaid All unchanged (fallback).
