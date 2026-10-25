# Changelog

What changed in each release of Versailles, newest first. Full notes and installers: [GitHub releases](https://github.com/crasyK/versailles/releases). Each release commit updates this file.

## 0.12.1 — 2026-10-25

- Fixed: quitting Versailles, or updating it from the tray, stalled for 45 s and was then ended by the watchdog while Versailles held the tray: the update never started. On exit the tray thread listed the app windows (to hand taskbar progress back to Explorer) and asked Versailles' own windows for their titles, which needed the main thread it was blocking. The exit now also waits at most 5 s for the tray.
- If you have 0.12.0 and Versailles hides your taskbar, the update from the tray stalls in the same way: install 0.12.1 from the release page instead.

## 0.12.0 — 2026-10-25

The last release before 1.0. The API is frozen, the docs are written, and Versailles does less while nothing happens.

- **Frozen for 1.x:** the dialect, the hook names and the SDK stay as they are through every 1.x release ([COMPATIBILITY.md](COMPATIBILITY.md)). Every refused or failed call throws one error shape with a `.code` (`hook-undeclared`, `needs-gesture`, `bad-argument`, …). Names that did not match the rest were renamed (`media.onChange`, `notify.onChange`, `audioControl.setDefaultOutput`, `display.setProjection`, `notifyControl.setBanners`, `tray.now`, `drag.enableNativeChrome`); the old names keep working. `on("media", fn)` now delivers the current track first, like the other hooks. TypeScript types (`.sdk/versailles.d.ts`) arrive with the SDK update, also on existing desktops (an edited copy is left alone). (#47)
- **Docs:** [Getting started](docs/GETTING-STARTED.md), a [cookbook](docs/cookbook/README.md) of seven recipes (top bar, taskbar, quick settings, command bar, tablet posture, storage, bus) and a new README. (#48)
- **Less work while idle:** a hook is read only while a piece that declares it can be seen (the wallpaper for a widget, the piece's own window for a bar or overlay), and nothing is read while the PC is locked or the display is off. Showing a piece again pushes what changed meanwhile. `media` polls slowly while no one sees it and reads cover art once per track; the accessibility and hotspot reads cost a fraction of before. The starter no longer repaints while idle. The starter with its top bar, taskbar and quick-settings examples went from about 7 % to 3 % of one core. `npm run e2e` now checks a performance budget (memory, idle CPU, start-up time). (#49)
- **Taskbar progress and badges:** while Versailles holds the tray, a window's download or copy progress and its badge (unread counts) appear in `windows` as `progress` and `badge`, and Explorer still gets them for when its taskbar comes back. Before, they could be lost while the tray was held. (#67)
- **Jump lists:** `windows.jumpList(id)` reads a window's tasks and recent files, `windows.openJump(id, key)` opens one. (#68)
- **Bluetooth pairing:** `bluetooth.scan()` finds new devices, `pair(id)` and `pairReply(id, accept, pin)` pair them, PIN steps included. (#69)
- **Security review:** pages can no longer create windows or register global shortcuts (a page could have opened a window that passed for one of its bars and skipped the click rule), terminal output reaches only the piece that opened it, media updates only pieces with `media`, and the localhost API compares its token in constant time. The threat model is in [docs/SECURITY.md](docs/SECURITY.md). (#51)
- Uninstalling while Versailles hides the Windows taskbar gives the taskbar back.
- `Versailles.log` uses local time, like the other logs, and no longer reports a missing `favicon.ico` many times an hour.
- SDK 0.12.0.

## 0.11.1 — 2026-10-25

The last 0.11 release: the gaps that still sent you to Windows' Start menu, taskbar and quick settings. The first release installed copies get from the tray.

- **Start menu in the command bar:** Store apps (Settings, Calculator, Photos, Store installs) with their icons; search through the Windows Search index, file names and contents, in milliseconds; Settings pages by name in English or German ("bluetooth", "anzeige"); recently opened files when the bar is empty. SDK: `shell.search`, `shell.recent`. (#54, #55, #56, #57)
- **Taskbar:** an app that wants attention marks its window (`attention` in `windows`), hovering a button shows Versailles' preview card with a live thumbnail (`windows.preview`), and a new `keyboard` hook shows and switches the input language. (#58, #59, #60)
- **Quick settings:** the microphone (mute, volume, input device) and a per-app volume mixer in `audio` / `audio-control`; new hooks `vpn` (Windows' own VPN profiles), `hotspot`, `sharing` (Windows' Cast panel and nearby sharing) and `accessibility` (magnifier, contrast theme, live captions, colour filters, sticky keys). Starting a cast is not possible for apps; Versailles opens Windows' Cast panel. (#61, #62, #63, #64, #65, #66)
- **`posture` hook:** whether a convertible is folded into tablet posture, docked, and its touch screen and pen, pushed when it changes. (#70)
- The starter's quick-settings and taskbar examples show all of it. SDK 0.11.1.
- Fixed: parts of the starter's commented-out quick-settings example showed on the page (an inner comment ended it early).

## 0.11.0 — 2026-10-24

- **Updates from the tray.** A minute after start and then once a day, Versailles looks for a newer release on GitHub and offers **Update to x.y.z** at the top of the tray menu. Nothing downloads before you click it. The download is checked against a key built into the app, then Versailles gives back the taskbar, the tray icons and the bars' screen space, runs the installer (a progress bar, no questions) and starts the new version. `"updates": { "check": false }` in `#versailles` turns the check off; pages get no access to it. This release is installed by hand; later ones arrive this way. (#41)
- **Diagnostics for bug reports:** tray menu **Copy diagnostics** (or `versailles.exe --diagnostics` when the app will not start) saves a zip to Downloads with versions, monitors, the page's pieces and hooks, the config and the logs. Values that look like passwords, tokens or keys are replaced, and your user name in paths becomes `%USERPROFILE%`. Nothing is sent anywhere. (#43)
- **First run:** without a `Documents\Widgets\desktop\index.html`, a welcome window offers to set up the example desktop or to let you bring your own page. It asks once, copies only on your click, and never overwrites a file. (#42)
- Code signing is prepared but off: releases stay unsigned until the project has a certificate, so Windows still shows "unknown publisher". (#40)
- Fixed: quitting could stall while the tray was being handed back to Explorer (it was started again in between).

## 0.10.0 — 2026-10-24

- **Win and Copilot hotkeys are reliable.** While one of Versailles' own windows had the focus (the desktop, an overlay, a bar), Windows skipped the keyboard hook and a bound Win press opened Start. The hook now runs in a small helper process (`versailles.exe --keys-hook`) and sees every press. Missed presses are logged ("missed by hook"), and the hook re-installs itself after unlock, resume and display-on. A fast key-up no longer leaves a replayed key stuck (Win+R). (#37)
- **Night light and airplane mode** for quick settings: `display.nightLight` / `display.setNightLight(on)` and `radios.airplane` / `radios.setAirplane(on)`. Night light is only written when Versailles understands the stored setting. (#36)
- **Tray:** hovering an icon shows the app's own pop-up (`tray.hover`), icons answer the keyboard (Enter / Space, the menu key), apps' balloon notifications are pushed to the page, and `showTip` says whether Windows would show a tooltip. Icons come back by themselves after Explorer restarts. (#39)
- **Testing:** `npm run e2e -- --take-taskbar` checks the hidden taskbar, the tray and an Explorer restart; a Windows Sandbox file runs the same on a clean system (Windows Pro). The log rotates at 1 MB and keeps three files. (#38)
- Fixed: docked bars lost their reserved space after Explorer restarted while the tray ran; a bar Explorer forgets is reserved again.
- Fixed: a crash when reading or switching radios from a short-lived thread.
- SDK 0.10.0.

## 0.9.1 — 2026-10-24

- **Tray icons in bars:** the `tray` hook lists the notification-area icons (tooltip, app, icon), pushes their changes and clicks them (left, right, double, middle) like Windows does, while Versailles hides the Windows taskbar. Versailles holds the tray with a window of its own, the way Cairo and RetroBar do; nothing is injected into Explorer, and Explorer keeps a copy of every icon, so nothing is lost when Versailles stops, crashes or Explorer restarts. The starter's taskbar example has a tray strip. SDK 0.9.1: `versailles.tray`. (#35)
- Fixed: a desktop SDK from a test build marked ahead of the release was never updated.

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
