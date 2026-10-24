# Agent notes

You are in **Versailles**, a Tauri v2 host for a single HTML desktop. This repository is the host. The user desktop is a separate folder.

## Two folders

| Role | Path |
| ---- | ---- |
| Host (this repo) | clone of `crasyK/versailles` |
| User desktop | `%USERPROFILE%\Documents\Widgets` |

Edit tiles, spawnables, and action-bar chrome in `Documents\Widgets\desktop\index.html`. Do not put personal notes, `.versailles/config.json`, or API tokens in this repo.

Use a **multi-root workspace** (host repo + `Documents\Widgets`) so you can see both trees. Do not commit `*.code-workspace` files.

First-time user setup: the host offers to set up the starter on first run, with consent (a welcome window when `desktop/index.html` is missing; `src-tauri/src/first_run.rs` embeds [`templates/starter/`](templates/starter) and never overwrites a file). By hand: copy `templates/starter/` onto `Documents\Widgets`.

## Product rules

- One HTML document. Widgets are `article.widget`. Overlays are `template.spawnable`. An example command bar may use `template.spawnable.action-bar` plus page JS (`desktop/bar.js`).
- Do not iframe per-widget HTML. Do not restore floating OS widget windows for desktop tiles.
- Paper is the look of the host's own chrome and of the starter template. Tokens: [`design/PAPER-THEME.md`](design/PAPER-THEME.md). A user's `desktop/index.html` can look like anything: the host never imposes styling on user pieces (host fallbacks use zero-specificity `:where()` rules). Do not ship a second built-in theme or glass launcher chrome.
- Customization is `desktop/index.html` (`#versailles` JSON) plus `.versailles/` runtime. Do not add a settings UI to the action bar.
- Slide-outs are spawn windows from Alt+Space / `versailles.spawn(id)`, not DOM overlays on the wallpaper. A spawnable with `data-anchor="c"` is a centered overlay (covers apps).
- Keep `npm run verify:acl` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together.

## Dialect (short)

See [`docs/DIALECT.md`](docs/DIALECT.md).

- `.widget` / `.spawnable` (optional `.action-bar` class)
- `data-id` (required). `calc` aliases `calculator`.
- `data-w` / `data-h` overlay size. `data-anchor`: grid overlays `tl tc ml c mr bl bc br` or `cursor`; `tr` = docked slide-out. `data-offset="x,y"` insets from the anchor. `data-dim="host|page|none"` picks the backdrop. `data-monitor="primary|all|<n>"` picks the monitor (widgets get a wallpaper per named monitor). `data-anchor="top|bottom|left|right"` + `data-dock="reserve|float|auto-hide"` = a docked bar (`data-monitor="all"`: one copy per monitor, windows `widget-<id>` / `widget-<id>:<n>`, all acting as `<id>`). Overlays other than `c` keep the page's own size/position across reveals.
- `data-hotkey` with the `hotkey` hook on any spawnable — each combo toggles that piece. `Win`, `Copilot` and `Win+` combos use a keyboard hook (`src/keys.rs`); `#versailles` `shell.suppress` swallows Win combos, `shell.hideTaskbar` hides Explorer's taskbar (`src/taskbar.rs`). Fallback for the first hook piece with no `data-hotkey`: `#versailles` / sidecar `launcher.hotkey`.
- `data-hooks` allowlist: `media`, `mouse`, `layout`, `spawn`, `shell`, `hotkey`, `pty`, `storage`, `clipboard`, `power`, `bus`, `battery`, `network`, `focus`, `audio`, `windows`, `windows-control`, `notify`, `radios`, `audio-control`, `display`, `power-mode`, `wifi`, `bluetooth`, `dnd`, `notify-control`, `vpn`, `hotspot`, `sharing`, `accessibility` (these twelve change settings only from spawn windows: control class), `tray`, `posture`, `keyboard`

Example command-bar ids (`#cli-*`) belong to `desktop/bar.js`, not the host.

## Preview

File server (accurate): `http://127.0.0.1:47831/files/desktop/index.html?m=…`

| Query | Surface |
| ----- | ------- |
| (none) or `?m=desktop` | Stage + widgets; spawnables hidden |
| `?m=widgets` | Widget cards on paper |
| `?m=spawn` | Spawnable templates as cards |
| `?m=spawn&id=calculator` | One spawnable, window-sized |
| `?m=action_bar` | Action bar chrome only |

Opening the file from disk works; hooks resolve `{ host: false }`.

## SDK

Load `/files/.sdk/versailles.js` (or `../.sdk/versailles.js` from `file:`). Call `versailles.for("now-playing").media.now()`, `versailles.spawn("calculator")`, `versailles.shell.open(url)`. The host never grants more than the piece’s `data-hooks`. Details: [`docs/HOOKS.md`](docs/HOOKS.md).

## Dev

```bash
npm install
npm run tauri:dev
```

Or [`dev.cmd`](dev.cmd) (prepends `%USERPROFILE%\.cargo\bin`). Port **47831**. Config dir: `Documents\Widgets\.versailles`.

Steal implementations from [`examples/`](examples) (dialect-native) rather than iframing old folders.

## Do not

- Do not iframe widgets.
- Do not ship a second built-in theme or glass the launcher. Do not force styles onto user pages.
- Do not put settings into the action bar.
- Do not commit `.versailles/config.json`, `desktop/local-restore.json`, or personal shortcuts.
- Do not introduce a Bearer control API beyond the existing localhost API.
