# 10. Structured code members (typed method params)

## Status
Accepted

## Context
C4 Level 4 uses UML classDiagram. Agents need typed method signatures
(`+send(message: Message) Message`) without freeform hallucination.
Freeform `description` lines remain for legacy models.

## Decision
- Add optional `members: CodeMember[]` on `Element` (`kind=code` only).
- Canonical JSON: `schemas/code_member.json`.
- Persist as `elements.members_json` (SQLite).
- Render via `CodeMember::to_uml_line` → Mermaid + WASM compartments.
- If `members` empty, fall back to UML-ish lines in `description`.

## Consequences
- MCP `upsert_element(..., members=[...])`.
- Sanitizers keep `:` for typed params.
