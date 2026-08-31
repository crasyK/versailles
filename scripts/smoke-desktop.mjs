/**
 * Static contracts for wallpaper efficiency:
 * no inlined bar engine, no 800ms media poll, no 50ms watch tick,
 * no location.reload in local-restore, desktop.ts retries only when src is empty.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(root, rel), "utf8");

let failed = false;
function fail(msg) {
  console.error(msg);
  failed = true;
}

const pages = [
  join(root, "..", "desktop", "index.html"),
  join(root, "templates/starter/desktop/index.html"),
];

for (const p of pages) {
  let html;
  try {
    html = readFileSync(p, "utf8");
  } catch {
    continue;
  }
  const start = html.indexOf("<!-- BAR_ENGINE_START -->");
  const end = html.indexOf("<!-- BAR_ENGINE_END -->");
  if (start >= 0 && end > start && end - start > 80) {
    fail(`${p}: inlined BAR_ENGINE payload still present`);
  }
  if (/setInterval\s*\(\s*refresh\s*,\s*800\s*\)/.test(html)) {
    fail(`${p}: now-playing still polls every 800ms`);
  }
  if (/setInterval\s*\(\s*tick\s*,\s*50\s*\)/.test(html)) {
    fail(`${p}: watch still ticks every 50ms`);
  }
  const restore = html.match(/function restoreLocalStorage[\s\S]*?\n\s*\}\)\(\);/);
  if (restore && /location\.reload\s*\(/.test(restore[0])) {
    fail(`${p}: local-restore still calls location.reload()`);
  }
}

const desktopTs = read("src/desktop.ts");
if (/if\s*\(\s*!pageEl\.src\s*\|\|/.test(desktopTs)) {
  fail("src/desktop.ts: boot retry still treats a set src as a miss");
}
if (/applyPage\(\s*ev\.payload\.pageUrl\s*,\s*true\s*\)/.test(desktopTs)) {
  fail("src/desktop.ts: desktop://layout still force-reloads the iframe");
}

if (failed) process.exit(1);
console.log("desktop contracts OK");
