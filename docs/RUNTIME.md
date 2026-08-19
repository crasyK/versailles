# Runtime

Versailles is a tray app. It serves `Documents\Widgets` over localhost and loads `desktop/index.html` as the wallpaper.

## Ports

| What | Default |
| ---- | ------- |
| Local API + file server | `http://127.0.0.1:47831` |
| Vite (dev only) | `http://localhost:1420` |

Override the API port in `#versailles` → `api.port` (or sidecar `versailles.json` if that block is missing). The bound port is also stored in `Documents\Widgets\.versailles\config.json` as `apiBoundPort`.

## File server

`GET /files/{path}` maps to `Documents\Widgets\{path}`. The desktop page should load the SDK as `/files/.sdk/versailles.js`.

Same-origin helpers (no CORS fight):

| Path | Source |
| ---- | ------ |
| `GET /quote` | Daily quote proxy |
| `GET /weather` | Open-Meteo proxy (`?location=` `&save=1`) |
| `GET /media/now` | GSMTC now-playing |
| `POST /media/play-pause` | Media control |
| `POST /media/next` | Skip |
| `POST /media/previous` | Previous |

## API (Bearer)

Enabled by default. The token lives in `.versailles/config.json` — never commit it.

```bash
curl -H "Authorization: Bearer <token>" http://127.0.0.1:47831/widgets
```

Authenticated routes include `/widgets`, `/widgets/{id}/open|close|move`, `/layouts/{name}/apply`, `/launcher/show`, and the `/media/*` controls.

## Config files

User folder (`Documents\Widgets`):

| File | Purpose |
| ---- | ------- |
| `desktop/index.html` | UI + `#versailles` JSON (autostart, hotkey, API, shortcuts) |
| `.sdk/versailles.js` | Widget SDK |
| `.versailles/` | Runtime state (token, layouts, desktop shown, session). Migrates from `.deck` |

Sidecar `versailles.json` / `shortcuts.json` are fallback if `#versailles` is missing. The host does not rewrite `index.html`. `desktop.enabled` lives in `.versailles/config.json`.

## ACL

`npm run verify:acl` must stay green. Adding a Tauri command means `src-tauri/src/lib.rs`, `src-tauri/build.rs`, and `src-tauri/capabilities/default.json` together.
