#!/usr/bin/env node
/**
 * End-to-end self-test of the running app (#26).
 *
 *   npm run e2e                         # test build, else release build
 *   npm run e2e -- --exe path/to/versailles.exe
 *
 * Starts the app isolated (VERSAILLES_BENCH: no single-instance bounce, no
 * hotkeys, no autostart) against a temporary desktop: the starter page plus
 * tests/e2e/pieces.html. The pieces report what they saw through the storage
 * hook; this script reads the store files, checks visible content and
 * reserved screen space with tests/e2e/probe.ps1, edits the page while the
 * app runs, restarts it, and always cleans up. Never touches the user's
 * desktop folder or their running Versailles.
 */
import { execFileSync, spawn } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const exeArg = (() => {
  const i = args.indexOf("--exe");
  return i >= 0 ? args[i + 1] : null;
})();
const keep = args.includes("--keep"); // leave the app and its folder up on failure
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function resolveExe() {
  if (exeArg) return exeArg;
  // Release-style builds embed the frontend; a debug build needs the dev server.
  return [
    join(repo, "src-tauri/target/release-test/versailles.exe"),
    join(repo, "src-tauri/target/release/versailles.exe"),
  ].find((p) => existsSync(p));
}

/**
 * A free port the app's capabilities allow for page IPC: they list
 * 127.0.0.1 or localhost on 47831-47841 (the host's own fallback range). 47831 is normally
 * the user's running Versailles, so start one above it.
 */
function canBind(port) {
  return new Promise((resolve) => {
    const s = createServer();
    s.once("error", () => resolve(false));
    s.listen(port, "127.0.0.1", () => s.close(() => resolve(true)));
  });
}

async function freePort() {
  for (let port = 47832; port <= 47841; port++) {
    if (await canBind(port)) return port;
  }
  throw new Error("no free port in 47832-47841 (the range page IPC is allowed on)");
}

/** Temporary desktop: starter page + SDK, with the e2e pieces injected. */
function makeDesktop() {
  const dest = mkdtempSync(join(tmpdir(), "versailles-e2e-"));
  cpSync(join(repo, "templates/starter/desktop"), join(dest, "desktop"), { recursive: true });
  cpSync(join(repo, "templates/starter/.sdk"), join(dest, ".sdk"), { recursive: true });
  const page = join(dest, "desktop/index.html");
  const html = readFileSync(page, "utf8");
  const anchor = '    <div id="preview-root" hidden></div>';
  if (!html.includes(anchor)) throw new Error("starter page changed: no #preview-root anchor");
  const pieces = readFileSync(join(repo, "tests/e2e/pieces.html"), "utf8");
  writeFileSync(page, html.replace(anchor, pieces + anchor));
  return { dest, page };
}

function launch(exe, dest, port) {
  return spawn(exe, [], {
    env: {
      ...process.env,
      VERSAILLES_BENCH: "1",
      VERSAILLES_NO_SUPERVISOR: "1",
      VERSAILLES_ROOT: dest,
      VERSAILLES_API_PORT: String(port),
      // Own WebView2 browser process, never shared with a running Versailles.
      WEBVIEW2_USER_DATA_FOLDER: join(dest, "webview2"),
    },
    stdio: "ignore",
    windowsHide: false,
  });
}

function kill(child) {
  if (!child || child.exitCode !== null) return;
  try {
    execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } catch {
    /* already gone */
  }
}

function store(dest, id) {
  const f = join(dest, ".versailles/storage", `${id}.json`);
  try {
    return JSON.parse(readFileSync(f, "utf8"));
  } catch {
    return null;
  }
}

function probe(pid) {
  const out = execFileSync(
    "powershell",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(repo, "tests/e2e/probe.ps1"), "-ProcessId", String(pid)],
    { encoding: "utf8" },
  );
  return JSON.parse(out.trim());
}

const reservedBottom = (p) => Math.max(...[p.monitors].flat().map((m) => m.bottom));

async function until(what, fn, ms = 60_000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const v = fn();
    if (v) return v;
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok: !!ok, detail });
}

async function main() {
  const exe = resolveExe();
  if (!exe) {
    console.error("No release or test build found. Run `npm run build:test` first, or pass --exe.");
    process.exit(2);
  }
  console.log(`e2e: ${exe}`);
  const baseline = reservedBottom(probe(process.pid));
  const { dest, page } = makeDesktop();
  let child = null;
  try {
    // Run 1: everything comes up.
    child = launch(exe, dest, await freePort());
    const wall = await until("wallpaper report", () => {
      const r = store(dest, "e2e-wall")?.report;
      return r && r.sdk && Object.keys(r.sensors).length + Object.keys(r.errors).length >= 4 && r.bus.length ? r : null;
    });
    check("wallpaper loads the SDK", wall.sdk, `sdk ${wall.sdk}`);
    for (const h of ["battery", "network", "focus", "audio"]) {
      check(`sensor ${h} answers`, wall.sensors[h], wall.errors[h] || wall.sensors[h]);
    }
    check("bus reaches a wallpaper widget", wall.bus.includes("e2e-overlay"), `from ${wall.bus.join(", ")}`);
    const overlay = await until("overlay report", () => store(dest, "e2e-overlay")?.report);
    check("overlay template script runs once", overlay.runs === 1, `runs ${overlay.runs}`);
    await until("bar load", () => store(dest, "e2e-bar")?.loads >= 1);
    const p1 = await until("bar visible", () => {
      const p = probe(child.pid);
      return p.texts.some((t) => /E2E BAR/i.test(t)) ? p : null;
    }, 20_000).catch(() => probe(child.pid));
    check("docked bar is visible", p1.texts.some((t) => /E2E BAR/i.test(t)));
    check("docked bar reserves space", reservedBottom(p1) > baseline, `bottom ${baseline} -> ${reservedBottom(p1)} px`);
    check("no errors in the wallpaper", !Object.keys(wall.errors).length, JSON.stringify(wall.errors));

    // A page edit reloads the bar in place (it used to hide it).
    writeFileSync(page, readFileSync(page, "utf8") + "\n<!-- e2e edit -->\n");
    await until("bar reload", () => store(dest, "e2e-bar")?.loads >= 2, 30_000).catch(() => null);
    check("page edit reloads the bar", store(dest, "e2e-bar")?.loads >= 2, `loads ${store(dest, "e2e-bar")?.loads}`);
    const p2 = await until("bar visible after edit", () => {
      const p = probe(child.pid);
      return p.texts.some((t) => /E2E BAR/i.test(t)) ? p : null;
    }, 20_000).catch(() => probe(child.pid));
    check("bar still visible after the edit", p2.texts.some((t) => /E2E BAR/i.test(t)));

    // Restart: storage survives, the reservation is released while down.
    kill(child);
    child = null;
    await sleep(3000);
    check("reservation released when the app dies", reservedBottom(probe(process.pid)) <= baseline,
      `bottom ${reservedBottom(probe(process.pid))} px`);
    // Keep what the app stored; drop only the last run's report so the new
    // run's report is the one we read.
    const wallStore = join(dest, ".versailles/storage/e2e-wall.json");
    const kept = store(dest, "e2e-wall") || {};
    const before = kept.persist; // one count per wallpaper load (the edit reloads it too)
    delete kept.report;
    writeFileSync(wallStore, JSON.stringify(kept));
    child = launch(exe, dest, await freePort());
    const again = await until("wallpaper report after restart", () => store(dest, "e2e-wall")?.report?.persisted != null
      ? store(dest, "e2e-wall").report : null);
    check("storage survives a restart", before >= 1 && again.persisted === before, `stored ${before}, read back ${again.persisted}`);
  } catch (err) {
    check("run completed", false, String(err && err.message ? err.message : err));
  } finally {
    if (keep && results.some((r) => !r.ok)) {
      console.log(`--keep: app pid ${child?.pid} left running on ${dest}`);
      for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}  ${r.detail}`);
      process.exit(1);
    }
    kill(child);
    await sleep(2500);
    const after = reservedBottom(probe(process.pid));
    check("cleanup: no screen space left reserved", after <= baseline, `bottom ${after} px (was ${baseline})`);
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch {
      /* WebView2 may still hold files for a moment */
    }
  }

  const width = Math.max(...results.map((r) => r.name.length));
  for (const r of results) {
    console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name.padEnd(width)}  ${r.detail}`);
  }
  const failed = results.filter((r) => !r.ok).length;
  console.log(failed ? `\n${failed} of ${results.length} checks failed` : `\nall ${results.length} checks passed`);
  process.exit(failed ? 1 : 0);
}

main();
