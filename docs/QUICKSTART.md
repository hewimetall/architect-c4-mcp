# Быстрый старт (sidecar)

## Локально без сборки (PyPI)

Нужен Python 3.12+ и [uv](https://github.com/astral-sh/uv) (или pip). **Rust не нужен.**

```bash
uvx architect-c4-mcp \
  --docs /abs/path/to/product/docs
```

Эквивалент через env: `ARCHITECT_C4_DOCS` (CLI `--docs` имеет приоритет).

Или поставить CLI в PATH:

```bash
uv tool install architect-c4-mcp
# либо: pip install architect-c4-mcp
architect-c4-mcp --docs /abs/path/to/product/docs
```

HTTP:

```bash
uvx architect-c4-mcp \
  --docs /abs/path/to/product/docs \
  --transport http --host 127.0.0.1 --port 8766
# MCP:  http://127.0.0.1:8766/mcp
# View: http://127.0.0.1:8766/view?layer=context
```

Флаги: `--docs`/`-d`, `--transport`, `--host`, `--port`, `--public-base`.

На диск продукта пишутся только:

```text
docs/model.toml
docs/adr/*.toml
docs/flows/*.toml
```

### Cursor (stdio, без клона репо)

```json
{
  "mcpServers": {
    "architect-c4": {
      "command": "uvx",
      "args": [
        "architect-c4-mcp",
        "--docs", "/ABS/product/docs",
        "--public-base", "https://c4.example.com"
      ]
    }
  }
}
```

Пакет: https://pypi.org/project/architect-c4-mcp/

## Docker (тоже без исходников)

```bash
docker pull ghcr.io/hewimetall/architect-c4-mcp:latest
docker run --rm -p 8766:8766 \
  -v /abs/path/to/product/docs:/docs \
  -e ARCHITECT_C4_DOCS=/docs \
  -e ARCHITECT_C4_TRANSPORT=http \
  -e ARCHITECT_C4_PUBLIC_BASE=https://c4.example.com \
  ghcr.io/hewimetall/architect-c4-mcp:latest
```

Или `docker compose -f docker-compose.sidecar.yml up` (сборка локального образа).

## Разработка из исходников

Нужны Python 3.12+, Rust stable, uv.

```bash
git clone https://github.com/hewimetall/architect-c4-mcp.git
cd architect-c4-mcp
uv sync --extra dev
uv run maturin develop
export ARCHITECT_C4_DOCS=/abs/path/to/product/docs
uv run architect-c4-mcp
```

## Промпты агента

`sidecar_onboard` · `model_c4` · `write_adr` · `write_flow` · `validate_architecture`

```text
upsert_adr с object. status draft|proposed.
context/decision/consequences = GFM. Без raw HTML.
Файл: docs/adr/{id}.toml
```
