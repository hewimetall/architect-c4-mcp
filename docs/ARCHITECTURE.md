# Архитектура (sidecar)

См. полный контракт: [CONCEPT.md](./CONCEPT.md).

```text
Agent → FastMCP tools/prompts
         → Rust Write Queue (serial)
         → TomlWriter → docs/**/*.toml
         → HashMap snapshot (load on bind_docs)
Browser → /, /adrs, /flows (Mermaid)
```

| Слой | Роль |
|------|------|
| `python/architect_c4` | FastMCP + prompts + `/` |
| `architect-c4-app` | PyO3 façade, bind_docs, очередь |
| `architect-c4-queue` | in-process write Q |
| `architect-c4-tomlio` | atomic TOML IO, rewrite json→toml |
| model/adr/flow/validate/render | домен и диаграммы (HashMap + TOML) |

На диск продукта — только `docs/**/*.toml`. Runtime без SQLite. История — git.
