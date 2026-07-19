# Viewer

По умолчанию — **Mermaid** в браузере: `/view?layer=context`.

| Режим | URL |
|-------|-----|
| Context / Container / Component / Code | `?layer=…&parent=…` |
| Все слои | `?mode=all` |
| Flows | `/view/flows`, `/view/flows/{id}` |
| ADR | `/view/adrs`, `/view/adrs/{id}` |
| WASM (опционально) | `&renderer=wasm` |

`ARCHITECT_C4_PUBLIC_BASE` должен быть `https://…` — иначе `get_view_links` отклонит базу.
