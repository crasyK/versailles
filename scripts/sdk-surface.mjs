/**
 * The public API surface, read from the code itself (shared by
 * `check-sdk-types.mjs` and `check-api-surface.mjs`):
 *
 * - `sdkSurface()`: every public SDK name, from running the SDK in a bare VM
 *   (no window, no host) and walking `versailles` and `versailles.for(id)`.
 * - `knownHooks()`, `dialectAttributes()`, `pieceEvents()`, `errorCodes()`.
 * - `hookClasses()`: hook -> commands per class (plain / control / exec),
 *   from the `require_hook` / `gate_*` calls in the host's commands.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

export const root = join(dirname(fileURLToPath(import.meta.url)), "..");
/** A repo file with `\n` line ends (a CRLF checkout reads the same). */
export const read = (rel) => readFileSync(join(root, rel), "utf8").replace(/\r\n/g, "\n");
export const SDK = "templates/starter/.sdk/versailles.js";
export const SDK_TYPES = "templates/starter/.sdk/versailles.d.ts";

/** Load the SDK into a fresh context and return its `versailles` object. */
export function loadSdk() {
  const sandbox = { console: { warn() {}, error() {}, log() {} }, setTimeout, clearTimeout, URLSearchParams };
  vm.createContext(sandbox);
  vm.runInContext(read(SDK), sandbox, { filename: SDK });
  if (!sandbox.versailles) throw new Error(`${SDK} did not define versailles`);
  return sandbox;
}

/**
 * Sorted dotted paths of every public name: `media.now`, `version`,
 * `for().media.now`, … Functions and plain values are leaves; plain objects
 * are walked. `for` is walked through what it returns (`for()`).
 */
export function sdkSurface() {
  const g = loadSdk();
  const out = new Set();
  const walk = (obj, prefix, seen) => {
    for (const key of Object.keys(obj)) {
      const value = obj[key];
      const path = prefix ? `${prefix}.${key}` : key;
      if (value && typeof value === "object" && !Array.isArray(value)) {
        if (seen.has(value)) continue;
        seen.add(value);
        walk(value, path, seen);
      } else {
        out.add(path);
      }
    }
  };
  walk(g.versailles, "", new Set());
  walk(g.versailles.for("x"), "for()", new Set());
  if (g.deck !== g.versailles) throw new Error("the deck global is no longer the versailles object");
  out.add("<global>.versailles");
  out.add("<global>.deck");
  return [...out].sort();
}

/** `KNOWN_HOOKS` in page.rs, in order. */
export function knownHooks() {
  const src = read("src-tauri/src/page.rs");
  const block = src.match(/pub const KNOWN_HOOKS: &\[&str\] = &\[([\s\S]*?)\];/)?.[1];
  if (!block) throw new Error("KNOWN_HOOKS not found in page.rs");
  return [...block.matchAll(/"([a-z-]+)"/g)].map((m) => m[1]);
}

/** `data-*` attributes the host parser reads (page.rs `attr(tag, "data-…")`). */
export function dialectAttributes() {
  const src = read("src-tauri/src/page.rs");
  return [...new Set([...src.matchAll(/attr\(tag, "(data-[a-z-]+)"\)/g)].map((m) => m[1]))].sort();
}

/** Piece events the SDK accepts in `on()`. */
export function pieceEvents() {
  const block = read(SDK).match(/const PIECE_EVENTS = new Set\(\[([^\]]*)\]\)/)?.[1];
  if (!block) throw new Error("PIECE_EVENTS not found in the SDK");
  return [...block.matchAll(/"([a-z-]+)"/g)].map((m) => m[1]);
}

/** Hooks `on()` accepts (pushed by the host). */
export function pushHooks() {
  const src = read(SDK);
  const quick = src.match(/const QUICK_HOOKS = \[([^\]]*)\]/)?.[1];
  const push = src.match(/const PUSH_HOOKS = new Set\(\[([^\]]*)\]/)?.[1];
  if (!quick || !push) throw new Error("QUICK_HOOKS / PUSH_HOOKS not found in the SDK");
  const names = (s) => [...s.matchAll(/"([a-z-]+)"/g)].map((m) => m[1]);
  return [...names(push), ...names(quick)];
}

/** Error codes the SDK documents (the `ErrorCode` list in its comment). */
export function errorCodes() {
  const block = read(SDK).match(/One error shape:[\s\S]*?Codes: ([\s\S]*?)\*\//)?.[1];
  if (!block) throw new Error("error code list not found in the SDK");
  return [...block.matchAll(/`([a-z-]+)`/g)].map((m) => m[1]);
}

const CLASSES = ["Plain", "Control", "Exec"];

/**
 * `{ hook: { Plain: [cmd], Control: [cmd], Exec: [cmd] } }` for every gated
 * Tauri command. Throws when a gated command's hook cannot be told.
 */
export function hookClasses() {
  const dir = "src-tauri/src";
  const sensors = read(`${dir}/sensors.rs`).match(/pub const SENSORS:[\s\S]*?\n\];/)?.[0] ?? "";
  const sensorHooks = [...sensors.matchAll(/\("([a-z-]+)",/g)].map((m) => m[1]);
  // `is_readable`: the sensors plus `notify`.
  const readable = [...sensorHooks, "notify"];
  const quickFn = read(`${dir}/quick.rs`).match(/pub fn hook\(&self\)[\s\S]*?\n {4}\}/)?.[0] ?? "";
  const quickHooks = [...new Set([...quickFn.matchAll(/=> "([a-z-]+)"/g)].map((m) => m[1]))];
  if (!sensorHooks.length || !quickHooks.length) throw new Error("SENSORS or QuickAction::hook not found");
  // Commands whose hook is a variable.
  const dynamic = { sense_now: readable, quick_set: quickHooks };

  const table = {};
  const add = (hook, cls, cmd) => {
    table[hook] ??= { Plain: [], Control: [], Exec: [] };
    if (!table[hook][cls].includes(cmd)) table[hook][cls].push(cmd);
  };
  for (const file of readdirSync(join(root, dir)).filter((f) => f.endsWith(".rs"))) {
    const src = read(`${dir}/${file}`);
    const chunks = src.split("#[tauri::command]").slice(1);
    for (const chunk of chunks) {
      const name = chunk.match(/fn\s+(\w+)/)?.[1];
      if (!name) continue;
      const end = chunk.search(/\n\}\r?\n/);
      const body = end >= 0 ? chunk.slice(0, end) : chunk;
      for (const m of body.matchAll(/require_hook\(([^;]*?)HookClass::(\w+)\)/g)) {
        const literal = m[1].match(/"([a-z-]+)"\s*,\s*$/)?.[1];
        const hooks = literal ? [literal] : dynamic[name];
        if (!hooks) throw new Error(`${file}: ${name} gates a hook this script cannot name`);
        for (const hook of hooks) add(hook, m[2], name);
      }
      for (const m of body.matchAll(/gate_shell\([^;]*?HookClass::(\w+)\)/g)) {
        add("shell", m[1], name);
      }
      if (/gate_pty\(/.test(body)) {
        add("pty", "Exec", name);
      }
    }
  }
  for (const hook of Object.keys(table)) {
    for (const cls of CLASSES) table[hook][cls].sort();
  }
  return table;
}

/** The HOOKS.md table (between the hook-classes markers), from `hookClasses()`. */
export function hookClassesMarkdown() {
  const table = hookClasses();
  const cell = (list) => (list.length ? list.map((c) => `\`${c}\``).join(", ") : "-");
  const lines = [
    "| Hook | Plain | Control | Exec |",
    "| ---- | ----- | ------- | ---- |",
  ];
  for (const hook of knownHooks()) {
    const row = table[hook];
    if (!row) {
      lines.push(`| \`${hook}\` | (no command: a declaration) | - | - |`);
      continue;
    }
    lines.push(`| \`${hook}\` | ${cell(row.Plain)} | ${cell(row.Control)} | ${cell(row.Exec)} |`);
  }
  for (const hook of Object.keys(table)) {
    if (!knownHooks().includes(hook)) throw new Error(`a command gates '${hook}', which is not in KNOWN_HOOKS`);
  }
  return lines.join("\n");
}
