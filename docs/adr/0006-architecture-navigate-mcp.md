# ADR 0006: навигация по архитектуре через MCP (общий индекс с LSP)

## Статус

Proposed

## Дата

2026-07-20

## Контекст

Архитектура продукта в sidecar живёт в `docs/**/*.toml` и меняется через MCP
(`upsert_*`, `validate_model`, viewer). Разведка **кода** при этом часто идёт
отдельным стеком ([agent-lsp-real-inspect](https://github.com/hewimetall/agent-lsp-real-inspect)):
`list_symbols`, `go_to_definition`, `find_references`, `inspect_symbol`.

Между мирами нет общего навигационного контракта:

1. Нельзя из `from_id = "api"` / `scope_element_id` получить определение и все
   ссылки по `model.toml` + `adr/*.toml` + `flows/*.toml` одним вызовом.
2. `get_model()` отдаёт весь граф без позиций в файле и без «где ссылаются».
3. `validate_model()` возвращает problems без `path` / `line` / `character` в TOML.
4. Если навигацию сделать **только** как language runtime внутри agent-lsp,
   для тех, кто крутит один `architect-c4-mcp` в Cursor, это будет мёртвый
   функционал.
5. Слепое копирование сырого LSP API в MCP (`file` + `line` + `column` как
   единственный вход) тоже мертво: агент мыслит **id** элементов, а не курсором.

Нужен способ навигировать архитектуру **отдельно от agent-lsp**, не дублируя
два разных индекса и не ломая текущий writer-путь (очередь Rust → TOML).

## Решение

### Один SemanticIndex — два фасада

```text
docs/**/*.toml
    → SemanticIndex (Rust): id → Location, refs, hover-card, diagnostic spans
         ├─ MCP Navigate (обязательный фасад sidecar)
         └─ LSP façade (опционально): architect-c4-lsp для IDE / agent-lsp
```

- **MCP** остаётся основным интерфейсом агента в sidecar.
- **LSP-бинарник** — тонкий адаптер того же индекса (stdio / TCP `:3737`), а не
  единственная точка входа.
- Запись модели по-прежнему только через MCP + write queue; не через
  `workspace/applyEdit` в v1.

### Новые MCP tools (id-first)

| Tool | Назначение |
|------|------------|
| `resolve_symbol(id)` | Карточка символа + позиция определения (`path`/`line`/`character`) |
| `find_symbol_refs(id, include_declaration?)` | Все ссылки: `parent_id` / `from_id` / `to_id`, ADR scope / related_*, flow steps |
| `list_doc_symbols(path?)` | Символы файла (`docs/model.toml`, `docs/adr/…`) с позициями |
| `explore_symbol(id)` | Композит: resolve + refs + связанные ADR/flow + `view_url` |
| `locate_at(path, line, column)` | Опционально: позиция → символ (паритет с LSP) |

В MCP **не** выносить сырой JSON-RPC lifecycle (`initialize`, `didOpen`,
`textDocument/*` как tools).

### Обновления существующих tools

| Tool | Изменение |
|------|-----------|
| `validate_model` | Additive: у problem поля `path`, `line`, `character`, опционально `symbol_id` |
| `get_model` | Opt-in: `include_locations?: bool` (default `false`) |
| `bind_docs` | После bind индекс готов / сброшен; навигация доступна сразу |

### Промпт

Добавить `explore_architecture`: сначала navigate (`explore_symbol` /
`find_symbol_refs`), затем запись через `upsert_*`.

### Совместимость с agent-lsp (позже, не блокер)

Тот же Index обслуживает `architect-c4-lsp`. В agent-lsp:
`ensure_runtime(language="architecture"|"c4")` + scout tools.
Без agent-lsp sidecar уже самодостаточен.

### Вне scope v1

- Запись модели через LSP / navigate.
- Обязательный line/column как единственный MCP API.
- Второй отдельный индекс «только для MCP».
- Полный LSP spec сразу (completion/rename/hierarchy — после MVP navigate).

## Последствия

### Плюсы

- Навигация жива в одном `architect-c4-mcp` без зависимости от agent-lsp.
- Агент получает закрытие дыр: проверка scope ADR, поиск ссылок перед
  `upsert_relationship`, actionable `validate_model` с `path:line`.
- Один Index → нет рассинхрона MCP vs будущего LSP.
- Контракт MCP остаётся id-centric (удобно LLM).

### Минусы / цена

- Нужен span-aware разбор TOML в Rust (serde без позиций недостаточен).
- После записи через очередь нужен invalidate Index (watched files / явный
  refresh на завершении job).
- `locate_at` будет редким без сценария «агент уже читал файл построчно» —
  держать optional.

### Критерии готовности

1. Fixture `docs/` → `explore_symbol("…")` возвращает definition и ≥1 ref.
2. `validate_model` на dangling `from_id` содержит `path`/`line`.
3. Промпт `explore_architecture` и описание тулов в `docs/MCP_TOOLS.md` /
   `docs/QUICKSTART.md`.
4. (Опционально) тот же Index через `architect-c4-lsp` отвечает
   `list_symbols` в agent-lsp.

## Связанные артефакты

- Flow (sidecar example): `architecture-mcp-navigate-path` — путь только MCP.
- Flow (sidecar example): `architecture-lsp-scout-path` — опциональный путь через agent-lsp.
- Пример модели: `examples/docs/` (`model.toml`, ADR/Flow TOML).
- Upstream scout: https://github.com/hewimetall/agent-lsp-real-inspect
- LSP: https://microsoft.github.io/language-server-protocol/

## Примечание

Remote workspace `architect-c4-self` на момент записи этого ADR через Cursor MCP
был недоступен (tools discovery error / bearer). Канон решения для репозитория
проекта — этот файл в `docs/adr/`. TOML-копия для sidecar-демо лежит в
`examples/docs/adr/0009-architecture-lsp-interface.toml`.
