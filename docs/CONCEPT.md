# architect-c4-mcp — полный концепт (sidecar)

База: дерево `ai-research-structure-with-lsp` (клон на момент планирования).  
Цель: публичный MCP-сервис, который живёт **рядом** с чужим git-репозиторием и пишет архитектуру только в его `docs/`.

Живой reference-модель:  
https://architecture.runmcp.ru/view/architect-c4-self?mode=all&renderer=wasm

---

## 1. Зачем

Агент (Cursor / другой MCP-клиент) моделирует C4 + ADR + Flow **как файлы в репозитории продукта**, без Structurizr и без отдельной БД в git.

```text
product-repo/                 ← чужой git
  docs/
    model.toml                ← C4 elements + relationships
    adr/{id}.toml             ← ADR (GFM prose)
    flows/{id}.toml           ← behavior flows
architect-c4                  ← sidecar (этот сервис)
  ARCHITECT_C4_DOCS=/…/docs
```

Хост коммитит `docs/` сам (или sidecar делает локальный `git commit` без push).

---

## 2. Нефункциональные решения (зафиксировано)

| # | Решение |
|---|---------|
| D1 | Persist **только TOML** в `docs/**` — **без JSON-файлов** |
| D2 | **SQLite не храним в репо** и не делаем source of truth |
| D3 | Все мутации идут через **Rust write-queue** (serial worker) |
| D4 | История = **git history** хост-репо, не SQL `rev_no` |
| D5 | Python = тонкий FastMCP; логика = Rust/PyO3 (`architect-c4-app`) |
| D6 | Default viewer = **Mermaid**; WASM/All-mode — опционально / later |
| D7 | MCP prompts по [FastMCP Prompts](https://gofastmcp.com/servers/prompts) |
| D8 | Happy path без `create_project` / bare worktree |

Wire-формат MCP по-прежнему **JSON object** в tool args (`upsert_adr(adr: object)`).  
Это не persist. JSON Schema в `schemas/*.json` остаётся контрактом валидации входа.

---

## 3. On-disk layout (истина)

```text
{ARCHITECT_C4_DOCS}/
  model.toml                 # единый C4 snapshot
  adr/
    0007-structured-adr.toml
    …
  flows/
    mcp-upsert-element.toml
    …
```

### 3.1 `model.toml` (черновик формы)

```toml
workspace_id = "default"

[[elements]]
id = "system"
kind = "software_system"
name = "My Product"
description = "…"
parent_id = ""

[[elements]]
id = "api"
kind = "container"
name = "API"
parent_id = "system"
technology = "Python"

[[relationships]]
id = "r1"
from_id = "agent"
to_id = "api"
description = "HTTPS"
```

### 3.2 `docs/adr/{id}.toml`

- Поля Nygard + policy/refs/related_flows (как `schemas/adr.json`, но файл — TOML).
- `context` / `decision` / `consequences` = **GFM** (tables, code, lists), **без raw HTML**.
- Multiline через TOML `'''…'''`.
- Prose max **20000** символов на поле.
- Agent status: только `draft` | `proposed`.
- Process status: `accepted` | `rejected` | `deprecated` | `superseded` через `set_adr_status`.
- Legacy `docs/adr/*.json` → одноразовый rewrite в `.toml` при bind (`rewrite_legacy_adrs`), после rewrite JSON = 0.

### 3.3 `docs/flows/{id}.toml`

- Kinds: `c4_dynamic` (default) | `sequence` | `state`.
- `c4_dynamic.steps` ссылаются на существующие element ids.
- Тоже только `.toml` (не `.json`).

### 3.4 Чего нет в репо

- `*.db`, `.architect-c4-data/`, bare git, worktrees сервиса
- JSON persist под `docs/`

---

## 4. Runtime архитектура

### 4.1 Контейнеры (sidecar v1)

```text
┌─────────────────────────────────────────────────────────┐
│ architect-c4 (process)                                  │
│  ┌──────────────┐   enqueue    ┌─────────────────────┐  │
│  │ FastMCP      │ ───────────► │ Rust Write Queue    │  │
│  │ tools/prompts│              │ (serial, in-proc)   │  │
│  │ + /view HTML │ ◄── wait ─── │        │            │  │
│  └──────┬───────┘              │        ▼            │  │
│         │ read                 │  TomlWriter worker  │  │
│         ▼                      │        │            │  │
│  In-memory Snapshot            │        ▼            │  │
│  (from docs/**/*.toml)         │  docs/**/*.toml     │  │
│         │                      │  (+ optional git)   │  │
│         ▼                      └─────────────────────┘  │
│  render (Mermaid) → /view/{ws}                          │
└─────────────────────────────────────────────────────────┘
         ▲ mounts
         │
   host docs/  +  (optional) host .git for commit
```

### 4.2 Write path (обязательный)

```text
MCP upsert_* / delete_* / set_adr_status
  → validate input (Pydantic / Rust schema)
  → enqueue Job { op, payload, workspace }
  → worker (один):
       load snapshot (если нужно)
       apply mutation
       atomic write toml (tmp + rename)
       optional: git add + commit in host repo (no push)
       publish SnapshotRev { mtime or git_sha }
  → tool ждёт done (default) или возвращает job_id
```

**Почему очередь**

- один writer → нет гонок toml;
- MCP UX может ждать через progress / optional FastMCP tasks;
- рестарт: in-flight jobs теряются, **истина на диске** перечитывается при bind.

Очередь = **Rust in-process Q** (не Redis, не Docket-as-queue, не SQLite TaskStore в репо).  
FastMCP `task=True` допустим только как protocol wait-слой (как в agent-lsp ADR-0003), без durable SQLite queue.

### 4.3 Read path

```text
get_model / get_adr / list_* / validate / get_*_diagram / get_view_links
  → Snapshot (RAM)
  → при cache miss / mtime change: reload toml tree
```

`get_scene` / WASM — вне critical path sidecar v1 (см. §8).

---

## 5. MCP surface

### 5.1 Оставляем (ядро)

| Группа | Tools |
|--------|-------|
| Model | `upsert_element`, `upsert_relationship`, `delete_relationship`, `get_model`, `validate_model` |
| ADR | `upsert_adr`, `set_adr_status`, `get_adr`, `list_adrs` |
| Flow | `upsert_flow`, `get_flow`, `list_flows`, `delete_flow`, `get_flow_diagram` |
| View | `get_overview_diagram`, `get_layer_diagram`, `get_view_links` |

### 5.2 Меняем happy path

| Было (клон) | Станет |
|-------------|--------|
| `create_project` → bare under DATA | **убрать из happy path** / deprecate |
| `checkout_workspace` → gix worktree | **`bind_docs`** (или auto-bind из `ARCHITECT_C4_DOCS`) |
| SQLite revisions | git sha / file mtime в ответах |
| `docs/adr/{id}.json` | `docs/adr/{id}.toml` |
| `docs/flows/{id}.json` | `docs/flows/{id}.toml` |

Новый минимальный онбординг:

```text
(start with ARCHITECT_C4_DOCS)
→ upsert_element / upsert_relationship
→ validate_model
→ upsert_adr / upsert_flow
→ get_view_links
→ host: git add docs && git commit
```

Sessions (`create_session` / `list_sessions`) — опционально, тонкий in-memory; не persist в репо.

### 5.3 Prompts (FastMCP `@mcp.prompt`)

Источник: https://gofastmcp.com/servers/prompts

| Prompt | Назначение |
|--------|------------|
| `sidecar_onboard` | mount docs → первый `software_system` |
| `model_c4` | слой context/container/component/code |
| `write_adr` | rigid ADR, GFM, status draft\|proposed, persist `.toml` |
| `write_flow` | flow kinds → `.toml` |
| `validate_architecture` | `validate_model` + интерпретация problems |

Текст для агента (ADR):

```text
upsert_adr с object. status draft|proposed.
context/decision/consequences = GFM (tables, code, lists). Без raw HTML.
Persist: docs/adr/{id}.toml
```

---

## 6. Crate map: было → sidecar v1

База — hex crates из клона.

| Crate (клон) | Судьба |
|--------------|--------|
| `architect-c4-domain` | **keep** — entities/ports/errors |
| `architect-c4-app` | **keep** — PyO3 façade + queue wiring |
| `architect-c4-model` | **rework** — apply mutations to snapshot; persist via toml writer |
| `architect-c4-adr` | **rework** — read/write `.toml`, GFM limits, legacy rewrite |
| `architect-c4-flow` | **rework** — `.toml` instead of `.json` |
| `architect-c4-validate` | **keep** — problems list + policy.forbid |
| `architect-c4-policy` | **keep** |
| `architect-c4-render` | **keep** — Mermaid + HTML chrome (+ GFM→HTML для ADR: pulldown-cmark + ammonia) |
| `architect-c4-git` | **shrink** — только commit в **уже существующий** host repo (опционально); убрать обязательный bare/worktree |
| `architect-c4-session` | **shrink** — optional in-memory / drop from happy path |
| `architect-c4-revision` | **remove from persist path** — history = git |
| `architect-c4-scene` | **optional** — нужен для WASM All-mode; не блокер sidecar |
| `architect-c4-wasm` | **optional / out of default image** |
| NEW `architect-c4-queue` | **add** — Rust write queue + worker |
| NEW `architect-c4-toml` | **add** (или внутри adr/flow/model) — serde toml, atomic write, `'''` prose |

Python: `python/architect_c4/server.py` + `prompts.py` — тонкая обёртка.

---

## 7. Env / запуск

```bash
# required
export ARCHITECT_C4_DOCS=/abs/path/to/product-repo/docs

# optional
export ARCHITECT_C4_WORKSPACE_ID=default
export ARCHITECT_C4_PUBLIC_BASE=https://c4.example.com   # https only for absolute links
export ARCHITECT_C4_TRANSPORT=http                       # or stdio
export ARCHITECT_C4_HOST=127.0.0.1
export ARCHITECT_C4_PORT=8766
export ARCHITECT_C4_GIT_COMMIT=1                         # sidecar commits docs/ locally
export ARCHITECT_C4_PROCESS_TOKEN=…                      # gate for set_adr_status
```

**Нет** `ARCHITECT_C4_DATA` с sqlite в продуктовом контракте.

### Docker sidecar

```yaml
services:
  architect-c4:
    image: ghcr.io/hewimetall/architect-c4-mcp:latest
    volumes:
      - ./docs:/docs
      # optional: whole repo if local git commit enabled
      # - ./:/repo
    environment:
      ARCHITECT_C4_DOCS: /docs
      ARCHITECT_C4_TRANSPORT: http
      ARCHITECT_C4_PORT: "8766"
      ARCHITECT_C4_PUBLIC_BASE: https://c4.example.com
    ports:
      - "8766:8766"
```

Cursor stdio: `uv run architect-c4` с `ARCHITECT_C4_DOCS`.

---

## 8. Viewer

| Режим | Sidecar v1 |
|-------|------------|
| Mermaid layer diagrams | **да** (`/view/{ws}?layer=…`) |
| ADR pages | **да** — GFM→HTML, `<div class="prose">` |
| Flows pages | **да** |
| WASM `?mode=all&renderer=wasm` | **не в default**; код можно оставить в repo как optional feature |
| Absolute `view_url` | через `ARCHITECT_C4_PUBLIC_BASE` |

---

## 9. Что выпилить из public tree (из клона)

| Удалить / не публиковать | Почему |
|--------------------------|--------|
| `docs/research/**` | research notes, не продукт |
| `.cursor/**` | agent noise |
| JSON ADR/Flow examples & docs saying `.json` persist | контракт toml |
| Happy-path bare/worktree docs | sidecar bind |
| Deploy templates с реальными секретами | только placeholders |
| SQL revision как продуктовая фича | git history |
| WASM как обязательный runtime | optional |

Документы клона, которые переписываем под концепт:  
`README.md`, `docs/QUICKSTART.md`, `docs/ARCHITECTURE.md`, `docs/MCP_TOOLS.md`, ADR-0001/0007/0008 (+ ADR-0011 toml/GFM).

---

## 10. Качество и CI

Сохраняем планку клона:

- TDD; median coverage **≥ 93%** (Python + Rust)
- ruff / rustfmt / clippy `-D warnings`

Workflows:

| Workflow | Триггер | Делает |
|----------|---------|--------|
| `ci.yml` | push/PR | uv sync, maturin develop, pytest, cargo test, coverage, lint |
| `ci.yml` (+job) | push/PR | `docker build` smoke |
| `release.yml` | tag `v*` | GitHub Release + **GHCR** image `architect-c4-mcp` |

Новые тесты (обязательные):

1. Toml round-trip ADR (GFM table/code golden)
2. Legacy json→toml rewrite
3. Flow toml round-trip
4. Queue serializes concurrent upserts
5. Bind docs без sqlite files
6. Prompt registration (`prompts/list` содержит 5 имён)

---

## 11. Миграция с клона (порядок работ)

```text
1. Import tree из склонированного ai-research-structure-with-lsp
2. Добавить architect-c4-queue + toml writers
3. Переключить adr/flow/model persist → docs/**/*.toml
4. Убрать SQL revision / DATA sqlite из happy path
5. bind_docs + rewrite_legacy_*(json→toml)
6. prompts.py + skill
7. Выпилить research / упростить README под sidecar
8. Docker compose example + CI docker/release image
9. Прогнать make test && make cov
```

Совместимость MCP object API: **сохранить** имена tools и форму `upsert_adr(workspace_id, adr: object)` — меняется только диск и внутренний write path.

---

## 12. Границы ответственности

| Кто | Делает |
|-----|--------|
| Sidecar | validate, queue write, toml IO, mermaid/view, prompts |
| Host git repo | source of truth files, review, push, CI продукта |
| Agent | вызывает tools/prompts; не пишет raw HTML в ADR |
| Process role | `set_adr_status` → accepted/rejected/… |

---

## 13. Критерии готовности public v1

- [ ] Запуск: только `ARCHITECT_C4_DOCS` (+ optional PUBLIC_BASE)
- [ ] После работы агента в репо появляются **только** `docs/**/*.toml`
- [ ] В репо **нет** `.db` / JSON ADR/Flow
- [ ] Параллельные `upsert_*` не портят toml (queue)
- [ ] Viewer отдаёт ADR prose как HTML из GFM
- [ ] `prompts/list` ≥ onboard/model/adr/flow/validate
- [ ] CI зелёный; image собирается
- [ ] README описывает sidecar за < 1 экрана

---

## 14. Нецели v1

- Multi-tenant bare git factory внутри DATA
- Redis / external broker
- Structurizr / DSL кроме toml + mermaid
- Обязательный WASM All-mode в default image
- Хранение модели «только в памяти без файлов»

---

*Концепт опирается на клон `ai-research-structure-with-lsp` и live workspace `architect-c4-self`; решения D1–D8 — продуктовые ограничения публичного sidecar.*
