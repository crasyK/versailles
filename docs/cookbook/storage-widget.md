# A widget that stores data

A to-do list on the wallpaper that keeps its items across restarts, page edits and updates. The data lives in a file of its own under `Documents\Widgets\.versailles\storage\`, not in the page and not in the browser's `localStorage`.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `storage` | plain | The piece's own key-value store: `get`, `set`, `delete`, `keys`, `clear` |

Works on the wallpaper and in bars and overlays alike.

## Snippet

Paste inside the wallpaper's markup (in the starter: `<section id="desktop">`):

```html
<article class="widget todo" data-id="todo" data-hooks="storage">
  <form class="todo-add"><input placeholder="add a to-do" spellcheck="false"></form>
  <ul class="todo-list"></ul>
</article>
<style>
  .todo { width: 280px; padding: 14px 16px; font: 14px/1.4 var(--font-sans, "Segoe UI", sans-serif); }
  .todo input { width: 100%; box-sizing: border-box; font: inherit; border: 0; border-bottom: 1px solid currentColor; background: none; outline: none; }
  .todo ul { list-style: none; margin: 8px 0 0; padding: 0; }
  .todo li { display: flex; gap: 8px; align-items: baseline; cursor: pointer; }
  .todo li.done span { text-decoration: line-through; opacity: 0.5; }
  .todo li button { margin-left: auto; border: 0; background: none; cursor: pointer; color: inherit; }
</style>
<script>
  (function todoWidget() {
    const list = document.querySelector('.todo .todo-list');
    const form = document.querySelector('.todo .todo-add');
    let items = [];

    (function whenSdk(fn) {
      typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
    })(async (v) => {
      const store = v.for('todo').storage;
      const saved = await store.get('items');      // null the first time; { host: false } in a browser
      if (Array.isArray(saved)) items = saved;
      draw();

      function save() { store.set('items', items).catch((e) => console.warn('todo: not saved', e.message)); }

      function draw() {
        list.replaceChildren(...items.map((item, i) => {
          const li = document.createElement('li');
          li.className = item.done ? 'done' : '';
          const text = Object.assign(document.createElement('span'), { textContent: item.text });
          const del = Object.assign(document.createElement('button'), { type: 'button', textContent: '×', title: 'delete' });
          li.append(text, del);
          li.onclick = () => { item.done = !item.done; save(); draw(); };
          del.onclick = (e) => { e.stopPropagation(); items.splice(i, 1); save(); draw(); };
          return li;
        }));
      }

      form.onsubmit = (e) => {
        e.preventDefault();
        const input = form.querySelector('input');
        if (!input.value.trim()) return;
        items.push({ text: input.value.trim(), done: false });
        input.value = '';
        save();
        draw();
      };
    });
  })();
</script>
```

## Gotchas

- **One file per piece.** The store is `.versailles\storage\<data-id>.json`, written atomically. Renaming `data-id` starts an empty store (rename the file too to keep the data). A piece cannot read another piece's store; share data through the [bus](bus.md) instead.
- **Values are JSON.** Anything `JSON.stringify` keeps: objects, arrays, strings, numbers, booleans, `null`. Dates come back as strings.
- **Limits.** Keys are 1–256 bytes; a store holds at most 1 MB. A write over the limit rejects (`host-error`).
- **Wallpaper and window share it.** A widget on the wallpaper and a spawnable with the same `data-id` (a widget that opens its own overlay, say) see the same store. Two windows writing the same key: the last write wins, and the other window does not learn about it. Publish a bus message after a write if another open window must redraw.
- **Several monitors.** Every wallpaper window loads the same page. A widget with `data-monitor="all"` runs once per monitor, so each copy reads and writes the same keys: keep writes rare (on change, not on a timer) or keep the widget on one monitor.
- **Outside Versailles** (`?m=widgets` in a browser) `get` answers `{ host: false }`: test for the shape you stored (`Array.isArray`) rather than for `null`.
- **Not `localStorage`.** The page's `localStorage` works too, but it lives in WebView2's profile, is shared by every piece, and is lost with the app's data folder. `storage` is a plain file in your Widgets folder, next to your page: back them up together.
- **Debounce typing.** For a text field, save 300–500 ms after the last keystroke, as the starter's note widget does.
