# Talking between pieces

Pieces on one page run in different windows: the wallpaper, each bar, each overlay. Two ways to pass data between them: the **bus** (publish and subscribe, any number of open windows) and **spawn data** (hand data to an overlay as you open it). This recipe has a focus timer on the wallpaper whose countdown a top bar shows, and a bar button that opens an overlay with a note already chosen.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `bus` | plain | `bus.publish(topic, data)` and `bus.subscribe(topic, fn)` |
| `spawn` | plain | `spawn(id, data)`: open another spawnable and hand it data |

Every piece that publishes or listens declares `bus`; a piece without it never sees a message. The piece that opens another declares `spawn`; the one opened needs nothing to read its data.

## Snippet: a timer and a bar

The timer, a widget on the wallpaper:

```html
<article class="widget focus-timer" data-id="focus-timer" data-hooks="bus">
  <button type="button">start 25:00</button>
</article>
<script>
  (function focusTimer() {
    const btn = document.querySelector('.focus-timer button');
    (function whenSdk(fn) {
      typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
    })((v) => {
      const bus = v.for('focus-timer').bus;
      let end = 0, timer = 0;
      btn.onclick = () => {
        end = Date.now() + 25 * 60 * 1000;
        clearInterval(timer);
        timer = setInterval(tick, 1000);
        tick();
      };
      function tick() {
        const left = Math.max(0, Math.round((end - Date.now()) / 1000));
        const label = String(Math.floor(left / 60)).padStart(2, '0') + ':' + String(left % 60).padStart(2, '0');
        btn.textContent = left ? label : 'start 25:00';
        bus.publish('focus', { left, label });   // to every open window of a piece with `bus`
        if (!left) clearInterval(timer);
      }
    });
  })();
</script>
```

The bar that shows it, and opens a note overlay with data:

```html
<template class="spawnable" data-id="statusbar" data-anchor="top" data-h="28" data-hooks="bus,spawn">
  <div class="statusbar" style="height: 100%; display: flex; gap: 12px; align-items: center; padding: 0 12px; background: var(--paper, #f9fafb); font: 600 12px/1 sans-serif;">
    <span class="focus">no timer</span>
    <button type="button" class="groceries" style="margin-left: auto;">groceries</button>
  </div>
  <script>
    (function statusBar() {
      const root = document.currentScript.closest('.spawn-surface');
      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        const me = v.for('statusbar');
        me.bus.subscribe('focus', (data, msg) => {      // msg: { topic, data, from: "focus-timer" }
          root.querySelector('.focus').textContent = data.left ? 'focus ' + data.label : 'break';
        });
        root.querySelector('.groceries').onclick = () => me.spawn('notepad', { open: 'groceries' });
      });
    })();
  </script>
</template>

<template class="spawnable" data-id="notepad" data-anchor="c" data-w="360" data-h="240">
  <div class="notepad" style="padding: 16px; font: 14px/1.4 sans-serif;">opened: <b class="which">nothing</b></div>
  <script>
    (function notepad() {
      const which = document.currentScript.closest('.spawn-surface').querySelector('.which');
      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        const show = (data) => { if (data && data.open) which.textContent = data.open; };
        show(v.spawnData());      // the data it was opened with (first open)
        v.onSpawn(show);          // and each later spawn(id, data) while it exists
      });
    })();
  </script>
</template>
```

## Gotchas

- **Only open windows hear a message.** A bar that is hidden, or an overlay never opened yet, misses it. Anything a late opener needs goes into [storage](storage-widget.md) (or is published again, like the timer above, which publishes every second).
- **A piece does not receive its own messages.** Two windows of the same piece (copies of a bar on two monitors) do not hear each other either.
- **Topics** are 1–128 bytes, **data** any JSON up to 256 KB. `subscribe('*', fn)` receives every topic; `msg.topic` says which.
- **`publish` resolves the number of windows reached**, which is handy for "is anyone listening?".
- **Private by default.** Messages are delivered by the host straight into the windows of pieces that declare `bus`, not broadcast to every window. Still, any piece with `bus` reads every topic: do not put secrets on the bus.
- **`spawn(id)` toggles; `spawn(id, data)` opens.** With data, an open overlay stays open and gets the new data through `onSpawn`. On the first open the data is there before the overlay's scripts run, so `spawnData()` has it at once.
- **Piece events need no bus.** For "my overlay was shown / hidden", listen to `on('shown')` / `on('hidden')` in the piece itself; `idle`, `active`, `theme-changed` and `monitor-changed` reach every window without any hook.
- **Unsubscribe** when a piece no longer needs a topic: `subscribe` resolves a function that ends it.
