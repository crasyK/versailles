# Deck

Lightweight HTML desktop for Windows — a Tauri v2 (Rust + TypeScript) host.

The desktop is one file: `Documents\Widgets\desktop\index.html`. Edit that page (wallpaper, HUD, iframes). Apps cover it. Calc / calendar / notes stay overlay slide-outs.

## Features

- Fullscreen HTML desktop page (`desktop/index.html`)
- Reuse widgets by iframing them from that page (`/files/clock/index.html`)
- Overlay slide-outs: calculator, calendar, notes
- GSMTC now-playing media info and controls
- Alt+Space launcher bar
- Silent tray startup
- Localhost API for AI / automation
- Legacy floating widgets parked under `Documents\Widgets\legacy`

## Develop / Launch

Close and reopen your terminal if you just installed Rust (so `cargo` is on PATH), then:

```bash
cd C:\Users\NediM\Documents\Widgets\app
npm install
npm run tauri:dev
```

Or double-click / run [`dev.cmd`](dev.cmd) — it prepends `%USERPROFILE%\.cargo\bin` automatically.

After start: look for the **Deck tray icon** (system tray). Left-click opens the manager. **Alt+Space** opens the launcher. Type `desk` to show the HTML desktop.

Widgets used on the desktop are iframes in `desktop/index.html`. Overlay slide-outs live next to that folder (`calculator`, `calendar`, `notes`). Old movable windows are under `legacy\`.

## Local API

Enabled by default on `http://127.0.0.1:47831`. Token is in Settings → API (or `.deck/config.json`).

```bash
curl -H "Authorization: Bearer <token>" http://127.0.0.1:47831/widgets
```
