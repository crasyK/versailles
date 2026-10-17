# Runtime

Versailles is a tray app. It serves `Documents\Widgets` over localhost and loads `desktop/index.html` as the wallpaper.

## Ports

| What | Default |
| ---- | ------- |
| Local API + file server | `http://127.0.0.1:47831` |
| Vite (dev only) | `http://localhost:1420` |

Override the API port in `#versailles` → `api.port` (or sidecar `versailles.json` if that block is missing). The bound port is also stored in `Documents\Widgets\.versailles\config.json` as `apiBoundPort`.

## File server

`GET /files/{path}` maps to `Documents\Widgets\{path}`, but only for `desktop/**`, `.sdk/**` and folder widgets the registry recognizes (`widget.json`). Everything else (the host repo, `archive/` outside registered widgets, root `versailles.json` / `shortcuts.json`, `.versailles/`) is a 404. The desktop page should load the SDK as `/files/.sdk/versailles.js`.

Every route checks the request first:

- `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (blocks DNS rebinding).
- An `Origin` header must be the server itself, the Tauri app origin (`http(s)://tauri.localhost`, `tauri://localhost`) or, in debug builds, Vite on `:1420`. CORS only reflects those origins.
- Without `Origin`, a `Sec-Fetch-Site: cross-site|same-site` request is refused unless it is a navigation to `/files/*` (the wallpaper shell iframes the desktop page).

`/weather` rounds returned coordinates to two decimals.

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

## Crash recovery

Release builds start as a small **supervisor** that runs the app in a child process (same exe, `VERSAILLES_SUPERVISED=1`) inside a kill-on-close Job Object. Exit code 0 (Quit, or a second launch that handed off to the running instance) ends both. Any other exit restarts the app after 1s, 2s, 5s, 10s, 30s; a sixth crash within 10 minutes gives up. Session restore brings back the wallpaper and floating widgets. Apps launched from the action bar are not in the job and survive a restart.

- Panics are written to `Versailles.log` and `last-crash.txt`; supervisor restarts go to `supervisor.log` (all in `%LOCALAPPDATA%\com.versailles.widgets\logs`).
- A watchdog in the supervised app terminates it (exit code 3, then restart) when the main thread has not answered for 45 s (WebView2 deadlock). System sleep does not count.
- Opt-outs: `VERSAILLES_NO_SUPERVISOR=1`, `VERSAILLES_NO_WATCHDOG=1`. Debug builds and bench mode never supervise.

A locked `config.json` at boot runs on in-memory defaults without overwriting the file. A corrupt one is copied to `config.json.corrupt-<unix-ts>` and reset, keeping `apiToken` when it can be read.

## ACL

`npm run verify:acl` must stay green. Adding a Tauri command means `src-tauri/src/lib.rs`, `src-tauri/build.rs`, and `src-tauri/capabilities/default.json` together.
