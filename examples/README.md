# Examples

Pieces to copy into your page, `%USERPROFILE%\Documents\Widgets\desktop\index.html`. Do not iframe them. For bars, a taskbar, quick settings and a launcher, see the [cookbook](../docs/cookbook/README.md) and the commented-out examples near the end of the [starter page](../templates/starter/desktop/index.html) (`try me` lines).

## Paste rules

1. Keep `data-id` unique on the page.
2. Declare `data-hooks` for every host call (`media`, `shell`, `layout`, …); the full list is in [HOOKS.md](../docs/HOOKS.md).
3. Paste a widget's `<article>` inside the wallpaper's markup (in the starter: `<section id="desktop">`), a `<template>` anywhere in `<body>`, and its `<script>` after it.
4. The examples use the starter's Paper variables (`--paper`, `--ink`, … on `:root`). On a page of your own, define them or replace them.
5. Keep **one** command bar (`template.spawnable.action-bar`). Its `#cli-*` ids are what `desktop/bar.js` expects: copy them from the [starter page](../templates/starter/desktop/index.html).

## Layout

| Path | What |
| ---- | ---- |
| [`gallery/index.html`](gallery/index.html) | A full page: quote, weather, watch, to-do, pomodoro, now playing, calculator, calendar, notes, command bar |
| [`widgets/`](widgets/) | One tile each |
| [`spawnables/`](spawnables/) | Calculator, calendar, notes as overlay templates |

## Widgets

`clock`, `coin-flip`, `dice`, `now-playing` (`data-hooks="media"`), `password`, `pomodoro`, `quick-links` (`data-hooks="shell"`), `quote`, `random-number`, `stopwatch`, `system-stats`, `timer`, `todo`, `watch`, `weather`.

`todo` and `pomodoro` keep their state in the page's `localStorage`; the [storage recipe](../docs/cookbook/storage-widget.md) shows the `storage` hook, which keeps it in a file next to your page.
