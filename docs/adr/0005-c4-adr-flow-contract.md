# ADR 0005: контракт C4, ADR и Flow

## Статус

Accepted

## Контекст

Агенту нужны структурные C4-слои, решения и сценарии поведения в одном
проверяемом контракте.

## Решение

- `model.toml` содержит elements и relationships.
- ADR содержит `context`, `decision`, `consequences`, refs и policy.
- Flow поддерживает `c4_dynamic`, `sequence`, `state`.
- Relationship и Flow steps ссылаются только на существующие element id.

## Последствия

- `validate_model` ловит висячие связи до публикации.
- Viewer показывает C4-диаграммы, ADR и Flow из одного `docs/`.
- Code level рендерится Mermaid `classDiagram`; WASM использует тот же смысловой
  scene graph.
