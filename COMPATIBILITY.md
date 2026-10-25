# Compatibility

What Versailles 1.x promises to pages built on it, what it does not, and how names are retired. In short: **a page that works with 1.0 keeps working with every 1.x release.** Additions are fine; nothing you may use is removed or changes shape before 2.0.

The surface was reviewed once before 1.0 ([docs/API-REVIEW-1.0.md](docs/API-REVIEW-1.0.md)) and is locked by a snapshot ([docs/api-surface.json](docs/api-surface.json)) that the build checks (see [How it is enforced](#how-it-is-enforced)).

## What 1.x guarantees

Each item below keeps its name, its meaning and its shape through 1.x. New names, values and fields may be added.

| Area | What stays | Reference |
| ---- | ---------- | --------- |
| **Dialect** | The classes `widget`, `spawnable`, `action-bar`; the attributes `data-id`, `data-w`, `data-h`, `data-anchor`, `data-offset`, `data-dim`, `data-monitor`, `data-dock`, `data-peek`, `data-hotkey`, `data-hooks`, `data-dismiss-on-blur`, and their values; the attributes the SDK reads (`data-hit`, `data-no-drag`, `data-monitor` on any element) and sets on `<html>` (`data-monitor`, `data-monitor-primary`); a piece inside an HTML comment is not on the page | [DIALECT.md](docs/DIALECT.md) |
| **Aliases** | `calc` = `calculator`, `center` / `overlay` = `c`, `main` = `primary`, `Super` = `Win`, a tag with only `action-bar` is a spawnable | [DIALECT.md](docs/DIALECT.md#attributes) |
| **Windows a page gets** | The URL the host loads into a spawn window (`?m=spawn&id=<id>`, plus `monitor=<n>&primary=0\|1`), the window labels `desktop`, `desktop-<n>`, `widget-<id>`, `widget-<id>:<n>`, `window.__VERSAILLES_STAGE__` and the `--piece-*` values for `data-dim="page"` | [DIALECT.md](docs/DIALECT.md#overlay-placement) |
| **Hooks** | All 33 hook names, and each hook's class (plain, control, exec): a hook never moves to a stricter class, so a call that works from a bar or the wallpaper today keeps working there | [HOOKS.md › Hook classes](docs/HOOKS.md#hook-classes) |
| **SDK** | Every name in `api-surface.json` → `sdk`, on `versailles` and on `versailles.for(id)`; their arguments and what they resolve; `{ host: false }` outside Versailles; subscriptions resolve a function that unsubscribes; state hooks deliver the current value first (`push.initial`) | [HOOKS.md](docs/HOOKS.md), [`.sdk/versailles.d.ts`](templates/starter/.sdk/versailles.d.ts) |
| **Payloads** | Every documented field of a push or an answer keeps its name, type and unit (fields camelCase; logical px for layout and monitors, physical px for `windows` and `tray` positions, ms since 1970 for times); the push frame `{ name, data, id?, initial? }`; bus messages `{ topic, data, from }` | [HOOKS.md › Push](docs/HOOKS.md#push), the types |
| **Events** | Push names (= hook names), piece events (`shown`, `hiding`, `hidden`, `idle`, `active`, `theme-changed`, `monitor-changed`), host events (`overlay://shown`, `overlay://hiding`, `overlay://hidden`, `pty://data`, `pty://exit`), the window events `versailles:spawn` and `versailles:stage` | [HOOKS.md › Events](docs/HOOKS.md#events) |
| **Errors** | Every rejection is an `Error` with a `code`; the eight codes (`no-host`, `hook-undeclared`, `hook-refused`, `unknown-hook`, `needs-gesture`, `unknown-event`, `bad-argument`, `host-error`) keep their meaning | [HOOKS.md › Errors](docs/HOOKS.md#errors) |
| **Documented raw commands** | Through `versailles.invoke`: `sys_stats`, `list_catalog`, `catalog_icon`, `dismiss_launcher`, `monitor_info`, `get_spawnable_engine_context`, and the `pty_*` / `cli_*` commands the starter's command bar uses | [API-REVIEW-1.0.md › Odd on purpose](docs/API-REVIEW-1.0.md#odd-on-purpose) |
| **Config** | The `#versailles` keys `autostart`, `snapThreshold`, `launcher.hotkey`, `api.enabled`, `api.port`, `shortcuts` (`n`, `t`, `d`, `target`, `cat`, `aliases`), `shell.hideTaskbar`, `shell.suppress`, `updates.check`, `spawnables.<id>.overlay` (`dim`, `dismissOnBlur`, `draggable`) and `spawnables.<id>.opts.coverMonitor`; the sidecar `versailles.json` fallback | [DIALECT.md › Config blob](docs/DIALECT.md#config-blob) |
| **Your folder** | Versailles never rewrites `desktop\index.html` or `desktop\bar.js`; each piece's store stays at `.versailles\storage\<id>.json`; `/files/desktop/**` and `/files/.sdk/**` are served as today | [RUNTIME.md](docs/RUNTIME.md) |
| **The SDK update** | On start, Versailles replaces `.sdk\versailles.js` only when it is an unchanged copy of an older release (keeping `versailles.js.bak`); a copy you edited, or one marked newer, is left alone; a missing one is served from the app | [HOOKS.md › Keeping it current](docs/HOOKS.md#sdk) |
| **Getting the taskbar back** | Removing `hideTaskbar`, Quit, a crash or kill (the guard process), the next logon (`RunOnce`) and `versailles.exe --restore-taskbar` each bring Explorer's taskbar back; your own auto-hide setting is restored | [DIALECT.md › Hiding the Windows taskbar](docs/DIALECT.md#hiding-the-windows-taskbar) |
| **Command line and local API** | `--restore-taskbar`, `--diagnostics`; the Bearer API routes and same-origin helpers listed in RUNTIME.md | [RUNTIME.md](docs/RUNTIME.md) |

### What "additions" can mean for your code

A 1.x release may add a hook, an SDK method, a payload field, an enumerated value (a new `type` of window change, a new hotspot `reason`), an error code or a config key. To stay safe:

- ignore fields you do not know, and give `switch` statements a default branch;
- test for a method before you call it if you also run on older releases: `typeof versailles.for(id).tray?.hover === 'function'`;
- branch on `err.code`, never on `err.message`;
- handle `{ host: false }` (a preview in a browser) and `null` values the docs mention (`airplane: null`, `tablet: null`).

## What is not covered

These can change in any release, minor or patch:

- **Text meant for people:** error `message` text, log lines and their format (`Versailles.log`, `taskbar.log`, `keys.log`), tray menu wording, the contents of the diagnostics zip.
- **The host's own chrome:** the Paper look of the welcome window, the overlay dim and the window preview card (which a page cannot style anyway), and their sizes and timings.
- **Undocumented things:** host commands that are not listed above (`toggle_slideout`, `*_cmd`, …: use the SDK), internal globals (`__VERSAILLES_PUSH__`, `__VERSAILLES_BUS__`, `__DECK_*`), files in `.versailles\` other than `storage\`, environment variables for testing (`VERSAILLES_BENCH`, `VERSAILLES_ROOT`, `VERSAILLES_UPDATE_URL`, …), the e2e and build scripts.
- **Timing:** how often a sensor is read (the intervals in HOOKS.md describe today's values), debounce windows, start-up time, memory use.
- **The starter and the examples.** `templates/starter/` and `examples/` change from release to release. Your copy is yours: Versailles never updates `index.html` or `bar.js` for you, so a new starter reaches you only if you copy it.
- **Windows itself.** Several features rest on what Windows offers, some of it undocumented (night light, Do Not Disturb, power mode, the default audio device, nearby sharing, airplane mode, the tray protocol). When a Windows update takes something away, Versailles reports it the documented way (`supported: false`, `null`, a `host-error` with a reason) instead of failing silently; it cannot bring the feature back. The same holds for Windows' key handling (Win+L, elevated apps) and for SmartScreen's view of unsigned releases.
- **Security fixes.** If a call turns out to be unsafe, a 1.x release may refuse it where it worked before (for example a dangerous URL handler in `shell.open`). Such a change is named in the changelog and in [SECURITY.md](docs/SECURITY.md).

## Deprecations

A name is never removed in 1.x. When a better name appears:

1. The new name ships in a 1.x release; the old name keeps working as an alias that does exactly what it did.
2. The old name logs `versailles: <name> is deprecated (still works through 1.x); use <new name>` once per page load, is marked `@deprecated` in `versailles.d.ts`, and is listed in the changelog and in [API-REVIEW-1.0.md › Deprecated names](docs/API-REVIEW-1.0.md#deprecated-names).
3. Deprecated events and payload fields keep being sent.
4. Removal happens only in 2.0, which lists every removal with what to use instead.

The names deprecated at 1.0: `media.nowPlaying`, `media.onUpdate`, `notify.onToast`, `audioControl.setOutput`, `display.project`, `notifyControl.banners`, `tray.state`, `drag.enable`, `window.deck`, and the events `launcher://shown` / `launcher://hidden`, `overlay://id`, `media://update`, `clipboard://changed`.

## Versioning

The app and the SDK use [Semantic Versioning](https://semver.org/):

| Change | App | SDK |
| ------ | --- | --- |
| Something a page uses is removed or changes shape | major (2.0) | major |
| A new hook, method, field, value, config key, attribute | minor (1.1) | minor |
| A fix that adds nothing | patch (1.0.1) | patch |

- The SDK's version is on its first line (`/*! versailles-sdk 1.0.0 */`) and in `versailles.version`. It changes only in releases that change the SDK, so it can trail the app's version (app 1.2.1 may ship SDK 1.2.0).
- The SDK ships inside the app and is updated with it (see the SDK update above). An SDK from an older 1.x keeps working on a newer 1.x host; it just lacks the newer names. A newer SDK on an older host may call names that host does not know: those calls reject (`unknown-hook` or `host-error`).
- Releases are published on [GitHub](https://github.com/crasyK/versailles/releases) and listed in the [changelog](CHANGELOG.md). They are not code-signed; updates installed from the tray are verified against the updater key built into the app.

## How it is enforced

- `npm run verify:sdk` (in CI on every push) fails when a name in [`docs/api-surface.json`](docs/api-surface.json) is gone (SDK names, hooks, attributes the parser reads, push names, piece events, error codes), when the types and the SDK disagree, when a deprecated name lacks `@deprecated`, or when the hook-class table in HOOKS.md no longer matches the code.
- A `cargo test` in the host checks the hook list and the parsed attributes against the same snapshot.
- Names are added to the snapshot with `npm run verify:sdk -- --update`, which never drops one. Removing a name means editing the snapshot by hand: a decision for 2.0.
