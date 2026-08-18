# Contributing

Versailles is the HTML desktop runtime. Deck (`Documents\Widgets\app`) is a separate product — do not “bring back” manager, canvas, anywhere bar, or floating widget windows here.

## Rules

1. Cal.com 2024 paper only. Read `design/CAL-COM-RULEBOOK.md` and `.cursor/rules/cal-com-design.mdc`.
2. Widgets are HTML in `Documents\Widgets`. This repo is the host.
3. Keep `scripts/verify-acl.mjs` green. Adding a Tauri command means `lib.rs`, `build.rs`, and `capabilities/default.json` together.
4. No Bearer control API. File server + weather/quote/media only.
5. Do not introduce a second theme.

## Dev

```bat
npm install
npm run tauri:dev
```

## PR

Branch from `main`, open a pull request against `main`. Title the work after the change, not after the product name (the repo is already Versailles).
