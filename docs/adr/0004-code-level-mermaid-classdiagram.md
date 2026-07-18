# 4. C4 Code level uses Mermaid classDiagram

## Status

Accepted

## Context

C4 Level 4 (Code) zooms into a component and shows classes, interfaces, or
similar implementation structure. Agents and the browser viewer need a generator
that Mermaid 11 can actually parse.

Official Mermaid C4 diagram types are only:

- `C4Context`
- `C4Container`
- `C4Component`
- `C4Dynamic`
- `C4Deployment`

There is **no** `C4Code`. Inventing one produces a syntax error in the viewer.

Simon Brown's C4 guidance treats Level 4 as optional and typically drawn with
UML class diagrams (or ER / similar), often generated from source.

## Decision

- Keep domain `ElementKind::code` and `C4Layer::code`.
- Generate Level 4 diagrams as Mermaid **`classDiagram`** (not flowchart, not a
  fake `C4Code` type).
- Mapping convention (no new DB columns):
  - `name` — human label (note when it differs from id)
  - `id` — Mermaid class alias (sanitized)
  - `technology` — stereotype (`class`, `interface`, `enum`, language hint)
  - `description` — members, split on `;` or newlines
  - relationships — `extends`/`implements` keywords select `<|--` / `<|..`, else
    dependency `..>`
- Empty code layer emits a placeholder `class Empty` so the viewer never bombs.

## Consequences

- Agents must call `get_layer_diagram(layer="code", parent_id=<component>)`.
- Docs and the viewer legend state clearly: Class / Interface via classDiagram.
- Future: optional import from agent-lsp symbols into `kind=code` elements.

## WASM parity (2026-07)

`architect-c4-scene::build_code_uml` builds the same Level 4 semantics for
`?layer=code&renderer=wasm`:

- namespace frame = parent component
- class boxes with «stereotype», name, member compartments (`description`)
- `extends` / `implements` → hollow triangle (+ dashed for implements)
- association labels short (`uses` / `owns`); gaps expand so chips don’t sit on boxes

Mermaid remains the DSL path; WASM draws the scene graph.
