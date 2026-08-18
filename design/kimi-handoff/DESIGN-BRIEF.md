# Deck desktop — design handoff for Kimi

**Canonical language:** [CAL-COM-RULEBOOK.md](../CAL-COM-RULEBOOK.md). Cal.com 2024 is the only live theme. SaaS / Aetherfield below is archive.

Restyle the **Deck HTML desktop** (`Documents\Widgets\desktop\index.html` plus the HUD in `app/desktop.html` / `app/src/desktop.css`) into **two looks**, with a **theme switch in the top bar**.

Do not clone a marketing landing page. Keep the same product: fullscreen desktop, HUD, clock / todo / now-playing widgets. Translate the *visual language* of the two references onto that shell.

---

## Product to restyle (current, disliked)

File: `screenshots/00-current-desktop.png`

- Dark charcoal wallpaper (`#0b0d12`) with teal/blue radial glows
- Thin 36px HUD: green “deck” wordmark, now-playing text, text links (`search start calc cal notes`), clock
- Three floating dark glass cards: clock (top-right), todo (bottom-left), now-playing (bottom-right)
- Heavy empty center, bulky rounded boxes, low-contrast glass

Keep these **functions**:
- Brand / launcher (`deck`)
- Now-playing title + play/pause
- `search`, `start`, `calc`, `cal`, `notes`
- Clock (opens calendar)
- Widget iframes: `/files/clock`, `/files/todo`, `/files/now-playing`

Add: a **theme switcher** in the HUD (Cal / SaaS). Persist the choice (`localStorage` is fine).

---

## Source files

### Theme A — cal.com 2024 (Figma Design, MCP-readable)

https://www.figma.com/design/rMNeQYt5PqqHY1gOW0Pa50/Top-16-Websites-of-2024---Awwwards--Community-?node-id=27-6378

| Frame | Node | Role |
|---|---|---|
| `cal.com` | `27:6378` | Full 1440×7960 page |
| Header | `27:7695` | Nav |
| Hero | `27:6381` | Type + product card + claim CTA |
| Dark value | `27:6502` | Charcoal grid card |
| Bento | `27:6561` | Feature grid |
| Integrations | `27:7055` | App-icon field |
| Adaptive pills | `27:7353` | Tab pills + dashed cards |
| Quote | `27:7436` | Large centered quote |
| Wall of love | `27:7448` | Testimonial cards |
| Final CTA | `27:7577` | Repeat claim CTA |
| Footer | `27:7645` | Dark rounded footer |
No Figma variables on this file (empty defs). Tokens below are extracted from `get_design_context`.

### Theme B — Modern, Clean SaaS Company / Aetherfield (Figma Sites)

https://www.figma.com/site/5iFk7NYQ14bL67RJNr3MPs/Modern--Clean-SaaS-Company--Community-?node-id=0-1  
Community listing: https://www.figma.com/community/file/1573023525218599861/modern-clean-saas-company

This is a **Figma Sites** file (`/site/…`), not a Design file. Figma MCP cannot read it without edit access on a duplicated copy in the user’s account (`nedimark@gmail.com`). Tokens below are taken from community preview slides plus the user’s full-page screenshots of the same template (branded **Aetherfield**).

---

## Theme A — Cal.com 2024

**Intent:** monochrome, high-contrast, neo-brutalist SaaS. Paper canvas, 2px black strokes, 32px pills, 4px hard offset shadows. No glass, no gradients except the dark-card inner fade.

### Screenshots

- `01-cal-header.png` — logo left, empty white pill center, black LOGIN pill right
- `02-cal-hero.png` — huge type, outlined “everyone.”, booking card, claim-username bar
- `03-cal-dark-value.png` — `#131212` rounded card + faint grid + white/muted step list
- `04-cal-bento.png` — bordered product-UI cards, ticker of features
- `05-cal-integrations.png` — faded icon field + white pill overlay
- `06-cal-adaptive-pills.png` — **use this for the HUD switcher**: solid black active pill vs dashed inactive pills
- `07-cal-quote.png` — giant centered quote + chevrons
- `08-cal-wall-of-love.png` — bordered tweet-style cards
- `09-cal-final-cta.png` — repeat claim bar
- `10-cal-footer.png` — `#141414` rounded 32px footer, white Inter columns

### Tokens (from Figma code)

```
Canvas          #f4f4f4
Paper / input   #f9fafb
Ink             #141414
Ink muted       #6b7280 / #888 / #494949
Hairline        #e5e7eb / #e1e1e1
Fill chip       #e5e7eb
Dark card       #131212
Dark fade       #232426 → transparent
White           #f9fafb / #ffffff
NEW badge       #111111
Overlay wash    rgba(18,18,18,0.05)

Radius pill     32px (buttons, inputs, header search, LOGIN)
Radius card     16px–38px (product cards ~32–38, inner cards 16)
Radius chip     4px (calendar day)
Radius full     9999px (NEW badge, avatars)

Stroke          2px solid #141414
Stroke dashed   2px dashed #141414  (inactive pills, splitters)
Shadow hard     0 4px 0 0 #141414   (CTAs, claim bar)
Grid overlay    faint light-gray graph paper on dark + light sections

Type display    Inter Regular, ~87–103px, tracking -0.2 to -1.35px
Type body       Inter Regular 16–22px
Type UI         Inter Semi Bold 16–17px, UPPERCASE, line-height ~18px
Type meta       Roboto Medium 13–14px inside product UI
```

Header (node `27:7695`): height 102px, canvas `#f4f4f4`, Cal.com wordmark 109×23 left @ 34px, white pill with 2px `#141414` stroke centered, LOGIN = `#141414` fill, 32px radius, Inter Semi Bold 17px uppercase `#f9fafb`.

Hero claim bar: 90px tall, 32px radius, 2px black stroke, `#e1e1e1` prefix cell with dashed right divider, `#f9fafb` field, sibling CLAIM USERNAME pill with the same 4px offset shadow.

Outlined “everyone.” in the Figma dump came through as `#f9fafb` fill on `#f4f4f4` (looks hollow). Recreate as **stroke-only / outlined type**, not a second color.

### How to map onto Deck (Theme A)

- Wallpaper: `#f4f4f4` + very faint grid (same language as `03` / `06`)
- HUD: paper bar, 2px bottom hairline or floating 32px pill cluster
- Actions: dashed 2px pills; **active / hover = solid `#141414` + `#f9fafb` text** (copy `06-cal-adaptive-pills.png`)
- Theme switch: two pills `CAL` / `SAAS` in that same active/dashed pair
- Widgets: white / `#f9fafb` cards, 2px `#141414` stroke, 16–32px radius, **no glass**. Optional 4px offset shadow
- Clock: huge Inter tabular numerals; seconds as a small black chip, not orange
- Todo / media: same bordered-card language as the Rick Astley booking widget

---

## Theme B — Modern Clean SaaS (Aetherfield)

**Intent:** editorial light SaaS. Soft sky-blue → cream canvas, serif display type, sans UI, black rectangular CTAs, white dashboard cards, one hard yellow accent. Opposite of Cal’s paper-brutalism: no 32px pills, no 4px offset shadows, no 2px black cages.

### Screenshots

- `12-aetherfield-responsive.png` — full landing: desktop / tablet / mobile (best overall map)
- `13-aetherfield-hero.png` — hero + dashboard mock close-up
- `20-saas-community-hero.png` — community preview of the same hero
- `21-saas-slide-3.png` — “Everything you need…” + 001–004 feature list + floating Forecast card
- `22-saas-slide-4.png` — mobile card grid, yellow “Aetherfield” CTA card, journal cards, duotone photos

### Tokens (from previews, not MCP)

```
Canvas gradient   pale sky blue (top) → warm cream / peach (bottom)
Paper             #ffffff
Ink               #111111 / near-black
Ink muted         warm gray, not Cal #6b7280-on-paper
Accent yellow     canary / #F5D000-ish solid square (metrics, Forecast tag, footer)
Accent green      forest / chart positive (energy trend)
Duotone photos    blue-tinted photography (team, portrait)
Hairline          1px black or very light gray — thinner than Cal’s 2px
Radius buttons    ~6–8px rectangles (NOT 32px pills)
Radius cards      ~12–16px
Radius dashboard  large rounded white panel, optional 1px black stroke
Type display      high-contrast serif (headlines, clock time)
Type UI / body    clean sans (nav, labels, buttons, todo)
CTA primary       solid black rectangle, white label, optional white square bullet
CTA secondary     ghost: white/transparent fill, thin black border
Nav               wordmark left; links + “Get started →” right; no center search pill
```

Hero HUD analogue: logo left, text links right, `Get started →` as the only filled action. Headline is centered serif; subcopy is lighter sans. Dashboard card: “Good morning, …”, metric tiles (56% + yellow bar, 583.7 MWh, Forecast chip).

Features analogue: numbered `001 Track / 002 Model / 003 Report / 004 Act`, floating white insight card over photography.

Do **not** put the neon-yellow full-bleed footer on the desktop. Steal yellow only as a small chip/square.

### How to map onto Deck (Theme B)

- Wallpaper: blue→cream vertical gradient (no teal glow, no charcoal)
- HUD: airy transparent/white bar, serif or bold sans “deck”, ghost links, one black rectangle CTA
- Theme switch: two **rectangles** (filled black vs 1px outline) — not Cal pills
- Clock widget: serif time, muted sans date, optional yellow seconds chip
- Todo / media: white 12–16px-radius cards, 1px hairline, generous padding, yellow used once (count pill or progress)
- Contrast check: if it looks like Cal (pills, 2px stroke, offset shadow, `#f4f4f4` gray), it is the wrong theme

---

## Top-bar switch (required)

Put it in the HUD, always visible, both themes:

```
[ deck ] [ now playing … ]          [ Cal | SaaS ]  search  start  calc  cal  notes  17:08
```

- Theme A: two 32px-radius pills, 2px `#141414` stroke; active = filled black
- Theme B: two rectangular buttons; active = filled black, idle = 1px outline
- Switching must restyle HUD **and** the desktop page (wallpaper + widget chrome). If widgets are iframes, pass a query/class or postMessage so clock/todo/now-playing pick up the theme.

---

## Constraints

- Stack is Tauri v2 + Vite + vanilla HTML/CSS/TS. No Tailwind unless asked.
- Desktop page lives at `C:\Users\NediM\Documents\Widgets\desktop\index.html` (loaded in an iframe by `desktop.html`).
- HUD currently exists in **both** `desktop.html` and the user page — match whichever is actually on screen; don’t leave one theme on the shell and another on the page.
- Keep German date formatting if the clock widget already uses it.
- Do not add marketing copy, fake testimonials, or a full landing page.

---

## Suggested implementation order

1. CSS variables for Theme A and Theme B on `:root` / `[data-theme="cal"]` / `[data-theme="saas"]`
2. HUD switcher + persistence
3. Restyle wallpaper + HUD
4. Restyle widget iframe chrome (and inner widget CSS if needed)
5. Visual QA: Theme A against `02` + `06`; Theme B against `13` + `12`
