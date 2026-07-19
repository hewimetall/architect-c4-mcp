# 2. Reject dangling relationships and invalid ADR scopes

## Status

Accepted

## Context

Smoke incident on `c4.example.com`: `upsert_relationship(r1, user→sys)` succeeded
even though elements were `s3ctl`/`cli`/`s3`. There was no delete tool, so
`validate_model` stayed `ok: false` with `relationship.dangling_endpoint` until
manual SQLite cleanup. ADR `#1` was also scoped to missing `sys`.

## Decision

1. `upsert_relationship` validates both endpoints exist before write.
2. Add `delete_relationship` (with delete revision) for recovery.
3. `upsert_adr` rejects `scope_element_id` that is not an element.

## Consequences

- Agents get immediate Validation errors instead of silent corruption.
- Ops can remove bad edges without SSH/SQLite.
- ADR scopes stay consistent with the C4 model.
