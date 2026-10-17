#!/usr/bin/env node
/**
 * Bundle the example command-bar engine (src/launcher.ts + engine/*) into
 * templates/starter/desktop/bar.js.
 *
 *   npm run build:bar               → template only
 *   npm run build:bar -- --install  → also copy to %USERPROFILE%\Documents\Widgets\desktop\bar.js
 *
 * Wallpaper HTML must load this file from the action-bar spawn template only.
 */
import * as esbuild from "esbuild";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const install = process.argv.slice(2).includes("--install");

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
    "@xterm/xterm": join(root, "scripts/bar-shims/xterm.js"),
    "@xterm/addon-fit": join(root, "scripts/bar-shims/fit.js"),
    "@xterm/addon-web-links": join(root, "scripts/bar-shims/web-links.js"),
    "@xterm/addon-search": join(root, "scripts/bar-shims/search.js"),
    "@xterm/xterm/css/xterm.css": join(root, "scripts/bar-shims/empty.css"),
  },
});

const bundle = result.outputFiles[0].text;

const outStarter = join(root, "templates/starter/desktop/bar.js");
writeFileSync(outStarter, bundle, "utf8");
console.log("wrote", outStarter, bundle.length, "bytes");

if (install) {
  const home = process.env.USERPROFILE || homedir();
  const liveDesktop = join(home, "Documents", "Widgets", "desktop");
  const userBar = join(liveDesktop, "bar.js");
  try {
    if (!existsSync(join(liveDesktop, "index.html"))) {
      throw new Error(`no live desktop at ${liveDesktop} (copy templates/starter first)`);
    }
    mkdirSync(liveDesktop, { recursive: true });
    writeFileSync(userBar, bundle, "utf8");
    console.log("installed", userBar, bundle.length, "bytes");
  } catch (e) {
    console.warn("install skipped:", e.message);
    process.exitCode = 1;
  }
} else {
  console.log("live desktop untouched (pass --install to copy bar.js there)");
}
