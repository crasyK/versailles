# Release checklist 1.0: a clean machine

The last step before tagging 1.0.0 ([#52](https://github.com/crasyK/versailles/issues/52)): install the release candidate on a Windows that has never seen Versailles and walk through what a new user does. The core run takes 20–30 minutes; each extra combination of language, scaling and monitors about 5 more. Tick the boxes as you go and paste the result table into #52.

The development e2e (`npm run e2e -- --take-taskbar`, [CONTRIBUTING.md](../CONTRIBUTING.md#dev)) runs on your own machine first. This checklist covers what it cannot: a fresh user profile, the real installer, SmartScreen, the first-run window, the upgrade and the uninstaller. (A VM with Node and a copy of the repo and the build can run the e2e on a clean Windows too; optional.)

[`scripts/clean-machine-check.ps1`](../scripts/clean-machine-check.ps1) checks what can be checked automatically. It only reads (installed version, processes, the taskbar's state, `RunOnce` / `Run`, the SDK versions, the Widgets folder, log errors since start, WebView2) and prints `PASS` / `FAIL` / `WARN` / `INFO` lines; it needs only Windows PowerShell. Run it where the list says **script**.

## Contents

- [Pick the machine](#pick-the-machine) · [Before you start](#before-you-start)
- [A. Install and first run](#a-install-and-first-run) · [B. The starter's examples](#b-the-starters-examples) · [C. Hiding the taskbar](#c-hiding-the-taskbar) · [D. Tray](#d-tray) · [E. Win and Copilot keys](#e-win-and-copilot-keys)
- [F. Upgrade from 0.9.x with a customised page](#f-upgrade-from-09x-with-a-customised-page) · [G. Update from the tray](#g-update-from-the-tray) · [H. Uninstall](#h-uninstall)
- [I. Languages, scaling, monitors](#i-languages-scaling-monitors) · [Results](#results)

## Pick the machine

Windows Home has no Windows Sandbox and no Hyper-V. In order of how much they cover:

| Option | Covers | Does not cover |
| ------ | ------ | -------------- |
| **Another PC** (a spare laptop, ideally a convertible) | Everything: real Wi-Fi, Bluetooth, battery, audio devices, touch and posture, the Copilot key, a second monitor | Only what that PC lacks |
| **A VM** (VirtualBox 7 or VMware Workstation, both free for personal use; Windows 11 from Microsoft's ISO, unactivated is fine for testing) | A clean Windows install: no WebView2 data, no Explorer tweaks, SmartScreen and Smart App Control as a new user sees them, a German ISO for the German run, two virtual monitors, any scaling | Wi-Fi networks, Bluetooth, battery, brightness, posture, the Copilot key (those show their "not available" states, which is worth seeing too) |
| **A new local user account** on your PC (Settings → Accounts → Other users → Add account → *I don't have this person's sign-in information* → *Add a user without a Microsoft account*) | A fresh profile: empty `Documents\Widgets`, fresh `HKCU` (Run, RunOnce, taskbar settings), first run, a per-user install, real hardware | A clean Windows: the same build, drivers and WebView2 as your own account, and tools you installed machine-wide. Sign out of your own account first (do not just switch users), so your own Versailles is not running in another session |
| Windows Sandbox (Pro / Enterprise / Education only) | A throwaway Windows, `scripts\sandbox-e2e.cmd` | Not available on Home |

Recommended: a VM for the core run and the upgrade (it is the only truly clean one, and a snapshot lets you repeat), plus the core run's sections B–E once on real hardware (another PC, or a new local user).

## Before you start

- [ ] The release candidate's `Versailles_1.0.0_x64-setup.exe` and `.sig`, built as in [CONTRIBUTING.md › Release](../CONTRIBUTING.md#release) (the release build, not `build:test`).
- [ ] The previous release's installer (0.12.0) for [G](#g-update-from-the-tray), and a 0.9.x installer (0.9.1) for [F](#f-upgrade-from-09x-with-a-customised-page), from [Releases](https://github.com/crasyK/versailles/releases).
- [ ] `clean-machine-check.ps1` copied to the test machine (from the repo, or `https://raw.githubusercontent.com/crasyK/versailles/main/scripts/clean-machine-check.ps1`). Run it as the test user, not elevated:
  ```
  powershell -NoProfile -ExecutionPolicy Bypass -File clean-machine-check.ps1 -Expect 1.0.0
  ```
- [ ] A VM: take a snapshot named *clean* now, so F and G can start from it.
- [ ] Note Windows' version (`winver`) and whether Smart App Control is on (Settings → Privacy & security → Windows Security → App & browser control); the script prints both.

## A. Install and first run

1. [ ] Download the installer with Edge (so it carries the mark of the web, as for a user). Note what Edge says; **Keep anyway** if asked.
2. [ ] Run it. SmartScreen shows *Windows protected your PC*: **More info** shows *Unknown publisher* and the file name; **Run anyway** installs. If Smart App Control is on, it blocks the installer with no way past: note it (expected for an unsigned app; [GETTING-STARTED.md](GETTING-STARTED.md#install) tells users), then turn it off to continue.
3. [ ] The installer asks for no administrator rights, installs to `%LOCALAPPDATA%\Versailles`, and starts Versailles from its last page.
4. [ ] The **Welcome to Versailles** window appears. Click **Set up the example desktop**: the wallpaper becomes the starter (clock, note), the tray icon appears.
5. [ ] **Alt+Space** opens the command bar at the top centre. Type `help`, `start` (apps with icons, Store apps such as Settings and Calculator among them), `bluetooth` + Enter (opens Settings → Bluetooth), `?? test` (search runs), Escape closes it.
6. [ ] The note widget: type a line, restart Versailles (tray → Quit, then start it from Start), the line is still there.
7. [ ] Preview: `http://127.0.0.1:47831/files/desktop/index.html?m=widgets` in Edge shows the widgets.
8. [ ] **script**: all PASS (with no `hideTaskbar`, the taskbar must be visible and `RunOnce` empty).

## B. The starter's examples

Open `Documents\Widgets\desktop\index.html` in Notepad. For each example near the end of `<body>`, delete its two `try me` marker lines and save; Versailles reloads within a second.

1. [ ] **Top bar** (`try me (top bar)`): a bar along the top with time, network and battery; maximized windows stop below it. Its **hello** button opens a small overlay 24 px from the bottom-right corner; a click elsewhere closes it.
2. [ ] **Taskbar** (`try me (taskbar)`): a bar along the bottom of every monitor with a button per window on that monitor. Click focuses, clicking the focused one minimizes, middle-click closes. Resting on a button shows the preview card. With two keyboard layouts installed, the language pill shows the current one and a click switches it.
3. [ ] **Quick settings** (`try me (quick settings)`): **Ctrl+Alt+Q** opens the slide-out at the top right. Each control does what it says or shows why it cannot (a VM has no Wi-Fi or Bluetooth: the lists say so, nothing errors silently): Wi-Fi on / off and a scan, Bluetooth, volume, mute, microphone, output, an app in the mixer (play a video), brightness, night light, projection, Do Not Disturb, a notification (from Edge, Mail or any app) and **clear all**, power mode, VPN (none: says so), hotspot, Cast (opens Windows' panel), nearby sharing, magnifier on and off, sticky keys.
4. [ ] **script**: all PASS, no errors in the log since start.

## C. Hiding the taskbar

1. [ ] In `#versailles` add `"shell": { "hideTaskbar": true }` and save. Explorer's taskbar disappears on every monitor; the starter's taskbar takes the bottom strip; the tray icons (Versailles, OneDrive, …) show in it; clicking and right-clicking them works. Win opens Start; Win+A Quick Settings; Win+N notifications.
2. [ ] **script**: `taskbar hidden`, `restore file`, `runonce` and `taskbar guard` PASS.
3. [ ] Restart Explorer (Task Manager → *Windows Explorer* → **Restart**): the taskbar stays hidden, the tray icons come back in the starter's taskbar within a few seconds.
4. [ ] A crash: Task Manager → *Details*, right-click the `versailles.exe` the script lists as `app` → **End task**. The supervisor starts it again within a few seconds (the taskbar may show for a moment), and the taskbar is hidden again.
5. [ ] A kill: right-click the one listed as `supervisor` → **End process tree**. The taskbar guard (outside that tree) brings the Windows taskbar back within a second. Start Versailles again from Start: hidden again.
6. [ ] Sign out and in again while hidden: Versailles starts with Windows and hides the taskbar again; nothing flickers for long.
7. [ ] Remove the line and save: the taskbar is back at once. **script**: `taskbar visible`, `restore file`, `runonce` PASS.
8. [ ] With *Automatically hide the taskbar* on before step 1: still on after step 7.

## D. Tray

1. [ ] Left click on the Versailles icon opens the command bar.
2. [ ] The menu: **Hide desktop page** hides the wallpaper (the normal Windows wallpaper shows), **Show desktop page** brings it back.
3. [ ] **Copy diagnostics** writes `versailles-diagnostics-….zip` to Downloads and shows it in Explorer. Its `info.json` has the versions, monitors, pieces and no token (`"<redacted>"`), and paths say `%USERPROFILE%`.
4. [ ] **Quit**: every Versailles process ends (Task Manager), the taskbar is visible, the bars' space is given back.

## E. Win and Copilot keys

1. [ ] Add `data-hotkey="Alt+Space, Win"` to the command bar's template and save. A tap of Win opens the bar, not Start; Win+R, Win+E and Win+L still work; Ctrl+Esc opens Start.
2. [ ] On a keyboard with a Copilot key: add `Copilot` to the list (`"Alt+Space, Win, Copilot"`): the key opens the bar.
3. [ ] `"shell": { "suppress": ["Win+A"] }`: Win+A does nothing; remove it again.
4. [ ] Remove the `Win` binding: a tap of Win opens Start again.
5. [ ] `%LOCALAPPDATA%\com.versailles.widgets\logs\Versailles.log` has no `missed by hook` lines during this section.

## F. Upgrade from 0.9.x with a customised page

On the *clean* snapshot (or after [H](#h-uninstall) on a machine with nothing left over):

1. [ ] Install 0.9.1, set up the example desktop, then customise the page: add a widget of your own (the [first widget](GETTING-STARTED.md#your-first-widget)), uncomment the top bar, and set `"shell": { "hideTaskbar": true }`. Leave `.sdk\versailles.js` unedited.
2. [ ] **script** (without `-Expect`): note the `page` and `bar.js` hashes and the SDK version (0.9.1).
3. [ ] Run the 1.0.0 installer by hand (0.9.x has no in-app updates). It closes (or asks to close) the running Versailles, installs, and starts 1.0.0.
4. [ ] **script** `-Expect 1.0.0`: `page` and `bar.js` hashes unchanged; `sdk` PASS with `versailles.js.bak kept from the update`; `taskbar hidden` PASS (the taskbar state is kept).
5. [ ] The custom widget, the top bar and the note's text are all there; no second welcome window.
6. [ ] Variant (optional, 3 minutes): before step 3, add a comment line to `.sdk\versailles.js`. After the upgrade the edited copy is left alone (`sdk` reports it, and `Versailles.log` says so once).

## G. Update from the tray

The in-app update can be tried only against a published release. Before 1.0.0 is published, try it with test builds and a local server ([CONTRIBUTING.md › Trying an update locally](../CONTRIBUTING.md#updates)). Right after publishing 1.0.0 (not as a pre-release), do it for real; if it fails, unpublish and fix:

1. [ ] On the *clean* snapshot, install 0.12.0, set up the starter, hide the taskbar.
2. [ ] About a minute after start the tray menu shows **Update to 1.0.0** at the top. Nothing installs before the click.
3. [ ] Click it. The taskbar comes back, the bars give their space back, a progress bar runs, Versailles 1.0.0 starts and hides the taskbar again. No SmartScreen prompt.
4. [ ] **script** `-Expect 1.0.0`: all PASS; the log says `updates: up to date` after its own check.

## H. Uninstall

1. [ ] **Worst case first**: with Versailles running and the taskbar hidden, Settings → Apps → Installed apps → Versailles → **Uninstall**. Note what the uninstaller asks (closing the app, deleting app data).
2. [ ] The Windows taskbar is back. If it is not: note it as a release blocker, then restore it (Task Manager → Windows Explorer → Restart; turn off auto-hide if it was not yours).
3. [ ] **script** `-Phase uninstalled`: no uninstall entry, no `versailles.exe`, no processes, taskbar visible, no `taskbar-restore.json`, no `VersaillesRestoreTaskbar` in `RunOnce`, no `Run` entry, `Documents\Widgets` kept.
4. [ ] Sign out and in: no error dialog about a missing `versailles.exe`; the taskbar is normal.
5. [ ] **The usual way**: reinstall, tray → Quit, then uninstall. Same checks as 2–4.

## I. Languages, scaling, monitors

Repeat A.4–A.5, B.1–B.2, C.1 and C.7 in each combination you can set up. In a VM: German from a German ISO (or Settings → Time & language → Language & region → add Deutsch, set it as the Windows display language, sign out and in), scaling in Settings → System → Display → Scale, a second monitor in the VM's display settings (VirtualBox: *Display* → *Monitor Count* 2).

| | English | German |
| - | ------- | ------ |
| 100 %, one monitor | [ ] | [ ] |
| 150 %, one monitor | [ ] | [ ] |
| 100 % + 150 %, two monitors | [ ] | [ ] |

What to look at:

- [ ] Text is sharp at 150 % (bars, overlays, the preview card, the welcome window).
- [ ] Bars run the full edge of each monitor; the taskbar example has one copy per monitor, each listing its own windows; dragging a window to the other monitor moves its button.
- [ ] Overlays open on the monitor under the mouse; the command bar sits 120 px from the top there.
- [ ] Hiding the taskbar hides it on both monitors; restoring brings both back.
- [ ] Unplugging the second monitor (VM: monitor count 1) closes its bar copy; plugging it back opens it.
- [ ] German: the command bar finds Settings pages by German names (`anzeige`, `lautstärke`); the language pill shows `DE`; the tray menu and the welcome window are in English (expected).
- [ ] **script** in each combination: the monitors line shows the scale you set; all PASS.

## Results

Paste into #52:

```
Machine: VM (VirtualBox 7.x) / other PC (model) / new local user
Windows: 11 25H2 build 26200.xxxx, English + German
Smart App Control: off / evaluation / on (blocked the installer: yes/no)
Installer: 1.0.0, SmartScreen: More info → Run anyway OK

A install + first run      PASS/FAIL  notes
B starter examples         PASS/FAIL
C hiding the taskbar       PASS/FAIL
D tray                     PASS/FAIL
E Win / Copilot keys       PASS/FAIL
F upgrade from 0.9.1       PASS/FAIL
G update from the tray     PASS/FAIL (local test / real)
H uninstall                PASS/FAIL
I EN/DE x 100/150 x 1/2    PASS/FAIL

clean-machine-check.ps1 output (last run of each phase): attached
```

A FAIL in A, C, F, G or H blocks the release; elsewhere, decide case by case and open an issue.
