# Публикация на PyPI

Пакет: **`architect-c4`** (готовые wheels + sdist).  
Триггер: git tag `v*` → workflow `.github/workflows/release.yml`.

## Один раз: Trusted Publisher

1. Аккаунт на https://pypi.org (для нового имени — «pending publisher» до первого upload).
2. PyPI → **Publishing** → **Add a new pending publisher** (или Settings проекта после первого релиза):
   - Owner: `hewimetall`
   - Repository: `architect-c4-mcp`
   - Workflow: `release.yml`
   - Environment: `pypi`
3. GitHub → Settings → Environments → создать **`pypi`** (опционально: required reviewers).
4. Secrets с API-токеном **не нужны** — OIDC (`id-token: write`).

## Релиз

```bash
# версия в pyproject.toml и Cargo crates = 0.3.0 → тег v0.3.0
git tag v0.3.0
git push origin v0.3.0
```

Workflow соберёт manylinux/musllinux/macOS/Windows wheels + sdist, зальёт на PyPI, сделает GitHub Release и пушнет образ в GHCR.

## Проверка после релиза

```bash
uvx architect-c4==0.3.0 --docs /path/to/docs
# или
pip install architect-c4==0.3.0
architect-c4 --docs /path/to/docs
```

Локально без PyPI (smoke wheel):

```bash
uv run maturin build --release --out dist
uvx --from ./dist/architect_c4-*.whl architect-c4 --docs /path/to/docs
```
