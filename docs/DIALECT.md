# Dialect

Versailles parses one HTML file: `Documents\Widgets\desktop\index.html`. The parser is [`src-tauri/src/page.rs`](../src-tauri/src/page.rs). It only looks at opening tags. Nested markup is not scanned.

The host is a small windowing + hooks layer. Pieces on the page are the product. The host does not style them: the page's CSS decides how everything looks (the starter's Paper look is just an example). A command bar in the starter (or your page) is an example of what the dialect can do, like a clock tile. Delete that template and Versailles still runs (tray, HTML desktop, hooks, spawn).

## Contents

- **Basics:** [Classes](#classes) · [Attributes](#attributes) · [Scripts inside templates](#scripts-inside-templates)
- **Surfaces:** [Overlay vs slide-out](#overlay-vs-slide-out) · [Overlay placement](#overlay-placement) · [Docked bars](#docked-bars) · [Multiple monitors](#multiple-monitors)
- **Configuration:** [Config blob](#config-blob)
- **Tools:** [Preview query](#preview-query) · [Example command bar](#example-command-bar)

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
| `data-anchor` | no | Placement. Overlays (cover apps): a point of the grid `tl tc · / ml c mr / bl bc br`, or `cursor`. `center` / `overlay` alias `c`. `tc` (action bar) rests 120 px (logical) from the top. `tr`, no anchor, or an unknown value = docked top-right slide-out. `top` / `bottom` / `left` / `right` = a docked bar (see [Docked bars](#docked-bars)). See [Overlay placement](#overlay-placement). |
| `data-offset` | no | Overlays only. `x,y` in logical px from the anchor, inward: from the left/top edge for `l`/`t` anchors, from the right/bottom edge for `r`/`b` anchors, a plain right/down shift on a centre axis, added to the cursor for `cursor`. Default `0,0` (`tc`: `0,120`). |
| `data-dim` | no | Overlays only. `host` (default) = the host's dim window behind the overlay. `page` = the overlay window covers the monitor and the page draws its own backdrop. `none` = no backdrop. Wins over `spawnables.<id>.overlay.dim`. |
| `data-monitor` | no | Which monitor. Widgets: `primary` (default), a display number (`2`, as in Windows display settings) or `all`; each monitor named gets its own wallpaper. Spawnables: `primary` or a number pins where they open; default (or `cursor`) is the monitor under the mouse. Docked bars also take `all`: one copy per monitor. See [Multiple monitors](#multiple-monitors). |
| `data-dock` | no | Docked bars only. `reserve` (default): reserves its strip like the taskbar, so maximized apps stop at its edge. `float`: always on top, no space reserved. `auto-hide`: tucked at the edge, slides out when the mouse touches it. |
| `data-peek` | no | `auto-hide` bars: logical px left showing while tucked away. Default 4. |
| `data-hotkey` | no | Global accelerator for this spawnable when `data-hooks` includes `hotkey`. Format: Tauri accelerators (`Alt+Space`, `Ctrl+Shift+D`, `CommandOrControl+K`), plus `Win`, `Copilot` and `Win+` combos (see [Overlay vs slide-out](#overlay-vs-slide-out)). Several combos, comma-separated: `"Alt+Space, Win"`. Duplicate combos: first piece on the page wins. A hook piece with no `data-hotkey` uses `#versailles` `launcher.hotkey` (once). |
| `data-hooks` | no | Comma-separated allowlist. See [`HOOKS.md`](HOOKS.md). |
| `data-dismiss-on-blur` | no | Overlays only. `true` / `false`: hide the overlay when its window loses focus. Wins over `#versailles` `spawnables.<id>.overlay.dismissOnBlur`. Default `true`, except pieces with the `pty` hook default to `false` (a terminal survives focus flicker). |

Duplicate `data-id` values are ignored after the first. A piece inside an HTML comment (`<!-- … -->`) is not on the page.

## Scripts inside templates

`<script>` inside `<template>` does not run until the host (or the preview boot) clones the template into a `.spawn-surface[data-id="…"]`. Query that surface, not `document`, from overlay scripts. The browser runs each cloned script once, inline or `src` (xterm, `bar.js`), when the clone enters the document, so a piece needs no guard against running twice.

**Pages copied before 0.7.0** re-create every template script after mounting it (`runScripts(wrap)` in the boot's `mountSpawn`), so each one ran twice: two subscribers, two bus messages per publish. Delete that call:

```diff
         function mountSpawn(tpl, solo, parent) {
           const wrap = materialize(tpl, solo);
           parent.appendChild(wrap);
-          runScripts(wrap);
           return wrap;
         }
```

The `runScripts` function is then unused; delete it too.
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

**The Windows and Copilot keys.** *Experimental:* a tap of Win or Copilot can still open Start on Windows 11 25H2 ([#37](https://github.com/crasyK/versailles/issues/37)); prefer a combo such as `Alt+Space` until that is fixed. `data-hotkey="Win"` binds a tap of the Windows key alone, `"Copilot"` binds the Copilot key, and any combo with `Win` (`"Win+A"`, `"Ctrl+Win+F1"`; `Super` means the same) can take over one Windows already uses. These go through a keyboard hook instead of a registered shortcut: Windows never sees the keys, and Start does not open when you let go of Win. The hook exists only while such a binding exists and ends with Versailles, so your keyboard is never left changed.

```html
<template class="spawnable" data-id="start" data-hooks="hotkey" data-hotkey="Win" data-anchor="bl"
          data-w="420" data-h="520"></template>
```

To stop a Windows shortcut without binding it, list it in `#versailles`: `"shell": { "suppress": ["Win+A", "Win+N", "Copilot"] }`. Limits: Win+L and Ctrl+Alt+Del cannot be taken over, and while a program running as administrator has focus the hook does not see keys. The Copilot key sends Shift+Win+F23, so `"Copilot"` is that combo.

## Overlay placement

**First open.** The host opens an overlay on the monitor under the cursor at `data-w` × `data-h`, placed at its `data-anchor` with `data-offset` (both logical px). `tc` with no offset rests centered, 120 px from the top. An overlay that covers its monitor (`data-dim="page"`, or the example bar's `coverMonitor`) is built hidden at the monitor's size and position instead, so its first frame is already full-size.

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

The host reads `coverMonitor` too (only on a `tc` piece): it builds the window monitor-sized and keeps it hidden on first open until the page sets `html[data-stage]`, at most 1.5 s, so the bar never shows stretched over the monitor before its chrome box is placed. Later reveals are not delayed.

## Docked bars

A spawnable with `data-anchor="top"`, `"bottom"`, `"left"` or `"right"` is a docked bar: a top bar, a taskbar, a dock, a side rail. It runs the full length of its monitor's edge; its depth is `data-h` (top / bottom) or `data-w` (left / right). It opens by itself when the page declares it, has no dim, is never dismissed by losing focus, and does not take focus when it appears.

```html
<template class="spawnable" data-id="topbar" data-anchor="top" data-h="36"></template>
<template class="spawnable" data-id="dock" data-anchor="bottom" data-dock="float" data-h="64" data-monitor="2"></template>
<template class="spawnable" data-id="rail" data-anchor="right" data-dock="auto-hide" data-w="320" data-peek="4"></template>
```

- `reserve` (default) registers a Windows AppBar: the work area shrinks, maximized windows stop at the bar, and it moves out of the way when the taskbar or another bar moves. While a fullscreen app (a video, a game) is up, the bar drops behind it. The space is given back when the bar hides or Versailles quits; if Versailles crashes, Windows releases it on its own. If Windows will not grant a usable reservation (seen for a bottom bar on a secondary monitor whose taskbar a shell mod moved to the top), the bar floats at its edge instead and the host logs a warning.
- `float` sits on top inside the work area without reserving space.
- `auto-hide` leaves a `data-peek` px strip at the edge. Touching the edge with the mouse slides it out (the piece gets `shown`); moving off it tucks it back after half a second (`hiding`, then `hidden`).

A bar is on the primary monitor unless `data-monitor` names a display number; it does not follow the mouse. With `hotkey` in `data-hooks`, its `data-hotkey` hides and shows it (hiding gives reserved space back); `spawn(id)` does the same. Everything inside the bar is the page's own HTML and CSS.

**On every monitor.** `data-monitor="all"` opens one window per monitor, each reserving, floating or auto-hiding on its own monitor's edge. Every copy is the same piece: same `data-hooks`, same storage, the same pushes and bus messages. Each loads with `monitor=<n>&primary=0|1` in its URL, so `versailles.monitor()` is its own monitor (a taskbar shows that monitor's windows). The copy on the primary monitor is the window `widget-<id>`; the others are `widget-<id>:<n>`. When a monitor is added or removed, its copy opens or closes; if the primary changes, the affected copies reload with their new `monitor`. The hotkey and `spawn(id)` hide and show all copies together; an auto-hide copy slides out on its own (only that window gets `shown` / `hiding` / `hidden`).

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44" data-monitor="all" data-hooks="windows,windows-control"></template>
```

## Multiple monitors

The wallpaper is one window per monitor that has something on it. The primary monitor always has one (`desktop`); another monitor gets one (`desktop-<n>`) only when a widget names it with `data-monitor`. A page without `data-monitor` looks exactly as on a single monitor, and the other screens keep the normal Windows wallpaper.

Every wallpaper loads the same page with `monitor=<n>&primary=0|1` in the URL. The SDK hides each `.widget`, and any other element with a `data-monitor` attribute, that does not belong on that monitor:

| `data-monitor` | Shown on |
| -------------- | -------- |
| (none), `primary` | the primary monitor |
| `2` | display 2 (the number Windows shows in Settings → System → Display) |
| `all` | every monitor |

```html
<article class="widget" data-id="clock" data-monitor="all">…</article>
<article class="widget" data-id="stats" data-monitor="2">…</article>
<template class="spawnable" data-id="notes" data-anchor="br" data-monitor="2"></template>
```

Page content that is not a widget (a notes layer, a HUD, a "new note" button) is on every wallpaper unless it carries `data-monitor` too, so mark it: `<div class="notes-layer" data-monitor="primary">`. Every wallpaper shares the page's `localStorage`, so state kept there should live on one monitor. For per-monitor styling the SDK also sets `<html data-monitor="2" data-monitor-primary="0">`:

```css
html[data-monitor-primary="0"] #heat-scene { opacity: 0.6; } /* quieter background on side screens */
```

A spawnable with `data-monitor="primary"` or a number opens on that monitor (anchor, dim and slide-out dock included); otherwise it opens on the monitor under the mouse. A display that is not connected falls back to the monitor under the mouse. Only docked bars take `all` (see [Docked bars](#docked-bars)); an overlay with `all` opens under the mouse.

When monitors are added, removed, resized or rescaled, the host re-places the wallpapers (creating or closing them as needed), moves windows that ended up off-screen back on, and sends the `monitor-changed` piece event. `versailles.monitors()` lists every monitor and `versailles.monitor()` returns the one the page is on.

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

`overlay.*` is read by the host. `overlay.dim` takes `"host"` / `"page"` / `"none"` (or `true` = host, `false` = none). `opts` is handed to the piece's page JS (the example bar reads it via `get_spawnable_engine_context`); the host interprets only `coverMonitor` (see [Overlay placement](#overlay-placement)). Attributes on the tag (`data-dismiss-on-blur`, `data-dim`) beat `overlay.*`.

### Hiding the Windows taskbar

```json
"shell": { "hideTaskbar": true }
```

Hides Explorer's taskbar on every monitor and frees the strip it reserves, so a docked bar can take its place. It applies on save and comes back when you remove the line or quit. Explorer stays the shell: Start, Quick Settings, the notification center and toasts keep working, and nothing is injected into Explorer. Win+A, Win+N and Win+X still open Explorer's panels unless you list them in `"suppress"` (see [Overlay vs slide-out](#overlay-vs-slide-out)), and toasts still appear in the corner.

The taskbar comes back even when Versailles does not end cleanly:
- a small guard process restores it the moment the app crashes or is killed;
- after a logoff or power loss while hidden, a one-time logon entry (`HKCU\...\RunOnce`) restores it;
- a start with hiding off restores a taskbar an earlier run left hidden;
- by hand: `versailles.exe --restore-taskbar` (does nothing while a running Versailles is hiding it on purpose).

Your own auto-hide setting is saved before hiding and put back as it was. Taskbar mods (Windhawk and similar) that move or restyle the taskbar may fight the hiding; turn them off while it is on. Only one Versailles hides at a time.

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

## Example command bar

The starter ships a command bar as page JS (`desktop/bar.js`), not a host feature. Its `#cli-*` ids are that example’s contract. Grammar lives in `bar.js`: `!` inline pwsh, `!!` detachable PTY, shortcuts from `#versailles` in the same HTML file.

`bar.js` is generated. Its source is [`src/launcher.ts`](../src/launcher.ts) (+ `src/engine/*`) in this repo; `npm run build:bar` bundles it into `templates/starter/desktop/bar.js`. `npm run build:bar -- --install` also copies it to `%USERPROFILE%\Documents\Widgets\desktop\bar.js`. Do not hand-edit `bar.js`.

The bar sizes itself from its tag (`data-w` / `data-h` / `data-anchor`) when it leaves terminal mode, shows its own `data-hotkey` in the footer, and honours `data-dismiss-on-blur`. The command line is a plain visible `<input id="cli-in">` after the `#cli-ps` prompt; `#cli-echo` / `.cli-caret` from older pages are optional and ignored.

