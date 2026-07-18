# 1. Hexagonal multi-crate + SQL revisions

## Status

Accepted

## Context

Need SOLID/DRY architecture, slim Python, no monolith Rust crate, ADR git fixation, TDD coverage ≥93%.

## Decision

- Hex ports in `architect-c4-domain`; adapters in small crates
- Append-only SQL `revisions` + `revision_heads` shared via `architect-c4-revision`
- ADR durable in git worktree with commit; SQLite indexes decisions
- Python FastMCP only calls `architect-c4-app` PyO3 façade

## Consequences

Clear testability per crate; higher coverage; ADR history via git log + SQL rev_no.
