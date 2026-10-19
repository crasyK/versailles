# Hooks

Named capabilities on a page piece. Declare them on the opening tag:

```html
<article class="widget now" data-id="now-playing" data-hooks="media">
```

The host never grants more than that list. The catalog is fixed in [`src-tauri/src/page.rs`](../src-tauri/src/page.rs):

| Hook | What it allows |
| ---- | -------------- |
| `media` | GSMTC now-playing (`media_now`, play/pause/next/previous) |
| `mouse` | Cursor position |
| `layout` | The piece's own window: move, resize, re-anchor, always-on-top, click-through regions (`move_widget_cmd`, `layout_update`) |
| `spawn` | Open/toggle another spawnable |
| `shell` | Open a URL or path (`cli_open`); from a spawn window also run commands (`cli_exec`, `cli_exec_term`, `cli_search_files`) |
| `hotkey` | Global accelerator for this spawnable (`data-hotkey` on that piece, else `#versailles` `launcher.hotkey` for the first hook piece) |
| `pty` | Embedded ConPTY (`pty_open` / write / resize / close) |

Unknown hook names are rejected.

## SDK

Load `Documents\Widgets\.sdk\versailles.js` (the starter copies it). Unscoped `versailles.media` infers the caller when exactly one piece declares that hook. Prefer `versailles.for("now-playing")` on a page with several pieces.

```js
const media = versailles.for("now-playing").media;
const info = await media.now();
await versailles.for("quick-links").shell.open("https://github.com");
await versailles.for("launcher").spawn("calculator");
```

Without Tauri (plain browser / `file:`), hook calls resolve `{ host: false }` instead of throwing.

## Enforcement

Identity comes from the **calling window**, never from an IPC argument:

| Window | Acts as |
| ------ | ------- |
| Spawn window (`widget-<id>`, opened from a `template.spawnable`) | Its own piece `<id>`. `caller` is ignored, so `bar.js` may call `pty_open` / `cli_open` without one. |
| Desktop wallpaper (`desktop`, all `article.widget` pieces in one document) | The piece named by `caller` (`versailles.for(id)` sets it). A missing or unknown `caller` is rejected. |
| Floating folder widget (`widget-<id>` from a `widget.json` folder) | Its `widget.json` `permissions`. |
| Host windows (`launcher-dim`, `guides`) | Nothing — they never call hooks. |

That piece must list the hook in `data-hooks`. A piece that omitted `data-hooks="media"` cannot call `media_now` even if the JS tries.

**Exec class.** `cli_exec`, `cli_exec_term`, `cli_search_files` (`shell`) and `pty_open` / `pty_write` / `pty_resize` / `pty_is_alive` / `pty_close` (`pty`) run code. They are refused from the desktop wallpaper entirely and only work from a spawn window whose piece declares the hook. `cli_open` (open a URL/path) is not exec class: the wallpaper may call it via `versailles.for(id).shell.open(...)`.

`cli_open` refuses network (UNC) paths and dangerous URL handlers (`ms-msdt:`, `search-ms:`, `search:`, `ms-officecmd:`, Office `ms-word:` / `ms-excel:` / … and their `ofe|u|` forms, `file://host/…`). `http(s)`, `ms-settings:`, `shell:`, local paths and installed app names keep working.

`layout` only ever touches the piece's own window (a wallpaper widget has none, so it is refused).

## Layout

```js
const layout = versailles.for("hud").layout;
await layout.resize(480, 120);                 // logical px
await layout.setAnchor("bc", [0, 24]);         // grid anchor + inset, on the window's monitor
await layout.setAlwaysOnTop(true);
await layout.update({ width: 480, anchor: "br", offset: [16, 16] }); // several at once
```

A size or anchor set this way survives later reveals, even for a `c` overlay (see [Overlay placement](DIALECT.md#overlay-placement)).

**Click-through.** `setHitRegions(rects)` makes only those rects (`{ x, y, width, height }`, CSS px inside the window) take clicks. Everywhere else the window passes clicks to whatever is below: other apps, or the host dim. `null` turns it off. `trackHits()` keeps the regions in sync with elements marked `data-hit` (pass another selector to change that) and returns a function that stops tracking.

```html
<template class="spawnable" data-id="hud" data-anchor="c" data-dim="page"
          data-hooks="layout" data-dismiss-on-blur="false" data-w="720" data-h="480">
  <div class="ring" data-hit>…</div>
  <script>versailles.for("hud").layout.trackHits();</script>
</template>
```

Windows has no per-pixel hit testing for these windows, so the host follows the cursor (about 60 times a second while the window is visible) and switches the window between catching and passing clicks at region edges. A click that passes through focuses the app below, which blurs the overlay: set `data-dismiss-on-blur="false"` on click-through pieces that should stay up. Windows are never click-through unless the page asks.

`sys_stats` is a host command, not a named hook. Example widgets call `versailles.invoke("sys_stats")` without a caller.

## Events

| Event | Payload |
| ----- | ------- |
| `pty://data` | `{ "session": <u64>, "data": "<base64>" }` |
| `pty://exit` | `{ "session": <u64> }` |
| `overlay://shown` | `{ "id": "<piece id>", "seed": "<string>" }` |
| `overlay://hidden` | `{ "id": "<piece id>" }` (`null` when only the backdrop hid) |
| `launcher://shown` / `launcher://hidden` | Legacy: bare seed string / `true` |

`pty_open` returns the new session id (counter starting at 1). Events from an older session keep their own id, so filter on the id `pty_open` returned. Overlay events go to every webview; filter on `id`.
