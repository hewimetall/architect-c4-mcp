# WASM viewer (опционально)

Не входит в базовый happy path sidecar. Mermaid — основной рендерер.

Для All-layers / board UI: `&renderer=wasm` + crate `architect-c4-wasm`.  
Сборка WASM и детали — только если нужен интерактивный All-режим; в типовом Docker-образе sidecar достаточно Mermaid.
