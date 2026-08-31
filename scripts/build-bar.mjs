#!/usr/bin/env node
/**
 * Bundle command-bar engine (launcher.ts + engine/*) into desktop/bar.js.
 * Wallpaper HTML must load this file from the action-bar spawn template only.
 */
import * as esbuild from "esbuild";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const result = await esbuild.build({
  entryPoints: [join(root, "src/launcher.ts")],
  bundle: true,
  write: false,
  format: "iife",
  platform: "browser",
  target: "es2022",
  loader: { ".css": "empty" },
  alias: {
    "@tauri-apps/api/core": join(root, "scripts/bar-shims/core.js"),
    "@tauri-apps/api/event": join(root, "scripts/bar-shims/event.js"),
    "@tauri-apps/api/dpi": join(root, "scripts/bar-shims/dpi.js"),
    "@tauri-apps/api/window": join(root, "scripts/bar-shims/window.js"),
    xterm: join(root, "scripts/bar-shims/xterm.js"),
    "@xterm/addon-fit": join(root, "scripts/bar-shims/fit.js"),
    "@xterm/addon-webgl": join(root, "scripts/bar-shims/webgl.js"),
    "@xterm/addon-web-links": join(root, "scripts/bar-shims/web-links.js"),
    "@xterm/addon-search": join(root, "scripts/bar-shims/search.js"),
    "xterm/css/xterm.css": join(root, "scripts/bar-shims/empty.css"),
  },
});

const bundle = result.outputFiles[0].text;

const outStarter = join(root, "templates/starter/desktop/bar.js");
writeFileSync(outStarter, bundle, "utf8");
console.log("wrote", outStarter, bundle.length, "bytes");

const userBar = join(root, "..", "desktop", "bar.js");
try {
  mkdirSync(dirname(userBar), { recursive: true });
  writeFileSync(userBar, bundle, "utf8");
  console.log("wrote", userBar, bundle.length, "bytes");
} catch (e) {
  console.warn("user desktop bar.js skipped:", e.message);
  process.exitCode = 1;
}
