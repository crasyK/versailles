# Cal.com 2024 — Versailles design rulebook

Cal.com 2024 is the **only** visual language for Versailles: desktop wallpaper, widgets, and native Windows chrome (taskbar, Start, notification center, Explorer, Settings). Aetherfield / SaaS is archived in `design/kimi-handoff/DESIGN-BRIEF.md` and must not ship.

Source: [cal.com 2024 Awwwards Figma](https://www.figma.com/design/rMNeQYt5PqqHY1gOW0Pa50/Top-16-Websites-of-2024---Awwwards--Community-?node-id=27-6378) (`cal.com` frame `27:6378`).

Intent: monochrome, high-contrast, neo-brutalist SaaS. Paper canvas, 2px ink strokes, 32px pills, 4px hard offset shadows. No glass. No gradients except the dark-card inner fade.

--------------------------------------------------------------------------------

## Tokens

| Token | Value | Use |
| ----- | ----- | --- |
| Canvas | `#f4f4f4` | Wallpaper, taskbar transparent fill, Explorer body |
| Paper | `#f9fafb` | Cards, inputs, Start/NC/tray islands |
| Ink | `#141414` | Type, strokes, active fills, running indicators |
| Muted | `#6b7280` | Secondary type, dates |
| Chip | `#e5e7eb` | Hover wash, tracks, idle wells |
| White | `#ffffff` | Nested fields only |
| Dark card | `#131212` | Rare inverted panels (not the default desktop) |
| Overlay | `rgba(18,18,18,0.05)` | Grid lines |

| Shape | Value |
| ----- | ----- |
| Stroke | `2px solid #141414` |
| Stroke dashed | `2px dashed #141414` (idle pills, splitters) |
| Shadow | `0 4px 0 0 #141414` |
| Radius pill | `32px` |
| Radius card | `16–20px` (product cards up to `38px`) |
| Radius chip | `9999px` |
| Grid | `28px` graph paper, `rgba(20,20,20,0.045)` |

Type: **Inter**. Display can go huge with negative tracking. UI labels: Inter Semi Bold, 11–12px, uppercase, tracking `0.08em`. Body 16px regular. Product meta may use Roboto Medium 13–14px.

Active control = ink fill + paper text. Idle = paper fill + dashed ink stroke. Hover = chip fill, still 2px ink stroke.

--------------------------------------------------------------------------------

## Component language

**Wallpaper.** Canvas + faint grid. The native top taskbar is transparent so this grid continues under the pills.

**Pills.** 32px radius, 2px stroke. Used for taskbar app buttons, Start, tray island, HUD-style actions, inputs.

**Cards.** Paper, 2px ink, 16–20px radius, optional 4px offset shadow. Widgets (clock, todo, now-playing) are cards. Start menu, Action Center, toasts, Explorer overflow menus are cards.

**No second bar.** Deck does not draw a HUD. Chrome is the Windows taskbar (Windhawk Cal YAML). `calc` / `calendar` / `notes` open from Alt+Space. Clock widget is the clock; tray clock is Windows.

**Action Bar.** Alt+Space is a paper card (`src/launcher.css`): paper fill, 2px ink stroke, 16–20px radius, Inter chrome. The nested xterm is the only dark-card (`#131212`) surface — inverted panel inside paper, not glass, not a second theme.

--------------------------------------------------------------------------------

## Files

| Surface | File |
| ------- | ---- |
| Desktop page | `Documents/Widgets/desktop/index.html` |
| Desktop shell | `app/desktop.html`, `app/src/desktop.css` |
| Action Bar | `app/launcher.html`, `app/src/launcher.css` |
| Widgets | `Documents/Widgets/{clock,todo,now-playing}/index.html` |
| Anywhere strip | `app/anywhere.html` (optional; off when desktop is on) |
| Taskbar YAML | `design/kimi-handoff/windhawk-cal-taskbar.yaml` |
| Start YAML | `design/kimi-handoff/windhawk-cal-start-menu.yaml` |
| Notification YAML | `design/kimi-handoff/windhawk-cal-notification-center.yaml` |
| Explorer YAML | `design/kimi-handoff/windhawk-cal-explorer.yaml` |
| Settings YAML | `design/kimi-handoff/windhawk-cal-settings.yaml` |
| Agent rule | `.cursor/rules/cal-com-design.mdc` |

Paste each YAML in Windhawk **Textual mode** with `theme: ""` so built-in themes do not override Cal.

--------------------------------------------------------------------------------

## Unifying the six stylers into Deck

Windhawk splits one look across six mods because each hooks a different process (or a different XAML tree in `explorer.exe`). Deck wants **one Cal token sheet** applied everywhere.

### Why not six pastes forever

- **One XAML diagnostics consumer per process.** Taskbar Styler and File Explorer Styler both inject into `explorer.exe`. They fight. A Deck mod must style taskbar + Explorer from a **single** explorer consumer, or sequence them.
- **Tokens drift.** Six YAML files copy `paper` / `ink` / `pill` by hand.
- **Missing Cal primitives** are not in any styler (see below).

### Target architecture (later)

One Windhawk mod (working name **Deck Shell Styler**) with:

1. Shared `styleConstants` (this rulebook's tokens).
2. Per-surface style packs: `taskbar`, `start`, `search`, `notification`, `explorer`, `settings`, `taskbarBackground`.
3. Process map: `explorer.exe`, `StartMenuExperienceHost.exe`, `SearchHost.exe`, `ShellExperienceHost.exe`, `ShellHost.exe`, `SystemSettings.exe`.
4. Companion knobs that today live in other mods: height/icon size, labels, tray tweaks, background-when-maximized.

Until that exists, ship the five Cal YAML files plus Taskbar Background Helper settings below.

### Options the catalog is missing (build these)

| Gap | Why Cal needs it | Today |
| --- | ---------------- | ----- |
| App-icon desaturation | Color logos break the ink/paper field | `OpacityMask` is a no-op on raster `Image#Icon`. No catalog mod. Needs an icon-load hook (same layer as `taskbar-icon-size`) or label-only pills |
| Dashed 2px borders | Idle Cal pills | XAML `Border` has no `StrokeDashArray`. Idle state is solid 2px + paper/chip, not true dashes |
| Hard offset shadow | `0 4px 0 0 #141414` | Stylers expose `Shadow:=` (clear) and ThemeShadow. No 4px ink offset |
| Inter in the shell | UI labels | Shell stays Segoe. Font injection is out of scope for control styles |
| Shared tokens across processes | One Cal | Copy-paste YAML |
| Transparent bar + maximized fill | Grid shows through idle; paper when an app eats the desktop | **Taskbar Background Helper** — use `whenMaximized` with canvas `#f4f4f4`, not acrylic |
| Work-area-aware widgets | Desktop page is fullscreen under the bar | Padding `72px` top is a hack; an AppBar would be correct |
| Single explorer diagnostics | Taskbar + Explorer together | Conflict; unify or pick one consumer |
| `TintSaturation=0` on icons | WindhawkBlur can desaturate **blurred backgrounds**, not `Image#Icon` | Do not confuse this with grayscale app icons |

Steal **layout** from Pills / DockLike / CleanSlate / Minimal Explorer. Do not steal glass, mica, accent rainbows, or nostalgia skins.

--------------------------------------------------------------------------------

## Taskbar Background Helper

Not a theme pack — a fill behind Taskbar Styler's transparent regions.

- **Always:** paints the bar even over wallpaper. Fights Cal (we want the grid through the bar).
- **When maximized:** paints only when a window covers the desktop. Use this. Color `#f4f4f4` (canvas), no blur.

Also works around Taskbar Styler's [single-taskbar limitation](https://github.com/ramensoftware/windhawk-mods/issues/742).

--------------------------------------------------------------------------------

## Catalog — every contributed theme

Sources: the official styling guides. Screenshots live in those repos. **Keep** = closest to Cal (steal structure). **Reject** = glass, dark, nostalgia, accent-heavy.

### 1. Windows 11 Taskbar Styler 1.8

Guide: [windows-11-taskbar-styling-guide](https://github.com/ramensoftware/windows-11-taskbar-styling-guide)

| Theme | Read |
| ----- | ---- |
| TranslucentTaskbar | Reject — acrylic |
| DockLike | Keep layout — floating cluster, transparent bar |
| SimplyTransparent | Keep — bar fill off |
| Squircle | Reject — squircles, not 32px pills |
| Matter | Reject — accent acrylic |
| WinXP / WinVista / Windows7 | Reject — nostalgia |
| Bubbles | Reject |
| RosePine | Reject — palette |
| CleanSlate | Keep — less chrome |
| Lucent | Reject — glow |
| SunValley / SunValley (Legacy) | Reject — stock Fluent |
| BottomDensy | Maybe density, not look |
| TaskbarXII | Reject |
| xdark | Reject — dark |
| Aeris / Plasma | Reject |
| WindowGlass / TintedGlass / FluentGlass / FrostyGlass / FrostedAcrylic / LayerMicaUI / LiquidGlass / LiquidGlass (Legacy) / OS26 Liquid Glass | Reject — glass |
| Surface | Reject |
| Oversimplified&Accentuated | Reject — accent |
| Luminosity / Fluid | Reject |
| TaskbarToStatusbar | Reject — wrong metaphor |
| UltraWideFriendly | Layout only |
| Borderless | Keep — no outer bar chrome |
| Command Center | Reject — dashboard |
| Pills | **Keep** — pill buttons, closest cousin |

### 2. Windows 11 Start Menu Styler 1.7

Guide: [windows-11-start-menu-styling-guide](https://github.com/ramensoftware/windows-11-start-menu-styling-guide)

| Theme | Read |
| ----- | ---- |
| TranslucentStartMenu | Reject — acrylic |
| NoRecommendedSection | Keep behavior — hide Recommended |
| SideBySide / SideBySide2 / SideBySideMinimal | Keep layout — denser pinned grid |
| Down Aero | Reject |
| Windows10 / Windows11_Metro10 / Windows11_Metro10Minimal / Windows10X | Reject — Metro/10 |
| Fluent2Inspired / LegacyFluent / SunValley / SunValley (Legacy) | Reject — Fluent |
| RosePine / Everblush | Reject — palettes |
| UniMenu / OnlySearch / FullScreen | Layout experiments |
| WindowGlass / Fluid / TintedGlass / LayerMicaUI / LiquidGlass / LiquidGlass (Legacy) / FrostyGlass | Reject — glass |
| Oversimplified&Accentuated | Reject |
| Borderless | Keep — no acrylic overlay |
| Command Center | Reject |
| ModernStartMenu / Hybrid | Win10 Start only (manual import) |

### 3. Windows 11 Notification Center Styler 1.6

Guide: [windows-11-notification-center-styling-guide](https://github.com/ramensoftware/windows-11-notification-center-styling-guide)

| Theme | Read |
| ----- | ---- |
| TranslucentShell | Reject — blur |
| Matter | Reject |
| Unified | Keep — one radius language |
| 10JumpLists | Reject — Win10 |
| WindowGlass / TintedGlass / Fluid / LiquidGlass / LayerMicaUI / FrostyGlass | Reject — glass |
| Oversimplified&Accentuated | Reject |
| BetterControl11 | Maybe density |
| Borderless | Keep |
| Densy | Keep density, restyle to paper |

Useful style examples (not themes): hide Focus Assist, hide NC, hide pips, hide footer, hide media controls, `Shadow:=`, `CornerRadius=16` (not 0 — Cal wants 16, not square).

### 4. Windows 11 File Explorer Styler 1.5

Guide: [windows-11-file-explorer-styling-guide](https://github.com/ramensoftware/windows-11-file-explorer-styling-guide)

| Theme | Read |
| ----- | ---- |
| Translucent Explorer11 | Reject |
| MicaBar | Reject — mica |
| NoCommandBar | Keep behavior if we want a quieter frame |
| Minimal Explorer11 | Keep — hide chrome |
| Tabless / MicaTabless | Layout |
| Matter / WindowGlass / TintedGlass / LiquidGlass / OS26 Liquid Glass | Reject |
| AddressSearchOnly | Keep — address + search, no extra bars |
| ZEUSosX_044 | Reject — macOS clone |
| Compact Explorer11 | Density |
| Float | Maybe — floating frame, restyle to paper |

Mod extra: `backgroundTranslucentEffect` should be **none** for Cal.

### 5. Taskbar Background Helper 1.2

No theme gallery. Modes: always vs maximized; color behind transparent Styler regions. Cal: maximized + `#f4f4f4`.

### 6. Windows 11 Settings Styler 1.1

Guide: [windows-11-settings-styling-guide](https://github.com/ramensoftware/windows-11-settings-styling-guide)

| Theme | Read |
| ----- | ---- |
| Densy | Keep density |
| ClassicSearchBar | Layout |
| StoreFrame11 | Reject — Store chrome |
| Blue | Reject — hue |
| Translucent Settings11 | Reject |
| WindowGlass | Reject |
| OLED | Reject — dark |

--------------------------------------------------------------------------------

## Cross-theme names

These labels recur across stylers. Treat them as **families**, not Cal:

- **Glass / Liquid / Frost / Mica / Translucent / Acrylic** — opposite of paper.
- **Pills / DockLike / Borderless / CleanSlate / Minimal** — steal geometry, repaint ink/paper.
- **Matter / Oversimplified&Accentuated / RosePine / Everblush / Blue** — foreign palettes.
- **WinXP / Vista / 7 / 10 / SunValley / Metro** — other OS eras.
- **Densy / Compact / UltraWide** — density, not color.
- **Command Center** — information dashboard, not a booking-SaaS shell.

--------------------------------------------------------------------------------

## Visual QA

A screen is Cal if: paper/canvas field, 2px ink cages, 32px pills or 16px cards, Inter, no blur, no yellow chip, no serif clock.

A screen is wrong if: sky→cream gradient, 1px hairline, 6–8px rectangles, soft shadow, acrylic, mica, colored app-icon field with no ink cages, a second “deck” HUD above the native taskbar.
