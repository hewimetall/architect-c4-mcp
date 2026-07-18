# Security

## Reporting

If you find a vulnerability in **architect-c4**, open a private GitHub security advisory
on this repository (or email the maintainers). Do not file a public issue with exploit details.

## Secrets & open-source hygiene

This repo is intended to be public. **Do not commit:**

| Kind | Examples |
|------|----------|
| Tokens | vmcp bearer tokens, `tokens.json`, API keys |
| Passwords | SSH passwords, argon2 master hashes with real salts |
| Private keys | `*.pem`, `id_rsa`, TLS material |
| Local data | `.data/`, SQLite DBs with real models |
| Env files | `.env`, `.env.local` |

Deploy templates under `infra/deploy/` use placeholders:

- `REPLACE_WITH_TOKEN_FROM_SERVER_…`
- `master_password_argon2 = "$argon2id$…$REPLACE_ME$REPLACE_ME"`
- Public base `https://c4.example.com` (override with `ARCHITECT_C4_PUBLIC_BASE`)

Generate a real argon2 hash on the server:

```bash
vmcp hash-password --password '…'
```

## Viewer `base_url`

`get_view_links` / HTML viewer links require **`https://`** absolute bases
(`normalize_public_base`). `javascript:`, `data:`, and userinfo (`user@host`) are rejected.

## Auth model

- Slim FastMCP process binds locally by default (`127.0.0.1`).
- Internet exposure is expected **behind vmcp** (or another reverse proxy) with
  static bearer tokens — not anonymous public MCP.
