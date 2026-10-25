# Tablet posture

On a convertible, fold the screen back (or take the keyboard off) and your bars and widgets grow finger-sized targets; fold it back and they shrink again. The page decides what changes; Versailles only says how the machine is held.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `posture` | plain | `tablet` (folded or keyboard off), `docked`, the touch screen and pen |

Read-only, so any piece can declare it, the wallpaper included. Each window is its own document: every bar or overlay that should change declares `posture` itself.

## Snippet

The idea: one attribute on `<html>`, set from the hook, and CSS that keys off it. Here for a bar (the same lines work in a widget script on the wallpaper):

```html
<template class="spawnable" data-id="dock" data-anchor="bottom" data-h="48" data-hooks="posture">
  <style>
    .dock { height: 100%; display: flex; align-items: center; justify-content: center; gap: 8px;
            background: var(--paper, #f9fafb); font: 600 12px/1 var(--font-sans, "Segoe UI", sans-serif); }
    .dock button { min-width: 36px; height: 32px; border-radius: 8px; border: 2px solid currentColor; background: none; }

    /* Tablet posture: bigger targets. */
    html[data-posture="tablet"] .dock { gap: 14px; }
    html[data-posture="tablet"] .dock button { min-width: 52px; height: 42px; font-size: 14px; }

    /* Whatever the posture: a finger (coarse pointer) needs room too. */
    @media (any-pointer: coarse) { .dock button { min-height: 40px; } }
  </style>
  <div class="dock"><button>1</button><button>2</button><button>3</button></div>
  <script>
    (function dockPosture() {
      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        // Called with the current posture now, then whenever it changes.
        v.for('dock').posture.onChange((p) => {
          if (p.tablet) document.documentElement.dataset.posture = 'tablet';
          else delete document.documentElement.dataset.posture;
        });
      });
    })();
  </script>
</template>
```

Grow the contents inside the bar's depth (`data-h`), as above: the strip a docked bar reserves stays the same.

## Gotchas

- **`tablet` is `null`** on a PC that reports no posture: a desktop, a plain laptop. Treat `null` like `false`. It is `true` only on a convertible that Windows knows as one.
- **Windows 11 has no tablet-mode switch.** The posture comes from the hinge or keyboard sensor, pushed as soon as Windows announces it, with a re-read every 5 s in case a broadcast is missed.
- **Posture is not the pointer.** A laptop with a touch screen is still in laptop posture when you touch it. For "what is pointing right now", use the event's `pointerType` (`"touch"`, `"pen"`, `"mouse"`) or the media queries `(any-pointer: coarse)` / `(pointer: coarse)`; they need no hook.
- **`touch`** says whether there is a touch screen (`screen`), a pen (`pen`) and how many fingers it tracks (`maxTouches`); use it to decide whether to offer touch-sized controls at all.
- **Every window, its own attribute.** The wallpaper is one document for all widgets; each bar, slide-out and overlay is a document of its own and needs its own `posture` subscription.
- **Test without a convertible.** Set `document.documentElement.dataset.posture = 'tablet'` in the preview's browser console (`?m=spawn&id=dock`) to see the tablet layout.

The starter's taskbar example (`try me (taskbar)`) grows its buttons in tablet posture the same way.
