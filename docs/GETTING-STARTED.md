# Getting started

Versailles turns one HTML page into your Windows desktop: widgets on the wallpaper, bars docked to the screen edges, and overlays that a hotkey opens. This guide goes from the download to your own bar in about half an hour.

## Contents

- [What you need](#what-you-need) · [Install](#install) · [First run](#first-run) · [Your folder](#your-folder)
- [Editing live](#editing-live) · [Preview in a browser](#preview-in-a-browser)
- [Your first widget](#your-first-widget) · [Your first bar](#your-first-bar) · [Your first overlay with a hotkey](#your-first-overlay-with-a-hotkey)
- [Hiding the Windows taskbar](#hiding-the-windows-taskbar) · [Updating](#updating) · [Uninstalling](#uninstalling) · [When something goes wrong](#when-something-goes-wrong)

Next: the [cookbook](cookbook/README.md) (a top bar, a taskbar, quick settings, a launcher, …) and the reference docs, [DIALECT.md](DIALECT.md) (the HTML attributes) and [HOOKS.md](HOOKS.md) (what a page can ask Windows for).

## What you need

- Windows 11, 64-bit. The WebView2 runtime Versailles draws with is part of Windows 11.
- A text editor. Any will do; VS Code gives you autocomplete for the SDK (see [Editing live](#editing-live)).
- No administrator rights: Versailles installs for your user only.

## Install

1. Download `Versailles_x.y.z_x64-setup.exe` from the [latest release](https://github.com/crasyK/versailles/releases/latest). Take it from that page only.
2. Your browser may warn that the file is not commonly downloaded. In Edge: **…** next to the download → **Keep** → **Show more** → **Keep anyway**.
3. Run it. Windows SmartScreen shows **Windows protected your PC**, because Versailles is not code-signed (the project has no certificate and does not plan to buy one). Click **More info**: the app is `Versailles_x.y.z_x64-setup.exe`, the publisher *Unknown publisher*. Then click **Run anyway**.
4. The installer puts Versailles in `%LOCALAPPDATA%\Versailles` and adds it to Start and to Settings → Apps. Leave **Run Versailles** ticked on its last page (or start it from Start). Versailles starts with Windows from then on (`"autostart"` in your page's config turns that off).

**Smart App Control.** On a freshly installed Windows 11, Smart App Control may be on (Settings → Privacy & security → Windows Security → App & browser control → Smart App Control). It blocks unsigned apps outright, with no *Run anyway*, so it blocks Versailles. The only way past it is to switch Smart App Control off, and Windows may not let you switch it on again without resetting the PC. Decide that for yourself; Versailles cannot work around it.

**Why unsigned is still checked.** The installer you download is not signed, so check that it comes from the project's GitHub releases. Updates that Versailles installs later are different: each one is verified against a key built into the app before it runs (see [Updating](#updating)). What Versailles protects and how: [SECURITY.md](SECURITY.md).

## First run

When there is no page yet (`Documents\Widgets\desktop\index.html`), a small **Welcome to Versailles** window offers two choices:

- **Set up the example desktop** copies the starter into `Documents\Widgets`: a page with a clock and a note on the wallpaper, a command bar on **Alt+Space**, a drawing pad, commented-out examples (a top bar, a taskbar, quick settings) and the page SDK. Files that already exist are never overwritten.
- **I'll bring my own** copies nothing. Write your own `desktop\index.html`; Versailles picks it up when you save it.

Versailles asks once. To be asked again, delete `%LOCALAPPDATA%\com.versailles.widgets\first-run-offered` and restart it.

Then:

- The wallpaper is your page. Apps cover it as they cover the normal wallpaper.
- The **Versailles icon** sits in the notification area. A left click opens the command bar. Its menu: **Show / Hide desktop page**, **Copy diagnostics** (a zip for bug reports), **Quit**, and **Update to x.y.z** when there is a new release.
- **Alt+Space** opens the command bar. Type `help` to see what it does: shortcuts, apps, `?? invoice` to search files and Settings, `recent`, `!` for a PowerShell line, `!!` for a terminal.

## Your folder

Everything that is yours lives in `%USERPROFILE%\Documents\Widgets`:

| Path | What |
| ---- | ---- |
| `desktop\index.html` | Your page: widgets, bars, overlays and the `#versailles` config block. Versailles never rewrites it. |
| `desktop\bar.js` | The starter's command bar. It is part of your page now: Versailles never updates it. |
| `.sdk\versailles.js` | The page SDK. Versailles replaces an unchanged older copy when it updates (keeping `versailles.js.bak`) and leaves a copy you edited alone. |
| `.sdk\versailles.d.ts` | Types for the SDK, for your editor. A folder set up before 0.12 does not have it: copy it from [`templates/starter/.sdk/`](../templates/starter/.sdk/versailles.d.ts). |
| `.versailles\` | Versailles' own state: the API token, each piece's storage (`storage\<id>.json`). Do not share `config.json`. |

Keep the folder in git or another backup: it is your desktop.

## Editing live

Open `Documents\Widgets\desktop\index.html` in your editor and save. Versailles watches the folder: the wallpaper, open bars and overlays reload a moment after each save, and hotkeys are bound again. There is no build step.

The page is plain HTML. The few things Versailles reads from it:

- `article.widget` elements are tiles on the wallpaper.
- `template.spawnable` elements are windows Versailles opens from the same page: bars, overlays, slide-outs.
- `data-*` attributes on those tags say where a window goes and what it may do ([DIALECT.md](DIALECT.md)).
- `data-hooks` lists what a piece may ask Windows for. A piece gets nothing it did not list ([HOOKS.md](HOOKS.md)).
- `<script type="application/json" id="versailles">` in `<head>` holds the settings (autostart, shortcuts, hiding the taskbar).

**Autocomplete.** Put a `jsconfig.json` next to `desktop` in `Documents\Widgets` so VS Code knows the SDK in your `.js` files:

```json
{
  "compilerOptions": { "allowJs": true, "checkJs": false },
  "include": ["desktop/**/*.js", ".sdk/versailles.d.ts"]
}
```

## Preview in a browser

While Versailles runs, it serves your folder at `http://127.0.0.1:47831/files/`. The starter's page shows a different surface for each `?m=`:

| URL | What you see |
| --- | ------------ |
| `http://127.0.0.1:47831/files/desktop/index.html` | The wallpaper and its widgets |
| `…/index.html?m=widgets` | Every widget as a card |
| `…/index.html?m=spawn` | Every spawnable (bar, overlay) as a card |
| `…/index.html?m=spawn&id=draw` | One spawnable (here the drawing pad) at its window size |
| `…/index.html?m=action_bar` | The command bar alone |

In a browser, hook calls answer `{ host: false }` instead of real values, so you see the layout without system data. Opening the file from disk works the same way. (`?m=` is implemented by a small boot script at the end of the starter page; keep it if you build your own page from the starter.)

## Your first widget

A widget is an `article.widget` on the wallpaper. Paste this into the starter page inside `<section id="desktop">`, next to the clock, and save:

```html
<article class="widget battery-tile" data-id="battery-tile" data-hooks="battery">
  <b>battery</b> <span class="pct">…</span>
</article>
<style>
  .battery-tile { width: 180px; padding: 16px 20px; font: 600 14px/1.4 var(--font-sans, sans-serif); }
</style>
<script>
  (function batteryTile() {
    const pct = document.querySelector('.battery-tile .pct');
    // The SDK loads at the end of the page: wait for it.
    (function whenSdk(fn) {
      typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
    })((v) => {
      // Called with the current value now, then on every change.
      v.for('battery-tile').battery.onChange((b) => {
        pct.textContent = b.present ? b.percent + '%' + (b.charging ? ' charging' : '') : 'no battery';
      });
    });
  })();
</script>
```

- `data-id` names the piece. It must be unique on the page; `versailles.for(id)` acts as that piece.
- `data-hooks="battery"` is what makes the battery readable. Without it the call fails with `hook-undeclared`.
- The wallpaper holds every widget in one document, so name the piece with `versailles.for('battery-tile')`.

More widgets to copy from: [`examples/`](../examples).

## Your first bar

A `template.spawnable` with `data-anchor="top"`, `"bottom"`, `"left"` or `"right"` is a docked bar. It runs along the whole edge, opens by itself, and (by default) reserves its strip like the Windows taskbar, so maximized windows stop at it. Paste this anywhere in `<body>`, outside `<section id="desktop">`:

```html
<template class="spawnable" data-id="mybar" data-anchor="top" data-h="30" data-hooks="network">
  <style>
    .mybar { height: 100%; display: flex; align-items: center; gap: 12px; padding: 0 12px;
             background: var(--paper, #f9fafb); color: var(--ink, #141414); font: 600 12px/1 var(--font-sans, sans-serif); }
    .mybar .net { margin-left: auto; }
  </style>
  <div class="mybar"><span class="time">--:--</span><span class="net">…</span></div>
  <script>
    (function myBar() {
      // A template's scripts run in the bar's own window, inside its surface.
      const root = document.currentScript.closest('.spawn-surface');
      const time = root.querySelector('.time');
      const net = root.querySelector('.net');
      setInterval(() => (time.textContent = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })), 1000);
      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        v.for('mybar').network.onChange((n) => (net.textContent = n.online ? (n.name || 'online') : 'offline'));
      });
    })();
  </script>
</template>
```

- `data-h` is the bar's depth (`data-w` for a left or right bar).
- `data-dock="float"` keeps it on top without reserving space; `data-dock="auto-hide"` tucks it away until the mouse touches the edge.
- `data-monitor="all"` puts a copy on every monitor.
- The starter frames spawn windows with a border. For a bar, add `.spawn-surface.solo:has(.mybar) { border: 0 !important; box-shadow: none !important; border-radius: 0 !important; }` to its `<style>`.

The starter has a complete top bar and a taskbar, commented out near the end of `<body>`: delete the two `try me` lines around one and save. Recipes: [top bar](cookbook/top-bar.md), [taskbar](cookbook/taskbar.md).

## Your first overlay with a hotkey

Any other `template.spawnable` is a window Versailles opens on demand. `data-anchor` picks the place: a point of the grid `tl tc ml c mr bl bc br`, `cursor`, or `tr` for a slide-out docked at the top right. To bind a key, add `hotkey` to `data-hooks` and the combo to `data-hotkey`. Paste this after the command bar's template (`data-id="action-bar"`): a left click on the tray icon opens the first piece on the page that has the `hotkey` hook.

```html
<template class="spawnable" data-id="scratch" data-anchor="c" data-w="420" data-h="260"
          data-hooks="hotkey,storage" data-hotkey="Ctrl+Alt+N">
  <textarea class="scratch" style="width: 100%; height: 100%; box-sizing: border-box; border: 0; padding: 16px; font: 14px/1.5 sans-serif;"></textarea>
  <script>
    (function scratch() {
      const box = document.currentScript.closest('.spawn-surface').querySelector('.scratch');
      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })(async (v) => {
        const store = v.for('scratch').storage;
        const text = await store.get('text');
        if (typeof text === 'string') box.value = text;
        box.addEventListener('input', () => store.set('text', box.value));
        v.for('scratch').on('shown', () => box.focus());
      });
    })();
  </script>
</template>
```

Press **Ctrl+Alt+N**: the overlay opens centred over your apps with a dimmed backdrop. Press it again, click the backdrop or switch to another app, and it hides (it is hidden, not closed, so it keeps its state). The text survives restarts through the `storage` hook.

- Several combos: `data-hotkey="Ctrl+Alt+N, Win+N"`. If two pieces bind the same combo, the first on the page wins.
- `data-dim="none"` drops the backdrop; `data-dismiss-on-blur="false"` keeps the overlay up when another app gets the focus.

**The Windows and Copilot keys.** `data-hotkey="Win"` binds a tap of the Windows key alone, `"Copilot"` the Copilot key, and combos such as `"Win+N"` can take over shortcuts Windows already uses. Windows then never sees those keys: Start does not open on that tap. Win+L and Ctrl+Alt+Del cannot be taken over, and while an app running as administrator is in front the keys go to Windows. To silence a Windows shortcut without binding it, list it in the config: `"shell": { "suppress": ["Win+A", "Copilot"] }`. If a press still reaches Windows now and then, the log says why ([DIALECT.md](DIALECT.md#overlay-vs-slide-out)).

## Hiding the Windows taskbar

Once you have a bar of your own (a [taskbar](cookbook/taskbar.md), or at least a [top bar](cookbook/top-bar.md) with a way to reach your apps), Versailles can hide Explorer's taskbar so your bar takes its place. In the `#versailles` block in `<head>`:

```json
"shell": { "hideTaskbar": true }
```

Save, and the taskbar and the strip it reserves are gone. Explorer stays the shell: Start (the Win key), Quick Settings (Win+A), the notification center (Win+N) and toasts keep working. While the taskbar is hidden, Versailles also holds the notification-area icons, so a bar with the `tray` hook can show them.

Getting it back is safe by design:

- Remove the line (or set it to `false`) and save, or **Quit** from the tray menu: the taskbar comes back at once.
- If Versailles crashes or is ended in Task Manager, a small guard process brings the taskbar back.
- After a power cut or a sign-out while it was hidden, a one-time logon entry brings it back.
- By hand: `"%LOCALAPPDATA%\Versailles\versailles.exe" --restore-taskbar`.

Your own *Automatically hide the taskbar* setting is saved first and put back as it was. Taskbar tools such as Windhawk mods that restyle the taskbar may fight the hiding: turn them off while it is on.

## Updating

A minute after it starts, and once a day, Versailles asks GitHub whether there is a newer release. If there is, the tray menu shows **Update to x.y.z** at the top. Nothing is downloaded before you click it. A click downloads the installer, checks its signature against the key built into Versailles (an installer that fails the check is never run), gives the taskbar and the bars' space back, runs the installer with only a progress bar, and starts the new version.

- Your page and `bar.js` are never touched. An unchanged `.sdk\versailles.js` is replaced by the new SDK (the old one stays as `versailles.js.bak`); one you edited is left alone.
- `"updates": { "check": false }` in `#versailles` stops the checks. You can always install a newer release by hand over the old one.
- Pages keep working across 1.x: what is promised is in [COMPATIBILITY.md](../COMPATIBILITY.md); what changed is in the [changelog](../CHANGELOG.md).

## Uninstalling

1. **Quit** Versailles from its tray menu. This puts the Windows taskbar back (if you hid it) and gives back the bars' space.
2. Settings → Apps → Installed apps → **Versailles** → **Uninstall** (or run `%LOCALAPPDATA%\Versailles\uninstall.exe`).

`Documents\Widgets` stays: your page, your storage and the SDK are yours. Delete the folder yourself if you no longer want it. Logs and the WebView2 cache are in `%LOCALAPPDATA%\com.versailles.widgets`.

If the taskbar is still hidden after uninstalling (for example because Versailles was not quit first): open Task Manager (Ctrl+Shift+Esc), right-click **Windows Explorer** → **Restart**. If it then hides itself whenever the mouse leaves it, turn off Settings → Personalization → Taskbar → Taskbar behaviours → *Automatically hide the taskbar*.

## When something goes wrong

- **Nothing shows after a save.** A broken `#versailles` block (invalid JSON) or an unclosed tag can stop the page. Open the preview URL in a browser and look at its console (F12).
- **A hook call fails.** The error's `code` says why: `hook-undeclared` (add the hook to `data-hooks`), `hook-refused` (the wallpaper asked for something only a bar or overlay may do), `needs-gesture` (a setting changes only inside a click or key press). The table: [HOOKS.md › Errors](HOOKS.md#errors).
- **Logs** are in `%LOCALAPPDATA%\com.versailles.widgets\logs` (`Versailles.log`).
- **Bug reports:** tray menu → **Copy diagnostics** saves a zip to Downloads (passwords and tokens removed, nothing is sent). Look through it, then attach it to an [issue](https://github.com/crasyK/versailles/issues).
