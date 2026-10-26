# Designing your desktop with an AI assistant

Your Versailles desktop is one page: `Documents\Widgets\desktop\index.html`. Any assistant that writes HTML can change it with you, one request at a time. Versailles has no chat of its own; you use the assistant you already have.

## How it goes

1. Open a chat with your assistant. Paste the **brief** below, then your current `index.html` (or one of the [showcase pages](../examples/showcase) as a start).
2. Say what you want, in your own words: "A calm background with a big clock."
3. Copy the page the assistant gives back into `Documents\Widgets\desktop\index.html` and save. Versailles reloads it. (A coding assistant that can edit files can save it for you.)
4. Look, then keep talking: "A little quieter." "Move the clock to the left." "Put my daily apps along the top." "Actually, keep the normal taskbar."

Stop whenever it feels right. A background and a clock is a complete desktop.

Before you save a big change, keep a copy of the page that works. If the assistant's page does not load, put the copy back. To see a page without installing it, open `http://127.0.0.1:47831/files/desktop/index.html?m=widgets` (or `?m=spawn`) in a browser while Versailles runs.

The desktop in the [Versailles film](../README.md) was made this way; its last version is [`examples/showcase/home`](../examples/showcase/home/index.html).

## The brief

Paste everything in this block at the start of the chat.

```text
You are helping me design my Windows desktop for Versailles. Versailles shows ONE HTML page
(Documents\Widgets\desktop\index.html) as my desktop. Always answer with the WHOLE page, ready to save.

Page rules
- Keep the existing <head> config block <script type="application/json" id="versailles">, the boot
  script and the script that loads the SDK (/files/.sdk/versailles.js). Keep data-id values unique.
- Wallpaper widgets: <article class="widget" data-id="..."> inside <section id="desktop">. Position
  them with CSS. A widget's <script> follows it.
- Bars and overlays: <template class="spawnable" data-id="..."> anywhere in <body>. Each opens in its
  own window. Its scripts find their root with document.currentScript.closest('.spawn-surface').
  - Docked bar: data-anchor="top|bottom|left|right" and data-h (top/bottom) or data-w (left/right).
    data-dock="float" or "auto-hide" to not reserve space. data-monitor="all" for every monitor.
  - Overlay: data-anchor="c|tc|tl|tr|ml|mr|bl|bc|br|cursor", data-w, data-h.
  - Hotkey: add "hotkey" to data-hooks and data-hotkey="Alt+Space" (or "Win", "Copilot", "Win+N").
- No iframes. Plain HTML, CSS and JavaScript; no build step. Google Fonts links are fine.

Talking to Windows
- Every piece lists what it may use in data-hooks="media,battery,...". Undeclared hooks fail.
- Wait for the SDK, then act as the piece:
    (function whenSdk(fn) { window.versailles && typeof window.versailles.for === 'function'
      ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50); })((v) => {
      v.for('my-id').media.onChange((m) => { /* m.title, m.artist, m.thumbnailDataUrl, m.status */ });
    });
- Useful hooks: media (now playing, playPause/next/previous), battery ({percent, charging}),
  network ({online, name}), windows (list/track open app windows, with icons), windows-control
  (focus/minimize/close, needs a click), shell (shell.open('Microsoft Edge' | 'ms-settings:' | 'https://...'
  | 'shell:Downloads')), audio + audio-control (volume, mute), radios (Wi-Fi/Bluetooth on/off),
  storage (save small values), keyboard (layouts).
- Settings changes (volume, Wi-Fi, ...) only work from a bar or overlay, inside a click or key press.
- Full reference: https://github.com/crasyK/versailles/blob/main/docs/HOOKS.md and docs/DIALECT.md.

Replacing the taskbar
- Only when I ask for it, and only if the page has its own bar with a way to reach my apps:
  set "shell": { "hideTaskbar": true } in the config block. Quitting Versailles brings the
  Windows taskbar back.

Answer briefly: one line about what you changed, then the whole page.
```

## When something does not work

- **The page is blank or a piece is missing.** Ask the assistant to check that the boot script and the SDK loader are still there, and that every `data-id` is unique.
- **A widget shows no data.** Its hook is probably missing from `data-hooks`.
- **A button that changes a setting does nothing.** Settings change only from a bar or overlay, inside a click.
- **The taskbar is gone and you want it back.** Set `"hideTaskbar": false` (or remove it) and save, or choose **Quit** in the Versailles tray menu.

More: [Getting started](GETTING-STARTED.md), [the cookbook](cookbook/README.md).
