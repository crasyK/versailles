# Examples

Steal these into `%USERPROFILE%\Documents\Widgets\desktop\index.html`. Do not iframe them.

## Paste rules

1. Keep `data-id` unique on the page.
2. Declare `data-hooks` for any host call (`media`, `shell`, `layout`, …).
3. Keep **one** `.action-bar` template. Copy its required ids from [`templates/starter/desktop/index.html`](../templates/starter/desktop/index.html).
4. Paste fragment `<style>` into the page stylesheet (tokens are already on `:root`).
5. Paste `<article>` or `<template>` next to the other pieces. Paste `<script>` at the bottom with the other widgets.

## Layout

| Path | What |
| ---- | ---- |
| [`gallery/index.html`](gallery/index.html) | Full showcase: quote, weather, watch, todo, pomodoro, now-playing, calculator, calendar, notes, action bar |
| [`widgets/`](widgets/) | One tile each (gallery set + archive ports) |
| [`spawnables/`](spawnables/) | Calculator, calendar, notes as standalone templates |

## Widgets

Gallery set: `quote`, `weather`, `watch`, `todo`, `pomodoro`, `now-playing`.

Archive ports (paper, dialect-native): `clock`, `dice`, `stopwatch`, `timer`, `coin-flip`, `password`, `random-number`, `quick-links` (`data-hooks="shell"`), `system-stats`.
