# Versailles

Lightweight HTML desktop for Windows — a Tauri v2 (Rust + TypeScript) widget host.

Versailles runs in the system tray and serves **one HTML document** from `%USERPROFILE%\Documents\Widgets\desktop\index.html`. That file is the UI for desktop tiles and spawnable overlays. A command bar on the page is an example of the dialect, not a host feature. Apps cover the wallpaper. Spawnables open as always-on-top windows from the same document (`data-anchor="c"` centers over apps; `tr` docks).

## Features

- One editable page (`desktop/index.html`) with a shared paper stylesheet
- `.widget` tiles on the canvas
- `.spawnable` overlays as native always-on-top windows (`data-anchor="c"` covers apps)
- Example command bar in the same file + `desktop/bar.js` (delete it and the host still works)
- Named hooks (`data-hooks`) — the host never grants more than the list
- GSMTC now-playing
- Alt+Space overlay (the page’s `hotkey` spawnable)
- Silent tray startup
- Localhost API for automation

## Install

After install, look for the **Versailles tray icon**. Left-click toggles the page’s hotkey spawnable (the example command bar). **Alt+Space** also toggles it when that piece declares `data-hooks="hotkey"`. The tray menu is **Show desktop page** / **Hide desktop page** and **Quit**. Type `desk` in the example bar to toggle the HTML desktop.

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

Customization is `desktop/index.html` plus `.versailles/` runtime. Host knobs and shortcuts live in a JSON blob:

```html
<script type="application/json" id="versailles">
{
  "autostart": true,
  "snapThreshold": 12,
  "launcher": { "hotkey": "Alt+Space" },
  "api": { "enabled": true, "port": 47831 },
  "shortcuts": [ { "n": "github", "t": "web", "d": "GitHub", "target": "https://github.com/", "cat": "dev" } ]
}
</script>
```

| File | Purpose |
| ---- | ------- |
| `desktop/index.html` | UI + `#versailles` config/shortcuts |
| `.sdk/versailles.js` | Widget SDK |
| `.versailles/config.json` | Runtime only (token, layouts, desktop shown, session) |

Sidecar `versailles.json` / `shortcuts.json` still work if `#versailles` is missing. The host never rewrites `index.html`. Use `%HOME%` in folder targets. Reload the overlay after edits.

## License

[MIT](LICENSE)
