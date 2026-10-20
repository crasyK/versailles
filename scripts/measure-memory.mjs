#!/usr/bin/env node
/**
 * Memory footprint per window kind (#27).
 *
 *   npm run measure:memory                 # test build, else release build
 *   npm run measure:memory -- --exe path/to/versailles.exe
 *
 * Starts the app isolated (like `npm run e2e`, plus its own WebView2 data
 * folder so it never shares processes with a running Versailles) three
 * times: wallpaper only; + a docked bar; + an open overlay. For each it
 * waits for the pages to settle and prints the private memory of the host
 * process and of every WebView2 process by type.
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
const settleMs = 20_000;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const exe =
  exeArg ||
  [join(repo, "src-tauri/target/release-test/versailles.exe"), join(repo, "src-tauri/target/release/versailles.exe")].find(
    (p) => existsSync(p),
  );

function canBind(port) {
  return new Promise((resolve) => {
    const s = createServer();
    s.once("error", () => resolve(false));
    s.listen(port, "127.0.0.1", () => s.close(() => resolve(true)));
  });
}
async function freePort() {
  for (let port = 47832; port <= 47841; port++) if (await canBind(port)) return port;
  throw new Error("no free port in 47832-47841");
}

const BAR = `
    <template class="spawnable" data-id="m-bar" data-anchor="bottom" data-h="24"><div>bar</div></template>`;
const OVERLAY = `
    <template class="spawnable" data-id="m-overlay" data-anchor="c" data-dim="none" data-dismiss-on-blur="false"
              data-w="200" data-h="80"><div>overlay</div></template>
    <article class="widget" data-id="m-open" data-hooks="spawn"></article>
    <script>
      (function wait(n) {
        if (/[?&]m=spawn/.test(location.search)) return;
        var v = window.versailles;
        if (!v || typeof v.for !== 'function') { if (n < 400) setTimeout(function () { wait(n + 1); }, 50); return; }
        v.for('m-open').spawn('m-overlay').catch(function () {});
      })(0);
    </script>`;

function makeDesktop(extra) {
  const dest = mkdtempSync(join(tmpdir(), "versailles-mem-"));
  cpSync(join(repo, "templates/starter/desktop"), join(dest, "desktop"), { recursive: true });
  cpSync(join(repo, "templates/starter/.sdk"), join(dest, ".sdk"), { recursive: true });
  const page = join(dest, "desktop/index.html");
  const anchor = '    <div id="preview-root" hidden></div>';
  writeFileSync(page, readFileSync(page, "utf8").replace(anchor, extra + "\n" + anchor));
  return dest;
}

/** Private MB of the host and of each WebView2 process using `dataDir`. */
function sample(hostPid, dataDir) {
  const ps = `
$data = '${dataDir.replace(/'/g, "''")}'
$h = Get-Process -Id ${hostPid} -ErrorAction SilentlyContinue
$rows = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$data*" } | ForEach-Object {
  $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
  if ($p) {
    $type = if ($_.CommandLine -match '--type=([a-z-]+)') { $matches[1] } else { 'browser' }
    [pscustomobject]@{ type = $type; mb = [math]::Round($p.PrivateMemorySize64 / 1MB) }
  }
}
@{ host = $(if ($h) { [math]::Round($h.PrivateMemorySize64 / 1MB) } else { $null }); webview = @($rows) } | ConvertTo-Json -Depth 3 -Compress`;
  const out = execFileSync("powershell", ["-NoProfile", "-Command", ps], { encoding: "utf8" });
  return JSON.parse(out.trim());
}

async function run(label, extra) {
  const dest = makeDesktop(extra);
  const dataDir = join(dest, "webview2");
  const child = spawn(exe, [], {
    env: {
      ...process.env,
      VERSAILLES_BENCH: "1",
      VERSAILLES_NO_SUPERVISOR: "1",
      VERSAILLES_ROOT: dest,
      VERSAILLES_API_PORT: String(await freePort()),
      WEBVIEW2_USER_DATA_FOLDER: dataDir,
    },
    stdio: "ignore",
  });
  try {
    await sleep(settleMs);
    const s = sample(child.pid, dataDir);
    const by = {};
    for (const r of s.webview) {
      by[r.type] = by[r.type] || { n: 0, mb: 0 };
      by[r.type].n += 1;
      by[r.type].mb += r.mb;
    }
    const total = s.webview.reduce((a, r) => a + r.mb, 0);
    return { label, host: s.host, webviewTotal: total, by };
  } finally {
    try {
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    } catch {
      /* gone */
    }
    await sleep(2500);
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch {
      /* files still held for a moment */
    }
  }
}

async function main() {
  if (!exe) {
    console.error("No release or test build found. Run `npm run build:test` first, or pass --exe.");
    process.exit(2);
  }
  console.log(`measure: ${exe} (settle ${settleMs / 1000} s each)`);
  const rows = [];
  rows.push(await run("wallpaper only", ""));
  rows.push(await run("+ docked bar", BAR));
  rows.push(await run("+ bar + open overlay", BAR + OVERLAY));
  for (const r of rows) {
    const parts = Object.entries(r.by)
      .map(([t, v]) => `${t} ${v.n}× ${v.mb}`)
      .join(", ");
    console.log(`${r.label.padEnd(22)} host ${String(r.host).padStart(4)} MB · WebView2 ${String(r.webviewTotal).padStart(4)} MB (${parts})`);
  }
}

main();
