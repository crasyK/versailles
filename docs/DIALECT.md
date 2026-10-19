# Dialect

Versailles parses one HTML file: `Documents\Widgets\desktop\index.html`. The parser is [`src-tauri/src/page.rs`](../src-tauri/src/page.rs). It only looks at opening tags. Nested markup is not scanned.

The host is a small windowing + hooks layer. Pieces on the page are the product. The host does not style them: the page's CSS decides how everything looks (the starter's Paper look is just an example). A command bar in the starter (or your page) is an example of what the dialect can do, like a clock tile. Delete that template and Versailles still runs (tray, HTML desktop, hooks, spawn).

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
| `data-anchor` | no | Placement. Overlays (cover apps): a point of the grid `tl tc · / ml c mr / bl bc br`, or `cursor`. `center` / `overlay` alias `c`. `tc` (action bar) rests 120 px (logical) from the top. `tr`, no anchor, or an unknown value = docked top-right slide-out. See [Overlay placement](#overlay-placement). |
| `data-offset` | no | Overlays only. `x,y` in logical px from the anchor, inward: from the left/top edge for `l`/`t` anchors, from the right/bottom edge for `r`/`b` anchors, a plain right/down shift on a centre axis, added to the cursor for `cursor`. Default `0,0` (`tc`: `0,120`). |
| `data-dim` | no | Overlays only. `host` (default) = the host's dim window behind the overlay. `page` = the overlay window covers the monitor and the page draws its own backdrop. `none` = no backdrop. Wins over `spawnables.<id>.overlay.dim`. |
| `data-hotkey` | no | Global accelerator for this spawnable when `data-hooks` includes `hotkey`. Format: Tauri accelerators (`Alt+Space`, `Ctrl+Shift+D`, `CommandOrControl+K`). Duplicate combos: first piece on the page wins. A hook piece with no `data-hotkey` uses `#versailles` `launcher.hotkey` (once). |
| `data-hooks` | no | Comma-separated allowlist. See [`HOOKS.md`](HOOKS.md). |
| `data-dismiss-on-blur` | no | Overlays only. `true` / `false`: hide the overlay when its window loses focus. Wins over `#versailles` `spawnables.<id>.overlay.dismissOnBlur`. Default `true`, except pieces with the `pty` hook default to `false` (a terminal survives focus flicker). |

Duplicate `data-id` values are ignored after the first.

## Overlay vs slide-out

A spawnable whose `data-anchor` is a grid point (`tl`, `tc`, `ml`, `c`, `mr`, `bl`, `bc`, `br`) or `cursor` is an overlay:

- Opens on the monitor under the cursor, at its anchor (see [Overlay placement](#overlay-placement))
- Backdrop per `data-dim`: the host dim by default
- Click-dim dismisses; blur dismisses unless `data-dismiss-on-blur="false"` (default off for `pty` pieces)
- Hide/show (not destroy) so an embedded PTY survives Alt+Space

`data-anchor="tr"` (or no anchor) stays a docked slide-out. Toggle closes it. Top-right is the slide-out's place, so the grid has no `tr` overlay.

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

### Per-spawnable settings

`spawnables.<id>` (id lower-case) tunes one spawnable without touching its tag:

```json
"spawnables": {
  "action-bar": {
    "overlay": { "dismissOnBlur": false, "draggable": false, "dim": "host" },
    "opts": {
      "blurDismissMs": 280,
      "suggestionLimit": 12,
      "compact": false,
      "launchTick": false,
      "searchHf": "https://huggingface.co/models?search={q}",
      "timeAwareDefaults": true,
      "autoDismissLaunch": true,
      "coverMonitor": false
    }
  }
}
```

`overlay.*` is read by the host. `overlay.dim` takes `"host"` / `"page"` / `"none"` (or `true` = host, `false` = none). `opts` is handed to the piece's page JS (the example bar reads it via `get_spawnable_engine_context`); the host does not interpret it. Attributes on the tag (`data-dismiss-on-blur`, `data-dim`) beat `overlay.*`.

## Overlay placement

**First open.** The host opens an overlay on the monitor under the cursor at `data-w` × `data-h`, placed at its `data-anchor` with `data-offset` (both logical px). `tc` with no offset rests centered, 120 px from the top.

**Later reveals.**

- `c` / `center` / `overlay`: the host re-applies `data-w` × `data-h` and re-centers every time.
- Every other anchor hands its window to the page. On each reveal the host carries it to the monitor under the cursor and keeps the page's layout: same size, same offset from the anchor point (e.g. the bottom-right corner for `br`), scaled to the target monitor's DPI and clamped onto it. A window that covers its monitor covers the target monitor.
- `cursor` keeps the window's size and opens it next to the cursor again.

So a page can resize or move its own overlay (the `layout` hook, or the window API) and it stays that way, including making a `tc` window as large as the monitor so motion above the chrome (a drop-in, a swing) is not clipped.

**`data-dim="page"`.** The overlay window covers the monitor (transparent) and the host shows no dim. The page draws its backdrop and decides what dismisses it (`dismiss_launcher`). The host hands the page the box its piece would have had:

- `window.__VERSAILLES_STAGE__` = `{ x, y, width, height }` (logical px inside the window)
- `--piece-x` / `--piece-y` / `--piece-w` / `--piece-h` on `<html>`
- a `versailles:stage` window event, and `stage` on the `overlay://shown` payload

These are values only; the host adds no styles.

```html
<template class="spawnable" data-id="toast" data-anchor="br" data-offset="16,16"
          data-w="320" data-h="120" data-dim="none"></template>
<template class="spawnable" data-id="spotlight" data-anchor="c" data-dim="page"
          data-w="720" data-h="480"></template>
```

**Example bar.** `"coverMonitor": true` in `spawnables.<id>.opts` (or `data-dim="page"`) makes the bar's window cover the monitor. It sets `html[data-stage]` and `--stage-x` / `--stage-y` / `--stage-w` / `--stage-h` (logical px) and pins its surface (`#cli-root`'s parent, marked `data-stage-box`) to that box: 120 px from the top for `tc` in action mode, centered otherwise. A press outside the surface dismisses, except on elements marked `data-stage-keep`. Without it, the bar sizes its window to its box and only knows `tc` and `c` placement.

## Example command bar

The starter ships a command bar as page JS (`desktop/bar.js`), not a host feature. Its `#cli-*` ids are that example’s contract. Grammar lives in `bar.js`: `!` inline pwsh, `!!` detachable PTY, shortcuts from `#versailles` in the same HTML file.

`bar.js` is generated. Its source is [`src/launcher.ts`](../src/launcher.ts) (+ `src/engine/*`) in this repo; `npm run build:bar` bundles it into `templates/starter/desktop/bar.js`. `npm run build:bar -- --install` also copies it to `%USERPROFILE%\Documents\Widgets\desktop\bar.js`. Do not hand-edit `bar.js`.

The bar sizes itself from its tag (`data-w` / `data-h` / `data-anchor`) when it leaves terminal mode, shows its own `data-hotkey` in the footer, and honours `data-dismiss-on-blur`. The command line is a plain visible `<input id="cli-in">` after the `#cli-ps` prompt; `#cli-echo` / `.cli-caret` from older pages are optional and ignored.

## Scripts inside templates

`<script>` inside `<template>` does not run until the host (or the preview boot) clones the template into a `.spawn-surface[data-id="…"]`. Query that surface, not `document`, from overlay scripts. The boot copies `src` attributes so external scripts (xterm, `bar.js`) load.
