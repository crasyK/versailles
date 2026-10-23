# Changelog

What changed in each release of Versailles, newest first. Full notes and installers: [GitHub releases](https://github.com/crasyK/versailles/releases). Each release commit updates this file.

## 0.9.0 — 2026-10-23

- **Hide the Windows taskbar:** `"shell": { "hideTaskbar": true }` hides Explorer's taskbar and its reserved strip so a Versailles bar can take its place. It comes back on its own if Versailles crashes or is ended (a guard process), at the next logon, or with `versailles.exe --restore-taskbar`. (#30)
- **Win combos and the Copilot key** as hotkeys through a keyboard hook, `shell.suppress` to block Windows shortcuts, and several combos per `data-hotkey` (`"Alt+Space, Win"`). A bare Win / Copilot tap is experimental: it can still open Start (#37). (#31)
- **Quick settings for pages:** `radios` (Wi-Fi / Bluetooth on-off), `audio-control` (volume, mute, default output), `display` (brightness, Win+P projection), `power-mode`. (#32)
- **Wi-Fi networks and Bluetooth devices:** `wifi` (scan, join with a password, disconnect) and `bluetooth` (paired devices, connect / disconnect headphones and speakers). (#34)
- **Notifications and Do Not Disturb:** `notify-control` (dismiss, clear all, open the app, an app's banners) and `dnd` (off / priority / alarms). (#33)
- Settings change only from bars and overlays, inside a click or key press: a new **control class** the host enforces. The starter has a quick-settings slide-out example (Ctrl+Alt+Q). SDK 0.9.0.
- Fixed: CSS and JS were cached for 60 s, so edits and SDK updates could show late.

## 0.8.0 — 2026-10-20

- **The host uses 31 MB instead of 388 MB.** The page IPC capability listed 22 URLs, and Tauri compiled a URL matcher per URL for every command; now it is one pattern for the same origins. A wallpaper-only desktop drops from ~610 MB to ~255 MB in total. `npm run measure:memory` prints the split. (#27)
- **End-to-end self-test:** `npm run e2e` starts the app isolated and checks the wallpaper, sensors, bus, overlays, a docked bar, a page edit and a restart. (#26)
- **Faster test builds:** `npm run build:test` (no LTO, incremental), ~40 s rebuilds at ~1.4 GB instead of 20+ min. (#28)
- **Docs:** this changelog, release steps in CONTRIBUTING, and DIALECT.md / HOOKS.md grouped with contents lists. (#29)

## 0.7.0 — 2026-10-20

- **`notify` hook:** read Windows notifications (`notify.now()`, `onToast(fn)`), read-only, without packaging. (#5)
- **Template scripts run once** in spawn windows; they ran twice. Own pages: delete `runScripts(wrap);` in `mountSpawn`. (#21)
- **No flash on first open** for monitor-covering overlays (`data-dim="page"`, `coverMonitor`). (#22)

## 0.6.0 — 2026-10-20

- **`windows` / `windows-control` hooks** for custom taskbars: list app windows with icons and pushed changes; focus, minimize, restore, maximize, close, move. (#12)
- **Docked bars on every monitor** with `data-monitor="all"`. (#23)
- **The desktop's SDK updates itself** (version line, `.bak` of the old copy, edited copies left alone). (#24)
- **Starter examples:** a storage note, a top bar, a taskbar. (#25)
- Fixed: editing the page hid docked bars; our own reservations counted as monitor changes (a loop); starter examples stopped because `window.versailles` is the `#versailles` element until the SDK loads; a bar Windows will not reserve now floats instead of collapsing.

## 0.5.0 — 2026-10-20

- **A wallpaper per monitor** that has something on it; `data-monitor="primary|all|<n>"` on widgets, any page element and spawnables; `monitor-changed`; `versailles.monitors()` / `monitor()`. (#20, #4)
- **Docked bars:** `data-anchor="top|bottom|left|right"` with `data-dock="reserve|float|auto-hide"`, reserving space like the taskbar, stepping behind fullscreen apps. (#10, #11)

## 0.4.1 — 2026-10-20

- Overlays get `overlay://hiding` and 60 ms to paint their closed state: no flash at rest before an animation.
- `power.sleep()` enables the privilege it needs, and explains Modern Standby PCs where apps cannot start sleep.

## 0.4.0 — 2026-10-20

- **Push channel:** `on(name, fn)` instead of polling; `media` pushes too. (#6)
- **Sensor hooks** `battery`, `network`, `focus`, `audio`. (#5)
- **Piece events** `shown`, `hidden`, `idle`, `active`, `theme-changed`. (#18)
- Fixed: bus messages did not reach wallpaper widgets (cross-origin iframe).

## 0.3.0 — 2026-10-20

- **`storage`** (per-piece JSON store), **`clipboard`**, **`power`** (lock, sleep, sign out, restart, shut down) and **`bus`** (messages between pieces) hooks; `spawn(id, data)`. (#16, #13, #14, #17)

## 0.2.0 — 2026-10-19

- Overlays other than `c` keep the page's own size and position across reveals. (#3)
- `data-anchor` grid (`tl tc ml c mr bl bc br`, `cursor`) and `data-offset`. (#7)
- `data-dim="host|page|none"`. (#8)
- Click-through windows with page-declared hit regions. (#9)
- `layout_update`: resize, re-anchor, always-on-top, hit regions. (#15)
- Paper is the starter look, not a rule. (#19)

## 0.1.7 — 2026-10-18

- Overlay windows are plain rectangles; pages draw their own corners. Centered overlays re-apply `data-w` / `data-h` on every open. (#2)
- Now Playing follows track and play-state changes; weather includes the next 12 hours.

## 0.1.6 — 2026-10-17

- Crash recovery: a supervisor restarts the app after a crash or a 45 s hang.
- The localhost file server only serves `desktop/` and `.sdk/`, checks Host and Origin, and decodes paths once.
- Hooks are bound to the calling window; commands that run programs only work from spawn windows that declare them.
- Action bar: the input is a real field again; terminal paste fires once; PTY events carry a session id.

## 0.1.5 — 2026-08-31

- The wallpaper opens without waiting for session restore and loads the page once; the page catalog is cached.
- The action bar engine moved into `bar.js`; overlays can pin top-center (`data-anchor="tc"`); new app icon; boot benchmark.

## 0.1.4 — 2026-08-22

- The action bar terminal behaves like VS Code's: WebGL, clickable links, Ctrl+F, ConPTY-safe wrapping.
- Generic spawnable engine config; click the dim to hide; a live PTY survives Alt+Space.

## 0.1.3 — 2026-08-20

- Fixed spawn windows hanging the host (config mutex re-lock). Docked spawnables open opaque.
- Starter draw pad is SVG.

## 0.1.2 — 2026-08-19

- Every hotkey spawnable gets its own global shortcut; overlays resize; terminal mode fills ~78% of the monitor.
- Saving the page no longer hangs the host.

## 0.1.1 — 2026-08-19

- Config and shortcuts live in `desktop/index.html` (`#versailles`).
- Alt+Space opens the page's hotkey spawnable; `!` runs pwsh inline, `!!` opens a PTY; overlays hide and show with a dim.
- `pty` / `shell` are gated by `data-hooks`.

## 0.1.0 — 2026-08-18

- First public cut: the runtime for `Documents\Widgets` pages, the Alt+Space action bar, and the localhost file server.
