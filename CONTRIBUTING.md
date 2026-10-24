# Contributing

Versailles is the HTML desktop runtime. User tiles live under `Documents\Widgets`, not in this repository.

## Rules

1. Paper is the look of the host's own chrome and the starter template ([`design/PAPER-THEME.md`](design/PAPER-THEME.md), [`.cursor/rules/paper-theme.mdc`](.cursor/rules/paper-theme.mdc)). User pages can look like anything; the host never styles them.
2. Single-file dialect. Read [`docs/DIALECT.md`](docs/DIALECT.md). Do not iframe widgets.
3. Keep `scripts/verify-acl.mjs` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together (or a capability file of its own for a host window, listed in `tauri.conf.json`, like `welcome.json`).
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
5. Build the release, signed (see [Signing](#signing)) and with the updater key (see [Updates](#updates)): log in to SimplySign Desktop, then `set VERSAILLES_SIGN=1`, `set TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\versailles-updater.key`, `set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=…` and `npm run tauri:build`. `src-tauri\target\release\bundle\nsis\` must hold `Versailles_x.y.z_x64-setup.exe` **and** `Versailles_x.y.z_x64-setup.exe.sig` (the build prints a warning when it makes no `.sig`). Check the signatures (`signtool verify /pa /v` on `src-tauri\target\release\versailles.exe` and on the installer), install it, and check the uninstaller too (`%LOCALAPPDATA%\Versailles\uninstall.exe`).
6. `node scripts/latest-json.mjs` writes `latest.json` next to the installer: the version, the notes from this release's section in `CHANGELOG.md`, the download URL under tag `vx.y.z`, and the signature.
7. One release commit (`Ship x.y.z: …`, with `closes #n` for each issue), tag `vx.y.z`, and a GitHub release (not a pre-release: `releases/latest` skips those) with the installer, its `.sig` and `latest.json`. Installed copies offer the update within a day or at their next start.

## Updates

Versailles checks `https://github.com/crasyK/versailles/releases/latest/download/latest.json` 60 s after start and then once a day (off with `"updates": { "check": false }` in `#versailles`), and offers **Update to x.y.z** at the top of the tray menu. Nothing downloads before a click. A click downloads the installer, checks its signature against the public key built into the app, gives back the taskbar, the tray and the docked bars' screen space as Quit does, and runs the installer in passive mode (a progress bar, no questions), which starts the new version. `tauri-plugin-updater` is registered from Rust only: no page gets an updater permission.

**Key pair (once).** `npx tauri signer generate -w %USERPROFILE%\.tauri\versailles-updater.key` (give it a password). Put the public key (one base64 line, also in `versailles-updater.key.pub`) into `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`, replacing the `REPLACE_WITH_UPDATER_PUBLIC_KEY …` placeholder, and commit it. Keep the private key and its password out of the repo, in a password manager with a backup: with a lost key, installed copies can never verify an update again and their users must reinstall by hand. While the placeholder is there, builds never check for updates and `scripts/run-tauri.mjs` turns `createUpdaterArtifacts` off (it says so), so builds without the key keep working.

**Trying an update locally** (no GitHub involved). Test builds (`--profile release-test`, which merges `src-tauri/tauri.test.conf.json`) accept a plain-http endpoint from `VERSAILLES_UPDATE_URL` and then check after 5 s instead of 60 s; release builds ignore the variable.

1. A throwaway key pair: `npx tauri signer generate -w %TEMP%\vu\dev.key --ci -p ""`. A config with its public key, `%TEMP%\vu\old.json`: `{ "plugins": { "updater": { "pubkey": "<contents of dev.key.pub>" } } }`, and `%TEMP%\vu\new.json`: the same plus `"version": "0.11.1"` (any version above the old one).
2. `set TAURI_SIGNING_PRIVATE_KEY_PATH=%TEMP%\vu\dev.key` and `set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=`.
3. Old build: `node scripts/run-tauri.mjs build --config %TEMP%\vu\old.json -- --profile release-test`. Install its `Versailles_<version>_x64-setup.exe` from `bundle\nsis\` under the target folder.
4. New build: the same with `new.json`. Copy its `Versailles_0.11.1_x64-setup.exe` and `.exe.sig` into `%TEMP%\vu\www`, then `node scripts/latest-json.mjs --installer %TEMP%\vu\www\Versailles_0.11.1_x64-setup.exe --version 0.11.1 --url-base http://127.0.0.1:8000/ --out %TEMP%\vu\www\latest.json`.
5. In `%TEMP%\vu\www`: `python -m http.server 8000`.
6. Quit Versailles. In a console: `set VERSAILLES_UPDATE_URL=http://127.0.0.1:8000/latest.json`, then start the installed `versailles.exe` from it (default `%LOCALAPPDATA%\Versailles\versailles.exe`). After about 5 s the tray menu shows **Update to 0.11.1**. Click it and check: with `"hideTaskbar": true` the Windows taskbar comes back and docked bars give back their space before the installer's progress bar; Versailles starts again as 0.11.1 (`Versailles.log`: `updates: up to date` after its own check) and hides the taskbar again.
7. Failure path: change one byte of the served installer and repeat from the old build. The log shows a signature error, the tray item is offered again, and the taskbar, tray and bars stay as they were.

## Signing

Releases are Authenticode-signed so SmartScreen and antivirus products see a known publisher; test builds (`npm run build:test`) are never signed, and a release build is signed only with `VERSAILLES_SIGN=1`, so building never needs the certificate. `scripts/run-tauri.mjs` then merges [`src-tauri/tauri.release.conf.json`](src-tauri/tauri.release.conf.json): the certificate's SHA-1 thumbprint, SHA-256 digests and Certum's timestamp server (`http://time.certum.pl`), with which Tauri signs `versailles.exe`, the NSIS plugin DLL, the uninstaller and the installer. It refuses to build while the thumbprint is still the placeholder.

**The certificate (once a year).** Certum's *Open Source Code Signing* certificate in the cloud variant (SimplySign; about €49 a year, issued to you as an individual; at most 459 days from February 2026): order it at certum.eu, verify your identity as Certum asks, activate it in the SimplySign mobile app, and install **SimplySign Desktop** (and Certum's root certificates if it asks). Azure Artifact Signing takes individuals only in the US and Canada; SignPath Foundation needs the whole release on GitHub-hosted CI; an EV certificate no longer skips SmartScreen's reputation check. See #40.

**Each signing session.** Start SimplySign Desktop and log in with the one-time code from the app; the certificate then shows up in your personal certificate store (`certmgr.msc` → Personal → Certificates). Its thumbprint (certificate → Details → Thumbprint, without spaces) goes into `tauri.release.conf.json` → `bundle.windows.certificateThumbprint` once per certificate; commit it (a thumbprint is not a secret). `signtool` comes with the Windows SDK; Tauri finds it.

**Checks** (release step 5): `signtool verify /pa /v <file>` must say *Successfully verified* with the timestamp for `versailles.exe`, the installer and, after installing, `uninstall.exe`. Before the first signed release, also try on the installed build: a crash restart through the supervisor (end `versailles.exe`'s app process in Task Manager), the taskbar guard (with `"hideTaskbar": true`, end the whole tree: the taskbar comes back), and `versailles.exe --restore-taskbar`.

**Order.** Tauri signs the installer before it writes the updater `.sig`, so the `.sig` covers the signed installer; re-signing an installer afterwards invalidates its `.sig` (sign again with `npx tauri signer sign` if you ever must).

**Later: package identity (#53).** A signed sparse package (an AppxManifest with `AllowExternalContent`, `MakeAppx pack /nv`, signed with this certificate, its `Publisher` equal to the certificate subject; registered by the NSIS installer with `Add-AppxPackage -ExternalLocation`) would give Versailles package identity: pushed notification changes instead of polling, `ClearNotifications`, maybe the Copilot key picker. It waits for the certificate; never ship a self-signed one for it.
