/**
 * Guard against the recurring "cli_exec not allowed" regression.
 * Every command in lib.rs invoke_handler must appear in build.rs AND capabilities.
 */
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(root, rel), "utf8");

const lib = read("src-tauri/src/lib.rs");
const build = read("src-tauri/build.rs");
const caps = JSON.parse(read("src-tauri/capabilities/default.json"));

const handlerBlock = lib.match(/generate_handler!\[\s*([\s\S]*?)\]/)?.[1] ?? "";
const registered = [...handlerBlock.matchAll(/(?:commands|cli|pty|apps|desktop|engine_runtime)::(\w+)/g)].map((m) => m[1]);

const buildBlock = build.match(/\.commands\(\&\[([\s\S]*?)\]\)/)?.[1] ?? "";
const manifest = [...buildBlock.matchAll(/"(\w+)"/g)].map((m) => m[1]);

const permissions = caps.permissions.filter((p) => p.startsWith("allow-"));

const toKebab = (s) => s.replace(/_/g, "-");
const permFor = (cmd) => `allow-${toKebab(cmd)}`;

const missing = [];
for (const cmd of registered) {
  if (!manifest.includes(cmd)) missing.push(`build.rs missing "${cmd}"`);
  if (!permissions.includes(permFor(cmd))) {
    missing.push(`capabilities/default.json missing "${permFor(cmd)}"`);
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

console.log(`ACL OK (${registered.length} commands)`);
