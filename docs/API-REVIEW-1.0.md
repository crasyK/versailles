# API review for 1.0

Issue [#47](https://github.com/crasyK/versailles/issues/47): before 1.0, every name a page can depend on was reviewed once, so that pages keep working across 1.x. This file records what was looked at, what changed, what stays odd on purpose, and how the surface is locked.

The rule for 1.x: **nothing a page may use is removed or changes shape.** A rename keeps the old name as a deprecated alias that works through 1.x. New names may be added.

## Contents

- [Summary](#summary) · [How it is locked](#how-it-is-locked)
- [Dialect attributes](#dialect-attributes) · [Hook names](#hook-names) · [SDK names](#sdk-names) · [Payloads](#payloads) · [Errors](#errors)
- [Legacy pieces](#legacy-pieces) · [Deprecated names](#deprecated-names) · [Odd on purpose](#odd-on-purpose)

## Summary

| Area | Verdict | Changes in 0.12.0 |
| ---- | ------- | ----------------- |
| `data-*` attributes | Fine | None. Undocumented aliases written down (`data-monitor="main"`). |
| Hook names | Fine | None. 33 hooks, all lower-case kebab-case, frozen. |
| Hook classes | Fine, now part of the contract | Written down in [HOOKS.md](HOOKS.md#hook-classes) as a table generated from the gating code. |
| SDK namespaces | Mostly fine | `vpn`, `hotspot`, `sharing`, `accessibility` were only on `versailles.for(id)`; now also on `versailles`. |
| SDK method names | Six inconsistent names | New names, old ones kept as deprecated aliases (see [Deprecated names](#deprecated-names)). |
| `now()` / `onChange()` / `track()` | One gap | `media.onChange` and `notify.onChange` added; `on("media")` now delivers the current value first like every other state hook. |
| Errors | Mixed (host: bare strings; SDK: `Error`, one with `code`) | One shape: an `Error` with `code` and `message` (see [Errors](#errors)). `message` and `String(err)` keep their old text. |
| Push / answer payloads | Fine | None. Fields camelCase, enum values lower-case / kebab-case. |
| Legacy pieces | Kept | Nothing removed; each decided below. |
| Types | Missing | `.sdk/versailles.d.ts` covers the whole SDK. |

## How it is locked

`npm run verify:sdk` (also in CI) runs:

| Check | Fails when |
| ----- | ---------- |
| `node --check` on the SDK | The SDK does not parse. |
| `scripts/check-sdk-types.mjs` | A public SDK name has no type in `versailles.d.ts`, a type has no SDK name, a deprecated name lacks `@deprecated`, or the declarations do not compile. The SDK side comes from running the SDK and walking `versailles` and `versailles.for(id)`; the type side from the TypeScript checker. |
| `scripts/check-sdk-behaviour.mjs` | The error shape, the aliases, the state-hook first value or the top-level namespaces regress (the SDK against a fake host). |
| `scripts/check-api-surface.mjs` | A name in [`api-surface.json`](api-surface.json) is gone (SDK names, hooks, `data-*` attributes the parser reads, push names, piece events, error codes), a new name is not in the snapshot yet, or the [hook-class table](HOOKS.md#hook-classes) differs from the code. |

The `page` test `known_hooks_match_the_locked_surface` in `src-tauri` checks `KNOWN_HOOKS` and the parsed attributes against the same snapshot, so `cargo test` catches a removed hook too.

Adding a name: add it, then `npm run verify:sdk -- --update` (it adds new names and rewrites the HOOKS.md table, and never drops a name). Removing one is a decision for 2.0: edit `api-surface.json` by hand.

## Dialect attributes

Read by the host parser (`page.rs`) on the opening tag of `article.widget` / `template.spawnable`.

| Attribute | Values | Verdict |
| --------- | ------ | ------- |
| `data-id` | id; `calc` = `calculator` | Fine. The alias is legacy, kept (see below). |
| `data-w`, `data-h` | CSS px | Odd on purpose: short names, used by every page. No `data-width` alias. |
| `data-anchor` | `tl tc ml c mr bl bc br cursor`, `tr` (slide-out), `top bottom left right` (bar); `center` / `overlay` = `c` | Fine. Aliases kept. |
| `data-offset` | `x,y` | Fine. |
| `data-dim` | `host page none` | Fine. |
| `data-monitor` | `primary`, `all`, `cursor`, a number; `main` = `primary` | Fine. `main` was undocumented; now written down. |
| `data-dock` | `reserve float auto-hide` | Fine. |
| `data-peek` | px | Fine. |
| `data-hotkey` | accelerators, comma-separated; `Super` = `Win` | Fine. |
| `data-hooks` | comma-separated hook names | Fine. |
| `data-dismiss-on-blur` | `true false` (also `1 0 yes no`) | Fine. |

Classes: `widget`, `spawnable`, and `action-bar` (which alone also makes a spawnable: kept). Attributes the SDK or the starter read, not the host: `data-hit` (`layout.trackHits`), `data-no-drag` (`enableNativeChrome`), `data-monitor` on any element (the wallpaper filter), and on `<html>` the values the host and SDK set: `data-monitor`, `data-monitor-primary`, `data-stage` (example bar). They are part of the contract as documented in [DIALECT.md](DIALECT.md).

## Hook names

All 33 names are lower-case kebab-case and stay as they are. The `-control` suffix marks a hook that acts where a sibling only reads (`windows` / `windows-control`, `notify` / `notify-control`, `audio` / `audio-control`): a privacy and permission split, not a naming slip. `hotkey` has no command (it is a declaration); `pty` has no SDK namespace (see [Odd on purpose](#odd-on-purpose)).

## SDK names

One namespace per hook, named after it in camelCase. Shapes: `now()`, `onChange(fn)` (= `on(hook, fn)`), `track(fn)` for lists, `set(…)` / `setX(…)` for changes, verbs for actions (see [HOOKS.md › Patterns](HOOKS.md#patterns)).

| Name | Verdict | Change |
| ---- | ------- | ------ |
| `versailles.vpn`, `.hotspot`, `.sharing`, `.accessibility` | Missing at the top level (only via `for(id)`) | Added. |
| `media.now()` / `media.nowPlaying()` | Two names for one read | `nowPlaying` deprecated. |
| `media.onUpdate(fn)` | Listened to the `media://update` broadcast (every window, no first value), unlike every other hook | New `media.onChange(fn)` on the gated push; `onUpdate` deprecated. |
| `on("media", fn)` | The only state hook without a first value | Now calls `fn` with `media.now()` first (`push.initial`). |
| `notify.onToast(fn)` | The only subscription not called `onChange` | New `notify.onChange(fn)`; `onToast` deprecated. |
| `audioControl.setOutput(id)` | Next to `setDefaultInput(id)` | New `setDefaultOutput(id)`; `setOutput` deprecated. |
| `display.project(mode)` | The value reads as `projection` | New `setProjection(mode)`; `project` deprecated. |
| `notifyControl.banners(aumid, on)` | A setter without `set` | New `setBanners(aumid, on)`; `banners` deprecated. |
| `tray.state()` | Every other hook reads with `now()` | New `tray.now()`; `state` deprecated. |
| `drag.enable(el)` | A second name for `enableNativeChrome(el)` | Deprecated. |
| `windows.focus/minimize/restore/maximize/close/move` | Granted by `windows-control` but on `windows` | Odd on purpose (they act on `windows.list()` items). |
| `clipboard.onChange(fn)` | Calls `fn()` without data | Fine: the push carries no content; read it if needed. |
| `keyboard.set(id)` | Answers the state plus `ok` | Fine. |
| `spawn(id, data)` | Resolves a boolean; outside the host `{ host: false, ok: false }` | Odd on purpose (pages test `ok`). |
| `mouse.position()` | `[x, y]`, not `{ x, y }` | Odd on purpose: kept as is. |
| `windows.list()`, `tray.now()`, `monitors()`, `monitor()` | Outside the host: `[]`, `{ active: false, icons: [] }`, `[]`, `null` instead of `{ host: false }` | Odd on purpose: lists render empty without a special case. |
| `invoke`, `listen`, `waitForTauri`, `widgetId`, `closeWidget`, `spawnData`, `onSpawn`, `version`, `for` | Fine | None. |

## Payloads

Pushes and answers were checked for field casing and value spelling: fields are camelCase (`secondsLeft`, `signalBars`, `nightLight`, `energySaver`, `maxClients`, `colorFilter`, `idleMs`, `positionMs`, `thumbnailDataUrl`), enumerated values lower-case or kebab-case (`"location-denied"`, `"my-devices"`, `"no-internet"`, `"opened"`, `"balloon"`), units documented per field (logical px for layout and monitors, physical px for `windows` rects and `tray` clicks, CSS px for hit regions and preview targets, ms since 1970 for times). Push names equal hook names; piece events are kebab-case (`theme-changed`, `monitor-changed`). Quick-settings changes answer the hook's new state, or `{ ok: true, dialog: true }` / `{ ok: true, settings: true }` when Windows' own window opened instead. Nothing changed; `.sdk/versailles.d.ts` now types every payload.

Push frame (`window.__VERSAILLES_PUSH__`): `{ name, data, id? }`; the SDK adds `initial: true` to the first value of a state hook. Bus message: `{ topic, data, from }`.

## Errors

Before: the host rejected with a bare message string; the SDK threw `Error`s, of which only `hook-undeclared` had a `code`. Now every SDK rejection, including `versailles.invoke`, is an `Error` with:

- `code`: `no-host`, `hook-undeclared`, `hook-refused`, `unknown-hook`, `needs-gesture`, `unknown-event`, `bad-argument`, `host-error` (table in [HOOKS.md › Errors](HOOKS.md#errors));
- `message`: the same text as before.

Compatibility: `err.message` is now always set (it was `undefined` for host strings). `String(err)` and `` `${err}` `` of a host rejection still give the bare message. Code that tested `typeof err === "string"` sees an object now; nothing in the repo or the starter does. The host's own wire format is unchanged (a string), so the bar and raw Tauri callers see what they always saw.

## Legacy pieces

| Piece | Decision | Why | Migration |
| ----- | -------- | --- | --------- |
| `launcher://shown` / `launcher://hidden` events | Deprecate, keep sending | Old `bar.js` copies on user desktops listen to them | Listen to `overlay://shown` / `overlay://hidden` (filter on `id`), or the piece events `shown` / `hidden`. |
| `overlay://id` event | Deprecate, keep sending | Undocumented; older pages may listen | `overlay://shown` carries `id`. |
| `media://update` event / `media.onUpdate` | Deprecate, keep sending | Broadcast to every window; the gated push replaced it | `media.onChange(fn)`. |
| `clipboard://changed` event | Deprecate, keep sending | The push replaced it | `clipboard.onChange(fn)`. |
| `deck` global | Alias, deprecated | The pre-rename name; pages guard with `window.versailles \|\| window.deck` | Use `versailles`. No warning is logged (a guard reads it). |
| `__DECK_WIDGET_ID__`, `deckWidgetId` query, `__DECK_NATIVE_CHROME__` | Keep, internal | Read (or set) alongside the `__VERSAILLES_*` names for old pages | None needed. |
| `calc` id alias | Keep | Pages and shortcuts use both names for the calculator | None. No new id aliases will be added. |
| `center` / `overlay` anchor aliases | Keep | Documented in DIALECT.md | `c`. |
| `.action-bar` without `.spawnable` | Keep | Older pages | Add `spawnable` (optional). |
| `#versailles` `launcher.hotkey` | Keep | The fallback hotkey; "launcher" is the old name of the bar | `data-hotkey` on the piece. |
| `.deck` runtime folder migration | Keep | Upgrades very old installs | None. |
| `drag.enable` | Alias, deprecated | Second name for `enableNativeChrome` | `enableNativeChrome(el)`. |
| Folder widgets (`widget.json`), `closeWidget`, `popup_widget_menu` | Keep | Floating folder widgets still load | Prefer page pieces. |
| HTTP API `/launcher/show` | Keep | Documented in RUNTIME.md; scripts may call it | None. |

## Deprecated names

Each works through 1.x, logs `versailles: … is deprecated` once per page load (except `deck`), and is marked `@deprecated` in the types.

| Deprecated | Use |
| ---------- | --- |
| `media.nowPlaying()` | `media.now()` |
| `media.onUpdate(fn)` | `media.onChange(fn)` (gets the current value first) |
| `notify.onToast(fn)` | `notify.onChange(fn)` |
| `audioControl.setOutput(id)` | `audioControl.setDefaultOutput(id)` |
| `display.project(mode)` | `display.setProjection(mode)` |
| `notifyControl.banners(aumid, on)` | `notifyControl.setBanners(aumid, on)` |
| `tray.state()` | `tray.now()` |
| `drag.enable(el)` | `enableNativeChrome(el)` |
| `window.deck` | `window.versailles` |
| events `launcher://*`, `overlay://id`, `media://update`, `clipboard://changed` | see [Legacy pieces](#legacy-pieces) |

## Odd on purpose

- **`windows-control` calls live on `windows`.** They take ids from `windows.list()`; a separate namespace would split one object in two. Declaring `windows-control` is what grants them.
- **`pty` has no SDK namespace.** The terminal is the example bar's business: `pty_open` / `pty_write` / `pty_resize` / `pty_is_alive` / `pty_close` through `versailles.invoke`, and `pty://data` / `pty://exit` events, are its contract (HOOKS.md › Events).
- **Raw commands through `versailles.invoke`.** Only the documented ones are part of the contract: `sys_stats`, `list_catalog`, `catalog_icon`, `dismiss_launcher`, `monitor_info`, `get_spawnable_engine_context`, and the `pty_*` / `cli_*` commands the example bar uses. Command names such as `toggle_slideout` or `*_cmd` are internal: use the SDK.
- **`audio` reads volume, `audio-control` sets it.** The split keeps per-app audio (who plays sound) behind its own hook.
- **`wifi.scan()` needs no click** but is control class on the host: it changes nothing the user can see, and the host still refuses it from the wallpaper.
- **`mouse.position()` answers `[x, y]`** and `spawn()` a boolean: kept, pages depend on them.
- **`data-w` / `data-h`**: short, everywhere; no long aliases.
- **`String(err)` of an SDK-made error** reads `Error: …` (as before), of a host error the bare message (as before).
