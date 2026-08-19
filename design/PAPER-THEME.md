# Paper theme

Monochrome, high-contrast visual language for Versailles widgets and desktop surfaces. Paper canvas, 2px ink strokes, rounded pills and cards, hard offset shadows. No glass, acrylic, mica, or soft drop shadows.

---

## Color tokens

| Token | Value | Use |
| ----- | ----- | --- |
| Canvas | `#f4f4f4` | Wallpaper, page background |
| Paper | `#f9fafb` | Cards, inputs, panels |
| Ink | `#141414` | Type, strokes, active fills |
| Muted | `#6b7280` | Secondary type, dates |
| Chip | `#e5e7eb` | Hover wash, tracks, idle wells |
| White | `#ffffff` | Nested fields only |
| Dark card | `#131212` | Inverted panels (e.g. nested terminal) |
| Overlay | `rgba(18,18,18,0.05)` | Grid lines |

---

## Shape tokens

| Token | Value |
| ----- | ----- |
| Stroke | `2px solid #141414` |
| Stroke dashed | `2px dashed #141414` (idle pills, splitters) |
| Shadow | `0 4px 0 0 #141414` |
| Radius pill | `32px` |
| Radius card | `16–20px` |
| Radius chip | `9999px` |
| Grid | `28px` graph paper, `rgba(20,20,20,0.045)` |

---

## Typography

**Inter** throughout.

- UI labels: Inter Semi Bold, 11–12px, uppercase, tracking `0.08em`
- Body: 16px regular
- Display: large sizes with negative tracking where appropriate

---

## Control states

- **Active:** ink fill, paper text
- **Idle:** paper fill, dashed ink stroke
- **Hover:** chip fill, still 2px ink stroke

---

## Surfaces

**Wallpaper.** Canvas with a faint 28px grid.

**Pills.** 32px radius, 2px stroke. Buttons, inputs, compact actions.

**Cards.** Paper fill, 2px ink border, 16–20px radius, optional 4px offset shadow. Widgets (clock, todo, now-playing) are cards.

**Launcher.** Paper card: paper fill, 2px ink stroke, 16–20px radius, Inter chrome. The nested terminal is the only dark-card (`#131212`) surface — an inverted panel inside paper, not a second theme. Chrome lives in `desktop/index.html` (`template.action-bar`); the host engine binds required ids.

---

## Single-file document

Widgets, spawnables, and the action bar share one stylesheet in `Documents\Widgets\desktop\index.html`. Preview:

| Query | Surface |
| --- | --- |
| (none) or `?m=desktop` | Desktop + widgets |
| `?m=widgets` | Widget cards, no wallpaper chrome |
| `?m=spawn` / `?m=spawn&id=calculator` | Spawnable templates |
| `?m=action_bar` | Action bar chrome |

Do not iframe per-widget HTML files. Put tiles in `article.widget` and overlays in `template.spawnable`.

---

## Do

- Keep widgets and desktop chrome on paper/canvas tokens
- Use hard offset shadows, not soft blur
- Reserve dark-card for nested terminal content only

## Don't

- Do not add a second theme switcher or alternate visual language
- Do not use serif display, canary yellow chips, 1px hairlines, or 6–8px rectangles as the system look
- Do not introduce gradients (except the dark-card inner fade where already used)
- Do not glass or blur launcher chrome — paper for the bar; nested terminal may stay dark-card

---

## CSS variables (reference)

```css
:root {
  --canvas: #f4f4f4;
  --paper: #f9fafb;
  --ink: #141414;
  --muted: #6b7280;
  --chip: #e5e7eb;
  --white: #ffffff;
  --dark-card: #131212;
  --stroke: 2px solid #141414;
  --stroke-dashed: 2px dashed #141414;
  --shadow: 0 4px 0 0 #141414;
  --radius-pill: 32px;
  --radius-card: 16px;
  --radius-chip: 9999px;
}
```
