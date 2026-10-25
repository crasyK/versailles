# Command bar and launcher

A Start menu you type into: installed apps (Store apps included, with their icons), Settings pages by name, files from the Windows Search index, recently opened files, your own shortcuts, a PowerShell line and a terminal. The starter ships one (`desktop/bar.js`); this page shows how to use and tune it, and how to build a small launcher of your own on the same calls.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `shell` | plain for `open`, exec for `search` / `recent` | Open apps, files, folders, URLs, Settings pages; search files and Settings; list recent files |
| `hotkey` | (declaration) | Alt+Space (or any combo) toggles the bar |
| `pty` | exec | The starter bar's terminal (`!!`); not needed for a launcher |

Exec class: only a bar, slide-out or overlay whose piece declares the hook gets these calls; the wallpaper never does. Search results and recent files reveal file names: declare `shell` only on the piece that needs it.

## The starter's bar

It is the `template.spawnable.action-bar` with `data-id="action-bar"` in the starter page, plus `desktop/bar.js`, which that template loads:

```html
<template class="spawnable action-bar" data-id="action-bar"
          data-hooks="shell,hotkey,pty" data-hotkey="Alt+Space"
          data-anchor="tc" data-w="640" data-h="420"> … </template>
```

The markup inside (the `#cli-*` ids) is what `bar.js` expects; restyle it freely, keep the ids. `bar.js` is generated from `src/launcher.ts` in the Versailles repository; your copy is part of your page and is never updated by Versailles, so a newer starter's bar arrives only if you copy it yourself.

What you can type (`help` lists it):

| Input | Does |
| ----- | ---- |
| a name | Your shortcuts and installed apps, best match first; Enter opens it |
| (nothing) | The last few recently opened files under **Recent** |
| `?? invoice` | Search the Windows Search index (names and contents), Settings pages on top |
| `recent [filter]` | Up to 30 recently opened files |
| `bluetooth`, `nachtmodus` | Enter on a word that is no shortcut opens the matching Settings page (English or German) |
| `start` | All installed apps |
| `? query`, `https://…` | Search the web, open a URL |
| `! command`, `!!` | One PowerShell line, a terminal in the bar |
| `= 2*21` | Calculator |
| `desk` | Show or hide the desktop page |

While you type, Settings pages and files appear below the shortcuts in their own groups.

### Your shortcuts

Shortcuts live in the `#versailles` block in `<head>`:

```json
"shortcuts": [
  { "n": "github", "t": "web", "d": "GitHub", "target": "https://github.com/", "cat": "dev", "aliases": ["gh"] },
  { "n": "downloads", "t": "folder", "d": "Downloads", "target": "%HOME%\\Downloads", "cat": "folders" },
  { "n": "code", "t": "app", "d": "VS Code", "target": "code", "cat": "apps" }
]
```

`n` is what you type, `t` one of `web`, `folder`, `app`, `term`, `d` the label, `cat` a group, `aliases` other names. `%HOME%` is your profile folder. Backslashes are doubled in JSON.

### Tuning

`spawnables.<id>.opts` in `#versailles` (the id in lower case) is handed to the bar:

```json
"spawnables": {
  "action-bar": {
    "overlay": { "dismissOnBlur": true },
    "opts": { "recentFiles": 5, "liveSearch": true, "suggestionLimit": 12, "coverMonitor": false }
  }
}
```

| Option | Default | Effect |
| ------ | ------- | ------ |
| `recentFiles` | `5` | Recent files shown on an empty line; `0` = none |
| `liveSearch` | `true` | Settings pages and files while you type |
| `suggestionLimit` | `12` | Rows of suggestions |
| `coverMonitor` | `false` | The bar's window covers the monitor and the bar lays out its own box (room for animations) |
| `blurDismissMs` | `280` | Delay before the bar hides after losing the focus |
| `autoDismissLaunch` | `true` | Hide after launching something |

To move it, change `data-anchor` (`tc` = top centre, `c` = centre) and `data-w` / `data-h`; to rebind it, `data-hotkey` (`"Alt+Space, Win"` binds both).

## A launcher of your own

The same calls from the SDK, in a small overlay: apps and recent files on an empty line, apps, Settings pages and files as you type, Enter opens the first row, Escape closes.

```html
<template class="spawnable" data-id="finder" data-anchor="tc" data-offset="0,160" data-w="560" data-h="380"
          data-hooks="shell,hotkey" data-hotkey="Ctrl+Alt+Space">
  <style>
    .finder { height: 100%; box-sizing: border-box; padding: 14px; display: flex; flex-direction: column; gap: 8px;
              background: var(--paper, #f9fafb); color: var(--ink, #141414); font: 14px/1.3 var(--font-sans, "Segoe UI", sans-serif); }
    .finder input { font: inherit; font-size: 18px; padding: 8px 10px; border: 2px solid currentColor; border-radius: 10px; }
    .finder ul { list-style: none; margin: 0; padding: 0; overflow: auto; }
    .finder li { display: flex; gap: 8px; align-items: center; padding: 6px 8px; border-radius: 8px; cursor: pointer; }
    .finder li:hover, .finder li:first-child { background: var(--chip, #e5e7eb); }
    .finder li small { margin-left: auto; color: var(--ink-mut, #6b7280); }
    .finder img { width: 20px; height: 20px; }
  </style>
  <div class="finder"><input placeholder="apps, settings, files" spellcheck="false"><ul></ul></div>
  <script>
    (function finder() {
      const root = document.currentScript.closest('.spawn-surface');
      const input = root.querySelector('input');
      const list = root.querySelector('ul');

      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })(async (v) => {
        const me = v.for('finder');
        const apps = await v.invoke('list_catalog');   // [{ id, name, target, kind, aumid, … }]
        let rows = [];

        const open = (row) => { me.shell.open(row.target); v.invoke('dismiss_launcher'); };
        const draw = () => list.replaceChildren(...rows.map((r) => {
          const li = document.createElement('li');
          if (r.icon) li.append(Object.assign(document.createElement('img'), { src: r.icon, alt: '' }));
          li.append(r.label, Object.assign(document.createElement('small'), { textContent: r.kind }));
          li.onclick = () => open(r);
          return li;
        }));

        let seq = 0;
        async function update() {
          const q = input.value.trim();
          const mine = ++seq;
          const appRows = apps.filter((a) => q && a.name.toLowerCase().includes(q.toLowerCase())).slice(0, 5)
            .map((a) => ({ label: a.name, kind: 'app', target: a.target, id: a.id }));
          if (!q) {
            const recent = await me.shell.recent(8);
            rows = recent.map((f) => ({ label: f.name, kind: 'recent', target: f.path }));
          } else {
            // As you type: never the slow folder walk.
            const r = await me.shell.search(q, { limit: 8, fallback: false });
            rows = [...appRows,
              ...r.settings.slice(0, 3).map((s) => ({ label: s.title, kind: 'settings', target: s.uri })),
              ...r.files.map((f) => ({ label: f.name, kind: f.kind, target: f.path }))];
          }
          if (mine !== seq) return;                     // a newer keystroke won
          draw();
          for (const r of rows.filter((r) => r.kind === 'app')) {
            r.icon = await v.invoke('catalog_icon', { id: r.id });
          }
          if (mine === seq) draw();
        }

        input.addEventListener('input', update);
        input.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' && rows[0]) open(rows[0]);
          if (e.key === 'Escape') v.invoke('dismiss_launcher');
        });
        me.on('shown', () => { input.value = ''; input.focus(); update(); });
        update();
      });
    })();
  </script>
</template>
```

## Gotchas

- **Raw commands.** `list_catalog`, `catalog_icon` and `dismiss_launcher` are host commands called with `versailles.invoke`; they are part of the documented surface ([API-REVIEW-1.0.md › Odd on purpose](../API-REVIEW-1.0.md#odd-on-purpose)). `dismiss_launcher` hides every open overlay.
- **Store apps** come back as `kind: "packaged"` with `target` `shell:AppsFolder\<AppUserModelID>`; `shell.open(target)` starts them. Other apps are Start-menu shortcuts (`kind: "shortcut"`). An app with the same name in both is listed once.
- **Search speed.** `search(q, { files: false })` searches Settings pages only and is instant. File search uses the Windows Search index; when the index is off or slow (1.5 s), the search walks Documents, Desktop, Downloads, Pictures and your profile instead, names only. Pass `fallback: false` while typing, and let Enter or a `??` prefix do the full search.
- **Settings pages** are a built-in table of about 120 `ms-settings:` pages with English and German names; `title` is in the Windows display language. `exact: true` means the query is a whole name or keyword.
- **Recent files** are empty when Windows' *Show recently opened items* (Settings › Personalization › Start) is off. Network paths and missing targets are left out.
- **Opening twice.** The host ignores a second `open` of the same target within 650 ms, so a double click starts an app once.
- **Typing order.** Searches can answer out of order; keep a counter (as above) and drop stale answers.
- **Focus.** An overlay takes the focus when it opens; focus your input on `shown`. With `data-anchor="tc"` and no `data-offset`, the window rests 120 px from the top.
- **One Alt+Space.** If two pieces bind the same combo, the first on the page wins. Give your own launcher another combo, or remove the starter's bar.
