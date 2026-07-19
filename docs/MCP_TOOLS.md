# MCP tools (sidecar)

Python — тонкая обёртка над Rust (`architect_c4_app`). Аргументы tool call — JSON-объекты; на диск продукта пишется **только TOML**.

Промпты: `sidecar_onboard`, `model_c4`, `write_adr`, `write_flow`, `validate_architecture`  
(см. https://gofastmcp.com/servers/prompts).

## Sidecar

| Tool | Notes |
|------|-------|
| `bind_docs(docs_dir?)` | Привязка к каталогу `docs/` продукта |

Happy path: `uvx architect-c4-mcp --docs /path/to/docs`, затем обычные tool calls на привязанном `docs/`.

## Model

| Tool | Notes |
|------|-------|
| `upsert_element(id, kind, name, parent_id?, description?, technology?, url?, members?)` | `kind`: person, software_system, container, component, code, external. Пишет `docs/model.toml` через очередь |
| `upsert_relationship(id, from_id, to_id, description?)` | оба конца должны существовать |
| `delete_relationship(id)` | tombstone + revision |
| `get_model()` | elements, relationships, decisions |
| `validate_model()` | проблемы по слоям |

## ADR

| Tool | Notes |
|------|-------|
| `upsert_adr(adr, commit?)` | схема `schemas/adr.json` (wire); на диск — `docs/adr/{id}.toml`; статус агента `draft\|proposed`; prose GFM ≤ 20000 |
| `set_adr_status(id, status, reason?, superseded_by_id?, commit?, process_token?)` | process: `accepted\|rejected\|deprecated\|superseded` |
| `get_adr(id)` | один ADR |
| `list_adrs(base_url?)` | список + `view_url` |

## Flows

| Tool | Notes |
|------|-------|
| `upsert_flow(flow, commit?)` | схема `schemas/flow.json` (wire); на диск — `docs/flows/{id}.toml`; kinds `c4_dynamic\|sequence\|state` |
| `get_flow(id)` / `list_flows(base_url?)` / `delete_flow(id, commit?)` | CRUD |
| `get_flow_diagram(id, base_url?)` | Mermaid + `view_url` |

## Диаграммы / ссылки

| Tool | Notes |
|------|-------|
| `get_overview_diagram(base_url?)` | Context Mermaid |
| `get_layer_diagram(layer, parent_id?, base_url?)` | `layer` + optional `parent_id` |
| `get_scene(mode?, layer?, focus?)` | scene graph (опционально WASM) |
| `get_view_links(base_url?)` | HTTPS URLs (`ARCHITECT_C4_PUBLIC_BASE`) |

Браузер: `/view?layer=context` (Mermaid по умолчанию), `/view/adrs/{id}`, `/view/flows/{id}`.
