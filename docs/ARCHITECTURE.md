# Архитектура (sidecar)

См. полный контракт: [CONCEPT.md](./CONCEPT.md).

```text
Agent → FastMCP tools/prompts
         → Rust Write Queue (serial)
         → TomlWriter → docs/**/*.toml
         → Snapshot in-memory (SQLite :memory: только как индекс)
Browser → /view/* (Mermaid)
```

| Слой | Роль |
|------|------|
| `python/architect_c4` | FastMCP + prompts + `/view` |
| `architect-c4-app` | PyO3 façade, bind_docs, очередь |
| `architect-c4-queue` | in-process write Q |
| `architect-c4-tomlio` | atomic TOML IO, rewrite json→toml |
| model/adr/flow/validate/render | домен и диаграммы |

На диск продукта — только `docs/**/*.toml`. История — git.
