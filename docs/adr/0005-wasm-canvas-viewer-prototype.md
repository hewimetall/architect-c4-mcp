# 5. WASM Canvas2D viewer prototype + All-layers mode

## Status

Accepted

## Context

Mermaid C4 layout piles Rel labels; users want a native canvas path and a mode
to see all C4 layers at once. WebGPU (`navigator.gpu` / wgpu-web) is not
universal in 2026; WebGL2 is wider; Canvas2D is universal.

## Decision

1. Add `architect-c4-scene` (model → scene graph + hierarchical layout).
2. Prototype renderer: **WASM + Canvas2D** (`wasm-bindgen` / `web-sys`).
3. Optional later: WebGL2 (`glow`) / WebGPU (`wgpu`) behind feature-detect.
4. Keep Mermaid as **default** for MCP and no-WASM clients (`renderer=mermaid`).
5. New view mode **`mode=all`**: nested Context→Container→Component→Code in one
   scene; `focus=` scopes to a container/system.
6. Top nav: Diagrams | All | ADRs.

## Consequences

- Deploy may ship prebuilt `python/architect_c4/static/wasm/`.
- Agents still get Mermaid via `get_layer_diagram`; `get_scene` exposes JSON.
- Coverage: include `architect-c4-scene` in rust median gate.
