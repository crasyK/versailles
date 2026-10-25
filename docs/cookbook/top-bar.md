# Top bar

A thin bar along the top of the primary monitor with the time and date, what is playing (with play / pause), the network and the battery. It reserves its strip like the Windows taskbar, so maximized windows stop below it.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `battery` | plain | Level, charging, no battery on a desktop PC |
| `network` | plain | Online, Wi-Fi name and signal |
| `media` | plain | What is playing (any app that shows in Windows' media flyout), play / pause / next |

Plain hooks work anywhere, including the wallpaper. Nothing here changes a setting, so no click is required by the host (play / pause is a button anyway).

## Snippet

```html
<template class="spawnable" data-id="topbar" data-anchor="top" data-h="32"
          data-hooks="battery,network,media">
  <style>
    .spawn-surface.solo:has(.topbar) { border: 0 !important; box-shadow: none !important; border-radius: 0 !important; }
    .topbar {
      height: 100%; display: flex; align-items: center; gap: 12px; padding: 0 12px;
      background: var(--paper, #f9fafb); color: var(--ink, #141414);
      border-bottom: 2px solid var(--ink, #141414);
      font: 600 12px/1 var(--font-sans, "Segoe UI", sans-serif); white-space: nowrap;
    }
    .topbar .time { font-variant-numeric: tabular-nums; }
    .topbar .date, .topbar .track { color: var(--ink-mut, #6b7280); }
    .topbar .track { min-width: 0; max-width: 40vw; overflow: hidden; text-overflow: ellipsis; }
    .topbar .right { margin-left: auto; display: flex; align-items: center; gap: 10px; }
    .topbar button { border: 0; background: none; color: inherit; font: inherit; cursor: pointer; padding: 4px 6px; }
    .topbar [hidden] { display: none; }
  </style>
  <div class="topbar">
    <span class="time">--:--</span>
    <span class="date"></span>
    <span class="music" hidden><button class="play" type="button">play</button><span class="track"></span></span>
    <span class="right"><span class="net">…</span><span class="bat">…</span></span>
  </div>
  <script>
    (function topBar() {
      const root = document.currentScript.closest('.spawn-surface');
      const $ = (s) => root.querySelector(s);

      function tick() {
        const now = new Date();
        $('.time').textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        $('.date').textContent = now.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
      }
      tick();
      setInterval(tick, 1000);

      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        const me = v.for('topbar');

        // Each onChange is called with the current value now, then on every change.
        me.network.onChange((n) => {
          $('.net').textContent = !n.online ? n.level
            : n.wifi ? (n.name || 'wi-fi') + (n.signalBars == null ? '' : ' ' + n.signalBars + '/5')
            : n.name || 'online';
        });

        me.battery.onChange((b) => {
          $('.bat').textContent = !b.present ? 'ac' : b.percent + '%' + (b.charging ? ' +' : '');
        });

        me.media.onChange((m) => {
          $('.music').hidden = !m.hasSession;
          $('.track').textContent = [m.artist, m.title].filter(Boolean).join(' – ');
          $('.play').textContent = m.status === 'Playing' ? 'pause' : 'play';
        });
        $('.play').addEventListener('click', () => me.media.playPause());
      });
    })();
  </script>
</template>
```

## Gotchas

- **Depth and edge.** `data-h` is the bar's height. For a bar on the side use `data-anchor="left"` / `"right"` and `data-w`. One edge per bar; a top bar and a taskbar together are two templates.
- **Reserve, float or auto-hide.** `data-dock="reserve"` (the default) shrinks the work area. `float` lies over apps without reserving space; `auto-hide` with `data-peek="4"` slides in when the mouse touches the edge.
- **Every monitor.** `data-monitor="all"` puts one copy on each monitor (windows `widget-topbar`, `widget-topbar:2`, …). Every copy is the same piece and gets the same pushes. Use `await v.monitor()` if a copy should show something about its own monitor.
- **No battery.** On a desktop PC `present` is `false` and `percent` is `null`.
- **`network.level`** is `"none"`, `"local"`, `"constrained"` or `"internet"`; `signalBars` is `null` on a wired connection.
- **Media.** `media.onChange` gets one value per change of track or play state, not a running position. `status` is Windows' own word (`"Playing"`, `"Paused"`, `"Stopped"`). `hasSession` is `false` when nothing is playing anywhere. `thumbnailDataUrl` is the cover, when the app gives one.
- **A bar never takes the focus** when it appears and is never dismissed by a click elsewhere. With `hotkey` in `data-hooks` and a `data-hotkey`, the combo hides and shows it (hiding gives the reserved space back).
- **Fullscreen.** While a fullscreen app (a video, a game) is up, a reserving bar drops behind it.
- **Pushes, not polling.** Versailles reads the battery every 15 s and the network every 3 s while some piece declares the hook, and pushes only changes. Do not poll `now()` in a timer.

The starter has a similar bar, commented out near the end of `<body>` (`try me (top bar)`), with a button that opens a pinned overlay.
