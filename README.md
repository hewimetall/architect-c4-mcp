# architect-c4-mcp

C4 + ADR + Flow **MCP sidecar** for a host git repo’s `docs/` directory.

> Full concept (storage, Rust write-queue, strip plan, CI): **[docs/CONCEPT.md](docs/CONCEPT.md)**

## Idea

```text
your-repo/docs/{model,adr,flows}/*.toml  ← source of truth
architect-c4 (sidecar)                   ← FastMCP + Rust queue writer
```

- Persist: **TOML only** (no JSON files, no SQLite in the repo)
- Writes: **Rust in-process queue** → serial toml writer
- History: **git** on the host repo
- Agent API: FastMCP tools + [prompts](https://gofastmcp.com/servers/prompts)

Live reference model: [architect-c4-self](https://architecture.runmcp.ru/view/architect-c4-self?mode=all&renderer=wasm)

## Status

Concept locked for public sidecar v1. Implementation will import/trim from the research tree and apply D1–D8 in `docs/CONCEPT.md`.
