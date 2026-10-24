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
| `shell` | Open a URL, path or installed app (`cli_open`); from a spawn window also run commands (`cli_exec`, `cli_exec_term`), search files and Settings pages (`cli_search`, `cli_search_files`) and list recently opened files (`cli_recent_files`); see [Search](#search-recent-files-and-settings) |
| `hotkey` | Global accelerator for this spawnable (`data-hotkey` on that piece, else `#versailles` `launcher.hotkey` for the first hook piece). Win and Copilot keys, and the log lines that diagnose them: [DIALECT](DIALECT.md#overlay-vs-slide-out) |
| `pty` | Embedded ConPTY (`pty_open` / write / resize / close) |
| `storage` | The piece's own key-value store under `.versailles/storage/` (`storage_op`) |
| `clipboard` | Read / write plain text, change notice (`clipboard_read`, `clipboard_write`, `clipboard://changed`) |
| `power` | Lock, sleep, sign out, restart, shut down (`power_action`) |
| `bus` | Messages between pieces on the page (`bus_publish`) |
| `battery` | Battery level, charging, plugged in, saver (`sense_now`, pushed on change) |
| `network` | Online state, connectivity level, profile name, Wi-Fi and signal bars |
| `focus` | The active window: title, app, exe path, pid |
| `audio` | Default output device, volume (0–100), mute; the default microphone, its volume and mute |
| `windows` | The app windows a taskbar would show, their icons, which ones ask for attention, pushed changes, and a hover preview card from a bar or overlay (`windows_list`, `windows_icon`, `windows_preview`) |
| `windows-control` | Focus, minimize, restore, maximize, close, move / resize one of those windows (`windows_control`) |
| `notify` | Windows toast notifications, read-only: the recent ones and each new one (see [Notifications](#notifications)) |
| `radios` | Wi-Fi, Bluetooth and airplane mode on / off; switch them from a bar or overlay (see [Quick settings](#quick-settings)) |
| `audio-control` | The active outputs and microphones, and the apps playing sound (per-app volume); set volume, mute, the default output and microphone, and each app's volume and mute from a bar or overlay. The app list reveals which apps play audio: declare it only where needed |
| `display` | Projection mode, brightness per screen and night light; change them from a bar or overlay |
| `power-mode` | Power mode (efficiency / balanced / performance), energy saver; change the mode from a bar or overlay |
| `wifi` | Wi-Fi networks in range; scan, join (with a password for a new one), disconnect from a bar or overlay. Network names reveal where you are: declare it only where needed |
| `bluetooth` | Paired Bluetooth devices and whether they are connected; connect / disconnect audio devices from a bar or overlay |
| `dnd` | Do Not Disturb: off, priority only, alarms only; change it from a bar or overlay |
| `notify-control` | Dismiss one notification, clear all, open the app that raised one, turn an app's own banners on / off (from a bar or overlay; read them with `notify`) |
| `vpn` | Windows' own VPN profiles and whether they are connected; connect / disconnect from a bar or overlay (see [Connections and accessibility](#connections-and-accessibility)) |
| `hotspot` | Mobile hotspot on / off and its client count; switch it from a bar or overlay |
| `sharing` | Whether the screen is cast (Miracast) and the nearby sharing mode; open Windows' Cast panel, change nearby sharing from a bar or overlay |
| `accessibility` | Magnifier, contrast theme, live captions, colour filters, sticky keys; switch them from a bar or overlay |
| `tray` | The notification-area icons (tooltip, app, icon), pushed changes and balloons, and clicking, hovering or pressing a key on one, while Versailles hides the Windows taskbar (see [Tray](#tray)). Tooltips and balloons can be private |
| `posture` | Whether a convertible is folded into tablet posture, whether it is docked, and its touch screen and pen (see [Posture](#posture)) |
| `keyboard` | The installed keyboard layouts and the one the app in front uses (pushed on change); switch that app's layout from a bar or overlay (see [Keyboard](#keyboard)) |

Unknown hook names are rejected.

## Contents

- **Basics:** [SDK](#sdk) · [Enforcement](#enforcement) · [Events](#events)
- **Reading the system:** [Push](#push) · [Notifications](#notifications) · [Windows](#windows) · [Tray](#tray) · [Posture](#posture)
- **Acting:** [Layout](#layout) · [Power](#power) · [Quick settings](#quick-settings) · [Connections and accessibility](#connections-and-accessibility) · [Keyboard](#keyboard) · [Clipboard](#clipboard) · [Search](#search-recent-files-and-settings)
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

**Control class.** `quick_set` (`radios`, `audio-control`, `display`, `power-mode`, `wifi`, `bluetooth`, `dnd`, `notify-control`, `keyboard`, `vpn`, `hotspot`, `sharing`, `accessibility`) changes a system setting. Like exec class it is refused from the desktop wallpaper and from folder widgets; only a spawn window (bar, slide-out, overlay) whose piece declares the hook may change a setting. The SDK also asks for a click or key press first. Reading those hooks works anywhere.

**Exec class.** `cli_exec`, `cli_exec_term`, `cli_search`, `cli_search_files`, `cli_recent_files` (`shell`) and `pty_open` / `pty_write` / `pty_resize` / `pty_is_alive` / `pty_close` (`pty`) run code. They are refused from the desktop wallpaper entirely and only work from a spawn window whose piece declares the hook. `cli_open` (open a URL/path) is not exec class: the wallpaper may call it via `versailles.for(id).shell.open(...)`.

`cli_open` refuses network (UNC) paths and dangerous URL handlers (`ms-msdt:`, `search-ms:`, `search:`, `ms-officecmd:`, Office `ms-word:` / `ms-excel:` / … and their `ofe|u|` forms, `file://host/…`). `http(s)`, `ms-settings:`, `shell:`, local paths and installed app names keep working. `shell:AppsFolder\<AppUserModelID>` opens only an app that is in Windows' Applications folder.

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
| `audio` | 1 s | `{ device, volume, muted, input: { device, volume, muted } }` (`input`: the default microphone, `null` without one) |
| `media` | on change | same as `media.now()` |
| `windows` | 0.25 s | `{ type: "opened"\|"closed"\|"focused"\|"changed", id, window }` (see [Windows](#windows)) |
| `notify` | 2 s | one new toast (see [Notifications](#notifications)) |
| `tray` | on change | `{ type: "added"\|"changed"\|"removed", id, icon }`, `{ type: "balloon", id, title, text, kind }` or `{ type: "active", active }` (see [Tray](#tray)) |
| `radios` | 2 s | `{ wifi, bluetooth, airplane }` (`true`, `false`, or `null` without that radio; `airplane: null` where Windows offers no airplane switch) |
| `power-mode` | 2 s | `{ supported, mode: "efficiency"\|"balanced"\|"performance"\|null, energySaver }` |
| `audio-control` | 3 s | `{ outputs: [{ id, name, default }], inputs: [{ id, name, default }], sessions: [{ id, app, exe, pid, icon, volume, muted, active, system }] }` (volume and mute of the default output and microphone are the `audio` hook) |
| `dnd` | 2 s | `{ supported, mode: "off"\|"priority"\|"alarms"\|null }` |
| `bluetooth` | 3 s | `{ devices: [{ id, name, connected, audio }] }` (`audio`: headphones / speakers, can be connected) |
| `wifi` | 5 s | `{ access: "ok"\|"location-denied"\|"no-adapter", connected, networks: [{ ssid, signal, secure, known, connected }] }` |
| `display` | 5 s | `{ projection: "pc"\|"duplicate"\|"extend"\|"second", brightness: [{ id, device, kind: "internal"\|"external", level }], nightLight: { supported, on } }` |
| `posture` | on change (5 s fallback) | `{ tablet, docked, touch: { screen, pen, maxTouches } }` (`tablet: null` on a PC that is not a convertible; see [Posture](#posture)) |
| `keyboard` | 0.5 s | `{ layouts: [{ id, name, short, lang, layout }], current }` (see [Keyboard](#keyboard)) |
| `vpn` | 2 s | `{ profiles: [{ name, connected, connecting }] }` |
| `hotspot` | 3 s | `{ supported, on, busy, clients, maxClients, reason }` |
| `sharing` | 3 s | `{ casting, nearby: "off"\|"my-devices"\|"everyone"\|null }` |
| `accessibility` | 2 s | `{ magnifier, contrast, captions, colorFilter, stickyKeys }` (`true`, `false`, or `null` when unknown) |

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

### Acting on notifications

`notify-control` (control class: bars and overlays, inside a click or key press) works on the toasts `notify` reads. Each call answers the list as `notify.now()` would.

```js
const qs = versailles.for("qs");                  // data-hooks="notify,notify-control,dnd"
const { items } = await qs.notify.now();
await qs.notifyControl.dismiss(items[0].id);      // remove one from Action Center
await qs.notifyControl.clear();                   // remove all
await qs.notifyControl.open(items[0].aumid);      // open the app that raised it
await qs.notifyControl.banners(items[0].aumid, false); // no Windows pop-ups from that app
await qs.dnd.set("priority");                     // "off" | "priority" | "alarms"
```

- A notification's own click action cannot be run from outside (Windows does not hand out its arguments); `open` starts the app instead.
- `banners(aumid, false)` hides that app's Windows pop-ups; its notifications still reach Action Center, so `notify` still sees them and a page can show its own. To take over for every app, turn on Do Not Disturb "priority": Windows keeps the notifications and only hides the pop-ups.
- Do Not Disturb uses the shell's undocumented quiet-hours setting; `supported: false` when Windows does not offer it.
- Dismissing needs the notification access the read already needs.

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
| `attention` | It flashed its taskbar button (a chat message, a finished download, an app Windows kept from coming to the front) and has not been in front since |
| `rect` | `{ x, y, width, height }` in physical screen pixels; where a minimized window restores to |
| `icon` | 32×32 PNG data URL, only with `{ icons: true }` or from `icon(id)`; `null` / absent if the app has none |

Read (`windows`):

- `windows.list({ icons })`: every window, top of the z-order first.
- `windows.icon(id)`: one window's icon. Icons are cached by the host.
- `windows.onChange(fn)` / `on("windows", fn)`: `fn({ type, id, window })` for each change. `closed` carries the last known window. `changed` covers title, minimized / maximized, focus lost, monitor and attention; moving or resizing alone sends nothing (read `rect` with `list()` when you need it).
- `windows.track(fn, { icons })`: `fn(list)` now and after every change, in a stable order (new windows last). Returns a function that stops tracking.
- `windows.preview(id, element)` / `windows.preview(null)`: Versailles' preview card for a window (see below).

**Attention.** Versailles listens to the shell's window messages (`RegisterShellHookWindow`, as Cairo and RetroBar do), which arrive whether or not Explorer's taskbar is shown. A window that flashes its taskbar button gets `attention: true` (a `changed` push) and keeps it through the blinking, and after a flash that ends lit, until it comes to the front (it is activated, or the poll finds it in front) or closes. An app that stops flashing (`FLASHW_STOP`) loses it about 2 s later. A title change does not clear it. Caption-only flashes are not attention. Draw it however you like; the starter's taskbar example blinks a dot (steady with reduced motion).

**Hover preview.** `windows.preview(id, target)` shows a card with a live thumbnail of the window and its title next to `target`: the hovered element, or its rect `{ x, y, width, height }` in the page's CSS pixels (`getBoundingClientRect()`). `windows.preview(null)` hides it.

```js
const wins = versailles.for("taskbar").windows;
let timer;
button.onmouseenter = () => (timer = setTimeout(() => wins.preview(w.id, button), 400));
button.onmouseleave = () => (clearTimeout(timer), wins.preview(null));
```

- The card is Versailles' own window (native, drawn with the Paper look; it is not part of the page and cannot be styled). It never takes focus or clicks: they go to what is below.
- It goes above the button for a bar in the lower half of a monitor, below it for a top bar, beside it for a side bar, and stays on the button's monitor (its work area), sized for that monitor's scale (the thumbnail is at most 240 × 160 px).
- A window that gives no thumbnail shows its icon and title instead.
- It hides about 0.1 s after `preview(null)` (moving to the next button and calling `preview` again keeps it up), when the pointer has left both the bar and the card for 0.3 s, when the window closes, and a minute after the last call. Call `preview` again when the button moves.
- Only from a bar or overlay (a spawn window), never from the wallpaper: the card is a topmost window placed from the calling window's position. It is display only, so `windows` is enough and no click is needed. At most 30 calls a second; only listed windows.

Control (`windows-control`): `focus(id)` (restores a minimized window), `minimize(id)`, `restore(id)`, `maximize(id)`, `close(id)` (posts `WM_CLOSE`: the app may still ask to save), `move(id, { x, y, width, height })` (physical px; `x` / `y` alone moves, `width` / `height` alone resizes; a maximized or minimized window is restored first).

Listed are visible, titled top-level windows without a tool-window style or owner (or with `WS_EX_APPWINDOW`). Never listed: Versailles' own windows, the shell's desktop and taskbars, and cloaked windows (other virtual desktops, suspended store apps). Control calls only accept a listed window and refuse Versailles' own. Store apps show as `ApplicationFrameHost`. The list is only read while a piece on the page declares `windows`.

## Tray

`tray` lists the notification-area icons apps put in the system tray and clicks them. Windows has no API for another app to read them, so Versailles becomes the tray: while it hides the Windows taskbar (`"shell": { "hideTaskbar": true }`) and some piece declares `tray`, it runs a `Shell_TrayWnd` window of its own in front of Explorer's, the way Cairo and RetroBar do (nothing is injected into Explorer). Apps then send their icons to Versailles. Every message also goes on to Explorer's taskbar, so Explorer keeps the same icons (balloons still become Windows notifications), and when Versailles stops, quits or crashes, nothing is lost. Without a hidden taskbar, Explorer's tray has the icons and `active` is false.

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44"
          data-monitor="all" data-hooks="windows,windows-control,tray"></template>
```

```js
const tray = versailles.for("taskbar").tray;
await tray.track((icons, active) => render(icons.filter((t) => !t.system)));

button.title = t.showTip ? t.tip : "";
button.onclick = (e) => e.detail === 1 && tray.click(t.id, "left");
button.ondblclick = () => tray.click(t.id, "double");
button.oncontextmenu = (e) => (e.preventDefault(), tray.click(t.id, "right"));
button.onkeydown = (e) => e.key === "Enter" && (e.preventDefault(), tray.click(t.id, "key"));
button.onmouseenter = () => tray.hover(t.id, true);
button.onmouseleave = () => tray.hover(t.id, false);
```

An icon:

| Field | |
| ----- | - |
| `id` | Versailles' number for the icon (for this run); pass it to `click` |
| `tip` | The tooltip, as the app set it (may have several lines) |
| `showTip` | Whether Windows would show `tip` as a tooltip: always for older icons; a version 4 icon asks for it with `NIF_SHOWTIP`, otherwise it shows its own pop-up on hover (or nothing). Show your own tooltip only when this is true |
| `app`, `exe`, `pid` | The owning app: executable name without `.exe`, full path, process id |
| `guid` | The app's own id for the icon, when it gives one (stable across runs) |
| `system` | One of Windows' own icons, drawn by Explorer (volume, battery). The quick-settings hooks cover these better |
| `clickable` | The app takes clicks. `click` refuses an icon that does not |
| `icon` | PNG data URL at the icon's own size (16–64 px) |

- `tray.state()`: `{ active, icons }`. `tray.list()`: the icons alone, oldest first.
- `tray.onChange(fn)` / `on("tray", fn)`: `fn({ type: "added"|"changed"|"removed", id, icon })` per change (an icon the app hides is `removed`; shown again, `added`), `{ type: "balloon", id, title, text, kind }` when an app shows a balloon, and `{ type: "active", active }` when Versailles starts or stops holding the icons. Changes within 100 ms go out together.
- Balloons: `title` and `text` as the app set them (one may be empty), `kind` one of `"info"`, `"warning"`, `"error"`, `"none"`, `"user"` (the app's own image; not sent). Windows still shows every balloon as a notification and tells the app when it is clicked or closed, so a page that shows one too is for information only: there is nothing to answer. `tray.track` ignores balloons.
- `tray.track(fn)`: `fn(icons, active)` now and after every change. Returns a function that stops tracking.
- `tray.click(id, button, x, y)`: what clicking the icon in Windows' tray does. `button` is `"left"`, `"right"` (usually the app's menu), `"double"` or `"middle"`. Map browser events one to one: `click` → `left`, `contextmenu` → `right`, `dblclick` → `double`; skip the second `click` of a double click (`event.detail` 2), since Windows sends one click and then the double. `x`, `y` are physical screen pixels where the app should open its menu; leave them out to use the cursor. From the keyboard: `"key"` for Enter / Space on a focused icon (`NIN_KEYSELECT`), `"menu"` for the menu key / Shift+F10 (`WM_CONTEXTMENU`); the app is told the icon's top-left corner, from `x`, `y` or else where the icon was last clicked or hovered. Icons older than `NOTIFYICON_VERSION` 3 get a left or right click instead. Only inside a click or key press, like quick settings; it is not control class, because a click does no more than the user clicking the icon in Explorer would. The app gets permission to come to the front.
- `tray.hover(id, over, x, y)`: the pointer entered (`over` true) or left the icon. The app gets the mouse messages Explorer's tray sends (move and hover, then leave) and, for version 4 icons, `NIN_POPUPOPEN` / `NIN_POPUPCLOSE`, so apps with their own pop-up show it next to the icon; others may refresh their tooltip (a `changed` push follows when they do). Call it on `mouseenter` / `mouseleave`, once each. No click needed: hovering does no more than moving the mouse over the icon in Explorer.

Icons arrive within about a second of the tray starting: apps add theirs again when Versailles tells them a taskbar was created (told twice, the second time 3 s later, for apps that were busy). An app that ignores that message shows up the next time it changes its icon. Versailles does not read the icons Explorer already has: on Windows 11 22H2 and later Explorer's tray is drawn with XAML (there is no `ToolbarWindow32` with buttons to read), and the only ways in are reading Explorer's memory or injecting into it, which Versailles does not do. An icon whose app quits without removing it is dropped within 2 s. Tooltips can be private (message counts, account names, VPN state): declare `tray` only on pieces that show them.

## Posture

`posture` tells a page how the machine is held, so it can reshape before the first tap: bigger taskbar buttons, a touch-first command bar. Read-only; any piece can declare it.

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44"
          data-monitor="all" data-hooks="windows,windows-control,posture"> … </template>
```

```js
const bar = versailles.for("taskbar");
await bar.on("posture", ({ tablet }) => {        // now, then on each change
  document.body.dataset.posture = tablet ? "tablet" : "laptop";
});
const { touch } = await bar.posture.now();       // one-off read
```

```css
body[data-posture="tablet"] .task { min-width: 56px; min-height: 48px; }
```

| Field | |
| ----- | - |
| `tablet` | `true` when a convertible is folded (or its keyboard detached) into tablet posture, `false` in laptop posture, `null` on a PC that does not report a posture (a desktop, a plain laptop) |
| `docked` | Windows reports the machine as docked |
| `touch.screen` | A touch screen (built-in or external) |
| `touch.pen` | A pen digitizer |
| `touch.maxTouches` | How many fingers the touch screen tracks at once (0 without one) |

A change is pushed as soon as Windows announces it (it broadcasts `ConvertibleSlateMode` / `SystemDockMode`), and the host reads again every 5 s in case a broadcast is missed. Windows' own "tablet mode" switch is gone on Windows 11; the posture comes from the hinge or keyboard sensor. `tablet` is `null` unless Windows keeps a convertible posture for this machine (`HKLM\SYSTEM\CurrentControlSet\Control\PriorityControl\ConvertibleSlateMode`).

`posture` says how the machine is held, not how the user is pointing right now. For that, a page can read each event's `pointerType` (`"touch"`, `"pen"`, `"mouse"`) or use the media queries `(any-pointer: coarse)` and `(pointer: coarse)`; neither needs a hook.

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

Six hooks for a quick-settings panel. Each reads like a sensor (`now()`, `onChange(fn)`, `on(hook, fn)`); each change goes through `quick_set` in the control class. It works only from a bar or overlay, only inside a click or key press, and answers the hook's new state. Every change is logged.

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
const { airplane } = await qs.radios.now();                    // null: no airplane switch here
if (airplane !== null) planeButton.onclick = () => qs.radios.setAirplane(!airplane);
volume.oninput = () => qs.audioControl.setVolume(volume.value);   // 0–100
muteButton.onclick = () => qs.audioControl.setMute(true);
const { outputs } = await qs.audioControl.now();               // [{ id, name, default }]
await qs.audioControl.setOutput(outputs[1].id);
const { input } = await qs.audio.now();                        // { device, volume, muted } or null
micMute.onclick = () => qs.audioControl.setInputMute(!input.muted);
mic.oninput = () => qs.audioControl.setInputVolume(mic.value);  // 0–100
const { inputs, sessions } = await qs.audioControl.now();      // inputs: [{ id, name, default }]
await qs.audioControl.setDefaultInput(inputs[1].id);
for (const s of sessions) {                                    // [{ id, app, exe, pid, icon, volume, muted, active, system }]
  slider(s.app, s.icon, s.volume).oninput = (e) => qs.audioControl.setSessionVolume(s.id, e.target.value);
}
await qs.audioControl.setSessionMute(sessions[0].id, true);
const { brightness, projection } = await qs.display.now();     // brightness: [{ id, kind, level }]
await qs.display.setBrightness(brightness[0].id, 60);          // 0–100
await qs.display.project("extend");                            // "pc" | "duplicate" | "extend" | "second"
const { nightLight } = await qs.display.now();                 // { supported, on }
if (nightLight.supported) await qs.display.setNightLight(!nightLight.on);
await qs.powerMode.set("balanced");                            // "efficiency" | "balanced" | "performance"

await qs.wifi.scan();                                          // no click needed; the list updates seconds later
await qs.on("wifi", ({ networks }) => draw(networks));         // [{ ssid, signal, secure, known, connected }]
await qs.wifi.connect("Home");                                 // a known network
await qs.wifi.connect("Café", "password");                     // a new secured one
await qs.wifi.disconnect();
const { devices } = await qs.bluetooth.now();                  // [{ id, name, connected, audio }]
await qs.bluetooth.connect(devices[0].id);                     // audio devices only
```

- **Radios:** Windows' radio switches. Wi-Fi and Bluetooth only.
- **Airplane mode** is Windows' own switch (the undocumented radio manager behind the Quick Settings tile, no admin needed). `airplane` reads `null` where Windows has no radio manager or the switch cannot be changed (a hardware slider); `setAirplane` then rejects. Offer `versailles.shell.open("ms-settings:network-airplanemode")` instead.
- **Wi-Fi** uses the WLAN API. Joining a new network saves a Windows profile (WPA2 / WPA3 personal or open; enterprise sign-in is not supported). Windows 11 24H2 may hide networks until location access is on: the read then says `access: "location-denied"`. Windows only refreshes the list on a scan, so call `scan()` when the panel opens.
- **Bluetooth** lists paired devices (classic and LE) from a device watcher, so a read is instant and changes are pushed. Windows has no public call to connect a device; headphones and speakers are connected and disconnected through their audio driver (as Windows' own audio flyout does). Mice and keyboards reconnect by themselves and show their state only.
- **Volume and mute** act on the default output; read them through `audio`. **The default output** uses the same undocumented call as Windows' sound settings and takes only ids from `outputs`.
- **Microphone:** `setInputVolume` and `setInputMute` act on the default microphone (Windows' default recording device); read them through `audio`'s `input`. `setDefaultInput` makes a microphone from `inputs` the default for every use (as Windows' sound settings do), through the same undocumented call as the default output, and takes only ids from `inputs`.
- **Per-app volume** is Windows' volume mixer for the default output: one entry per audio session (an app can have several). `id` is the session's instance identifier and stays the same while the session lives; a closed session drops out. `app` is the name the app gives its session, else its executable's description (`Firefox`), else the file name; `icon` is a 32×32 PNG data URL of the executable's icon, or `null`; `active` is whether it is playing now; the system sounds session (`system: true`, `app: "System sounds"`, no `exe` or `icon`) comes last. The list is read every 3 s and after each change, so a change made in Windows' mixer shows up within 3 s. **Privacy:** the list reveals which apps play or played sound; declare `audio-control` only on the piece that needs it.
- **Brightness:** built-in panels through WMI (`kind: "internal"`), external monitors over DDC/CI (`kind: "external"`, `device` is the screen, e.g. `\\.\DISPLAY2`; slow and not every monitor answers). A screen that offers neither is missing from the list.
- **Projection** is Win+P. `"second"` turns the built-in screen off.
- **Night light** has no Windows API: its state is an undocumented registry value that Explorer watches. `on` is whether night light is on now (by hand or by its schedule). Switching it works like the Quick Settings tile: by hand, so a schedule takes over again at its next start or end. Versailles writes only when it can re-encode the stored value exactly; otherwise `supported: false` (and `on: null` when it cannot be read at all) and `setNightLight` rejects. Offer `versailles.shell.open("ms-settings:nightlight")` instead. Changes made elsewhere show up within the 5 s `display` read.
- **Power mode** is the Settings slider (an undocumented Windows call). It needs the Balanced power plan; `supported: false` when Windows does not offer it. Energy saver is read-only.
- A setting Windows refuses fails with a message (`radios: Windows refused access`, `power-mode: … the power plan must be Balanced`) instead of failing silently.

## Keyboard

`keyboard` reads the installed keyboard layouts (input languages) and the one the app in front types with, and switches that app's layout, like Win+Space. Reading works anywhere; `set` is control class (a bar or overlay, inside a click or key press).

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44"
          data-hooks="windows,windows-control,keyboard"></template>
```

```js
const kb = versailles.for("taskbar").keyboard;
await kb.onChange(({ layouts, current }) => {
  const now = layouts.find((l) => l.id === current);
  pill.hidden = layouts.length < 2;
  pill.textContent = now ? now.short : "?";        // "DE", "EN"
});
pill.onclick = () => kb.next();                       // the next layout, wrapping around
const { ok } = await kb.set(layouts[1].id);           // false: the app did not switch
```

`now()` / `onChange(fn)`: `{ layouts, current }`. `current` is the `id` of the layout of the app in front (`null` when none is known). A layout:

| Field | |
| ----- | - |
| `id` | The layout handle (HKL) as 8 hex digits, `"04070407"`; pass it to `set` |
| `name` | The language in Windows' display language, `"Deutsch (Deutschland)"` |
| `short` | The language code in capitals, `"DE"`, for a pill. Two layouts of one language share it: show `layout` too |
| `lang` | The locale name, `"de-DE"` |
| `layout` | The keyboard layout's name, `"Deutsch"`, `"US"`; `null` when Windows has none |

- `set(id)` asks the app in front to switch (`WM_INPUTLANGCHANGEREQUEST` to its focus window, as the language switcher does) and answers the new state with `ok`. Clicking a bar can bring the bar to the front, so "the app in front" is the last app window in front that is not Versailles'. `next()` switches to the layout after `current`.
- `ok: false` when the app did not switch within 0.4 s: an app running as administrator ignores requests from Versailles (which runs as the user), and a few apps (some games, remote desktops) handle the keyboard themselves.
- With Windows' default setting (one input method for all apps) the switch applies to every app; with "Let me use a different input method for each app window" only to that app. `current` always reads the app in front.
- Windows has no event for another app's layout change: the host reads it every 0.5 s while a piece declares `keyboard`, and pushes only a change.

## Connections and accessibility

Four more quick-settings hooks: `vpn`, `hotspot`, `sharing` and `accessibility`. They read and change like the ones above: `now()` / `onChange(fn)` / `on(hook, fn)` anywhere, changes only from a bar or overlay, inside a click or key press, logged. Windows sends none of them as events an app outside the Store can get, so the host reads them every 2–3 s while a piece declares the hook.

A change answers the hook's new state, except where Windows' own window opened instead. Then it answers `{ ok: true, dialog: true }` (Windows' dialog) or `{ ok: true, settings: true }` (a Settings page), and the new state follows by push once the user is done.

```html
<template class="spawnable" data-id="qs" data-anchor="tr" data-w="340" data-h="600"
          data-hooks="vpn,hotspot,sharing,accessibility"> … </template>
```

```js
const qs = versailles.for("qs");

const { profiles } = await qs.vpn.now();             // [{ name, connected, connecting }]
const r = await qs.vpn.connect(profiles[0].name);    // the new state, or { ok, dialog: true }
await qs.vpn.disconnect(profiles[0].name);

const h = await qs.hotspot.now();                    // { supported, on, busy, clients, maxClients, reason }
if (h.supported) await qs.hotspot.set(!h.on);

const { casting, nearby } = await qs.sharing.now();  // nearby: "off" | "my-devices" | "everyone" | null
castButton.onclick = () => qs.sharing.openCast();    // Windows' Cast panel (Win+K)
await qs.sharing.setNearby("my-devices");

const a = await qs.accessibility.now();              // { magnifier, contrast, captions, colorFilter, stickyKeys }
await qs.accessibility.set("stickyKeys", !a.stickyKeys);
await qs.accessibility.set("magnifier", true);       // "magnifier" | "contrast" | "captions" | "colorFilter" | "stickyKeys"
```

**VPN** lists the VPN connections Windows itself manages (Settings > Network & internet > VPN: IKEv2, SSTP, L2TP, PPTP), through the documented RAS API. `connect(name)` dials with the sign-in Windows saved for the profile. Without a saved sign-in, for EAP (smart card, certificate) profiles, and when Windows rejects the saved one, Windows' own VPN dialog opens and asks (`{ ok: true, dialog: true }`). A profile a VPN app added to Windows, or a name Windows' phone book does not know, opens Settings > VPN instead (`settings: true`). A dial takes a few seconds; the promise waits for it. VPN apps with their own client (Tailscale, ProtonVPN, WireGuard, OpenVPN, company clients) are not Windows VPN profiles: they are not listed and keep their own tray icons (see [Tray](#tray)). Privacy: pages get profile names and their state only, never user names, passwords or servers, and the host never logs credentials. Profile names can say where you work: declare `vpn` only where needed.

**Mobile hotspot** is Settings > Mobile hotspot (`NetworkOperatorTetheringManager`, documented, no admin needed). It shares the connection the PC is online with: offline, or where Windows does not offer it, `supported` is false and `reason` says why (`"no-internet"`, `"hardware"`, `"group-policy"`, `"system"`, …); offer `versailles.shell.open("ms-settings:network-mobilehotspot")` instead. `on` is null and `busy` true while Windows switches it. A start Windows refuses rejects with its reason (Wi-Fi off, the adapter cannot share on this band, …). Windows turns the hotspot off by itself 5 minutes after the last device leaves, unless that is switched off in Settings. Privacy: the hotspot's network name and password are never read.

**Cast.** No Windows API lets an app start casting the screen, so `openCast()` opens Windows' own Cast panel (what Win+K opens), where the user picks a screen or disconnects. `casting` is true while a wireless display (Miracast) shows this PC's screen. Where the panel appears while Versailles hides Explorer's taskbar is up to Windows.

**Nearby sharing** (Settings > System > Nearby sharing) has no Windows API either: `nearby` reads and `setNearby(mode)` writes the undocumented value Settings uses (`HKCU\Software\Microsoft\Windows\CurrentVersion\CDP`, `NearShareChannelUserAuthzPolicy`). `null` (and `setNearby` rejecting) means this PC has not set nearby sharing up: offer `versailles.shell.open("ms-settings:crossdevice")`. Nearby sharing also needs Bluetooth on (`radios`).

**Accessibility:**

| Toggle | On | Off |
| ------ | -- | --- |
| `stickyKeys` | Windows' setting (documented, immediate) | the same |
| `contrast` | Windows applies the contrast theme last used (documented; takes a few seconds, Windows shows "Please wait") | opens Settings > Accessibility > Contrast themes (`{ ok: true, settings: true }`), where turning it off brings back your own theme |
| `magnifier` | starts Magnifier | Win+Esc, Magnifier's own shortcut |
| `captions` | starts Live captions (the first start asks to download a speech model) | Win+Ctrl+L, Live captions' own shortcut |
| `colorFilter` | Windows' colour filter shortcut (undocumented) | the same |

- `magnifier` and `captions` read whether the app runs; `null` where it is not installed (Live captions needs Windows 11 22H2). The shortcuts Versailles sends to close them pass its own keyboard hook untouched (also with `shell.suppress`); an elevated window in front can block them, and then the call rejects.
- A **contrast theme** makes Windows draw pages in its own colours: CSS `forced-colors: active` applies to every page, Paper chrome included, until it is off again. Style for it with `@media (forced-colors: active)` if your page needs to.
- **Colour filters** have no Windows API: `colorFilter` reads the filter setting (`HKCU\Software\Microsoft\ColorFiltering`, `Active`) and `set` runs Windows' colour filter shortcut (`atbroker /colorfiltershortcut`), which switches the filter Settings has chosen (greyscale by default) on or off, only when the state differs. If Windows does not switch within 3 s, `set` rejects: offer `versailles.shell.open("ms-settings:easeofaccess-colorfilter")`.

## Search, recent files and settings

The example command bar's Start menu: installed apps (with Store apps and icons), Settings pages, files from the Windows Search index, and recently opened files. All of it is `shell`; searching and recent files are exec class, so only a bar or overlay that declares `shell` gets them.

```js
const bar = versailles.for("action-bar");
const r = await bar.shell.search("rechnung");
// { query, source: "index" | "walk" | "off", ms,
//   settings: [{ kind: "settings", title, uri, en, de, score, exact }],
//   files: [{ kind: "file" | "folder", path, name, folder, modified, match: "name" | "contents" }] }
await bar.shell.open(r.settings[0].uri);                       // ms-settings:…
await bar.shell.search("bluetooth", { files: false });         // Settings pages only: instant
await bar.shell.search("rep", { limit: 8, fallback: false });  // as you type: never the slow walk
const recent = await bar.shell.recent(10);                     // [{ kind, path, name, folder, opened }]
const apps = await versailles.invoke("list_catalog");          // [{ id, name, target, kind: "shortcut" | "packaged", aumid, fresh, … }]
const icon = await versailles.invoke("catalog_icon", { id: apps[0].id }); // 32×32 PNG data URL or null
```

- **Files** come from the Windows Search index (the provider PowerToys Run uses): names first (starting with the query, then containing it), then contents, at most `limit` (default 40, up to 100). Hidden and dot folders, `AppData`, `node_modules`, `target` and the recycle bin are left out. `modified` is ms since 1970. When the index is off or does not answer within 1.5 s, the search walks Documents, Desktop, Downloads, Pictures and your profile instead (`source: "walk"`, names only, up to 2 s); `fallback: false` skips the walk (`source: "off"`). `cli_search_files` still answers a plain list of paths, index first.
- **Settings pages** are a built-in table of about 120 `ms-settings:` pages with English and German names and keywords; case, umlauts and hyphens do not matter (`lautstarke`, `wifi`). `title` is in the Windows display language; `exact` is true when the query is a whole name or keyword.
- **Recent files** are the shortcuts Windows keeps in `%APPDATA%\Microsoft\Windows\Recent`, newest first, at most `limit` (default 30, up to 100); missing targets and network paths are left out. The list is empty when "Show recently opened items" (Settings › Personalization › Start) is off.
- **Store apps** (packaged apps without a Start menu shortcut) are in `list_catalog` as `kind: "packaged"` with their AppUserModelID; `target` is `shell:AppsFolder\<id>` and `cli_open` starts them. A Store app with the same name as a shortcut is listed once.
- **Privacy:** search results and recent files reveal file names and folders. Declare `shell` only on the piece that needs it; the wallpaper and folder widgets never get these calls.

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

