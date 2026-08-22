#!/usr/bin/env node
/** Validate #versailles shortcuts in user desktop/index.html */
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const paths = [
  join(root, "..", "desktop", "index.html"),
  join(root, "templates/starter/desktop/index.html"),
];

let failed = false;
for (const p of paths) {
  let html;
  try {
    html = readFileSync(p, "utf8");
  } catch {
    continue;
  }
  const m = html.match(/<script[^>]*id="versailles"[^>]*>([\s\S]*?)<\/script>/i);
  if (!m) {
    console.warn("skip (no #versailles):", p);
    continue;
  }
  let block;
  try {
    block = JSON.parse(m[1]);
  } catch (e) {
    console.error(p, "invalid JSON:", e.message);
    failed = true;
    continue;
  }
  const shortcuts = block.shortcuts;
  if (!Array.isArray(shortcuts)) {
    console.warn(p, "no shortcuts array");
    continue;
  }
  const seen = new Map();
  for (const s of shortcuts) {
    const keys = [s.n, ...(s.aliases || [])].filter(Boolean).map((k) => k.toLowerCase());
    for (const k of keys) seen.set(k, (seen.get(k) || 0) + 1);
    if (!s.n || !s.t || !s.target) {
      console.error(p, "incomplete shortcut:", s.n || s);
      failed = true;
    }
    if (s.t === "web" && s.target && !/^https?:\/\//i.test(s.target)) {
      console.warn(p, "web shortcut may need URL:", s.n, s.target);
    }
  }
  const dups = [...seen.entries()].filter(([, n]) => n > 1).map(([k]) => k);
  if (dups.length) {
    console.error(p, "duplicate ids/aliases:", dups.join(", "));
    failed = true;
  } else {
    console.log("OK", p, shortcuts.length, "shortcuts");
  }
}

process.exit(failed ? 1 : 0);
