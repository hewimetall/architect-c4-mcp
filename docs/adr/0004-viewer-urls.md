# ADR 0004: публичные URL viewer

## Статус

Accepted

## Контекст

Sidecar обслуживает один каталог `docs/`, поэтому viewer не должен требовать
идентификатор проекта в пути.

## Решение

Публичные маршруты:

```text
/view
/view/adrs
/view/adrs/{id}
/view/flows
/view/flows/{id}
```

Слой и режим диаграммы передаются query-параметрами: `layer`, `parent`, `mode`,
`renderer`.

## Последствия

- `get_view_links` возвращает URL от `ARCHITECT_C4_PUBLIC_BASE`.
- Mermaid остается рендерером по умолчанию.
- WASM включается опционально через `renderer=wasm`.
