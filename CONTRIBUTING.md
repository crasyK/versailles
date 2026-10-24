# Contributing

Versailles is the HTML desktop runtime. User tiles live under `Documents\Widgets`, not in this repository.

## Rules

1. Paper is the look of the host's own chrome and the starter template ([`design/PAPER-THEME.md`](design/PAPER-THEME.md), [`.cursor/rules/paper-theme.mdc`](.cursor/rules/paper-theme.mdc)). User pages can look like anything; the host never styles them.
2. Single-file dialect. Read [`docs/DIALECT.md`](docs/DIALECT.md). Do not iframe widgets.
3. Keep `scripts/verify-acl.mjs` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together.
4. Do not add a settings UI to the action bar. Config is `#versailles` in `desktop/index.html` plus `.versailles/` runtime.
5. Do not ship a second built-in theme.

## Dev

```bash
npm install
npm run tauri:dev
```

`npm run build:test` builds a test installer with the `release-test` profile (no LTO, incremental: ~40 s after a code change instead of 20+ min, ~1.4 GB peak; never ship it). `npm run e2e` runs the end-to-end self-test against that build (or the release build): it starts the app isolated (`VERSAILLES_BENCH=1`, a temporary `VERSAILLES_ROOT`, a port in 47832–47841, the range page IPC is allowed on), checks wallpaper, sensors, bus, overlays, a docked bar, a page edit, a restart and hiding the Windows taskbar (restored after a kill), and cleans up. It never touches your desktop folder or your running Versailles (except with `--take-taskbar`, below). `-- --keep` leaves the app up when a check fails.

### The taskbar phase

The taskbar phase hides Explorer's taskbar from the page config and checks the Versailles tray (#35), restarts Explorer (`taskkill /f /im explorer.exe`; the test starts Explorer itself if Windows has not brought it back within 5 s) and checks that the new taskbar is hidden again, the Versailles tray is still the one apps find first and holds icons, and the docked bar keeps its strip; then config off, on again, and a kill of the app, after which the taskbar must come back. The Explorer restart closes open File Explorer windows.

The phase skips itself when the taskbar is already hidden, which is the case whenever your own Versailles hides it. Two ways to run it anyway:

- **`npm run e2e -- --take-taskbar`** stops your running Versailles for the phase and starts it again afterwards. It finds the top of each Versailles process tree (the supervisor; it never stops a taskbar guard or the test's own app), ends it with `taskkill /PID <pid> /F` (there is no clean external quit; the supervisor's job ends the app, and the taskbar guard brings the taskbar back), waits for Explorer's taskbar, runs the phase, and then always starts the same exe again, with `--autostart` if it had it, also when checks fail or on Ctrl+C. Apps you started from Versailles keep running. It prints every step. A dev build (`target\debug`) is not stopped: quit `npm run tauri:dev` yourself. `-- --take-taskbar --dry-run` prints the plan (which processes it would stop and what it would start) and changes nothing.
- **Windows Sandbox** (Windows Pro, Enterprise or Education, with the *Windows Sandbox* feature on; Windows Home has none): after `npm run build:test`, run `scripts\sandbox-e2e.cmd`. It fills in [`tests/e2e/sandbox.wsb`](tests/e2e/sandbox.wsb), maps the repo, the build folder and a Node folder read-only (`VERSAILLES_SANDBOX_NODE`, else the folder of `node.exe` on `PATH`; it needs a standalone `node.exe`, such as the nodejs.org `.zip`, not a version-manager shim) and `%TEMP%\versailles-sandbox` read-write, and runs the e2e inside on a clean Windows, with no network, 4 GB of memory and `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` set to the newest WebView2 runtime (WebView2 does not find it in Sandbox otherwise). It waits for `done.json` and prints `e2e.log`; the app's logs land in `logs\`. Not yet tried on a Pro machine.

`npm run measure:memory` starts the same isolated app three times (wallpaper only, plus a docked bar, plus an open overlay) and prints the private memory of the host and of each WebView2 process type. Keep `remote.urls` in `capabilities/default.json` to one pattern: Tauri compiles one URL matcher (~76 KB) per remote URL for every allowed command at startup, so 22 URLs cost the host ~360 MB.

## PR

Branch from `main`, open a pull request against `main`. Title the work after the change, not after the product name.

## Release

1. Bump the version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`.
2. If the SDK changed, bump its first line (`/*! versailles-sdk x.y.z */`) and `SDK_VERSION`, and add a line to `sdk::SHIPPED` (the `sdk` test prints the hash).
3. Add the release to [`CHANGELOG.md`](CHANGELOG.md), newest first.
4. Run the e2e with the taskbar phase: `npm run build:test`, then `npm run e2e -- --take-taskbar` (or `scripts\sandbox-e2e.cmd`). Every check passes and the taskbar phase is not skipped.
5. One release commit (`Ship x.y.z: …`, with `closes #n` for each issue), tag `vx.y.z`, and attach the NSIS installer to the GitHub release.
