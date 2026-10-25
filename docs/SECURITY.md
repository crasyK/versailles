# Security

What Versailles protects, from whom, how, and what it leaves to you. Written for the 1.0 review (#51, October 2026); keep it current when a boundary changes.

Versailles is an unpackaged, unelevated, unsigned (no Authenticode) Tauri v2 app for one Windows user. It renders **your** HTML page as the desktop, bars and overlays, and gives that page system features through hooks. The page is code you chose to run. The design goal is that a page only gets the hooks it declares, that nothing outside the page (web sites, other accounts, other apps' data) can use Versailles to get more than it already has, and that Versailles never leaves the machine in a worse state than it found it.

## Assets

| Asset | Where |
| ----- | ----- |
| Running code as the user | `cli_exec`, `cli_exec_term`, `pty_*` (exec class); `cli_open` (opens files, URLs and apps) |
| System settings | `quick_set` (control class: radios, audio, display, power mode, Wi-Fi, Bluetooth, DND, notifications, keyboard, VPN, hotspot, sharing, accessibility) |
| Keystrokes | The low-level keyboard hook in `versailles --keys-hook`; `data-hotkey` bindings |
| What the user sees and does | Window titles, the focused app, tray tooltips, notifications, clipboard, media, recent files, file search results |
| Secrets | The localhost API token (`.versailles/config.json`), Wi-Fi passwords a page passes to `wifi` (never stored by Versailles), saved VPN credentials (never read), the hotspot passphrase (never read) |
| The user's files | `Documents\Widgets` (served over localhost), everything the user can read (search, recent files) |
| The shell | Explorer's taskbar (hidden while Versailles runs), the tray (`Shell_TrayWnd` of our own), AppBar space |
| Updates | The installer Versailles downloads and runs |

## Who can talk to Versailles

```
 your page (+ the scripts it loads) ──IPC──▶ host (Rust) ──▶ Windows
   │  wallpaper `desktop*`, spawn windows `widget-<id>`,         ▲
   │  folder widgets `widget-<folder>`                            │
   └── fetch ─▶ http://127.0.0.1:47831  ◀── any local process / account, browsers
                                                                  │
 other apps ── WM_COPYDATA (Shell_NotifyIcon, SHAppBarMessage) ──▶ our Shell_TrayWnd
 GitHub release ── latest.json + installer (minisign) ──▶ updater
 Windows Search index / Recent folder / window titles ──▶ data pages render
```

| Attacker | Can | Versailles' answer |
| -------- | --- | ------------------ |
| **A1 Your page and every script it loads** (CDN libraries, embeds, folder widgets you copied in) | Run JavaScript in a Versailles window | Trusted with the hooks the piece declares, nothing more. Identity comes from the window label, not from arguments. See [What is not defended](#what-versailles-does-not-defend). |
| **A2 A web page in a normal browser** | Send requests to `127.0.0.1:47831`, navigate or iframe it, DNS-rebind a host name to 127.0.0.1 | No IPC (browsers have none). The HTTP gate refuses foreign `Host` and `Origin`, and cross-site requests that are not a navigation to `/files/*`. The control API needs the token. |
| **A3 Another process of the same user** | Anything the user can do: read `.versailles/config.json`, inject into Versailles, edit the page | Out of scope: same user, same integrity level. Versailles adds no privilege it could take. |
| **A4 Another account on the same PC** (fast user switching, a service account) | Connect to 127.0.0.1 (loopback is shared between sessions), bind ports | Cannot read `Documents\Widgets` or the token (profile ACLs). Can read what `/files` serves and use the unauthenticated helper routes; see [Known limitations](#known-limitations). |
| **A5 A lower-integrity or sandboxed process** (browser renderer, AppContainer) | Little: UIPI blocks its window messages to our windows, AppContainers have no loopback | Nothing extra is opened for it (no `ChangeWindowMessageFilter`). |
| **A6 Another app on the desktop** | Put any text in its window title, tray tooltip, notification, file names; send `WM_COPYDATA` to the tray window | Text is handed to pages as JSON data, never as HTML. Tray and AppBar messages are parsed with bounds checks. |
| **A7 A network attacker or a compromised release channel** | Serve a different `latest.json` / installer | The updater installs only what verifies against the minisign public key in `tauri.conf.json`, over HTTPS. |

## What Versailles defends

### IPC: who may call what

- **Capabilities.** One capability (`capabilities/default.json`) covers the page windows (`desktop`, `desktop-*`, `widget-*`) and host chrome (`launcher-dim`, `guides`), for the app origin and for `http://127.0.0.1|localhost` on ports 47831–47841. The first-run window has its own (`welcome.json`: two commands). Pages cannot create windows or webviews, and cannot register global shortcuts; `scripts/verify-acl.mjs` fails the build if a capability grants them. The updater has no capability: pages cannot call it.
- **Identity is the calling window's label** (`page.rs` `classify_invoker`, `commands.rs` `require_hook`), never an argument. A spawn window acts as its piece; the wallpaper must name a piece (`versailles.for(id)`) and may only act as a piece on the page; a folder widget gets its `widget.json` permissions; host windows get no hooks. The piece must list the hook in `data-hooks`.
- **Classes.** Exec class (`cli_exec*`, `cli_search*`, `cli_recent_files`, `pty_*`) and control class (`quick_set`) are refused from the wallpaper and from folder widgets; only a spawn window whose piece declares the hook may use them. The SDK also waits for a click or key press before a control call (a convenience, not a host check).
- **Pushed data follows the hook.** Hook data is delivered by script only into windows of pieces that declared the hook (`push.rs`, `bus.rs`). The Tauri events that carry data do the same: `clipboard://changed` and `media://update` go to the hook's windows, `pty://data` / `pty://exit` to the windows of the piece that opened the terminal.
- **Layout** only moves or resizes the caller's own window; `windows-control` never acts on Versailles' own windows.

### The localhost server (`api.rs`, `protocol.rs`)

- Binds `127.0.0.1` only (47831, or the next free port up to 47841).
- Every request: `Host` must be `127.0.0.1:<port>` / `localhost:<port>` (DNS rebinding); a present `Origin` must be the server or the app origin; without `Origin`, `Sec-Fetch-Site: cross-site|same-site` is refused unless it is a navigation to `/files/*`. CORS reflects only allowlisted origins.
- The control routes (`/widgets*`, `/layouts/*`, `/launcher/show`, `/media/*`) need `Authorization: Bearer <token>`: a random UUIDv4 kept in `.versailles/config.json`, compared in constant time; an empty token never matches.
- `/files/*` serves only `desktop/**`, `.sdk/**`, the configured page and registered folder widgets, never `.versailles/`. The path is decoded once (by axum); `:` (drive letters, alternate data streams), NUL, rooted, UNC and `.`/`..` components are refused before any file system call; the canonical path (symlinks, junctions, 8.3 names resolved) is checked against the same scope again.

### Opening things (`cli_open`, `apps.rs`)

Refuses UNC paths and `file://host/…` before touching the file system (a `stat` alone would send the user's NTLM hash), and URL handlers that turn a link into code execution (`ms-msdt:`, `search-ms:`, `search:`, `ms-officecmd:`, Office `ms-word:` … and their `|u|` / `ofe|` forms, `ms-appinstaller:`, `ms-cxh*`). `shell:AppsFolder\<id>` must be an app in Windows' Applications folder. Anything else must be an `http(s)` / `ms-` / `shell:` URL, an existing absolute path or an installed app.

### Search and recent files (`search.rs`, `recent.rs`)

Windows Search SQL is built from escaped literals: `'` doubled, `LIKE` wildcards bracketed, `CONTAINS` terms reduced to letters and digits. Results outside local drives are dropped. Recent shortcuts are read without `IShellLink::Resolve`; targets on the network are skipped before any `stat`.

### Tray and AppBar messages (`tray.rs`)

Our `Shell_TrayWnd` parses `SHELLTRAYDATA` / `WINNOTIFYICONIDENTIFIER` / `APPBARMSGDATAV3` with bounds-checked reads; strings are length-limited. Icon handles are only read (`GetIconInfo`, size clamped to 16–64 px). AppBar shared memory is opened with `SHLockShared` for the handle and process the message names and only 40 bytes are copied. Clicks reach an app only through `tray_click` (the `tray` hook), as the callback message the app registered. Every message is also forwarded to Explorer, so nothing is lost when Versailles stops.

### Keyboard (`keys.rs`)

The hook lives in a helper process (`versailles --keys-hook`) started by the app with anonymous pipes for stdin/stdout; no named pipe or port exists for other processes to feed it bindings. It exits when its stdin closes. Logs name only Win, Copilot, modifier and function keys that arrived late (other keys only as a count). `VERSAILLES_KEYS_DEBUG=1` logs **every** key to `keys.log` (a deliberate opt-in for debugging; that file is not part of the diagnostics zip).

### Updates (`updater.rs`)

`tauri-plugin-updater` checks `https://github.com/crasyK/versailles/releases/latest/download/latest.json` and installs only a package whose minisign signature matches the public key compiled in from `tauri.conf.json`. No public key, no updates. A test endpoint (`VERSAILLES_UPDATE_URL`) is honoured only in debug builds and in test builds whose config sets `dangerousInsecureTransportProtocol`; shipping builds never have that flag (a unit test checks the config). Before the installer starts, Versailles only releases the tray, the taskbar and docks.

### Logs and diagnostics

Wi-Fi passwords are logged as `<set>`; VPN credentials and the hotspot passphrase are never read into Versailles. Search queries, clipboard text, file names and terminal output are not logged. The diagnostics zip replaces the profile path with `%USERPROFILE%` and redacts every config key that looks like a secret (`token`, `secret`, `password`, `key`, `auth`); it is written locally and never sent.

## What Versailles does not defend

- **Your page is trusted with what you give it.** A page you install, and every script it loads from a CDN, runs with the hooks its pieces declare. A piece with `shell` and `pty` can run any command as you. Pin third-party scripts (`integrity=`), or vendor them into `desktop/`.
- **All pieces on the page are one document.** The wallpaper may act as any `article.widget` or `template.spawnable` on the page (`versailles.for(id)`), including their storage. A spawnable that runs commands from data it is handed (`versailles.spawn(id, data)`) extends that power to every piece with `spawn`.
- **Same user, same integrity level.** Another program running as you can already do everything Versailles can.
- **Rendering untrusted text is the page's job.** Window titles, tray tooltips, notifications, file names and search results are other people's text. The host hands them over as JSON; render them with `textContent`, not `innerHTML`.
- **Windows itself**: an elevated window ignores Versailles (UIPI); Win+L and Ctrl+Alt+Del cannot be bound.

## Known limitations

| # | Severity | Area | What | Status |
| - | -------- | ---- | ---- | ------ |
| L1 | medium | Folder widgets | Folder widgets (`widget.json`) are served from the same origin as the desktop page (`http://127.0.0.1:<port>/files/…`). One can read the page and other folders over `fetch`, share `localStorage`, and call the host commands that need no hook (`get_api_info` with the API token, `get_desktop_html`, `list_catalog`, `open_widget`, `toggle_launcher`, `patch_engine_runtime`, …). It cannot use hooks it did not declare, and never exec or control class. Treat a folder widget like a script on your page. | documented |
| L2 | low | Localhost server | Loopback is shared by every account on the PC. Without a token, another account can read what `/files` serves (your page, `.sdk`, folder widgets), call `/weather` and `/quote`, and change the saved weather location with `/weather?save=1`. | accepted |
| L3 | low | Capabilities | The IPC capability covers ports 47831–47841, not only the bound one. If another account holds a port in that range and a Versailles window is navigated there (a hard-coded link while Versailles fell back to another port), that page gets IPC with the window's label. | documented |
| L4 | info | `cli_open` | Plain class: the wallpaper may call it. Opening an existing local path runs it by file association (`.exe`, `.bat`, `.vbs`, `.lnk`, …), and `ms-` / `shell:` handlers other than the denied ones open. This is what a launcher tile does; Windows' own Mark-of-the-Web prompts still apply. | accepted |
| L5 | info | Plain class | `power` (shut down, sign out), `windows-control` (close other apps' windows) and `tray_click` work from the wallpaper and without a user gesture. | accepted |
| L6 | info | Events and windows | `core:event:default` lets any page emit any event to any window (the host needs `overlay://staged` from pages), and the window commands accept any label: a page can close, hide or move other Versailles windows and fake UI events (for example text in the bar's terminal view). No host action trusts an event payload beyond reveal timing. | accepted |
| L7 | info | `notify-control` | `NotifyOpen` opens `shell:AppsFolder\<aumid>` for any well-formed id, including `{KNOWNFOLDERID}\path.exe` forms, without the Applications-folder check `cli_open` does. Control class, spawn windows only, and `cli_open` already opens local paths. | documented |
| L8 | info | Unsigned builds | Releases carry no Authenticode signature (owner's decision): SmartScreen warns on first install, and a user cannot tell a genuine first download from a fake one by signature. Updates after that are protected by minisign. | accepted |
| L9 | info | Mapped drives | Recent files and search skip UNC paths but still `stat` targets on mapped network drive letters. | accepted |
| L10 | info | API token | `get_api_info` returns the token to every page window, and `.versailles/config.json` syncs wherever `Documents` syncs (OneDrive folder backup). The token only reaches the control API, which can do less than the page's own IPC. | accepted |

## Review log (1.0, October 2026)

| Severity | Area | Where | Scenario | Status |
| -------- | ---- | ----- | -------- | ------ |
| high | IPC classes | `capabilities/default.json` (`core:webview:allow-create-webview-window`, `core:window:allow-create`) | The wallpaper, a folder widget or a CDN script creates a webview labelled `widget-<id of a pty/shell spawnable>` on a `/files` URL; `classify_invoker` takes it for that spawn window and allows exec class. | fixed |
| medium | Keyboard | `capabilities/default.json` (`global-shortcut:allow-register`) | Any page registers plain letter keys as global shortcuts and sees (and swallows) typing in other apps, bypassing the `hotkey` hook. | fixed |
| medium | Terminal | `pty.rs` (`pty://data` was `app.emit`) | Any window listens for `pty://data` and reads terminal output. Now sent only to the opening piece's windows. | fixed |
| low | Media | `media.rs` (`media://update` was `app.emit`) | A piece without `media` listens for now-playing data. Now sent only to `media` windows. | fixed |
| low | API token | `api.rs` `bearer_matches` | Non-constant-time comparison; a hand-edited empty `apiToken` made the expected header `Bearer ` (only HTTP whitespace trimming kept it closed). | fixed |
| medium | Folder widgets | `api.rs` `current_file_scope`, `commands.rs` ungated commands | L1 | documented |
| low | Localhost server | `api.rs` `start_api_server` | L2 | accepted |
| low | Capabilities | `capabilities/default.json` `remote.urls` | L3 | documented |
| info | `cli_open`, plain class, events, notify-control, signing, mapped drives, token | `cli.rs` `cli_open`, `toasts.rs` `open_app`, … | L4–L10 | accepted / documented |
| info | File server | `protocol.rs` `sanitize_request_path`, `resolve_in_root` | `..`, `%2e%2e`, `%5c`, ADS, 8.3 names, symlinks: all refused (unit tests, and probes against a running 0.11.1). | ok |
| info | Search / recent | `search.rs`, `recent.rs` | SQL quoting, LIKE escaping, UNC skipped (unit tests). | ok |
| info | Tray | `tray.rs` `parse`, `forward_appbar` | Bounds-checked parsing, clamped icon sizes, no message filter opened for low integrity. | ok |
| info | Keys helper | `keys.rs` | Anonymous pipes; logs only nameable keys; debug keylog is opt-in. | ok |
| info | Updater | `updater.rs` | Signature required; test endpoint only in debug/test builds. | ok |
| info | Logs / diagnostics | `quick.rs` `describe`, `diagnostics.rs` `is_secret_key` | No passwords or credentials logged; secrets redacted. | ok |

## Follow-ups

- Give each folder widget its own origin (a per-folder host name or port), or gate the hook-free host commands on the invoker kind (host chrome and page pieces only) (L1).
- Add the IPC capability at runtime for the bound port only (`AppHandle::add_capability`) instead of the static port range (L3).
- Run `NotifyOpen` ids through the same Applications-folder check as `cli_open` once it is clear no notifying app is missing from that folder (L7).
- Consider a per-session secret for `/files` (cookie set on the window's first navigation) so other accounts cannot read the page (L2).

## Reporting

Open a private security advisory on GitHub (`crasyK/versailles` → Security → Report a vulnerability), or an issue without details asking for a contact.
