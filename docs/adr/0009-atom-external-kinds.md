# 0009 — Atom endpoints + `external` kind

## Status

Accepted

## Context

C4 truth for this product is code-level: class / interface / function. Databases, queues, SaaS, and similar entities are not OOP classes in the repo. Shell links (container→container) obscure that.

## Decision

1. **Atoms** = `kind=code` with `technology` stereotype `class` | `interface` | `function`.
2. **`kind=external`** = outside codebase (`datastore` | `queue` | `saas` | `identity` | `other` via technology/role).
3. **Canon relationships**: code↔code, code↔external, person↔system|external, system↔system|external.
4. **V1 default**: shell endpoints rejected on write (`ARCHITECT_C4_ATOM_EDGES=0` restores legacy dual-mode with warnings).
5. **V3 views**: Context/Container/Component Mermaid diagrams **project** atom edges upward (`project_relationships`). WASM All **bundles** atom magistrals onto shared outer trunks.

## Consequences

- Agents can model Postgres/S3/users without fake classes.
- Existing models keep working until strict mode is enabled.
- ADR `policy.forbid` kinds include `external`.
