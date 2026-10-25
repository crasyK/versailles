/**
 * Lock the public surface (#47): the SDK's public names, the hook names, the
 * dialect's `data-*` attributes, push names, piece events and error codes
 * are compared with `docs/api-surface.json`, and the hook-class table in
 * `docs/HOOKS.md` with the host's gating code.
 *
 * - A name missing from the code that the snapshot lists fails: pages may use
 *   it (1.x keeps every name). Put it back, or keep it as an alias.
 * - A new name fails until the snapshot lists it:
 *   `npm run verify:sdk -- --update` adds it (and rewrites the HOOKS.md
 *   table). `--update` never drops a name; removing one is a decision made
 *   by editing the snapshot by hand.
 */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  root,
  read,
  sdkSurface,
  knownHooks,
  dialectAttributes,
  pieceEvents,
  pushHooks,
  errorCodes,
  hookClassesMarkdown,
} from "./sdk-surface.mjs";

const update = process.argv.includes("--update");
const SNAPSHOT = "docs/api-surface.json";
const HOOKS_MD = "docs/HOOKS.md";
const BEGIN = "<!-- hook-classes:begin (generated: npm run verify:sdk -- --update) -->";
const END = "<!-- hook-classes:end -->";

const current = {
  sdk: sdkSurface(),
  hooks: knownHooks(),
  attributes: dialectAttributes(),
  pushHooks: pushHooks(),
  pieceEvents: pieceEvents(),
  errorCodes: errorCodes(),
};

const snapshot = JSON.parse(read(SNAPSHOT));
const problems = [];
const added = [];

for (const [key, names] of Object.entries(current)) {
  const locked = snapshot[key] ?? [];
  for (const name of locked) {
    if (!names.includes(name)) problems.push(`${key}: '${name}' is gone (pages may use it: keep it or alias it)`);
  }
  for (const name of names) {
    if (!locked.includes(name)) added.push(`${key}: '${name}'`);
  }
}
for (const name of snapshot.sdkDeprecated ?? []) {
  if (!current.sdk.includes(name)) problems.push(`sdkDeprecated: '${name}' is not an SDK name`);
}

const md = read(HOOKS_MD);
const table = hookClassesMarkdown();
const begin = md.indexOf(BEGIN);
const end = md.indexOf(END);
if (begin < 0 || end < begin) {
  problems.push(`${HOOKS_MD}: hook-classes markers missing`);
}
const inDoc = begin >= 0 && end > begin ? md.slice(begin + BEGIN.length, end).trim() : "";
const tableStale = inDoc !== table;

if (update && !problems.length) {
  const next = { ...snapshot };
  for (const [key, names] of Object.entries(current)) {
    const keep = snapshot[key] ?? [];
    next[key] = key === "hooks" || key === "pieceEvents" || key === "errorCodes"
      ? [...keep, ...names.filter((n) => !keep.includes(n))]
      : [...new Set([...keep, ...names])].sort();
  }
  writeFileSync(join(root, SNAPSHOT), JSON.stringify(next, null, 2) + "\n");
  if (tableStale) {
    writeFileSync(join(root, HOOKS_MD), md.slice(0, begin + BEGIN.length) + "\n" + table + "\n" + md.slice(end));
  }
  console.log(`check-api-surface: snapshot updated (${added.length} added)${tableStale ? ", HOOKS.md hook classes rewritten" : ""}`);
  process.exit(0);
}

if (added.length) {
  problems.push(`new names not in ${SNAPSHOT} (run \`npm run verify:sdk -- --update\`):\n    ${added.join("\n    ")}`);
}
if (tableStale) {
  problems.push(`${HOOKS_MD}: the hook-classes table differs from the code (run \`npm run verify:sdk -- --update\`)`);
}
if (problems.length) {
  console.error(`check-api-surface: ${problems.length} problem(s)\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(
  `check-api-surface: ok (${current.sdk.length} SDK names, ${current.hooks.length} hooks, ${current.attributes.length} attributes)`
);
