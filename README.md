# architect-c4-mcp

MCP-sidecar: агент пишет **C4 + ADR + Flow** в `docs/` вашего git-репозитория.

**Концепт:** [docs/CONCEPT.md](docs/CONCEPT.md) · **Старт:** [docs/QUICKSTART.md](docs/QUICKSTART.md) · **PyPI:** [docs/PUBLISH.md](docs/PUBLISH.md)

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

## Локально без сборки

```bash
uvx architect-c4 \
  --docs /abs/path/to/product-repo/docs \
  --workspace-id default \
  --public-base https://c4.example.com
```

Cursor:

```json
{
  "mcpServers": {
    "architect-c4": {
      "command": "uvx",
      "args": [
        "architect-c4",
        "--docs", "/ABS/product/docs",
        "--workspace-id", "default",
        "--public-base", "https://c4.example.com"
      ]
    }
  }
}
```

Альтернатива: `docker pull ghcr.io/hewimetall/architect-c4-mcp:latest`.

## Разработка

```bash
uv sync --extra dev
uv run maturin develop
uv run architect-c4
```

## CI / release

| Событие | Workflow |
|---------|----------|
| push / PR | `.github/workflows/ci.yml` — pytest, cargo, coverage ≥93%, lint, docker |
| tag `v*` | `.github/workflows/release.yml` — PyPI wheels + GitHub Release + GHCR |
