# Taskbar

A bar along the bottom of every monitor that replaces Windows' taskbar: one button per app window on that monitor (click to bring it up, click the focused one to minimize, middle-click to close), a dot on windows that want attention, Versailles' live preview card on hover, a language pill, and the notification-area (tray) icons.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `windows` | plain | The app windows a taskbar would show, their icons, attention, pushed changes; the hover preview (bars and overlays only) |
| `windows-control` | plain | Focus, minimize, restore, maximize, close, move a listed window |
| `tray` | plain | The tray icons, and clicking / hovering them (a click only inside a click or key press) |
| `keyboard` | plain to read, control to switch | The keyboard layout of the app in front; `next()` / `set()` switch it from a bar, inside a click |

Control calls work only from a bar, slide-out or overlay, never from the wallpaper, and only inside a click or key press.

## Hide Windows' taskbar

Tray icons come to Versailles only while it hides Explorer's taskbar. In `#versailles`:

```json
"shell": { "hideTaskbar": true }
```

Without it, this bar sits above the Windows taskbar, which keeps the icons (`tray.track` then reports `active: false` and no icons). How the taskbar comes back (Quit, a crash, a sign-out): [Getting started › Hiding the Windows taskbar](../GETTING-STARTED.md#hiding-the-windows-taskbar).

## Snippet

```html
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-h="44" data-monitor="all"
          data-hooks="windows,windows-control,tray,keyboard">
  <style>
    .spawn-surface.solo:has(.taskbar) { border: 0 !important; box-shadow: none !important; border-radius: 0 !important; }
    .taskbar { height: 100%; display: flex; align-items: center; gap: 6px; padding: 0 8px; overflow: hidden;
               background: var(--paper, #f9fafb); color: var(--ink, #141414); border-top: 2px solid var(--ink, #141414);
               font: 600 12px/1 var(--font-sans, "Segoe UI", sans-serif); }
    .taskbar button { border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
    .taskbar .wins { flex: 1; min-width: 0; display: flex; gap: 6px; overflow: hidden; }
    .taskbar .win { position: relative; display: inline-flex; align-items: center; gap: 8px; height: 32px;
                    max-width: 200px; padding: 0 12px 0 8px; border-radius: 8px; }
    .taskbar .win:hover { background: var(--chip, #e5e7eb); }
    .taskbar .win.focused { background: var(--ink, #141414); color: var(--paper, #f9fafb); }
    .taskbar .win.minimized { opacity: 0.55; }
    .taskbar .win.attention::after { content: ""; position: absolute; top: 4px; right: 4px; width: 7px; height: 7px;
                                     border-radius: 50%; background: currentColor; }
    .taskbar .win span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .taskbar img { width: 18px; height: 18px; flex: none; }
    .taskbar .lang { height: 26px; padding: 0 8px; border: 2px solid currentColor; border-radius: 999px; }
    .taskbar .tray { display: flex; gap: 2px; }
    .taskbar .tray button { width: 30px; height: 30px; display: grid; place-items: center; border-radius: 6px; }
    .taskbar .tray button:hover { background: var(--chip, #e5e7eb); }
  </style>
  <div class="taskbar"><div class="wins"></div><button class="lang" type="button" hidden></button><div class="tray"></div></div>
  <script>
    (function taskbar() {
      const root = document.currentScript.closest('.spawn-surface');
      const winsBox = root.querySelector('.wins');
      const trayBox = root.querySelector('.tray');
      const lang = root.querySelector('.lang');

      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })(async (v) => {
        const me = v.for('taskbar');

        // ── windows: one button per window on this copy's monitor ──
        const here = (await v.monitor())?.number;
        let previewTimer = 0;
        winsBox.addEventListener('mouseleave', () => { clearTimeout(previewTimer); me.windows.preview(null); });
        me.windows.track((list) => {
          winsBox.replaceChildren(...list.filter((w) => here == null || w.monitor === here).map((w) => {
            const b = document.createElement('button');
            b.type = 'button';
            b.className = 'win' + (w.focused ? ' focused' : '') + (w.minimized ? ' minimized' : '') + (w.attention ? ' attention' : '');
            if (w.icon) b.append(Object.assign(document.createElement('img'), { src: w.icon, alt: '' }));
            b.append(Object.assign(document.createElement('span'), { textContent: w.title })); // text only: titles come from other apps
            b.onclick = () => (w.focused && !w.minimized ? me.windows.minimize(w.id) : me.windows.focus(w.id));
            b.onauxclick = (e) => e.button === 1 && me.windows.close(w.id);
            b.onmouseenter = () => { clearTimeout(previewTimer); previewTimer = setTimeout(() => me.windows.preview(w.id, b), 400); };
            return b;
          }));
        }, { icons: true });

        // ── tray: the notification-area icons, clicked as in Windows ──
        me.tray.track((icons) => {
          trayBox.replaceChildren(...icons.filter((t) => !t.system && t.icon).map((t) => {
            const b = document.createElement('button');
            b.type = 'button';
            if (t.showTip) b.title = t.tip;
            b.append(Object.assign(document.createElement('img'), { src: t.icon, alt: '' }));
            b.onclick = (e) => e.detail === 1 && me.tray.click(t.id, 'left');   // the 2nd click of a double is skipped
            b.ondblclick = () => me.tray.click(t.id, 'double');
            b.oncontextmenu = (e) => { e.preventDefault(); me.tray.click(t.id, 'right'); };
            b.onkeydown = (e) => { if (e.key === 'Enter') { e.preventDefault(); me.tray.click(t.id, 'key'); } };
            b.onmouseenter = () => me.tray.hover(t.id, true);
            b.onmouseleave = () => me.tray.hover(t.id, false);
            return b;
          }));
        });

        // ── language pill: shown with two or more keyboard layouts ──
        const showLang = (s) => {
          const now = s.layouts.find((l) => l.id === s.current);
          lang.hidden = s.layouts.length < 2;
          lang.textContent = now ? now.short : '?';
        };
        me.keyboard.onChange(showLang);
        lang.onclick = () => me.keyboard.next().then(showLang);
      });
    })();
  </script>
</template>
```

## Gotchas

- **One copy per monitor.** `data-monitor="all"` opens a window on every monitor, each loading the page with its own `monitor` number; `v.monitor()` tells a copy where it is, and `w.monitor` where a window is. Without `data-monitor`, the bar is on the primary monitor only.
- **What is listed.** Visible, titled top-level windows, as Windows' taskbar shows them. Never Versailles' own windows, never windows on other virtual desktops. Store apps show as `ApplicationFrameHost` in `app`; use `title`.
- **Click to minimize.** Clicking the bar does not take the focus from the app: `w.focused` still names the app window, so "click the focused window to minimize it" works.
- **Attention** is set when an app flashes its taskbar button (a chat message, a finished download) and cleared when the window comes to the front. Draw it as you like; respect `prefers-reduced-motion` if it blinks.
- **Preview card.** `windows.preview(id, element)` shows Versailles' own card (a live thumbnail and the title) next to the element; `preview(null)` hides it. It is display only, cannot be styled, works only from a bar or overlay, and accepts at most 30 calls a second. It hides by itself when the pointer leaves the bar and the card.
- **Tray clicks map one to one:** `click` → `left`, `dblclick` → `double`, `contextmenu` → `right`, middle → `middle`; Enter / Space → `key`, the menu key or Shift+F10 → `menu`. Skip the second `click` of a double click (`e.detail === 2`): Windows sends one click and then the double. A click is accepted only inside a real click or key press.
- **Tray icons** arrive within about a second after the tray starts; an app that ignores Windows' "taskbar created" message shows up the next time it changes its icon. `system` icons (volume, network, battery) are drawn by Explorer: use the [quick-settings hooks](quick-settings.md) instead. Tooltips can be private (message counts, account names): declare `tray` only on the bar that shows them.
- **Keyboard.** `keyboard.next()` and `set(id)` switch the app in front, like Win+Space; `ok: false` means the app did not switch (an app running as administrator ignores the request). `short` is the language code (`"DE"`); two layouts of one language share it, so show `layout` too if you have both.
- **Redrawing everything on each change is fine** for a sketch like this one. The starter's taskbar example keeps the preview card on the button across redraws, keeps keyboard focus on a tray icon, and has a tablet-posture style; copy that one for daily use (`try me (taskbar)` near the end of the starter page).
