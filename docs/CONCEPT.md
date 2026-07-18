# Концепт: architect-c4 как sidecar

Сервис рядом с чужим git-репозиторием. Архитектура пишется **только** в его `docs/`.

Эталон модели:  
https://architecture.runmcp.ru/view/architect-c4-self?mode=all&renderer=wasm

---

## Зачем

Агент через MCP ведёт C4, ADR и потоки поведения как файлы в репо продукта — без Structurizr и без БД в git.

```text
репо-продукта/
  docs/
    model.toml           ← C4
    adr/{id}.toml        ← решения
    flows/{id}.toml      ← сценарии
architect-c4 (sidecar)   ← этот сервис, mount на docs/
```

Коммит `docs/` делает хост (или sidecar локально, без push).

---

## Правила

1. На диске в `docs/` — **только TOML**, без JSON-файлов.
2. **SQLite в репо нет.** Истина — toml; история — git.
3. Все записи идут через **очередь на Rust** (один writer).
4. Python — тонкий FastMCP; логика — Rust/PyO3.
5. Просмотр по умолчанию — **Mermaid**. WASM — по желанию, не в базовом образе.
6. Старт: смонтировал `docs/` и работаешь. Без `create_project` / bare / worktree.

Аргументы MCP по-прежнему объекты JSON в tool call — это не файлы на диске.  
`schemas/*.json` — только схема входа.

---

## Файлы

```text
docs/
  model.toml
  adr/{id}.toml
  flows/{id}.toml
```

**ADR:** поля Nygard + policy/refs; `context` / `decision` / `consequences` — GFM (таблицы, код, списки), без raw HTML; многострочники через `'''`; лимит prose **20000**; агент ставит только `draft` | `proposed`.  
Старые `.json` при bind один раз переписываются в `.toml`.

**Flow:** `c4_dynamic` | `sequence` | `state`; шаги ссылаются на существующие id элементов.

**model.toml:** список elements + relationships.

---

## Как работает запись

```text
агент → MCP upsert_* 
     → очередь Rust 
     → worker пишет toml (atomic)
     → опционально git commit
     → tool дожидается результата
```

Чтение — из снимка в памяти (перечитывается с диска при изменении файлов).

Очередь **в процессе**, не Redis и не SQLite. После рестарта недописанные jobs пропадают; на диске остаётся то, что уже записано в toml.

---

## Инструменты MCP

**Оставляем:**  
`upsert_element`, `upsert_relationship`, `delete_relationship`, `get_model`, `validate_model`,  
`upsert_adr`, `set_adr_status`, `get_adr`, `list_adrs`,  
`upsert_flow`, `get_flow`, `list_flows`, `delete_flow`, `get_flow_diagram`,  
`get_overview_diagram`, `get_layer_diagram`, `get_view_links`.

**Убираем из основного сценария:**  
`create_project`, `checkout_workspace` → вместо них bind на `ARCHITECT_C4_DOCS`.

**Сценарий:**

```text
старт с ARCHITECT_C4_DOCS
→ элементы и связи
→ validate_model
→ ADR / flow
→ get_view_links
→ git add docs && commit
```

---

## Промпты (FastMCP)

См. https://gofastmcp.com/servers/prompts

| Промпт | Зачем |
|--------|--------|
| `sidecar_onboard` | первый system в docs |
| `model_c4` | слой C4 |
| `write_adr` | ADR → `.toml` |
| `write_flow` | flow → `.toml` |
| `validate_architecture` | проверка модели |

Для ADR агенту:

```text
upsert_adr с object. status draft|proposed.
context/decision/consequences = GFM (таблицы, код, списки). Без raw HTML.
Файл: docs/adr/{id}.toml
```

---

## Границы продукта

| Есть | Нет |
|------|-----|
| domain, app, validate, policy, render | SQLite / JSON как SoT в `docs/` |
| model / adr / flow → toml + очередь | bare git + worktree как обязательный путь |
| git — commit на хосте в репо продукта | research-заметки в поставке |
| Mermaid viewer по умолчанию | WASM в базовом образе |

---

## Запуск

```bash
export ARCHITECT_C4_DOCS=/путь/к/репо/docs
export ARCHITECT_C4_PUBLIC_BASE=https://c4.example.com   # опционально
export ARCHITECT_C4_TRANSPORT=http                       # или stdio
uv run architect-c4
```

Docker:

```yaml
services:
  architect-c4:
    image: ghcr.io/hewimetall/architect-c4-mcp:latest
    volumes:
      - ./docs:/docs
    environment:
      ARCHITECT_C4_DOCS: /docs
      ARCHITECT_C4_TRANSPORT: http
      ARCHITECT_C4_PORT: "8766"
    ports:
      - "8766:8766"
```

---

## CI

- push/PR: тесты Python + Rust, coverage ≥ 93%, lint, сборка Docker  
- tag `v*`: GitHub Release + образ в GHCR  

Обязательные тесты: toml round-trip ADR/flow, rewrite json→toml, очередь сериализует запись, bind без `.db`, список промптов.

---

## Готово, когда

- для работы достаточно `ARCHITECT_C4_DOCS`
- в репо продукта появляются только `docs/**/*.toml`
- нет `.db` и JSON ADR/Flow
- параллельные upsert не ломают файлы
- ADR в viewer — HTML из GFM
- есть 5 промптов и зелёный CI с образом

## Не делаем в v1

Мульти-bare в DATA, Redis, обязательный WASM, хранение модели только в RAM без файлов.
