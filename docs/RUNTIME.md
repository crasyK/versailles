# Runtime

Versailles hosts two native windows and one localhost file server.

## Windows

| Label | File | Role |
| ----- | ---- | ---- |
| `desktop` | `desktop.html` | Always-on-bottom shell. Iframes `http://127.0.0.1:{port}/files/desktop/index.html`. |
| `launcher` | `launcher.html` | Alt+Space action bar. |
| `launcher-dim` | `launcher-dim.html` | Click-catch backdrop. |

There is no manager, canvas, anywhere bar, or `widget-*` window.

## Localhost runtime

Default port **47841** (Deck used 47831 so both can exist). Config may bump +0..10 if bound.

Always on, no auth:

| Route | Used by |
| ----- | ------- |
| `GET /health` | Diagnostics |
| `GET /files/{*path}` | Desktop page and widget iframes |
| `GET /weather` | Weather widget |
| `GET /quote` | Quote widget |
| `GET /media/now` | Now-playing widget |
| `POST /media/play-pause\|next\|previous` | Now-playing controls |

This is **not** an automation API. There is no Bearer token, no `/widgets`, no `/manager/show`.

## Config

`Documents\Widgets\.versailles\config.json`

- `autostart` — plugin login item
- `launcherHotkey` — default `Alt+Space`
- `desktop.enabled` — default `true`
- `desktop.page` — default `desktop/index.html`
- `apiPort` — file-server port
- `catalog` — hidden/seen app ids for the action bar

Weather city: `Documents\Widgets\.versailles\weather-location.txt` (and the widget’s in-place city edit).

## Commands (Tauri)

Action bar and desktop shell use invoke: launcher toggle, CLI/PTY, catalog, media (also HTTP), desktop surface, config, runtime status. ACL is guarded by `npm run verify:acl` — every `generate_handler` command must be in `build.rs` and `capabilities/default.json`.
