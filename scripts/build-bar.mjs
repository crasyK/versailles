#!/usr/bin/env node
/**
 * Bundle command-bar engine (launcher.ts + engine/*) into IIFE for desktop/index.html.
 * Only replaces content between <!-- BAR_ENGINE_START --> and <!-- BAR_ENGINE_END -->.
 * Never deletes the action-bar DOM (#cli-root).
 */
import * as esbuild from "esbuild";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const START = "<!-- BAR_ENGINE_START -->";
const END = "<!-- BAR_ENGINE_END -->";

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

const userDesktop = join(root, "..", "desktop", "index.html");
try {
  let html = readFileSync(userDesktop, "utf8");
  if (!html.includes('id="cli-root"')) {
    throw new Error("refusing to inject: #cli-root missing from action-bar template");
  }
  const start = html.indexOf(START);
  const end = html.indexOf(END);
  if (start < 0 || end < 0 || end <= start) {
    throw new Error("BAR_ENGINE_START/END markers missing — fix index.html first");
  }
  html =
    html.slice(0, start + START.length) +
    "\n" +
    bundle +
    "\n" +
    html.slice(end);
  writeFileSync(userDesktop, html, "utf8");
  console.log("injected engine into", userDesktop, html.length, "chars");
} catch (e) {
  console.warn("user desktop inject skipped:", e.message);
  process.exitCode = 1;
}
