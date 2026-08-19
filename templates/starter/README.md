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
| `desktop/index.html` | One clock widget, a calculator spawnable, action-bar chrome |
| `versailles.json` | Paper theme, desktop on, port 47831, Alt+Space |
| `shortcuts.json` | Generic web + `%HOME%` folder shortcuts |
| `.sdk/versailles.js` | Named-hook SDK |

Then start Versailles and type `desk` in the action bar. Add more tiles from [`../../examples/`](../../examples).
