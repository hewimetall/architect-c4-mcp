# Быстрый старт (sidecar)

## Требования

- Python 3.12+ ([uv](https://github.com/astral-sh/uv))
- Rust stable

## Установка

```bash
git clone https://github.com/hewimetall/architect-c4-mcp.git
cd architect-c4-mcp
uv sync --extra dev
uv run maturin develop --manifest-path packages/architect-c4-app/Cargo.toml
```

## Sidecar к репозиторию продукта

В продукте создайте `docs/` (или используйте существующий).

```bash
export ARCHITECT_C4_DOCS=/abs/path/to/product/docs
export ARCHITECT_C4_WORKSPACE_ID=default
export ARCHITECT_C4_PUBLIC_BASE=https://c4.example.com
export ARCHITECT_C4_TRANSPORT=http
export ARCHITECT_C4_PORT=8766
uv run architect-c4
```

- MCP: `http://127.0.0.1:8766/mcp`
- Viewer: `/view/{workspace_id}?layer=context`

На диск продукта пишутся только:

```text
docs/model.toml
docs/adr/*.toml
docs/flows/*.toml
```

SQLite-индексы живут **в памяти** процесса sidecar — не в git продукта.

## Cursor (stdio)

```json
{
  "mcpServers": {
    "architect-c4": {
      "command": "uv",
      "args": ["run", "--directory", "/ABS/architect-c4-mcp", "architect-c4"],
      "env": {
        "ARCHITECT_C4_DOCS": "/ABS/product/docs",
        "ARCHITECT_C4_WORKSPACE_ID": "default",
        "ARCHITECT_C4_PUBLIC_BASE": "https://c4.example.com"
      }
    }
  }
}
```

## Docker

```bash
docker compose -f docker-compose.sidecar.yml up --build
```

Смонтируйте `./docs` продукта в `/docs`.

## Промпты агента

`sidecar_onboard` · `model_c4` · `write_adr` · `write_flow` · `validate_architecture`

ADR:

```text
upsert_adr с object. status draft|proposed.
context/decision/consequences = GFM. Без raw HTML.
Файл: docs/adr/{id}.toml
```
