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
| `spawn` | Open/toggle another spawnable, optionally handing it data |
| `shell` | Open a URL or path (`cli_open`); from a spawn window also run commands (`cli_exec`, `cli_exec_term`, `cli_search_files`) |
| `hotkey` | Global accelerator for this spawnable (`data-hotkey` on that piece, else `#versailles` `launcher.hotkey` for the first hook piece) |
| `pty` | Embedded ConPTY (`pty_open` / write / resize / close) |
| `storage` | The piece's own key-value store under `.versailles/storage/` (`storage_op`) |
| `clipboard` | Read / write plain text, change notice (`clipboard_read`, `clipboard_write`, `clipboard://changed`) |
| `power` | Lock, sleep, sign out, restart, shut down (`power_action`) |
| `bus` | Messages between pieces on the page (`bus_publish`) |
| `battery` | Battery level, charging, plugged in, saver (`sense_now`, pushed on change) |
| `network` | Online state, connectivity level, profile name, Wi-Fi and signal bars |
| `focus` | The active window: title, app, exe path, pid |
| `audio` | Default output device, volume (0–100), mute |
| `windows` | The app windows a taskbar would show, their icons, and pushed changes (`windows_list`, `windows_icon`) |
| `windows-control` | Focus, minimize, restore, maximize, close, move / resize one of those windows (`windows_control`) |
| `notify` | Windows toast notifications, read-only: the recent ones and each new one (see [Notifications](#notifications)) |
| `radios` | Wi-Fi and Bluetooth on / off; switch them from a bar or overlay (see [Quick settings](#quick-settings)) |
| `audio-control` | The active outputs; set volume, mute and the default output from a bar or overlay |
| `display` | Projection mode and brightness per screen; change them from a bar or overlay |
| `power-mode` | Power mode (efficiency / balanced / performance), energy saver; change the mode from a bar or overlay |

Unknown hook names are rejected.

## Contents

- **Basics:** [SDK](#sdk) · [Enforcement](#enforcement) · [Events](#events)
- **Reading the system:** [Push](#push) · [Notifications](#notifications) · [Windows](#windows)
- **Acting:** [Layout](#layout) · [Power](#power) · [Quick settings](#quick-settings) · [Clipboard](#clipboard)
- **Between pieces:** [Storage](#storage) · [Bus](#bus) · [Piece events](#piece-events)

## SDK

Load `Documents\Widgets\.sdk\versailles.js` (the starter copies it). Unscoped `versailles.media` infers the caller when exactly one piece declares that hook. Prefer `versailles.for("now-playing")` on a page with several pieces.

```js
const media = versailles.for("now-playing").media;
const info = await media.now();
await versailles.for("quick-links").shell.open("https://github.com");
await versailles.for("launcher").spawn("calculator");
```

Without Tauri (plain browser / `file:`), hook calls resolve `{ host: false }` instead of throwing.

**Waiting for the SDK.** Scripts that run before the SDK has loaded must check `typeof window.versailles?.for === "function"`, not just `window.versailles`: a page with the `<script type="application/json" id="versailles">` config block has an element named `versailles`, and browsers expose it as `window.versailles` until the SDK replaces it.

**Keeping it current.** The SDK ships inside Versailles; its first line carries its version (`/*! versailles-sdk 0.6.0 */`, also `versailles.version`). On start the host compares `.sdk\versailles.js` with the one it ships:

| Desktop copy | What happens |
| ------------ | ------------ |
| Same as the shipped SDK | Nothing |
| An unchanged older release (any line endings) | Replaced; the old file is kept as `.sdk\versailles.js.bak` |
| Edited by hand | Left alone; the log says so once per start (new hooks may be missing: copy `templates/starter/.sdk/versailles.js` over it, or delete it) |
| Marked newer than this Versailles | Left alone |
| Missing | `/files/.sdk/versailles.js` serves the shipped SDK |

To keep your own SDK, edit it (any change opts out of updates). The host writes nothing outside `.sdk\`. `get_spawnable_engine_context` reports `hostVersion` and `sdk: { bundled, desktop: { state, version }, updated }`.

## Enforcement

Identity comes from the **calling window**, never from an IPC argument:

| Window | Acts as |
| ------ | ------- |
| Spawn window (`widget-<id>`, opened from a `template.spawnable`) | Its own piece `<id>`. `caller` is ignored, so `bar.js` may call `pty_open` / `cli_open` without one. |
| Monitor copy of a bar (`widget-<id>:<n>`, from `data-monitor="all"` on a docked bar) | The same piece `<id>`, like `widget-<id>`. |
| Desktop wallpaper (`desktop`, all `article.widget` pieces in one document) | The piece named by `caller` (`versailles.for(id)` sets it). A missing or unknown `caller` is rejected. |
| Floating folder widget (`widget-<id>` from a `widget.json` folder) | Its `widget.json` `permissions`. |
| Host windows (`launcher-dim`, `guides`) | Nothing — they never call hooks. |

That piece must list the hook in `data-hooks`. A piece that omitted `data-hooks="media"` cannot call `media_now` even if the JS tries.

**Control class.** `quick_set` (`radios`, `audio-control`, `display`, `power-mode`) changes a system setting. Like exec class it is refused from the desktop wallpaper and from folder widgets; only a spawn window (bar, slide-out, overlay) whose piece declares the hook may change a setting. The SDK also asks for a click or key press first. Reading those hooks works anywhere.

**Exec class.** `cli_exec`, `cli_exec_term`, `cli_search_files` (`shell`) and `pty_open` / `pty_write` / `pty_resize` / `pty_is_alive` / `pty_close` (`pty`) run code. They are refused from the desktop wallpaper entirely and only work from a spawn window whose piece declares the hook. `cli_open` (open a URL/path) is not exec class: the wallpaper may call it via `versailles.for(id).shell.open(...)`.

`cli_open` refuses network (UNC) paths and dangerous URL handlers (`ms-msdt:`, `search-ms:`, `search:`, `ms-officecmd:`, Office `ms-word:` / `ms-excel:` / … and their `ofe|u|` forms, `file://host/…`). `http(s)`, `ms-settings:`, `shell:`, local paths and installed app names keep working.

`layout` only ever touches the piece's own window (a wallpaper widget has none, so it is refused). From a bar's monitor copy it changes that copy's window.

## Events

| Event | Payload |
| ----- | ------- |
| `pty://data` | `{ "session": <u64>, "data": "<base64>" }` |
| `pty://exit` | `{ "session": <u64> }` |
| `overlay://shown` | `{ "id": "<piece id>", "seed": "<string>" }` |
| `clipboard://changed` | `null` — to windows of pieces that declare `clipboard` |
| `overlay://hiding` | `{ "id": "<piece id>" }` — about 60 ms before the overlay hides |
| `overlay://hidden` | `{ "id": "<piece id>" }` (`null` when only the backdrop hid) |
| `launcher://shown` / `launcher://hidden` | Legacy: bare seed string / `true` |

`pty_open` returns the new session id (counter starting at 1). Events from an older session keep their own id, so filter on the id `pty_open` returned. Overlay events go to every webview; filter on `id`.
## Push

The host pushes changes instead of pieces polling. `on(name, fn)` subscribes and returns a function that unsubscribes.

```js
const hud = versailles.for("hud");
await hud.on("battery", (b) => render(b.percent, b.charging)); // called now, then on each change
await hud.on("media", (info) => showTrack(info));
const audio = await hud.audio.now();                            // one-off read
await hud.focus.onChange((w) => (title.textContent = w.title));
```

| Hook | Read every | Payload |
| ---- | ---------- | ------- |
| `battery` | 15 s | `{ present, percent, plugged, charging, secondsLeft, saver }` (`present: false` on a desktop PC) |
| `network` | 3 s | `{ online, level: "none"\|"local"\|"constrained"\|"internet", name, wifi, signalBars }` |
| `focus` | 0.5 s | `{ title, app, exe, pid, self }` (`self`: a Versailles window) |
| `audio` | 1 s | `{ device, volume, muted }` |
| `media` | on change | same as `media.now()` |
| `windows` | 0.25 s | `{ type: "opened"\|"closed"\|"focused"\|"changed", id, window }` (see [Windows](#windows)) |
| `notify` | 2 s | one new toast (see [Notifications](#notifications)) |
| `radios` | 2 s | `{ wifi, bluetooth }` (`true`, `false`, or `null` without that radio) |
| `power-mode` | 2 s | `{ supported, mode: "efficiency"\|"balanced"\|"performance"\|null, energySaver }` |
| `audio-control` | 3 s | `{ outputs: [{ id, name, default }] }` (volume and mute are the `audio` hook) |
| `display` | 5 s | `{ projection: "pc"\|"duplicate"\|"extend"\|"second", brightness: [{ id, device, kind: "internal"\|"external", level }] }` |

A sensor is only read while some piece on the page declares its hook, and a value is pushed only when it changed. Pushes go only to the windows of pieces that declare the hook, by a script run in those windows (not a broadcast event). Sensor subscriptions also get the current value right away.

## Notifications

```js
const feed = versailles.for("inbox").notify;
const { access, items } = await feed.now();   // newest first, up to 50
await feed.onToast((t) => prepend(t));        // each new toast, once
```

A toast is `{ id, app, aumid, title, body, time }`: `app` is the sending app's display name (`null` when Windows does not know it), `aumid` its app user model id, `title` the first text line, `body` the other lines joined by `\n`, `time` ms since the Unix epoch. Read-only: a piece cannot dismiss, clear or raise toasts.

`access` is Windows' answer for Versailles:

| `access` | Meaning |
| -------- | ------- |
| `allowed` | `items` holds the toasts in Action Center |
| `denied` | The user turned notification access off (Settings > Privacy & security > Notifications). `items` is empty |
| `unspecified` | Not decided yet. `items` is empty; the host never shows Windows' consent prompt |
| `unavailable` | No notification listener on this system (or not Windows) |

Windows offers no change event to an unpackaged app, so the host checks for new toasts every 2 s while some piece declares `notify` and pushes each one it has not seen (oldest first). The first check only notes what is already there. `onToast` does not fire on subscribe: call `now()` for the current list. Toast text can be private: declare `notify` only on pieces that need it.

## Windows

`windows` reads the app windows a taskbar would show; `windows-control` acts on them. Declare only what a piece needs: a window switcher that only lists needs `windows`.

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44"
          data-monitor="all" data-hooks="windows,windows-control"></template>
```

```js
const bar = versailles.for("taskbar");
const here = (await versailles.monitor())?.number;
await bar.windows.track((list) => render(list.filter((w) => w.monitor === here)), { icons: true });

button.onclick = () => (w.focused && !w.minimized ? bar.windows.minimize(w.id) : bar.windows.focus(w.id));
```

A window:

| Field | |
| ----- | - |
| `id` | Window handle (a number); pass it to the control calls |
| `title`, `app`, `exe`, `pid` | Title, executable name without `.exe`, full path, process id |
| `monitor` | Display number (as in `versailles.monitors()`) |
| `minimized`, `maximized` | Window state |
| `focused` | The app window with focus. Clicking a Versailles window (the taskbar itself, the wallpaper) does not take it away, so "click the focused window to minimize it" works. |
| `rect` | `{ x, y, width, height }` in physical screen pixels; where a minimized window restores to |
| `icon` | 32×32 PNG data URL, only with `{ icons: true }` or from `icon(id)`; `null` / absent if the app has none |

Read (`windows`):

- `windows.list({ icons })`: every window, top of the z-order first.
- `windows.icon(id)`: one window's icon. Icons are cached by the host.
- `windows.onChange(fn)` / `on("windows", fn)`: `fn({ type, id, window })` for each change. `closed` carries the last known window. `changed` covers title, minimized / maximized, focus lost and monitor; moving or resizing alone sends nothing (read `rect` with `list()` when you need it).
- `windows.track(fn, { icons })`: `fn(list)` now and after every change, in a stable order (new windows last). Returns a function that stops tracking.

Control (`windows-control`): `focus(id)` (restores a minimized window), `minimize(id)`, `restore(id)`, `maximize(id)`, `close(id)` (posts `WM_CLOSE`: the app may still ask to save), `move(id, { x, y, width, height })` (physical px; `x` / `y` alone moves, `width` / `height` alone resizes; a maximized or minimized window is restored first).

Listed are visible, titled top-level windows without a tool-window style or owner (or with `WS_EX_APPWINDOW`). Never listed: Versailles' own windows, the shell's desktop and taskbars, and cloaked windows (other virtual desktops, suspended store apps). Control calls only accept a listed window and refuse Versailles' own. Store apps show as `ApplicationFrameHost`. The list is only read while a piece on the page declares `windows`.

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

## Power

```js
button.onclick = () => versailles.for("power-menu").power.lock();
// also: sleep(), signOut(), restart(), shutdown()
```

The SDK only runs these inside a click or key press (`navigator.userActivation`), so a page cannot lock or shut down the machine on its own. `restart` and `shutdown` are immediate. The host logs every request.

`sleep()` needs classic (S3) sleep. Many newer laptops only have Modern Standby (`powercfg /a` lists "S0 low power idle"), which Windows gives apps no supported way to start; there `sleep()` rejects with an error saying so. Catch it and hide or disable your sleep button.

## Quick settings

Four hooks for a quick-settings panel. Each reads like a sensor (`now()`, `onChange(fn)`, `on(hook, fn)`); each change goes through `quick_set` in the control class. It works only from a bar or overlay, only inside a click or key press, and answers the hook's new state. Every change is logged.

```html
<template class="spawnable" data-id="qs" data-anchor="tr" data-w="320" data-h="420"
          data-hooks="radios,audio,audio-control,display,power-mode"> … </template>
```

```js
const qs = versailles.for("qs");
wifiToggle.onclick = async () => {
  const { wifi } = await qs.radios.now();
  await qs.radios.set("wifi", !wifi);                  // "wifi" | "bluetooth"
};
volume.oninput = () => qs.audioControl.setVolume(volume.value);   // 0–100
muteButton.onclick = () => qs.audioControl.setMute(true);
const { outputs } = await qs.audioControl.now();               // [{ id, name, default }]
await qs.audioControl.setOutput(outputs[1].id);
const { brightness, projection } = await qs.display.now();     // brightness: [{ id, kind, level }]
await qs.display.setBrightness(brightness[0].id, 60);          // 0–100
await qs.display.project("extend");                            // "pc" | "duplicate" | "extend" | "second"
await qs.powerMode.set("balanced");                            // "efficiency" | "balanced" | "performance"
```

- **Radios:** Windows' radio switches. Wi-Fi and Bluetooth only; airplane mode is not covered.
- **Volume and mute** act on the default output; read them through `audio`. **The default output** uses the same undocumented call as Windows' sound settings and takes only ids from `outputs`.
- **Brightness:** built-in panels through WMI (`kind: "internal"`), external monitors over DDC/CI (`kind: "external"`, `device` is the screen, e.g. `\\.\DISPLAY2`; slow and not every monitor answers). A screen that offers neither is missing from the list.
- **Projection** is Win+P. `"second"` turns the built-in screen off.
- **Power mode** is the Settings slider (an undocumented Windows call). It needs the Balanced power plan; `supported: false` when Windows does not offer it. Energy saver is read-only.
- A setting Windows refuses fails with a message (`radios: Windows refused access`, `power-mode: … the power plan must be Balanced`) instead of failing silently.

## Clipboard

```js
const clip = versailles.for("clips").clipboard;
const text = await clip.readText();   // null when the clipboard holds no text
await clip.writeText("hello");
await clip.onChange(async () => console.log(await clip.readText()));
```

Plain text only. `onChange` fires on every clipboard change (the notice carries no content; read it if you need it). The host keeps no history: a clipboard manager keeps its own, e.g. in `storage`.

## Storage

```js
const store = versailles.for("todo").storage;
await store.set("items", [{ text: "milk", done: false }]);
const items = (await store.get("items")) ?? [];
await store.keys();   // ["items"]
await store.delete("items");
await store.clear();
```

Values are any JSON. Each piece has its own file (`.versailles/storage/<id>.json`, written atomically) and cannot read another piece's. The wallpaper and a spawn window of the same piece share it. Keys are 1–256 bytes; a store is capped at 1 MB.

## Bus

```js
// Publisher
await versailles.for("player").bus.publish("track", { title: "…" });

// Subscriber (another piece; "*" receives every topic)
const off = await versailles.for("ticker").bus.subscribe("track", (data, msg) => {
  console.log(msg.from, data.title);
});
```

The host relays each message to every open window of a piece that declares `bus`. It is delivered by a script run in those windows only, not as a broadcast event, so pieces without the hook never see it. A piece does not receive its own messages. Windows that are not open miss the message (keep state in `storage` if a late opener needs it). Topics are 1–128 bytes; data is any JSON up to 256 KB.

**Spawn data.** `spawn(id, data)` opens a spawnable (or keeps it open) and hands it `data`; the spawnable reads it with `versailles.spawnData()` or `versailles.onSpawn(fn)`. On first open the data is there before the page's scripts run.

```js
await versailles.for("launcher").spawn("notes", { open: "groceries" });
// inside notes:
versailles.onSpawn((data) => openNote(data.open));
const first = versailles.spawnData();
```

## Piece events

No hook needed; every piece can listen.

| Event | When | Data |
| ----- | ---- | ---- |
| `shown` | The piece's overlay was shown | `null` |
| `hiding` | The piece's overlay is about to hide (about 60 ms ahead) | `null` |
| `hidden` | The piece's overlay was hidden | `null` |
| `idle` | No keyboard / mouse input for 60 s | `{ idleMs }` |
| `active` | Input again after `idle` | `null` |
| `theme-changed` | Windows apps switched light / dark | `{ dark }` |
| `monitor-changed` | A monitor was added, removed, resized or rescaled | `{ monitors }` (same shape as `versailles.monitors()`) |

```js
const me = versailles.for("clock");
me.on("idle", () => document.body.classList.add("dim"));
me.on("active", () => document.body.classList.remove("dim"));
```

`shown` / `hiding` / `hidden` go to that piece only; `idle`, `active`, `theme-changed` and `monitor-changed` go to every page window.

```js
const list = await versailles.monitors(); // [{ number, primary, name, scale, bounds, work }] (logical sizes)
const here = await versailles.monitor();  // the monitor this page / window is on
``` The first `shown` of a newly created window can arrive before its page has loaded; the page is visible when its scripts run anyway.

**Animating in.** Windows shows a hidden window's last frame again the moment it is shown, before the page draws anything new. A piece that animates in should paint its animation's start pose on `hiding` (the host waits about 60 ms before hiding), so that stale frame is the start of the animation rather than the piece at rest.

