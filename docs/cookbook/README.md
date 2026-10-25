# Cookbook

Complete, small recipes for the pieces people build first. Each one says what it does, which hooks it needs (and their [class](../HOOKS.md#hook-classes)), gives a snippet you can paste into your page, and lists the gotchas. New to Versailles? Start with [Getting started](../GETTING-STARTED.md).

| Recipe | What you get | Hooks |
| ------ | ------------ | ----- |
| [Top bar](top-bar.md) | A bar along the top: clock, battery, network, now playing | `battery`, `network`, `media` |
| [Taskbar](taskbar.md) | Window buttons, hover previews, attention, a language pill and the tray icons, replacing Windows' taskbar | `windows`, `windows-control`, `tray`, `keyboard` |
| [Quick settings](quick-settings.md) | A slide-out with Wi-Fi, Bluetooth, sound, microphone, mixer, display, Do Not Disturb, notifications, VPN, hotspot, accessibility | `radios`, `wifi`, `bluetooth`, `audio`, `audio-control`, `display`, `dnd`, `notify`, `notify-control`, `vpn`, `hotspot`, `accessibility` |
| [Command bar and launcher](command-bar.md) | Store apps, file search, Settings pages, recent files: the starter's bar, and how to build your own | `shell`, `hotkey` |
| [Tablet posture](tablet-posture.md) | Bigger touch targets when a convertible folds into a tablet | `posture` |
| [A widget that stores data](storage-widget.md) | A to-do list that survives restarts | `storage` |
| [Talking between pieces](bus.md) | A timer on the wallpaper that a bar shows; handing data to an overlay | `bus`, `spawn` |

## Conventions in these recipes

**Where the snippet goes.** A widget (`article.widget`) goes inside the wallpaper's markup (in the starter: `<section id="desktop">`). A `template.spawnable` goes anywhere in `<body>`. Save the page and Versailles reloads it.

**Waiting for the SDK.** The starter loads the SDK at the end of the page, after your pieces' scripts. Every recipe waits for it with the same few lines:

```js
(function whenSdk(fn) {
  typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
})((v) => {
  // v is the SDK
});
```

Check `versailles.for`, not just `window.versailles`: the `#versailles` config block is an element with that id, and browsers expose it as `window.versailles` until the SDK replaces it.

**Finding your own markup.** A template's scripts run in the piece's own window, inside the surface the template was copied into. Find your elements from there, before any `await`:

```js
const root = document.currentScript.closest('.spawn-surface');
```

**Acting as a piece.** `versailles.for('<data-id>')` gives the API of one piece. It is required on the wallpaper (one document holds every widget) and harmless in a bar or overlay (the window already knows its piece).

**Colours.** The snippets use the starter's Paper variables with fallbacks (`var(--paper, #f9fafb)`), so they look right in the starter and still work on a page of your own. Versailles itself never styles your page.

**The starter's frame.** The starter draws a border around spawn windows. Bars look better without it: each bar recipe has a line like `.spawn-surface.solo:has(.topbar) { border: 0 !important; … }`. Leave it out on a page of your own.

**Outside Versailles.** In a browser or from `file:`, hook calls answer `{ host: false }` and subscriptions stay silent, so the [preview](../GETTING-STARTED.md#preview-in-a-browser) shows your layout without system data.

**Names.** The recipes use only the 1.x names. Old names (`media.nowPlaying`, `tray.state`, …) still work but log a warning; see [COMPATIBILITY.md](../../COMPATIBILITY.md). `npm run verify:docs` checks every SDK call in these pages against the SDK.
