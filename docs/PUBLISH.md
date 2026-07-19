# Публикация на PyPI

Пакет: **`architect-c4-mcp`** (готовые wheels + sdist).  
Триггер: git tag `v*` → workflow `.github/workflows/release.yml`  
(сначала job `quality`: pytest + cargo + coverage ≥93%, затем wheels / PyPI / GHCR).

## Один раз: Trusted Publisher

1. Аккаунт на https://pypi.org (для нового имени — «pending publisher» до первого upload).
2. PyPI → **Publishing** → **Add a new pending publisher** (или Settings проекта после первого релиза):
   - **PyPI project name:** `architect-c4-mcp` (должно совпадать с `[project].name`)
   - Owner: `hewimetall`
   - Repository: `architect-c4-mcp`
   - Workflow: `release.yml`
   - Environment: `pypi`
3. GitHub → Settings → Environments → создать **`pypi`** (опционально: required reviewers).
4. Secrets с API-токеном **не нужны** — OIDC (`id-token: write`).

## Релиз

```bash
# версия в pyproject.toml и Cargo crates = 0.3.6 → тег v0.3.6
git tag v0.3.6
git push origin v0.3.6
```

Workflow соберёт manylinux/musllinux/macOS/Windows wheels + sdist, зальёт на PyPI, сделает GitHub Release и пушнет образ в GHCR.

## Проверка после релиза

```bash
uvx architect-c4-mcp==0.3.6 --docs /path/to/docs
# или
pip install architect-c4-mcp==0.3.6
architect-c4-mcp --docs /path/to/docs
# алиас CLI: architect-c4
```

Локально без PyPI (smoke wheel):

```bash
uv run maturin build --release --out dist
uvx --from ./dist/architect_c4_mcp-*.whl architect-c4-mcp --docs /path/to/docs
```
