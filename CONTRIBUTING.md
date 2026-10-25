# Contributing

Versailles is the HTML desktop runtime. User tiles live under `Documents\Widgets`, not in this repository.

## Rules

1. Paper is the look of the host's own chrome and the starter template ([`design/PAPER-THEME.md`](design/PAPER-THEME.md), [`.cursor/rules/paper-theme.mdc`](.cursor/rules/paper-theme.mdc)). User pages can look like anything; the host never styles them.
2. Single-file dialect. Read [`docs/DIALECT.md`](docs/DIALECT.md). Do not iframe widgets.
3. Keep `scripts/verify-acl.mjs` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together (or a capability file of its own for a host window, listed in `tauri.conf.json`, like `welcome.json`).
4. Do not add a settings UI to the action bar. Config is `#versailles` in `desktop/index.html` plus `.versailles/` runtime.
5. Do not ship a second built-in theme.
6. The dialect, hooks and SDK are frozen for 1.x ([`docs/API-REVIEW-1.0.md`](docs/API-REVIEW-1.0.md)). Keep `npm run verify:sdk` green: it checks that `.sdk/versailles.d.ts` types exactly the SDK, that no name in [`docs/api-surface.json`](docs/api-surface.json) is gone, and that the hook-class table in `docs/HOOKS.md` matches the code. After adding a name (and its type), `npm run verify:sdk -- --update` adds it to the snapshot. What 1.x promises users: [`COMPATIBILITY.md`](COMPATIBILITY.md).
7. User docs: [`docs/GETTING-STARTED.md`](docs/GETTING-STARTED.md) and the [cookbook](docs/cookbook/README.md) use only current (not deprecated) names; `npm run verify:docs` checks every SDK call, hook, event and attribute in their code blocks (and in README, DIALECT and HOOKS) against the snapshot.

## Dev

```bash
npm install
npm run tauri:dev
```

`npm run build:test` builds a test installer with the `release-test` profile (no LTO, incremental: ~40 s after a code change instead of 20+ min, ~1.4 GB peak; never ship it). `npm run e2e` runs the end-to-end self-test against that build (or the release build): it starts the app isolated (`VERSAILLES_BENCH=1`, a temporary `VERSAILLES_ROOT`, a port in 47832–47841, the range page IPC is allowed on), checks wallpaper, sensors, bus, overlays, a docked bar, a page edit, a restart and hiding the Windows taskbar (restored after a kill), then the performance budget (below), and cleans up. It never touches your desktop folder or your running Versailles (except with `--take-taskbar`, below). `-- --keep` leaves the app up when a check fails.

### The taskbar phase

The taskbar phase hides Explorer's taskbar from the page config and checks the Versailles tray (#35), restarts Explorer (`taskkill /f /im explorer.exe`; the test starts Explorer itself if Windows has not brought it back within 5 s) and checks that the new taskbar is hidden again, the Versailles tray is still the one apps find first and holds icons, and the docked bar keeps its strip; then config off, on again, and a kill of the app, after which the taskbar must come back. The Explorer restart closes open File Explorer windows.

The phase skips itself when the taskbar is already hidden, which is the case whenever your own Versailles hides it. Two ways to run it anyway:

- **`npm run e2e -- --take-taskbar`** stops your running Versailles for the phase and starts it again afterwards. It finds the top of each Versailles process tree (the supervisor; it never stops a taskbar guard or the test's own app), ends it with `taskkill /PID <pid> /F` (there is no clean external quit; the supervisor's job ends the app, and the taskbar guard brings the taskbar back), waits for Explorer's taskbar, runs the phase, and then always starts the same exe again, with `--autostart` if it had it, also when checks fail or on Ctrl+C. Apps you started from Versailles keep running. It prints every step. A dev build (`target\debug`) is not stopped: quit `npm run tauri:dev` yourself. `-- --take-taskbar --dry-run` prints the plan (which processes it would stop and what it would start) and changes nothing.
- **Windows Sandbox** (Windows Pro, Enterprise or Education, with the *Windows Sandbox* feature on; Windows Home has none): after `npm run build:test`, run `scripts\sandbox-e2e.cmd`. It fills in [`tests/e2e/sandbox.wsb`](tests/e2e/sandbox.wsb), maps the repo, the build folder and a Node folder read-only (`VERSAILLES_SANDBOX_NODE`, else the folder of `node.exe` on `PATH`; it needs a standalone `node.exe`, such as the nodejs.org `.zip`, not a version-manager shim) and `%TEMP%\versailles-sandbox` read-write, and runs the e2e inside on a clean Windows, with no network, 4 GB of memory and `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` set to the newest WebView2 runtime (WebView2 does not find it in Sandbox otherwise). It waits for `done.json` and prints `e2e.log`; the app's logs land in `logs\`. Not yet tried on a Pro machine.

### The performance phase

Last, the e2e starts the isolated app once more on the e2e page (the starter page plus the test pieces: a wallpaper that declares every sensor hook, a docked bar, the overlay the wallpaper opens; a page edit then registers the hotkeys, which starts the key helper), waits 20 s, and checks four budgets (#49):

| Check | Measures | Budget key |
| ----- | -------- | ---------- |
| start-up to wallpaper ready | the app's own clock from process start to the wallpaper page's load event (bench mode writes it to `.versailles/boot.json`) | `startupMs` |
| host private memory | the app process only; the key helper is reported next to it, not counted (with `VERSAILLES_NO_SUPERVISOR` there is no supervisor, and no taskbar guard runs) | `hostPrivateMB` |
| WebView2 total memory | private memory of every WebView2 process of this instance, split by type | `webviewTotalMB` |
| idle CPU over 30 s | CPU time of the host, the key helper and its WebView2 processes, in percent of one core (100 = one core busy), split by process | `idleCpuPercent` |

Only this instance is measured: its process tree (the WebView2 browser process is a child of the app, its renderers, GPU and utility processes children of that) and its own WebView2 data folder, so your running Versailles never counts. Each check prints the value and the budget (`PASS  host private memory  41 MB (budget 55); key helper 2 MB`); over the budget is a FAIL. `-- --no-perf` skips the phase, `-- --perf-only` runs only it (about a minute).

The budgets are in [`tests/e2e/budget.json`](tests/e2e/budget.json): `{ "hostPrivateMB", "webviewTotalMB", "idleCpuPercent", "startupMs" }`, numbers only. They are today's values on the development laptop (0.11.1, 16 GB: host 41–43 MB, WebView2 271–273 MB, idle CPU 4.5–6.2 %, start-up 0.9–1.2 s) plus about 25 % (start-up more: it varies most with load). To change one, run `npm run e2e -- --perf-only` two or three times, take the highest value, add about 25 % and say why in the commit. Raise a budget only for a change that is worth the cost; a FAIL after an unrelated change is a regression to find (`npm run measure:memory` prints the split per window kind). The idle CPU is mostly the host polling the sensors the test wallpaper declares (all of them); the plain starter page idles at about 1 %.

`npm run measure:memory` starts the same isolated app three times (wallpaper only, plus a docked bar, plus an open overlay) and prints the private memory of the host and of each WebView2 process type. Keep `remote.urls` in `capabilities/default.json` to one pattern: Tauri compiles one URL matcher (~76 KB) per remote URL for every allowed command at startup, so 22 URLs cost the host ~360 MB.

## PR

Branch from `main`, open a pull request against `main`. Title the work after the change, not after the product name.

## Release

1. Bump the version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`.
2. If the SDK changed, bump its first line (`/*! versailles-sdk x.y.z */`) and `SDK_VERSION`, and add a line to `sdk::SHIPPED` (the `sdk` test prints the hash). `npm run verify:sdk` passes.
3. Add the release to [`CHANGELOG.md`](CHANGELOG.md), newest first.
4. Run the e2e with the taskbar phase: `npm run build:test`, then `npm run e2e -- --take-taskbar` (or `scripts\sandbox-e2e.cmd`). Every check passes, including the performance budget (find the cause of a FAIL there before raising a budget; see [The performance phase](#the-performance-phase)), and the taskbar phase is not skipped.
5. Build the release with the updater key (see [Updates](#updates)): `set TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\versailles-updater.key`, `set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=…` and `npm run tauri:build`. `src-tauri\target\release\bundle\nsis\` must hold `Versailles_x.y.z_x64-setup.exe` **and** `Versailles_x.y.z_x64-setup.exe.sig` (the build prints a warning when it makes no `.sig`). Releases are not Authenticode-signed (see [Signing](#signing)).
6. **1.0.0** (and any later release that changes the installer, updates, first run or the taskbar): install that build on a clean machine and work through [`docs/RELEASE-CHECKLIST-1.0.md`](docs/RELEASE-CHECKLIST-1.0.md) (20–30 minutes), running [`scripts/clean-machine-check.ps1`](scripts/clean-machine-check.ps1) where it says so. It only reads and prints `PASS` / `FAIL` lines; every section that blocks a release must pass. Its tray-update section runs against the published release, right after step 8.
7. `node scripts/latest-json.mjs` writes `latest.json` next to the installer: the version, the notes from this release's section in `CHANGELOG.md`, the download URL under tag `vx.y.z`, and the signature.
8. One release commit (`Ship x.y.z: …`, with `closes #n` for each issue), tag `vx.y.z`, and a GitHub release (not a pre-release: `releases/latest` skips those) with the installer, its `.sig` and `latest.json`. Installed copies offer the update within a day or at their next start.

## Updates

Versailles checks `https://github.com/crasyK/versailles/releases/latest/download/latest.json` 60 s after start and then once a day (off with `"updates": { "check": false }` in `#versailles`), and offers **Update to x.y.z** at the top of the tray menu. Nothing downloads before a click. A click downloads the installer, checks its signature against the public key built into the app, gives back the taskbar, the tray and the docked bars' screen space as Quit does, and runs the installer in passive mode (a progress bar, no questions), which starts the new version. `tauri-plugin-updater` is registered from Rust only: no page gets an updater permission.

**Key pair (once).** `npx tauri signer generate -w %USERPROFILE%\.tauri\versailles-updater.key` (give it a password). Put the public key (one base64 line, also in `versailles-updater.key.pub`) into `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`, replacing the `REPLACE_WITH_UPDATER_PUBLIC_KEY …` placeholder, and commit it. Keep the private key and its password out of the repo, in a password manager with a backup: with a lost key, installed copies can never verify an update again and their users must reinstall by hand. While the placeholder is there, builds never check for updates and `scripts/run-tauri.mjs` turns `createUpdaterArtifacts` off (it says so), so builds without the key keep working.

**Trying an update locally** (no GitHub involved). Test builds (`--profile release-test`, which merges `src-tauri/tauri.test.conf.json`) accept a plain-http endpoint from `VERSAILLES_UPDATE_URL` and then check after 5 s instead of 60 s; release builds ignore the variable.

1. A throwaway key pair, with a password (an empty one cannot be passed: Windows drops an empty environment variable and the build then waits on a hidden password prompt): `npx tauri signer generate -w %TEMP%\vu\dev.key --ci -p test`. A config with its public key, `%TEMP%\vu\old.json`: `{ "plugins": { "updater": { "pubkey": "<contents of dev.key.pub>" } } }`, and `%TEMP%\vu\new.json`: the same plus `"version": "0.11.1"` (any version above the old one).
2. `set TAURI_SIGNING_PRIVATE_KEY_PATH=%TEMP%\vu\dev.key` and `set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=test`. (The Tauri CLI itself reads only `TAURI_SIGNING_PRIVATE_KEY`, the key or its path; `scripts/run-tauri.mjs` passes `TAURI_SIGNING_PRIVATE_KEY_PATH` on as that.) Run the builds through `npm run` or with `node_modules\.bin` on the `PATH`: the script starts `tauri` by name.
3. Old build: `node scripts/run-tauri.mjs build --config %TEMP%\vu\old.json -- --profile release-test`. Install its `Versailles_<version>_x64-setup.exe` from `bundle\nsis\` under the target folder.
4. New build: the same with `new.json`. Copy its `Versailles_0.11.1_x64-setup.exe` and `.exe.sig` into `%TEMP%\vu\www`, then `node scripts/latest-json.mjs --installer %TEMP%\vu\www\Versailles_0.11.1_x64-setup.exe --version 0.11.1 --url-base http://127.0.0.1:8000/ --out %TEMP%\vu\www\latest.json`.
5. In `%TEMP%\vu\www`: `python -m http.server 8000`.
6. Quit Versailles. In a console: `set VERSAILLES_UPDATE_URL=http://127.0.0.1:8000/latest.json`, then start the installed `versailles.exe` from it (default `%LOCALAPPDATA%\Versailles\versailles.exe`). After about 5 s the tray menu shows **Update to 0.11.1**. Click it and check: with `"hideTaskbar": true` the Windows taskbar comes back and docked bars give back their space before the installer's progress bar; Versailles starts again as 0.11.1 (`Versailles.log`: `updates: up to date` after its own check) and hides the taskbar again.
7. Failure path: change one byte of the served installer and repeat from the old build. The log shows a signature error, the tray item is offered again, and the taskbar, tray and bars stay as they were.

## Signing

Versailles is not code-signed and will not be (no certificate; #40 and #53 are closed as not planned). Windows SmartScreen therefore asks on first install (**More info** → **Run anyway**), and on a fresh Windows with Smart App Control on, unsigned apps cannot run at all (Getting started says so). Updates are still protected: the tray updater only installs an installer whose minisign signature matches the public key built into the app ([Updates](#updates)). `scripts/run-tauri.mjs` and `src-tauri/tauri.release.conf.json` keep the old Authenticode wiring (`VERSAILLES_SIGN=1`), unused.
