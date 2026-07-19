# Viewer

По умолчанию — **Mermaid** в браузере: `/?layer=context`.

| Режим | URL |
|-------|-----|
| Context / Container / Component / Code | `?layer=…&parent=…` |
| Все слои | `?mode=all` |
| Flows | `/flows`, `/flows/{id}` |
| ADR | `/adrs`, `/adrs/{id}` |
| WASM (опционально) | `&renderer=wasm` |

`ARCHITECT_C4_PUBLIC_BASE` должен быть `https://…` — иначе `get_view_links` отклонит базу.
