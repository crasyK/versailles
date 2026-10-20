# Contributing

Versailles is the HTML desktop runtime. User tiles live under `Documents\Widgets`, not in this repository.

## Rules

1. Paper is the look of the host's own chrome and the starter template ([`design/PAPER-THEME.md`](design/PAPER-THEME.md), [`.cursor/rules/paper-theme.mdc`](.cursor/rules/paper-theme.mdc)). User pages can look like anything; the host never styles them.
2. Single-file dialect. Read [`docs/DIALECT.md`](docs/DIALECT.md). Do not iframe widgets.
3. Keep `scripts/verify-acl.mjs` green. A new Tauri command needs `lib.rs`, `build.rs`, and `capabilities/default.json` together.
4. Do not add a settings UI to the action bar. Config is `#versailles` in `desktop/index.html` plus `.versailles/` runtime.
5. Do not ship a second built-in theme.

## Dev

```bash
npm install
npm run tauri:dev
```

## PR

Branch from `main`, open a pull request against `main`. Title the work after the change, not after the product name.

## Release

1. Bump the version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`.
2. If the SDK changed, bump its first line (`/*! versailles-sdk x.y.z */`) and `SDK_VERSION`, and add a line to `sdk::SHIPPED` (the `sdk` test prints the hash).
3. Add the release to [`CHANGELOG.md`](CHANGELOG.md), newest first.
4. One release commit (`Ship x.y.z: …`, with `closes #n` for each issue), tag `vx.y.z`, and attach the NSIS installer to the GitHub release.
