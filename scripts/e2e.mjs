#!/usr/bin/env node
/**
 * End-to-end self-test of the running app (#26).
 *
 *   npm run e2e                         # test build, else release build
 *   npm run e2e -- --exe path/to/versailles.exe
 *   npm run e2e -- --take-taskbar       # see below
 *   npm run e2e -- --take-taskbar --dry-run   # print the plan, change nothing
 *
 * Starts the app isolated (VERSAILLES_BENCH: no single-instance bounce, no
 * hotkeys, no autostart) against a temporary desktop: the starter page plus
 * tests/e2e/pieces.html. The pieces report what they saw through the storage
 * hook; this script reads the store files, checks visible content and
 * reserved screen space with tests/e2e/probe.ps1, edits the page while the
 * app runs, restarts it, and always cleans up. Never touches the user's
 * desktop folder or their running Versailles.
 *
 * Except with --take-taskbar (#38): when the user's own Versailles hides the
 * taskbar, the taskbar phase would be skipped. The flag stops that Versailles
 * (its supervisor: there is no clean external quit, and the taskbar guard
 * restores the taskbar), runs the phase, and always starts it again
 * afterwards (same exe, `--autostart` if it had it), even when checks fail.
 * The taskbar phase also kills and restarts Explorer.
 *
 * Last, the performance phase (#49) starts the e2e page once more (wallpaper,
 * the docked bar, the overlay the wallpaper opens, a page edit so the key
 * helper runs), lets it settle and checks the budgets in
 * tests/e2e/budget.json: host private memory, WebView2 total, idle CPU over
 * 30 s and start-up time to the wallpaper being ready.
 *
 *   npm run e2e -- --no-perf            # skip the performance phase
 *   npm run e2e -- --perf-only          # only the performance phase
 */
import { execFileSync, spawn } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { get as httpGet } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { bootSnapshot, formatByType, idleCpu, loadBudget, summarize } from "./perf.mjs";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const exeArg = (() => {
  const i = args.indexOf("--exe");
  return i >= 0 ? args[i + 1] : null;
})();
const keep = args.includes("--keep"); // leave the app and its folder up on failure
const take = args.includes("--take-taskbar"); // stop the user's Versailles for the taskbar phase
const dryRun = args.includes("--dry-run"); // print the plan only
const noPerf = args.includes("--no-perf"); // skip the performance phase
const perfOnly = args.includes("--perf-only"); // run only the performance phase
const budgetFile = join(repo, "tests/e2e/budget.json");
const perfSettleMs = 20_000; // after the pieces are up, before measuring
const perfCpuMs = 30_000; // idle CPU window
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
  // A floating folder widget (widget.json), opened by the wallpaper piece.
  cpSync(join(repo, "tests/e2e/folder"), join(dest, "e2e-folder"), { recursive: true });
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

/** Status of a plain GET to the app's server (no browser headers). */
function httpStatus(port, path) {
  return new Promise((resolve) => {
    const req = httpGet({ host: "127.0.0.1", port, path, timeout: 10_000 }, (res) => {
      res.resume();
      resolve(res.statusCode);
    });
    req.on("timeout", () => req.destroy());
    req.on("error", (err) => resolve(String(err.message || err)));
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

/** Press a chord of virtual-key codes ("5B,87" = Win+F24). */
function press(chord) {
  const out = execFileSync(
    "powershell",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(repo, "tests/e2e/keys.ps1"), "-Chord", chord],
    { encoding: "utf8" },
  );
  return JSON.parse(out.trim());
}

function ps(command) {
  return execFileSync("powershell", ["-NoProfile", "-Command", command], { encoding: "utf8" }).trim();
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return err.code === "EPERM";
  }
}

/** Running versailles.exe processes, not the taskbar guard or a --restore-taskbar run. */
function versaillesProcesses() {
  const out = ps(
    "@(Get-CimInstance Win32_Process -Filter \"Name='versailles.exe'\" | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine) | ConvertTo-Json -Compress",
  );
  return (out ? [JSON.parse(out)].flat() : [])
    .map((p) => ({ pid: p.ProcessId, parent: p.ParentProcessId, exe: p.ExecutablePath, cmd: (p.CommandLine || "").trim() }))
    .filter((p) => !/--taskbar-guard|--restore-taskbar/.test(p.cmd));
}

/**
 * The user's Versailles: the top of each Versailles process tree (the
 * supervisor; a dev build has none), never this test's own app (`exclude`).
 */
function userVersailles(exclude = []) {
  const all = versaillesProcesses();
  const pids = new Set(all.map((p) => p.pid));
  return all
    .filter((p) => !pids.has(p.parent) && !exclude.includes(p.pid))
    .map((p) => ({
      ...p,
      app: all.filter((c) => c.parent === p.pid).map((c) => c.pid),
      autostart: /(^|\s)--autostart(\s|$)/.test(p.cmd),
    }));
}

/**
 * Start a program that outlives this test and its terminal (detached, out of
 * the terminal's job where allowed: tests/e2e/start.ps1), without the test's
 * VERSAILLES_* / WEBVIEW2_* variables.
 */
function startDetached(exe, argline = "") {
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^(VERSAILLES_|WEBVIEW2_)/i.test(k)));
  const params = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(repo, "tests/e2e/start.ps1"), "-Exe", exe];
  // A leading space keeps PowerShell from reading `--autostart` as a parameter.
  if (argline) params.push("-Arguments", ` ${argline}`);
  return JSON.parse(execFileSync("powershell", params, { encoding: "utf8", env }).trim());
}

const say = (text) => console.log(`e2e: --take-taskbar: ${text}`);

/** The user's Versailles stopped by --take-taskbar, to start again. */
const taken = [];

/**
 * Stop the user's Versailles so this test can hide the taskbar. Returns why
 * not, or null once Explorer's taskbar is visible again.
 */
async function takeTaskbar(exclude) {
  const found = userVersailles(exclude);
  if (!found.length) return "the taskbar is hidden, but no other Versailles is running";
  const dev = found.find((p) => /\\target\\debug\\/i.test(p.exe));
  if (dev) return `a dev build hides it (pid ${dev.pid}, ${dev.exe}); stop it yourself`;
  for (const p of found) {
    // No clean external quit: ending the supervisor ends the app (its
    // kill-on-close job), and the taskbar guard, outside that job, restores
    // the taskbar. Not a tree kill: apps started from Versailles live on.
    say(`stopping your Versailles: pid ${p.pid} (app ${p.app.join(", ") || "-"}), ${p.cmd}`);
    execFileSync("taskkill", ["/PID", String(p.pid), "/F"], { stdio: "ignore" });
    taken.push(p);
  }
  await until("your Versailles to exit", () => taken.every((p) => ![p.pid, ...p.app].some(alive)), 15_000);
  say("stopped; waiting for the taskbar guard to show Explorer's taskbar");
  const shown = () => until("Explorer's taskbar", () => probe(process.pid).taskbar, 15_000).catch(() => false);
  if (await shown()) return null;
  say("still hidden; running --restore-taskbar");
  try {
    execFileSync(taken[0].exe, ["--restore-taskbar"], { stdio: "ignore" });
  } catch {
    /* checked below */
  }
  return (await shown()) ? null : "Explorer's taskbar did not come back after stopping your Versailles";
}

/** Start what takeTaskbar stopped. Safe to call twice. */
function giveBack() {
  for (const p of taken.splice(0)) {
    const line = `"${p.exe}"${p.autostart ? " --autostart" : ""}`;
    if (alive(p.pid)) {
      say(`pid ${p.pid} is still running; not starting another`);
      continue;
    }
    try {
      const r = startDetached(p.exe, p.autostart ? "--autostart" : "");
      if (!r.pid) throw new Error(r.error);
      say(`started your Versailles again: ${line} (pid ${r.pid})`);
      if (!r.breakaway) say("note: it runs inside this terminal's job; closing the terminal may end it");
    } catch (err) {
      console.error(`e2e: --take-taskbar: could not start your Versailles again; start it yourself: ${line} (${err.message || err})`);
    }
  }
}

function explorerPids() {
  const out = execFileSync("tasklist", ["/FI", "IMAGENAME eq explorer.exe", "/FO", "CSV", "/NH"], { encoding: "utf8" });
  return [...out.matchAll(/^"explorer\.exe","(\d+)"/gim)].map((m) => Number(m[1]));
}

/** Kill Explorer; start it when Windows does not bring it back within 5 s. */
async function restartExplorer() {
  const old = explorerPids();
  console.log(`e2e: restarting Explorer (pid ${old.join(", ")})`);
  execFileSync("taskkill", ["/F", "/IM", "explorer.exe"], { stdio: "ignore" });
  const back = await until("Explorer back", () => explorerPids().some((pid) => !old.includes(pid)), 5_000).catch(() => false);
  if (back) return "came back by itself";
  startDetached(join(process.env.SystemRoot || "C:\\Windows", "explorer.exe"));
  return "started by the test";
}

const perfPlan = `Performance phase: start the app isolated once more on the e2e page, settle ${perfSettleMs / 1000} s,
   measure idle CPU over ${perfCpuMs / 1000} s and memory, check them and the start-up time against
   tests/e2e/budget.json, stop it.`;

function printPlan(exe) {
  if (perfOnly) {
    console.log(`e2e --dry-run: nothing is started or stopped.
  app under test: ${exe || "none found (npm run build:test, or --exe)"}
1. ${perfPlan}`);
    return;
  }
  const p = probe(process.pid);
  const found = userVersailles();
  console.log(`e2e --dry-run: nothing is started or stopped.
  app under test: ${exe || "none found (npm run build:test, or --exe)"}
  Explorer's taskbar: ${p.taskbar ? "visible" : p.explorerTaskbar ? "hidden" : "not found"}
  running Versailles: ${found.map((f) => `pid ${f.pid} (app ${f.app.join(", ") || "-"}) ${f.cmd}`).join("; ") || "none"}
1. Start the app isolated on a temporary desktop (twice) and run the checks.
2. Taskbar phase: hide the taskbar from the page config, check the tray, restart Explorer
   (taskkill /f /im explorer.exe, start it if it is not back within 5 s) and check the
   taskbar stays hidden, the Versailles tray stays in front with its icons and the docked
   bar keeps its strip; config off, on again, kill the app: the taskbar comes back.`);
  if (p.taskbar) {
    console.log("   The taskbar is visible: the phase runs.");
  } else if (!take) {
    console.log("   The taskbar is hidden: the phase is skipped (--take-taskbar runs it).");
  } else if (!found.length) {
    console.log("   The taskbar is hidden but no Versailles runs: the phase is skipped.");
  } else {
    for (const f of found) {
      console.log(`   --take-taskbar: before it, stop pid ${f.pid} (taskkill /PID ${f.pid} /F; its app ${f.app.join(", ") || "-"} ends with it),
   wait for Explorer's taskbar (the taskbar guard restores it; else ${f.exe} --restore-taskbar).
3. Always, even when checks fail: stop the test app, then start "${f.exe}"${f.autostart ? " --autostart" : ""} again.`);
    }
  }
  console.log(noPerf ? "Last: the performance phase is skipped (--no-perf)." : `Last: ${perfPlan}`);
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
/** `known`: an open issue makes this check unreliable; reported, not counted. */
function check(name, ok, detail = "", known = null) {
  results.push({ name, ok: !!ok, detail, known });
}

async function main() {
  const exe = resolveExe();
  if (noPerf && perfOnly) {
    console.error("--no-perf and --perf-only exclude each other.");
    process.exit(2);
  }
  if (dryRun) {
    printPlan(exe);
    process.exit(0);
  }
  if (!exe) {
    console.error("No release or test build found. Run `npm run build:test` first, or pass --exe.");
    process.exit(2);
  }
  console.log(`e2e: ${exe}`);
  try {
    if (!perfOnly) await checksPhase(exe);
    if (!noPerf) await perfPhase(exe);
  } finally {
    giveBack();
  }

  const width = Math.max(...results.map((r) => r.name.length));
  for (const r of results) {
    const status = r.ok ? "PASS" : r.known ? "KNOWN" : "FAIL";
    console.log(`${status.padEnd(5)}  ${r.name.padEnd(width)}  ${r.detail}${!r.ok && r.known ? ` (${r.known})` : ""}`);
  }
  const failed = results.filter((r) => !r.ok && !r.known).length;
  const known = results.filter((r) => !r.ok && r.known).length;
  const note = known ? ` (${known} known issue${known > 1 ? "s" : ""} not counted)` : "";
  console.log(failed ? `\n${failed} of ${results.length} checks failed${note}` : `\nall ${results.length} checks passed${note}`);
  process.exit(failed ? 1 : 0);
}

/** The functional checks: two runs and the taskbar phase. */
async function checksPhase(exe) {
  const baseline = reservedBottom(probe(process.pid));
  const { dest, page } = makeDesktop();
  let child = null;
  // Ctrl+C: the user's Versailles comes back too.
  process.once("SIGINT", () => {
    kill(child);
    giveBack();
    process.exit(130);
  });
  try {
    // Run 1: everything comes up.
    const port1 = await freePort();
    child = launch(exe, dest, port1);
    const wall = await until("wallpaper report", () => {
      const r = store(dest, "e2e-wall")?.report;
      return r && r.sdk && Object.keys(r.sensors).length + Object.keys(r.errors).length >= 17 && r.bus.length && r.controlRefused
        ? r : null;
    });
    check("wallpaper loads the SDK", wall.sdk, `sdk ${wall.sdk}`);
    for (const h of ["battery", "network", "focus", "audio", "radios", "audio-control", "display", "power-mode", "wifi", "bluetooth", "dnd", "posture", "keyboard", "vpn", "hotspot", "sharing", "accessibility"]) {
      check(`sensor ${h} answers`, wall.sensors[h], wall.errors[h] || wall.sensors[h]);
    }
    check("bus reaches a wallpaper widget", wall.bus.includes("e2e-overlay"), `from ${wall.bus.join(", ")}`);
    check("wallpaper cannot change a setting", /settings cannot change from the desktop wallpaper/.test(wall.controlRefused),
      wall.controlRefused);
    const overlay = await until("overlay report", () => store(dest, "e2e-overlay")?.report);
    const quick = await until("overlay quick_set", () => store(dest, "e2e-overlay")?.quick, 15_000).catch(() => null);
    check("an overlay can change a setting", quick === "ok", `mute unchanged: ${quick}`);
    const dismiss = await until("overlay dismiss", () => store(dest, "e2e-overlay")?.dismiss, 15_000).catch(() => null);
    check("dismissing a missing notification is refused", /no notification 0|not allowed access/.test(dismiss || ""), dismiss);
    check("overlay template script runs once", overlay.runs === 1, `runs ${overlay.runs}`);
    execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(repo, "tests/e2e/click.ps1")]);
    const viaClick = await until("overlay click", () => store(dest, "e2e-overlay")?.clickControl, 10_000).catch(() => null);
    check("window control inside a click gets past the click check", /no window 0/.test(viaClick || ""), viaClick);
    const tray = await until("tray report", () => store(dest, "e2e-wall")?.report?.trayClick && store(dest, "e2e-wall").report, 15_000)
      .catch(() => store(dest, "e2e-wall")?.report || {});
    check("tray answers", tray.tray && tray.tray.active === false, JSON.stringify(tray.tray) + (tray.errors?.tray || ""));
    check("tray click without a click is refused", /needs a click or key press/.test(tray.trayClick || ""), tray.trayClick);
    const win = await until("windows-control report", () => store(dest, "e2e-wall")?.report?.winControl, 15_000).catch(() => null);
    check("window control without a click is refused", /needs a click or key press/.test(win || ""), win);
    const folder = await until("folder widget report", () => store(dest, "e2e-overlay")?.folder, 20_000).catch(() => null);
    check("folder widget gets no API token", /folder widget 'e2e-folder' cannot use get_api_info/.test(folder?.apiInfo || ""),
      folder ? folder.apiInfo : "no report");
    check("folder widget cannot read the page", /cannot use get_desktop_html/.test(folder?.desktopHtml || ""), folder?.desktopHtml);
    check("folder widget may read monitors", folder?.monitorInfo === "allowed", folder?.monitorInfo);
    check("folder widget cannot close other widgets", /only for itself/.test(folder?.closeOther || ""), folder?.closeOther);
    const save = await httpStatus(port1, "/weather?save=1&location=Berlin");
    check("saving the weather location needs the token", save === 401, `status ${save}`);
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

    // Win combos and the Copilot key go through the keyboard hook (#31); the
    // edit above registered the page's hotkeys.
    await sleep(1500);
    const winF24 = press("5B,87");
    const keyOpened = await until("Win+F24 piece", () => store(dest, "e2e-key")?.opened, 10_000).catch(() => false);
    check("Win+F24 opens its piece", keyOpened, "", "#37");
    check("Win+F24 does not open Start", !winF24.shellOpen, `foreground ${winF24.foreground}`, "#37");
    // Again, now that its piece is in front: it closes, and Start stays shut
    // (the first press hid a broken mask because opening the piece took focus).
    await sleep(800);
    const keyShown = probe(child.pid).texts.some((t) => /e2e key/i.test(t));
    const winF24Again = press("5B,87");
    await sleep(800);
    const keyHidden = !probe(child.pid).texts.some((t) => /e2e key/i.test(t));
    check("Win+F24 again closes its piece", keyShown && keyHidden, `shown ${keyShown}, hidden after ${keyHidden}`, "#37");
    check("Win+F24 again does not open Start", !winF24Again.shellOpen, `foreground ${winF24Again.foreground}`, "#37");
    const copilot = press("A2,A0,5B,86");
    const copilotOpened = await until("Copilot piece", () => store(dest, "e2e-copilot")?.opened, 10_000).catch(() => false);
    check("Ctrl+Copilot opens its piece", copilotOpened, "", "#37");
    check("Ctrl+Copilot does not open Windows' own", !copilot.shellOpen, `foreground ${copilot.foreground}`, "#37");

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

    // Hiding Explorer's taskbar (#30): config on, off, on, then a kill.
    let skip = probe(process.pid).taskbar ? null : "the taskbar was already hidden (another Versailles? --take-taskbar runs the phase)";
    if (skip && take) {
      const why = await takeTaskbar([child.pid]);
      skip = why && `--take-taskbar: ${why}`;
    }
    if (skip) {
      check("taskbar phase skipped", true, skip);
    } else {
      const html = readFileSync(page, "utf8");
      const on = html.replace('"autostart": true,', '"shell": { "hideTaskbar": true }, "autostart": true,');
      if (on === html) throw new Error("starter page changed: no autostart line in #versailles");
      const taskbar = (want, what) =>
        until(what, () => {
          const p = probe(process.pid);
          return p.taskbar === want ? p : null;
        }, 15_000).catch(() => probe(process.pid));
      writeFileSync(page, on);
      const hidden = await taskbar(false, "taskbar hidden");
      check("config hides the taskbar", !hidden.taskbar);
      check("hidden taskbar sets the logon restore", hidden.runOnce);
      // The tray (#35): our Shell_TrayWnd in front; apps (at least this
      // Versailles' own icon) add their icons to it.
      const held = await until("tray icons", () => {
        const t = store(dest, "e2e-wall")?.report?.tray;
        return t && t.active && t.icons > 0 ? t : null;
      }, 15_000).catch(() => store(dest, "e2e-wall")?.report?.tray);
      check("tray holds the icons while the taskbar is hidden", held && held.active && held.icons > 0, JSON.stringify(held));
      const front = probe(process.pid).trayFirst;
      check("apps find the Versailles tray first", front === child.pid, `Shell_TrayWnd of pid ${front}, app ${child.pid}`);
      // Explorer restarts (a crash, an update): its new taskbar is hidden
      // again, ours stays in front, apps add their icons again (#38).
      const strip = reservedBottom(probe(process.pid));
      const how = await restartExplorer();
      await until("Explorer's new taskbar, hidden", () => {
        const p = probe(process.pid);
        return p.explorerTaskbar && !p.taskbar;
      }, 30_000).catch(() => null);
      await sleep(2000); // Explorer shows its taskbar more than once while it starts
      const after = probe(process.pid);
      check("taskbar stays hidden after an Explorer restart", after.explorerTaskbar && !after.taskbar,
        `Explorer ${how}; taskbar found ${after.explorerTaskbar}, visible ${after.taskbar}`);
      const ahead = await until("Versailles tray in front", () => probe(process.pid).trayFirst === child.pid, 15_000).catch(() => false);
      check("apps find the Versailles tray first after an Explorer restart", ahead,
        `Shell_TrayWnd of pid ${probe(process.pid).trayFirst}, app ${child.pid}`);
      const icons = await until("tray icons after the restart", () => {
        const t = store(dest, "e2e-wall")?.report?.tray;
        return t && t.active && t.icons > 0 ? t : null;
      }, 20_000).catch(() => store(dest, "e2e-wall")?.report?.tray);
      check("tray holds the icons after an Explorer restart", icons && icons.active && icons.icons > 0, JSON.stringify(icons));
      const stripKept = await until("bar strip after the restart", () => reservedBottom(probe(process.pid)) >= strip, 30_000)
        .catch(() => false);
      check("docked bar keeps its strip after an Explorer restart", stripKept,
        `bottom ${strip} -> ${reservedBottom(probe(process.pid))} px`);
      writeFileSync(page, html);
      const shown = await taskbar(true, "taskbar shown");
      check("config off restores the taskbar", shown.taskbar && !shown.runOnce);
      const handed = await until("tray handed back", () => {
        const t = store(dest, "e2e-wall")?.report?.tray;
        return t && !t.active ? t : null;
      }, 15_000).catch(() => store(dest, "e2e-wall")?.report?.tray);
      check("tray goes back to Explorer with the taskbar", handed && !handed.active && probe(process.pid).trayFirst !== child.pid,
        JSON.stringify(handed));
      writeFileSync(page, on);
      await taskbar(false, "taskbar hidden again");
      kill(child);
      child = null;
      const back = await taskbar(true, "taskbar back after a kill");
      check("taskbar comes back when the app is killed", back.taskbar && !back.runOnce,
        `visible ${back.taskbar}, RunOnce ${back.runOnce}`);
    }
  } catch (err) {
    check("run completed", false, String(err && err.message ? err.message : err));
  } finally {
    if (keep && results.some((r) => !r.ok)) {
      console.log(`--keep: app pid ${child?.pid} left running on ${dest}`);
      for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}  ${r.detail}`);
      giveBack();
      process.exit(1);
    }
    kill(child);
    await sleep(2500);
    if (!probe(process.pid).taskbar) {
      // Last resort if the taskbar checks failed half-way.
      try {
        execFileSync(exe, ["--restore-taskbar"], { stdio: "ignore" });
      } catch {
        /* reported by the checks */
      }
    }
    const after = reservedBottom(probe(process.pid));
    check("cleanup: no screen space left reserved", after <= baseline, `bottom ${after} px (was ${baseline})`);
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch {
      /* WebView2 may still hold files for a moment */
    }
    // The user's Versailles comes back after the performance phase (main), so
    // its bars do not reserve space while that phase measures.
  }
}

/**
 * Performance budget (#49): a fresh isolated app on the e2e page (wallpaper,
 * the docked bar, the overlay the wallpaper opens; a page edit registers the
 * hotkeys, which starts the key helper as in the checks above). Measures the
 * app's own processes only (scripts/perf.mjs): its process tree and its own
 * WebView2 data folder, never the user's running Versailles.
 */
async function perfPhase(exe) {
  let budget;
  try {
    budget = loadBudget(budgetFile);
  } catch (err) {
    check("performance budget readable", false, String(err && err.message ? err.message : err));
    return;
  }
  console.log(`e2e: performance phase (settle ${perfSettleMs / 1000} s, idle CPU over ${perfCpuMs / 1000} s)`);
  const baseline = reservedBottom(probe(process.pid));
  const { dest, page } = makeDesktop();
  const dataDir = join(dest, "webview2");
  let child = null;
  // Ctrl+C: stop this app too; a handler from the checks phase exits after it.
  process.prependOnceListener("SIGINT", () => {
    kill(child);
    if (!process.listenerCount("SIGINT")) process.exit(130);
  });
  try {
    const port = await freePort();
    const started = Date.now();
    child = launch(exe, dest, port);
    // Start-up: the app's own clock from process start to the wallpaper
    // page's load event (bench mode writes it to .versailles/boot.json).
    let boot = null;
    while (!boot && Date.now() - started < 60_000) {
      const b = bootSnapshot(dest);
      if (b && b.ready) boot = b;
      else await sleep(100);
    }
    if (!boot) throw new Error("timed out waiting for the wallpaper to be ready (.versailles/boot.json)");
    const seen = Date.now() - started;
    await until("wallpaper report", () => store(dest, "e2e-wall")?.report?.bus?.length); // the overlay is open
    await until("bar load", () => store(dest, "e2e-bar")?.loads >= 1);
    writeFileSync(page, readFileSync(page, "utf8") + "\n<!-- e2e perf edit -->\n");
    await until("bar reload", () => store(dest, "e2e-bar")?.loads >= 2, 30_000).catch(() => null);
    await sleep(perfSettleMs);
    const cpu = await idleCpu(child.pid, dataDir, perfCpuMs);
    const s = summarize(cpu.rows);
    const total = (s.host || 0) + (s.keys || 0) + s.webviewTotal;
    console.log(`e2e: perf: app total ${total} MB (host ${s.host}, key helper ${s.keys ?? "-"}, WebView2 ${s.webviewTotal})`);
    if (s.other.length) console.log(`e2e: perf: also started by the app, not counted: ${s.other.map((r) => `${r.type} ${r.mb} MB`).join(", ")}`);
    const cpuSplit = Object.entries(cpu.byType)
      .filter(([, ms]) => ms > 0)
      .sort((a, b) => b[1] - a[1])
      .map(([t, ms]) => `${t} ${ms}`)
      .join(", ");

    check("start-up to wallpaper ready", boot.readyMs <= budget.startupMs,
      `${boot.readyMs} ms (budget ${budget.startupMs}); seen by the test after ${seen} ms`);
    check("host private memory", s.host != null && s.host <= budget.hostPrivateMB,
      `${s.host} MB (budget ${budget.hostPrivateMB}); key helper ${s.keys == null ? "not running" : `${s.keys} MB`}`);
    check("WebView2 total memory", s.webviewCount > 0 && s.webviewTotal <= budget.webviewTotalMB,
      `${s.webviewTotal} MB (budget ${budget.webviewTotalMB}); ${s.webviewCount} processes: ${formatByType(s.byType)}`);
    check(`idle CPU over ${perfCpuMs / 1000} s`, s.host != null && cpu.percent <= budget.idleCpuPercent,
      `${cpu.percent.toFixed(2)} % of one core (budget ${budget.idleCpuPercent}); ${cpu.usedMs} ms CPU in ${(cpu.elapsedMs / 1000).toFixed(1)} s: ${cpuSplit}`);
  } catch (err) {
    check("performance phase completed", false, String(err && err.message ? err.message : err));
  } finally {
    kill(child);
    child = null;
    await sleep(2500);
    const after = reservedBottom(probe(process.pid));
    check("perf cleanup: no screen space left reserved", after <= baseline, `bottom ${after} px (was ${baseline})`);
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch {
      /* WebView2 may still hold files for a moment */
    }
  }
}

main();
