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

- Panics are written to `Versailles.log` and `last-crash.txt`; supervisor restarts go to `supervisor.log` (all in `%LOCALAPPDATA%\com.versailles.widgets\logs`). `Versailles.log` rolls over at 1 MB and keeps the last three files (the older ones get a date in their name).
- A watchdog in the supervised app terminates it (exit code 3, then restart) when the main thread has not answered for 45 s (WebView2 deadlock). System sleep does not count.
- Opt-outs: `VERSAILLES_NO_SUPERVISOR=1`, `VERSAILLES_NO_WATCHDOG=1`. Debug builds and bench mode never supervise.

A locked `config.json` at boot runs on in-memory defaults without overwriting the file. A corrupt one is copied to `config.json.corrupt-<unix-ts>` and reset, keeping `apiToken` when it can be read.

## Diagnostics

Tray menu **Copy diagnostics** writes `versailles-diagnostics-<yyyymmdd-hhmmss>.zip` to your Downloads folder, copies its path to the clipboard and shows it in Explorer. When the app will not start, `versailles.exe --diagnostics` writes the same zip (it prints the path when run from a console). Nothing is sent anywhere: attach the zip to a bug report yourself.

| In the zip | What |
| ---------- | ---- |
| `info.json` | Host, SDK and WebView2 versions; Windows `DisplayVersion`, build and UBR; monitors with their scale; the page's pieces with their hooks; the `#versailles` config (or `versailles.json`); with the tray item also the open windows and whether the taskbar is hidden |
| `Versailles.log*`, `supervisor.log`, `taskbar.log`, `last-crash.txt` | Logs, whichever exist |
| `taskbar-restore.json` | The saved taskbar state, while the taskbar is hidden |

In the config, the value of every key containing `token`, `secret`, `password`, `key` or `auth` (any case, at any depth; `hotkey` excepted) is replaced by `"<redacted>"`. Your profile folder (`C:\Users\<name>`) becomes `%USERPROFILE%` in every file. Shortcut names and targets stay as they are: look through `info.json` before you post it publicly.

## Updates

A minute after start and then once a day, Versailles fetches `latest.json` from the newest GitHub release (`"updates": { "check": false }` in `#versailles` turns that off; bench / e2e runs never check). When it names a newer version, the tray menu gets **Update to x.y.z** at the top. A click downloads the installer and checks its signature against the public key built into the app; then Versailles gives back the taskbar, the tray icons and the docked bars' screen space as Quit does, starts the installer in passive mode and exits. The installer starts the new version. If the download, the signature or the installer start fails, the log says why and everything stays as it was. Release steps and a local test: [`CONTRIBUTING.md`](../CONTRIBUTING.md#updates).

## ACL

`npm run verify:acl` must stay green. Adding a Tauri command means `src-tauri/src/lib.rs`, `src-tauri/build.rs`, and `src-tauri/capabilities/default.json` together.
