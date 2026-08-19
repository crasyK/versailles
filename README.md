# Versailles

Lightweight HTML desktop for Windows — a Tauri v2 (Rust + TypeScript) widget host.

Versailles runs in the system tray and serves **one HTML document** from `%USERPROFILE%\Documents\Widgets\desktop\index.html`. That file is the UI for desktop tiles, spawnable overlays, and the action bar. Apps cover the wallpaper. Calculator, calendar, and notes open as always-on-top spawn windows from the same document.

## Features

- One editable page (`desktop/index.html`) with a shared paper stylesheet
- `.widget` tiles on the canvas
- `.spawnable` overlays as native always-on-top windows
- Action bar chrome in the same file (`template.action-bar`); the engine stays in the host
- Named hooks (`data-hooks`) — the host never grants more than the list
- GSMTC now-playing
- Alt+Space action bar
- Silent tray startup
- Localhost API for automation

## Install

Download a release installer, or build from source (below). After install, look for the **Versailles tray icon**. Left-click opens the action bar. **Alt+Space** also opens it. Type `desk` to show the HTML desktop.

Copy [`templates/starter/`](templates/starter) onto `%USERPROFILE%\Documents\Widgets` if that folder is empty. The host does not seed HTML for you.

## Develop

```bash
npm install
npm run tauri:dev
```

Or run [`dev.cmd`](dev.cmd) — it prepends `%USERPROFILE%\.cargo\bin`. Close and reopen the terminal if you just installed Rust.

Edit `Documents\Widgets\desktop\index.html`. Preview via the file server (accurate) or by opening the file (hooks stub `{ host: false }`):

| Query | Surface |
| ----- | ------- |
| `index.html` or `?m=desktop` | Stage + widgets; spawnables hidden |
| `?m=widgets` | Widgets on a blank paper canvas |
| `?m=spawn` | Spawnable templates as cards |
| `?m=spawn&id=calculator` | One spawnable, window-sized (`calc` aliases `calculator`) |
| `?m=action_bar` | Action bar chrome only |

Example: `http://127.0.0.1:47831/files/desktop/index.html?m=widgets`

Dialect: `.widget` / `.spawnable` / `.action-bar`, `data-id`, `data-w`, `data-h`, `data-anchor`, `data-hooks`. Full spec: [`docs/DIALECT.md`](docs/DIALECT.md). Steal pieces from [`examples/`](examples).

Paper theme: [`design/PAPER-THEME.md`](design/PAPER-THEME.md). Agent notes: [`AGENTS.md`](AGENTS.md).

## Local API

Enabled by default on `http://127.0.0.1:47831`. The live token is in `Documents\Widgets\.versailles\config.json` — do not commit it.

```bash
curl -H "Authorization: Bearer <token>" http://127.0.0.1:47831/widgets
```

See [`docs/RUNTIME.md`](docs/RUNTIME.md).

## Config

User-facing setup lives at `Documents\Widgets`:

| File | Purpose |
| ---- | ------- |
| `versailles.json` | Theme, startup, desktop, shortcuts, hotkey, API, snap |
| `shortcuts.json` | Action bar shortcuts (web links, folders, apps) |
| `desktop/index.html` | Desktop, spawnables, and action-bar chrome |
| `.sdk/versailles.js` | Widget SDK |
| `.versailles/config.json` | Runtime state only (token, layouts) |

Example `versailles.json`:

```json
{
  "theme": "paper",
  "shortcuts": "shortcuts.json",
  "autostart": true,
  "snapThreshold": 12,
  "launcher": { "hotkey": "Alt+Space" },
  "api": { "enabled": true, "port": 47831 },
  "desktop": { "enabled": true, "page": "desktop/index.html" }
}
```

Use `%HOME%` for folder paths in shortcuts. Reload the action bar after changes.

## License

[MIT](LICENSE)
