# architect-c4-mcp

MCP-sidecar: агент пишет **C4 + ADR + Flow** в `docs/` вашего git-репозитория.

**Концепт:** [docs/CONCEPT.md](docs/CONCEPT.md) · **Старт:** [docs/QUICKSTART.md](docs/QUICKSTART.md)

```text
ваш-репо/docs/
  model.toml
  adr/{id}.toml
  flows/{id}.toml
architect-c4 (sidecar)  ← FastMCP + очередь записи Rust
```

- на диске продукта — **только TOML** (без JSON и без SQLite в репо)
- запись через **очередь Rust**
- история — **git**
- промпты: https://gofastmcp.com/servers/prompts

Эталон: [architect-c4-self](https://architecture.runmcp.ru/view/architect-c4-self?mode=all&renderer=wasm)

## Быстрый старт

```bash
uv sync --extra dev
uv run maturin develop --manifest-path packages/architect-c4-app/Cargo.toml

export ARCHITECT_C4_DOCS=/abs/path/to/product-repo/docs
export ARCHITECT_C4_WORKSPACE_ID=default
export ARCHITECT_C4_PUBLIC_BASE=https://c4.example.com
uv run architect-c4
```

```text
bind_docs (или auto при ARCHITECT_C4_DOCS)
→ upsert_element / upsert_relationship
→ upsert_adr / upsert_flow
→ validate_model → get_view_links
→ git add docs && commit
```

## CI

| Событие | Workflow |
|---------|----------|
| push / PR | `.github/workflows/ci.yml` — pytest, cargo, coverage ≥93%, lint, docker build |
| tag `v*` | `.github/workflows/release.yml` — Release + GHCR |
