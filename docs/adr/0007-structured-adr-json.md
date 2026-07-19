# 0007 — Structured ADR JSON + executable policy

## Status

Accepted

## Context

Free-form markdown ADRs let agents hallucinate fields and cannot attach machine-enforceable graph rules. Policies must hot-reload without rebuilding the Rust/Python binary.

## Decision

1. ADRs are **rigid JSON** documents (`schemas/adr.json`) with Nygard fields: `context`, `decision`, `consequences`.
2. Optional `policy.forbid[]` rules are embedded in the ADR; only **`accepted`** ADRs enforce them.
3. Agent tools may set status **`draft` | `proposed`** only via `upsert_adr`.
4. Process tool `set_adr_status` sets `accepted|rejected|deprecated|superseded` (`rejected` requires `reason`).
5. Git fixation writes `docs/adr/{id}.json`; SQLite stores `body_json` as the index.
6. Baseline C4 kind/parent matrix always runs in `architect-c4-policy`; ADR forbids **add** denies only.

## Consequences

- Breaking: free-form ADR markdown writes were replaced by `upsert_adr(adr)`.
- Agents must pass schema-valid JSON (`deny_unknown_fields`).
- Viewer renders structured sections + policy rules.
