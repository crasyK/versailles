# Versailles

Versailles makes one HTML page your Windows 11 desktop: widgets on the wallpaper, bars docked to the screen edges (up to a full replacement for the taskbar), and overlays a hotkey opens. The page asks Windows for what it needs (windows, tray icons, quick settings, notifications, media, search) through named hooks, and gets nothing it did not declare.

It is a small Tauri v2 host (Rust, WebView2) that lives in the notification area. You write the page; Versailles never styles it.

## Features

- **One editable page**: `Documents\Widgets\desktop\index.html`, reloaded on save. Widgets are `article.widget`, windows are `template.spawnable`, and a few `data-*` attributes say where they go ([DIALECT.md](docs/DIALECT.md)).
- **Wallpaper widgets** on every monitor or on the ones you name.
- **Overlays and slide-outs** as native always-on-top windows, anchored to a corner, an edge, the centre or the cursor, each with its own hotkey, including the **Windows and Copilot keys**.
- **Docked bars** (top bar, taskbar, dock, side rail) that reserve their strip like the Windows taskbar, float, or auto-hide; one copy per monitor if you like.
- **Replace the taskbar**: hide Explorer's taskbar while your bar takes its place, with window buttons, live hover previews, attention, the tray icons and a keyboard-layout pill. The taskbar always comes back (Quit, a crash, the next logon).
- **Quick settings** from your page: Wi-Fi networks, Bluetooth devices, volume, microphone and a per-app mixer, brightness, night light, projection, power mode, Do Not Disturb, notifications, Windows VPN profiles, the mobile hotspot, Cast and nearby sharing, accessibility toggles.
- **System readings**: battery, network, the focused window, audio, media (now playing), notifications, the tablet posture of a convertible, the keyboard layout.
- **A command bar** in the starter page: apps (Store apps included), Settings pages by name in English and German, the Windows Search index, recent files, shortcuts, PowerShell and a terminal.
- **Between pieces**: per-piece storage, a message bus, data handed to an overlay as it opens.
- **A frozen API**: 33 hooks, a page SDK with TypeScript types (`.sdk/versailles.d.ts`), one error shape, stable through 1.x ([COMPATIBILITY.md](COMPATIBILITY.md)).
- **Safe by default**: a piece gets only the hooks it lists; settings change only from bars and overlays, inside a click; web sites and other apps cannot use Versailles to get more than they already have ([SECURITY.md](docs/SECURITY.md)).
- **Updates from the tray**, verified against a key built into the app; diagnostics for bug reports with secrets removed; a supervisor that restarts the app after a crash.

## Install

Download `Versailles_x.y.z_x64-setup.exe` from the [latest release](https://github.com/crasyK/versailles/releases/latest) and run it. Releases are not code-signed, so Windows SmartScreen asks first: **More info** → **Run anyway**. The installer needs no administrator rights. On first start a welcome window offers to set up the example desktop.

The whole path, from the download to your own bar: [Getting started](docs/GETTING-STARTED.md).

## Documentation

| | |
| - | - |
| [Getting started](docs/GETTING-STARTED.md) | Install, the starter, editing live, your first widget, bar and hotkey overlay, hiding the taskbar, updating, uninstalling |
| [Cookbook](docs/cookbook/README.md) | Recipes: a top bar, a taskbar, quick settings, a command bar, tablet posture, storage, talking between pieces |
| [DIALECT.md](docs/DIALECT.md) | Reference: classes, `data-*` attributes, placement, docked bars, monitors, the `#versailles` config |
| [HOOKS.md](docs/HOOKS.md) | Reference: every hook, the SDK, payloads, errors, hook classes |
| [RUNTIME.md](docs/RUNTIME.md) | The local file server and API, config files, crash recovery, diagnostics, updates |
| [SECURITY.md](docs/SECURITY.md) | Threat model: what Versailles protects, from whom, and what it leaves to you |
| [COMPATIBILITY.md](COMPATIBILITY.md) | What 1.x guarantees, deprecations, versioning |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each release |
| [`examples/`](examples) | Widgets and overlays to copy into your page |

## Develop

Requirements: Node 22, Rust (stable), the WebView2 runtime (part of Windows 11).

```bash
npm install
npm run tauri:dev
```

Or run [`dev.cmd`](dev.cmd), which puts `%USERPROFILE%\.cargo\bin` on the `PATH`. The dev build serves the same `Documents\Widgets` folder on port 47831.

- `templates/starter/` is the example desktop the app ships and copies on first run. Its command bar, `desktop/bar.js`, is generated from [`src/launcher.ts`](src/launcher.ts) with `npm run build:bar`. The page SDK is [`templates/starter/.sdk/versailles.js`](templates/starter/.sdk/versailles.js) (plain script, no build step) with its types next to it.
- Checks: `npm run verify:acl` (Tauri permissions), `npm run verify:sdk` (the SDK, its types and the locked surface), `npm run verify:docs` (the SDK calls in the docs), `cargo test` in `src-tauri`, and the end-to-end self-test `npm run e2e`.

Rules, tests, the performance budget and the release steps: [CONTRIBUTING.md](CONTRIBUTING.md). Notes for coding agents: [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)
