# Deploy templates

Copy these files to your host and **replace every placeholder**:

- `CURSOR.mcp.json` / `CURSOR.mcp.proxy.json` — `REPLACE_WITH_TOKEN_…`
- `vmcp.toml` — `master_password_argon2` (`vmcp hash-password`)
- `Caddyfile` / systemd — hostname and paths for **your** domain
- Public base URL via `ARCHITECT_C4_PUBLIC_BASE=https://…`

## architect-c4 env (V1 atom canon)

```bash
sudo mkdir -p /etc/architect-c4
sudo cp infra/deploy/systemd/architect-c4.env /etc/architect-c4/env
# edit PUBLIC_BASE to your https domain
sudo cp infra/deploy/systemd/architect-c4.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl restart architect-c4
```

Key vars in `/etc/architect-c4/env`:

| Var | V1 value | Notes |
|-----|----------|--------|
| `ARCHITECT_C4_ATOM_EDGES` | `1` | Writes: only code/external/person/system endpoints |
| `ARCHITECT_C4_PUBLIC_BASE` | `https://your.domain` | Viewer absolute links |
| `ARCHITECT_C4_DATA` | `/var/lib/architect-c4` | SQLite + worktrees |

Opt out only while migrating legacy shell↔shell models: `ARCHITECT_C4_ATOM_EDGES=0`.

Do not commit real tokens, password hashes, or SSH credentials.
