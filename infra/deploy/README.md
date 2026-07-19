# Deploy templates

Скопируйте шаблоны на хост и замените плейсхолдеры:

- `CURSOR.mcp.json` / `CURSOR.mcp.proxy.json` — `REPLACE_WITH_TOKEN_…`
- `vmcp.toml` — `master_password_argon2` (`vmcp hash-password`)
- `Caddyfile` / systemd — hostname и локальные пути для вашего домена
- `ARCHITECT_C4_PUBLIC_BASE=https://…` — публичная база viewer
- `ARCHITECT_C4_DOCS=/abs/product/docs` — единственный каталог данных продукта

## architect-c4 env

```bash
sudo mkdir -p /etc/architect-c4
sudo cp infra/deploy/systemd/architect-c4.env /etc/architect-c4/env
# edit PUBLIC_BASE to your https domain
sudo cp infra/deploy/systemd/architect-c4.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl restart architect-c4
```

Ключевые переменные в `/etc/architect-c4/env`:

| Переменная | Значение | Комментарий |
|-----|----------|--------|
| `ARCHITECT_C4_DOCS` | `/abs/product/docs` | mount на `docs/` продукта |
| `ARCHITECT_C4_PUBLIC_BASE` | `https://your.domain` | абсолютные ссылки viewer |
| `ARCHITECT_C4_TRANSPORT` | `http` | streamable HTTP MCP |

Не коммитьте реальные tokens, password hashes или SSH credentials.
