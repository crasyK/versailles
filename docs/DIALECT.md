# Dialect

Versailles parses one HTML file: `Documents\Widgets\desktop\index.html`. The parser is [`src-tauri/src/page.rs`](../src-tauri/src/page.rs). It only looks at opening tags. Nested markup is not scanned.

The host is a small windowing + hooks layer. Pieces on the page are the product. A command bar in the starter (or your page) is an example of what the dialect can do, like a clock tile. Delete that template and Versailles still runs (tray, HTML desktop, hooks, spawn).

## Classes

| Class | Kind | Typical tag |
| ----- | ---- | ----------- |
| `widget` | Desktop tile | `<article class="widget" data-id="clock">` |
| `spawnable` | Overlay / slide-out template | `<template class="spawnable" data-id="draw">` |
| `action-bar` | Optional CSS class on a spawnable | `<template class="spawnable action-bar" data-id="action-bar">` |

`.action-bar` is just a spawnable. The host does not clone chrome or ship a second engine for it.

## Attributes

| Attribute | Required | Meaning |
| --------- | -------- | ------- |
| `data-id` | yes | Catalog id. Unique, case-insensitive. `calc` aliases `calculator`. |
| `data-w` | no | Window width in CSS pixels. Default 280. |
| `data-h` | no | Window height. Default 200. |
| `data-anchor` | no | Placement. `c` / `center` / `overlay` = mid-screen overlay (covers apps, dim). `tc` = top-center overlay (action bar). `tr` = docked top-right slide-out. |
| `data-hotkey` | no | Global accelerator for this spawnable when `data-hooks` includes `hotkey`. Format: Tauri accelerators (`Alt+Space`, `Ctrl+Shift+D`, `CommandOrControl+K`). Duplicate combos: first piece on the page wins. A hook piece with no `data-hotkey` uses `#versailles` `launcher.hotkey` (once). |
| `data-hooks` | no | Comma-separated allowlist. See [`HOOKS.md`](HOOKS.md). |

Duplicate `data-id` values are ignored after the first.

## Overlay vs slide-out

A spawnable with `data-anchor="c"` (or `center` / `tc` / `overlay`) is an overlay:

- Mid-screen on the monitor under the cursor (`tc` stays near the top, action-bar style)
- Dim behind it (`launcher-dim`)
- Blur / click-dim dismisses
- Hide/show (not destroy) so an embedded PTY survives Alt+Space

`data-anchor="tr"` stays a docked slide-out. Toggle closes it.

Every spawnable with `hotkey` in `data-hooks` can bind its own combo. Pressing that shortcut toggles that piece (overlay hide/show, slide-out open/close). Tray left-click still toggles the first such piece.

```html
<template class="spawnable action-bar" data-id="action-bar"
          data-hooks="shell,hotkey,pty" data-hotkey="Alt+Space" data-anchor="tc"
          data-w="640" data-h="420"></template>
<template class="spawnable" data-id="draw"
          data-hooks="layout,hotkey" data-hotkey="Ctrl+Shift+D" data-anchor="tr"
          data-w="480" data-h="360"></template>
```

No `hotkey` piece → those shortcuts stay idle. Saving `desktop/index.html` rebinds them; otherwise restart Versailles.

## Preview query

Same file, different surfaces:

| `?m=` | What you see |
| ----- | ------------ |
| `desktop` (default) | Wallpaper + widgets. Templates stay in `<template>`. |
| `widgets` | Every `article.widget` on a blank paper canvas |
| `spawn` | Each spawnable materialized as a card |
| `spawn&id=draw` | One spawnable, sized to `data-w` / `data-h` |
| `action_bar` | The example command-bar spawnable, window-sized |

Unknown `id` → a placeholder. Best preview: `http://127.0.0.1:47831/files/desktop/index.html?m=…`. A small boot script in the page implements `?m=`. The host always loads `?m=spawn&id=…` into spawn windows.

## Config blob

Host knobs are not visual pieces. Put them in `<head>`:

```html
<script type="application/json" id="versailles">
{ "autostart": true, "launcher": { "hotkey": "Alt+Space" }, "api": { "enabled": true, "port": 47831 }, "shortcuts": [] }
</script>
```

The widget/spawnable parser ignores this. The JSON must not contain the literal `</script>`. Sidecar `versailles.json` is fallback only. The host never rewrites this file.

## Example command bar

The starter ships a command bar as page JS (`desktop/bar.js`), not a host feature. Its `#cli-*` ids are that example’s contract. Grammar lives in `bar.js`: `!` inline pwsh, `!!` detachable PTY, shortcuts from `#versailles` in the same HTML file.

## Scripts inside templates

`<script>` inside `<template>` does not run until the host (or the preview boot) clones the template into a `.spawn-surface[data-id="…"]`. Query that surface, not `document`, from overlay scripts. The boot copies `src` attributes so external scripts (xterm, `bar.js`) load.
