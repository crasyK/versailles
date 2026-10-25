// Checks the code in the user docs against the locked public surface
// (docs/api-surface.json): every SDK call, hook name, push / event name,
// dialect attribute and raw command in a fenced code block must exist, and
// none may be a deprecated name. Run: npm run verify:docs
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const surface = JSON.parse(readFileSync(join(root, "docs/api-surface.json"), "utf8"));

const sdk = new Set(surface.sdk.map((n) => n.replace(/^for\(\)\./, "")));
const deprecated = new Set(surface.sdkDeprecated);
const hooks = new Set(surface.hooks);
const pushNames = new Set([...surface.pushHooks, ...surface.pieceEvents]);
const attributes = new Set(surface.attributes);
const namespaces = new Set([...sdk].filter((n) => n.includes(".") && !n.startsWith("<")).map((n) => n.split(".")[0]));
const topLevel = new Set([...sdk].filter((n) => !n.includes(".")));
// Raw commands that are part of the contract (docs/API-REVIEW-1.0.md › Odd on purpose).
const commands = /^(sys_stats|list_catalog|catalog_icon|dismiss_launcher|monitor_info|get_spawnable_engine_context|pty_\w+|cli_\w+)$/;
const deprecatedEvents = /^(launcher:\/\/(shown|hidden)|overlay:\/\/id|media:\/\/update|clipboard:\/\/changed)$/;

const cookbook = join(root, "docs/cookbook");
const files = [
  "README.md",
  "COMPATIBILITY.md",
  "docs/GETTING-STARTED.md",
  "docs/HOOKS.md",
  "docs/DIALECT.md",
  ...(existsSync(cookbook) ? readdirSync(cookbook).filter((f) => f.endsWith(".md")).map((f) => `docs/cookbook/${f}`) : []),
];

const problems = [];
let calls = 0;

for (const rel of files) {
  const path = join(root, rel);
  if (!existsSync(path)) continue;
  const lines = readFileSync(path, "utf8").split(/\r?\n/);
  let inCode = false;
  lines.forEach((line, i) => {
    if (/^\s*```/.test(line)) {
      inCode = !inCode;
      return;
    }
    if (!inCode) return;
    const where = `${rel}:${i + 1}`;
    const code = line.replace(/(^|[^:])\/\/.*$/, "$1"); // comments may name old names on purpose (not "://")

    // ns.method( : a hook namespace on versailles or versailles.for(id)
    for (const m of code.matchAll(/\.([a-zA-Z]+)\.([a-zA-Z]+)\s*\(/g)) {
      const [, ns, method] = m;
      if (!namespaces.has(ns)) continue;
      const name = `${ns}.${method}`;
      calls++;
      if (!sdk.has(name)) problems.push(`${where}: ${name}() is not in the SDK`);
      else if (deprecated.has(name)) problems.push(`${where}: ${name}() is deprecated`);
    }
    // versailles.x( / v.x( : top-level names
    for (const m of code.matchAll(/\b(?:versailles|v)\.([a-zA-Z]+)\s*\(/g)) {
      const name = m[1];
      if (namespaces.has(name)) continue;
      calls++;
      if (!topLevel.has(name)) problems.push(`${where}: versailles.${name}() is not in the SDK`);
    }
    // on('name' : push hooks and piece events
    for (const m of code.matchAll(/\.on\(\s*['"]([\w-]+)['"]/g)) {
      calls++;
      if (!pushNames.has(m[1])) problems.push(`${where}: on('${m[1]}') is neither a push hook nor a piece event`);
    }
    // invoke('cmd' : documented raw commands only
    for (const m of code.matchAll(/\binvoke\(\s*['"]([\w]+)['"]/g)) {
      calls++;
      if (!commands.test(m[1])) problems.push(`${where}: invoke('${m[1]}') is not a documented command`);
    }
    // listen('event' : no deprecated events
    for (const m of code.matchAll(/\blisten\(\s*['"]([^'"]+)['"]/g)) {
      if (deprecatedEvents.test(m[1])) problems.push(`${where}: event ${m[1]} is deprecated`);
    }
    // data-hooks="a,b"
    for (const m of code.matchAll(/data-hooks="([^"]*)"/g)) {
      for (const hook of m[1].split(",").map((h) => h.trim()).filter(Boolean)) {
        if (!hooks.has(hook)) problems.push(`${where}: unknown hook '${hook}'`);
      }
    }
  });
  if (inCode) problems.push(`${rel}: a code block is not closed`);

  // data-* on the opening tag of a widget or spawnable (tags may span lines).
  const text = lines.join("\n");
  for (const m of text.matchAll(/<(article|template)\b[^>]*>/g)) {
    const tag = m[0];
    if (!/class="[^"]*\b(widget|spawnable|action-bar)\b/.test(tag)) continue;
    const line = text.slice(0, m.index).split("\n").length;
    for (const a of tag.matchAll(/\s(data-[\w-]+)=/g)) {
      if (!attributes.has(a[1])) problems.push(`${rel}:${line}: ${a[1]} is not a dialect attribute`);
    }
  }
}

if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`verify:docs: ${problems.length} problem(s)`);
  process.exit(1);
}
console.log(`verify:docs: ${calls} SDK uses in ${files.length} files match docs/api-surface.json`);
