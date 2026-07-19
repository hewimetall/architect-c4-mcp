"""MCP prompts (FastMCP ``@mcp.prompt``) — sidecar docs workflow.

See https://gofastmcp.com/servers/prompts
"""

from __future__ import annotations

from typing import Any


def register_prompts(mcp: Any) -> None:
    import os

    docs = os.environ.get("ARCHITECT_C4_DOCS", "./docs")

    @mcp.prompt(
        name="sidecar_onboard",
        title="Sidecar: старт с docs/",
        description="Привязать docs/ и создать первый software_system.",
        tags={"sidecar", "onboard"},
    )
    def sidecar_onboard(system_name: str, system_id: str = "system") -> str:
        return "\n".join(
            [
                "# Sidecar onboard",
                f"- docs: `{docs}`",
                f"- system: `{system_id}` / {system_name}",
                "",
                "Шаги:",
                "1. bind_docs()  # или --docs / ARCHITECT_C4_DOCS при старте",
                f"2. upsert_element(..., id={system_id!r}, kind='software_system', name=…)",
                "3. validate_model → get_view_links",
                "4. git add docs && commit на хосте",
                "",
                "На диске только docs/**/*.toml. Runtime без SQLite.",
            ]
        )

    @mcp.prompt(
        name="model_c4",
        title="Моделировать слой C4",
        description="Элементы и связи одного слоя C4.",
        tags={"c4"},
    )
    def model_c4(layer: str = "container", parent_id: str = "") -> str:
        return "\n".join(
            [
                "# Model C4",
                f"- layer: {layer}",
                f"- parent_id: {parent_id or '(нет)'}",
                "",
                "kinds: person|software_system|external|container|component|code",
                "1. upsert_element / upsert_relationship (через очередь Rust)",
                "2. validate_model",
                "3. get_layer_diagram",
                "Файл модели: docs/model.toml",
            ]
        )

    @mcp.prompt(
        name="write_adr",
        title="Записать ADR (toml + GFM)",
        description="Rigid ADR → docs/adr/{id}.toml",
        tags={"adr"},
    )
    def write_adr(adr_id: str, title: str, status: str = "draft") -> str:
        return "\n".join(
            [
                "# Write ADR",
                f"- id: {adr_id}",
                f"- title: {title}",
                f"- status: {status}  # агент: draft|proposed",
                "",
                "upsert_adr с object.",
                "context/decision/consequences = GFM (таблицы, код, списки). Без raw HTML.",
                f"Файл: docs/adr/{adr_id}.toml",
                "accepted/rejected — только set_adr_status (process).",
            ]
        )

    @mcp.prompt(
        name="write_flow",
        title="Записать Flow (toml)",
        description="c4_dynamic|sequence|state → docs/flows/{id}.toml",
        tags={"flow"},
    )
    def write_flow(flow_id: str, title: str, kind: str = "c4_dynamic") -> str:
        return "\n".join(
            [
                "# Write Flow",
                f"- id: {flow_id}",
                f"- title: {title}",
                f"- kind: {kind}",
                "",
                "steps.from_id/to_id должны существовать как C4 elements.",
                f"Файл: docs/flows/{flow_id}.toml",
            ]
        )

    @mcp.prompt(
        name="validate_architecture",
        title="Проверить модель",
        description="validate_model и разбор problems.",
        tags={"validate"},
    )
    def validate_architecture() -> str:
        return "\n".join(
            [
                "# Validate",
                "1. validate_model()",
                "2. Починить dangling relationships / parents / ADR scope",
                "3. Учесть policy.forbid у accepted ADR",
                "4. get_overview_diagram / get_view_links",
            ]
        )
