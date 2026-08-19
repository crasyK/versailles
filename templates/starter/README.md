# Starter kit

Copy this folder onto `%USERPROFILE%\Documents\Widgets`:

```bash
xcopy /E /I templates\starter %USERPROFILE%\Documents\Widgets
```

On Unix-like shells:

```bash
cp -R templates/starter/. "$HOME/Documents/Widgets/"
```

Do not overwrite an existing `desktop/index.html` unless you mean to. The host does not seed this tree automatically.

| File | Purpose |
| ---- | ------- |
| `desktop/index.html` | Widgets, spawnables, example command bar, `#versailles` config |
| `desktop/bar.js` | Example command-bar engine |
| `.sdk/versailles.js` | Named-hook SDK |
| `.sdk/xterm.js` | Nested terminal for `!!` |

Host knobs and shortcuts live in `<script type="application/json" id="versailles">` in the page. Then start Versailles and type `desk` in the overlay (Alt+Space). Add more tiles from [`../../examples/`](../../examples).
