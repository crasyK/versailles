# Contributing

Versailles is the HTML desktop runtime. User tiles live under `Documents\Widgets`, not in this repository.

## Rules

1. Paper theme only. Read [`design/PAPER-THEME.md`](design/PAPER-THEME.md) and [`.cursor/rules/paper-theme.mdc`](.cursor/rules/paper-theme.mdc).
2. Single-file dialect. Read [`docs/DIALECT.md`](docs/DIALECT.md). Do not iframe widgets.
3. Keep `scripts/verify-acl.mjs` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together.
4. Do not add a settings UI to the action bar. Config is `#versailles` in `desktop/index.html` plus `.versailles/` runtime.
5. Do not introduce a second theme.

## Dev

```bash
npm install
npm run tauri:dev
```

## PR

Branch from `main`, open a pull request against `main`. Title the work after the change, not after the product name.
