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
| `layout` | Move the spawn window |
| `spawn` | Open/toggle another spawnable |
| `shell` | Open a URL or path (`cli_open`) |
| `hotkey` | Global hotkey registration (action bar) |
| `pty` | Embedded terminal (action bar) |

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

IPC may include `caller`. If `caller` is set, that id must exist on the page and list the hook. Unscoped invokes (the launcher engine) are allowed. A piece that omitted `data-hooks="media"` cannot call `media_now` even if the JS tries.

`sys_stats` is a host command, not a named hook. Example widgets call `versailles.invoke("sys_stats")` without a caller.
