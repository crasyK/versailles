# Versailles

The HTML desktop you command — a sophisticated castle you furnish yourself.

Versailles is a Windows **runtime** for `index.html` widgets. You write (or keep) pages under `Documents\Widgets`. The app pins that desktop under your windows, serves the files, and gives you an Alt+Space action bar. It is not a widget editor, not a manager, not a second HUD.

Deck still exists at `Documents\Widgets\app` if you want the old floating windows, manager, canvas, or control API. This repo is the cleaned product.

## What you get

- **Desktop** — always-on-bottom fullscreen shell. The page is `Documents\Widgets\desktop\index.html`. Iframe widgets from `/files/{id}/index.html`.
- **Action bar** — Alt+Space. Apps, search, `= expr`, terminal, `desk` to toggle the surface. Cal.com 2024 paper chrome; nested xterm may stay dark.
- **Tray** — Desktop, Launcher, Quit. Left-click toggles the desktop.
- **Localhost runtime** (not a public API) on `127.0.0.1:47841` — `/files`, `/weather`, `/quote`, `/media/*`. No Bearer token. No remote widget control.

## What this is not

- No manager window
- No canvas / anywhere HUD
- No floating OS widget windows
- No calc/calendar/notes slide-outs (use the action bar and desktop notes)
- No second visual theme

## Develop

```bat
cd C:\Users\NediM\Documents\Versailles
npm install
npm run tauri:dev
```

Or `dev.cmd`. After start: tray icon, **Alt+Space**, type `desk` if the desktop is hidden.

Widgets used on the desktop are iframes in `Documents\Widgets\desktop\index.html`. Edit those HTML files on disk; the watcher reloads the page.

## Install / release

```bat
npm run tauri:build
```

NSIS installer lands in `src-tauri\target\release\bundle\nsis\`.

## Design

Cal.com 2024 paper-brutalism only. Rulebook: [`design/CAL-COM-RULEBOOK.md`](design/CAL-COM-RULEBOOK.md). Cursor rule: `.cursor/rules/cal-com-design.mdc`.

## Docs

- [Runtime](docs/RUNTIME.md) — windows, ports, config
- [Widgets](docs/WIDGETS.md) — how to add a card
- [Contributing](CONTRIBUTING.md)
