# architect-c4-mcp

MCP-sidecar: агент пишет C4, ADR и потоки в `docs/` вашего git-репозитория.

**Концепт:** [docs/CONCEPT.md](docs/CONCEPT.md)

```text
ваш-репо/docs/**/*.toml  ← истина
architect-c4 (sidecar)   ← FastMCP + очередь записи на Rust
```

- на диске только **TOML** (без JSON и без SQLite в репо)
- запись через **очередь Rust**
- история — **git**
- промпты FastMCP: https://gofastmcp.com/servers/prompts

Эталон: [architect-c4-self](https://architecture.runmcp.ru/view/architect-c4-self?mode=all&renderer=wasm)

Сейчас зафиксирован концепт публичного v1; дальше — trim кода из research-дерева под этот контракт.
