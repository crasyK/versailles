# Design

Versailles uses the **paper theme** as its visual language for widgets and the desktop shell.

## Documents

| File | Purpose |
| ---- | ------- |
| [PAPER-THEME.md](./PAPER-THEME.md) | Token reference — colors, shape, typography, control states |
| [index.html](./index.html) | Live swatch page for the same tokens (open in a browser) |

## Principles

Paper theme is monochrome and high-contrast: a light canvas (`#f4f4f4`), paper cards (`#f9fafb`), and ink strokes (`#141414`). Widgets are cards on the canvas. The launcher is paper chrome with a dark-card terminal nested inside.

Implementation lives in `src/launcher.css` and the single desktop document `Documents\Widgets\desktop\index.html`. When adding or restyling UI, match the tokens in `PAPER-THEME.md`.
