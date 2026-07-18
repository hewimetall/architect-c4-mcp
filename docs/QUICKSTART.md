# Быстрый старт (sidecar)

## Локально без сборки (PyPI)

Нужен Python 3.12+ и [uv](https://github.com/astral-sh/uv) (или pip). **Rust не нужен.**

```bash
uvx architect-c4 \
  --docs /abs/path/to/product/docs \
  --workspace-id default \
  --public-base https://c4.example.com
```

Эквивалент через env: `ARCHITECT_C4_DOCS` (CLI `--docs` имеет приоритет).

Или поставить CLI в PATH:

```bash
uv tool install architect-c4
# либо: pip install architect-c4
architect-c4 --docs /abs/path/to/product/docs
```

HTTP:

```bash
uvx architect-c4 \
  --docs /abs/path/to/product/docs \
  --transport http --host 127.0.0.1 --port 8766
# MCP:  http://127.0.0.1:8766/mcp
# View: http://127.0.0.1:8766/view/default?layer=context
```

Флаги: `--docs`/`-d`, `--workspace-id`/`-w`, `--transport`, `--host`, `--port`, `--public-base`.

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
        "architect-c4",
        "--docs", "/ABS/product/docs",
        "--workspace-id", "default",
        "--public-base", "https://c4.example.com"
      ]
    }
  }
}
```

Пакет: https://pypi.org/project/architect-c4/

> Пока релиза на PyPI нет — сделайте tag `v*` после настройки Trusted Publisher (см. [PUBLISH.md](./PUBLISH.md)). Альтернатива без PyPI: Docker ниже.

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
uv run architect-c4
```

## Промпты агента

`sidecar_onboard` · `model_c4` · `write_adr` · `write_flow` · `validate_architecture`

```text
upsert_adr с object. status draft|proposed.
context/decision/consequences = GFM. Без raw HTML.
Файл: docs/adr/{id}.toml
```
