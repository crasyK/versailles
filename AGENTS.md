# Agent notes

You are in **Versailles**, a Tauri v2 host for a single HTML desktop. This repository is the host. The user desktop is a separate folder.

## Two folders

| Role | Path |
| ---- | ---- |
| Host (this repo) | clone of `crasyK/versailles` |
| User desktop | `%USERPROFILE%\Documents\Widgets` |

Edit tiles, spawnables, and action-bar chrome in `Documents\Widgets\desktop\index.html`. Do not put personal notes, `.versailles/config.json`, or API tokens in this repo.

Use a **multi-root workspace** (host repo + `Documents\Widgets`) so you can see both trees. Do not commit `*.code-workspace` files.

First-time user setup: copy [`templates/starter/`](templates/starter) onto `Documents\Widgets`. The host does not seed HTML automatically.

## Product rules

- One HTML document. Widgets are `article.widget`. Overlays are `template.spawnable`. An example command bar may use `template.spawnable.action-bar` plus page JS (`desktop/bar.js`).
- Do not iframe per-widget HTML. Do not restore floating OS widget windows for desktop tiles.
- Paper theme only. Tokens: [`design/PAPER-THEME.md`](design/PAPER-THEME.md). No second theme, no glass launcher chrome.
- Customization is `desktop/index.html` (`#versailles` JSON) plus `.versailles/` runtime. Do not add a settings UI to the action bar.
- Slide-outs are spawn windows from Alt+Space / `versailles.spawn(id)`, not DOM overlays on the wallpaper. A spawnable with `data-anchor="c"` is a centered overlay (covers apps).
- Keep `npm run verify:acl` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together.

## Dialect (short)

See [`docs/DIALECT.md`](docs/DIALECT.md).

- `.widget` / `.spawnable` (optional `.action-bar` class)
- `data-id` (required). `calc` aliases `calculator`.
- `data-w` / `data-h` overlay size. `data-anchor` `c` = mid-screen overlay; `tc` = top-center overlay; `tr` = docked slide-out.
- `data-hotkey` with the `hotkey` hook on any spawnable — each combo toggles that piece. Fallback for the first hook piece with no `data-hotkey`: `#versailles` / sidecar `launcher.hotkey`.
- `data-hooks` allowlist: `media`, `mouse`, `layout`, `spawn`, `shell`, `hotkey`, `pty`

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
- Do not add a second theme or glass the launcher.
- Do not put settings into the action bar.
- Do not commit `.versailles/config.json`, `desktop/local-restore.json`, or personal shortcuts.
- Do not introduce a Bearer control API beyond the existing localhost API.
