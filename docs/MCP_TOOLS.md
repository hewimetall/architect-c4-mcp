# MCP tools (sidecar)

Python — тонкая обёртка над Rust (`architect_c4_app`). Аргументы tool call — JSON-объекты; на диск продукта пишется **только TOML**.

Промпты: `sidecar_onboard`, `model_c4`, `write_adr`, `write_flow`, `validate_architecture`  
(см. https://gofastmcp.com/servers/prompts).

## Sidecar / сессия

| Tool | Notes |
|------|-------|
| `bind_docs` | Привязка к каталогу `docs/` продукта; rewrite legacy `*.json` → `*.toml` |
| `create_session` | id сессии |
| `get_session` | + `active_workspace_id` |
| `list_sessions` | все сессии |
| `list_workspaces` | workspace + `view_url` |

`create_project` / `checkout_workspace` — legacy; happy path sidecar — `ARCHITECT_C4_DOCS` + `bind_docs`.

## Model

| Tool | Notes |
|------|-------|
| `upsert_element` | `kind`: person, software_system, container, component, code, external. Пишет `docs/model.toml` через очередь |
| `upsert_relationship` | оба конца должны существовать |
| `delete_relationship` | tombstone + revision |
| `get_model` | elements, relationships, decisions |
| `validate_model` | проблемы по слоям |

## ADR

| Tool | Notes |
|------|-------|
| `upsert_adr` | схема `schemas/adr.json` (wire); на диск — `docs/adr/{id}.toml`; статус агента `draft\|proposed`; prose GFM ≤ 20000 |
| `set_adr_status` | process: `accepted\|rejected\|deprecated\|superseded` |
| `get_adr` | один ADR |
| `list_adrs` | список + `view_url` |

## Flows

| Tool | Notes |
|------|-------|
| `upsert_flow` | схема `schemas/flow.json` (wire); на диск — `docs/flows/{id}.toml`; kinds `c4_dynamic\|sequence\|state` |
| `get_flow` / `list_flows` / `delete_flow` | CRUD |
| `get_flow_diagram` | Mermaid + `view_url` |

## Диаграммы / ссылки

| Tool | Notes |
|------|-------|
| `get_overview_diagram` | Context Mermaid |
| `get_layer_diagram` | `layer` + optional `parent_id` |
| `get_scene` | scene graph (опционально WASM) |
| `get_view_links` | HTTPS URLs (`ARCHITECT_C4_PUBLIC_BASE`) |

Браузер: `/view/{ws}?layer=context` (Mermaid по умолчанию).
