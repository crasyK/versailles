# Widgets

Versailles does not own widget source. You keep HTML under `Documents\Widgets`. The app is a runtime.

## Desktop page

`Documents\Widgets\desktop\index.html` is the wallpaper. Embed widgets with:

```html
<iframe class="tile weather" src="/files/weather/index.html" title="Weather"></iframe>
```

`/files/{folder}/` maps to `Documents\Widgets\{folder}/`. Relative `fetch('/weather')` from an iframe hits the same origin as the file server.

## Adding a card

1. Create `Documents\Widgets\{id}/index.html` (and optional `widget.json` for size hints).
2. Iframe it from `desktop/index.html`.
3. Style with Cal paper: canvas behind the page, paper cards, 2px ink, hard `0 4px 0 0 #141414` shadow. See `design/CAL-COM-RULEBOOK.md`.

Do not open a floating OS window. If it is not on the desktop page, Versailles will not show it.

## Runtime helpers

- Weather and quote: HTTP `/weather`, `/quote` (the host proxies Open-Meteo / ZenQuotes so the iframe has no CORS fight).
- Now playing: HTTP `/media/now` (GSMTC). Nested iframes cannot use Tauri `invoke`.
- `Documents\Widgets\.sdk\deck.js` still exists for `enableNativeChrome` inside a real widget window. On the desktop page it is a no-op besides optional chrome.

## What stays in Deck

Floating widgets, slide-out calculator/calendar/notes, canvas, anywhere bar, and the Bearer control API live in `Documents\Widgets\app` only.
