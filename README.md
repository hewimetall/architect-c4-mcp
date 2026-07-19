# architect-c4

MCP-сервис рядом с вашим git-репозиторием: агент ведёт **C4**, **ADR** и **потоки поведения** как файлы в `docs/`.

```text
ваш-репо/
  docs/
    model.toml
    adr/{id}.toml
    flows/{id}.toml
```

На диске продукта — только TOML. Запись идёт через очередь на Rust. История — git.

Подробнее: [концепт](docs/CONCEPT.md) · [быстрый старт](docs/QUICKSTART.md) · [инструменты](docs/MCP_TOOLS.md) · [публикация](docs/PUBLISH.md)

## Установка

Нужен Python 3.12+. Rust не требуется.

```bash
uvx architect-c4-mcp --docs /abs/path/to/product/docs
```

или

```bash
uv tool install architect-c4-mcp
architect-c4-mcp --docs /abs/path/to/product/docs
```

пакет: https://pypi.org/project/architect-c4-mcp/

Альтернатива: образ `ghcr.io/hewimetall/architect-c4-mcp`.

## Быстрый старт

```bash
uvx architect-c4-mcp \
  --docs /abs/path/to/product/docs \
  --public-base https://c4.example.com
```

Эквивалент через env: `ARCHITECT_C4_DOCS` (CLI `--docs` важнее).

HTTP:

```bash
uvx architect-c4-mcp \
  --docs /abs/path/to/product/docs \
  --transport http --host 127.0.0.1 --port 8766
```

- MCP: `http://127.0.0.1:8766/mcp`
- Viewer: `http://127.0.0.1:8766/?layer=context`

### Cursor

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

## Что умеет агент

| Действие | Tools / prompts |
|----------|-----------------|
| Привязать `docs/` | `bind_docs` · prompt `sidecar_onboard` |
| Модель C4 | `upsert_element`, `upsert_relationship`, `validate_model` · `model_c4` |
| ADR (GFM → `.toml`) | `upsert_adr`, `set_adr_status` · `write_adr` |
| Сценарии | `upsert_flow`, `get_flow_diagram` · `write_flow` |
| Ссылки на схемы | `get_view_links`, `get_overview_diagram` · `validate_architecture` |

Промпты: https://gofastmcp.com/servers/prompts

## Docker

```bash
docker pull ghcr.io/hewimetall/architect-c4-mcp:latest
docker run --rm -p 8766:8766 \
  -v /abs/path/to/product/docs:/docs \
  -e ARCHITECT_C4_DOCS=/docs \
  -e ARCHITECT_C4_TRANSPORT=http \
  ghcr.io/hewimetall/architect-c4-mcp:latest
```

## Разработка

```bash
git clone https://github.com/hewimetall/architect-c4-mcp.git
cd architect-c4-mcp
uv sync --extra dev
uv run maturin develop
uv run architect-c4-mcp --docs ./examples/docs
```

## CI

| Событие | Что делает |
|---------|------------|
| push / PR | pytest, `cargo test`, lint, docker build |
| tag `v*` | coverage ≥93% (py+rust) → PyPI wheels, GitHub Release, GHCR |

## Лицензия

MIT — см. [LICENSE](LICENSE).
