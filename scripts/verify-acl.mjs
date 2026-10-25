/**
 * Guard against the recurring "cli_exec not allowed" regression.
 * Every command in lib.rs invoke_handler must appear in build.rs AND in a
 * capability. Pages get `capabilities/default.json`; a host window with a
 * capability of its own (`welcome.json`: the first-run window) keeps its
 * commands to itself: default.json must not grant them too. Every
 * capability file must be listed in tauri.conf.json.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(root, rel), "utf8");

const lib = read("src-tauri/src/lib.rs");
const build = read("src-tauri/build.rs");
const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const capDir = "src-tauri/capabilities";
const caps = readdirSync(join(root, capDir))
  .filter((f) => f.endsWith(".json"))
  .map((f) => ({ file: f, ...JSON.parse(read(`${capDir}/${f}`)) }));
const byId = Object.fromEntries(caps.map((c) => [c.identifier, c]));

const handlerBlock = lib.match(/generate_handler!\[\s*([\s\S]*?)\]/)?.[1] ?? "";
const registered = [...handlerBlock.matchAll(/\b\w+::(\w+)/g)].map((m) => m[1]);

const buildBlock = build.match(/\.commands\(\&\[([\s\S]*?)\]\)/)?.[1] ?? "";
const manifest = [...buildBlock.matchAll(/"(\w+)"/g)].map((m) => m[1]);

const toKebab = (s) => s.replace(/_/g, "-");
const permFor = (cmd) => `allow-${toKebab(cmd)}`;
const grants = (cap, cmd) => (cap?.permissions ?? []).includes(permFor(cmd));

const missing = [];

const listed = conf.app?.security?.capabilities ?? [];
for (const cap of caps) {
  if (!listed.includes(cap.identifier)) {
    missing.push(`tauri.conf.json app.security.capabilities missing "${cap.identifier}" (${cap.file})`);
  }
}
for (const id of listed) {
  if (typeof id === "string" && !byId[id]) missing.push(`tauri.conf.json lists unknown capability "${id}"`);
}

const pages = byId.default;
if (!pages) missing.push(`${capDir}/default.json missing`);
const own = caps.filter((c) => c.identifier !== "default");

for (const cmd of registered) {
  if (!manifest.includes(cmd)) missing.push(`build.rs missing "${cmd}"`);
  const holders = own.filter((c) => grants(c, cmd));
  if (holders.length && grants(pages, cmd)) {
    missing.push(
      `capabilities/default.json must not grant "${permFor(cmd)}" (it belongs to ${holders.map((c) => c.file).join(", ")})`,
    );
  }
  if (!holders.length && !grants(pages, cmd)) {
    missing.push(`capabilities/default.json missing "${permFor(cmd)}"`);
  }
}

// Permissions pages must never get (docs/SECURITY.md). Hook classes are
// enforced per calling window label: a page that could create a window
// could pick a spawn window's label (`widget-<id>`) and run exec hooks from
// the wallpaper. Raw global shortcuts would let any page take system-wide
// keys around the `hotkey` hook. The host does both from Rust.
const forbidden = [
  "core:window:allow-create",
  "core:webview:allow-create-webview",
  "core:webview:allow-create-webview-window",
  "global-shortcut:default",
  "global-shortcut:allow-register",
  "global-shortcut:allow-register-all",
  "global-shortcut:allow-unregister",
  "global-shortcut:allow-unregister-all",
  "global-shortcut:allow-is-registered",
];
for (const cap of caps) {
  for (const perm of cap.permissions ?? []) {
    const id = typeof perm === "string" ? perm : perm?.identifier;
    if (forbidden.includes(id)) {
      missing.push(`${capDir}/${cap.file} must not grant "${id}" (see docs/SECURITY.md)`);
    }
  }
}

const extras = manifest.filter((cmd) => !registered.includes(cmd));
for (const cmd of extras) {
  missing.push(`build.rs lists unused command "${cmd}"`);
}

if (missing.length) {
  console.error("ACL verification failed:\n" + missing.map((m) => `  - ${m}`).join("\n"));
  process.exit(1);
}

console.log(`ACL OK (${registered.length} commands, ${caps.length} capabilities)`);
