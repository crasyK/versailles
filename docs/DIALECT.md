# Dialect

Versailles parses one HTML file: `Documents\Widgets\desktop\index.html`. The parser is [`src-tauri/src/page.rs`](../src-tauri/src/page.rs). It only looks at opening tags. Nested markup is not scanned.

## Classes

| Class | Kind | Typical tag |
| ----- | ---- | ----------- |
| `widget` | Desktop tile | `<article class="widget" data-id="clock">` |
| `spawnable` | Overlay template | `<template class="spawnable" data-id="calculator">` |
| `action-bar` | Launcher chrome | `<template class="spawnable action-bar" data-id="action-bar">` |

A tag with `action-bar`, or a spawnable whose `data-id` is `action-bar`, is catalogued as the action bar.

## Attributes

| Attribute | Required | Meaning |
| --------- | -------- | ------- |
| `data-id` | yes | Catalog id. Unique, case-insensitive. `calc` aliases `calculator`. |
| `data-w` | no | Overlay width in CSS pixels. Default 280 (640 for the action bar). |
| `data-h` | no | Overlay height. Default 200 (420 for the action bar). |
| `data-anchor` | no | Spawn dock hint. `tr` = top-right. |
| `data-hooks` | no | Comma-separated allowlist. See [`HOOKS.md`](HOOKS.md). |

Duplicate `data-id` values are ignored after the first.

## Preview query

Same file, different surfaces:

| `?m=` | What you see |
| ----- | ------------ |
| `desktop` (default) | Wallpaper + widgets. Templates stay in `<template>`. |
| `widgets` | Every `article.widget` on a blank paper canvas |
| `spawn` | Each spawnable (except the action bar) materialized as a card |
| `spawn&id=calculator` | One spawnable, sized to `data-w` / `data-h` |
| `action_bar` | Action bar chrome only |

Unknown `id` → a placeholder. Best preview: `http://127.0.0.1:47831/files/desktop/index.html?m=…`.

A small boot script in the page (see [`templates/starter/desktop/index.html`](../templates/starter/desktop/index.html)) implements `?m=`. The host always loads `?m=spawn&id=…` into spawn windows.

## Action bar required ids

If any of these are missing, the host falls back to built-in [`launcher.html`](../launcher.html) chrome:

`cli-root`, `cli-title`, `cli-mode-label`, `cli-term-wrap`, `cli-term`, `cli-foot-l`, `cli-foot-m`, `cli-foot-r-hint`, `cli-foot-r`, `cli-in`, `cli-ps`, `cli-echo`, `cli-sug`, `cli-res`

Keep one `.action-bar` template. The engine stays in the host (`src/launcher.ts`); the HTML is chrome only.

## Scripts inside templates

`<script>` inside `<template>` does not run until the host (or the preview boot) clones the template into a `.spawn-surface[data-id="…"]`. Query that surface, not `document`, from overlay scripts.
