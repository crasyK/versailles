/**
 * Process accounting for one isolated Versailles instance (#27, #49), shared
 * by `npm run measure:memory` and the e2e performance phase.
 *
 * The instance is the app process started by the script (no supervisor) and
 * everything below it in the process tree, plus any WebView2 process using
 * its own WebView2 data folder: a WebView2 browser process is a child of the
 * app, its renderers, GPU and utility processes are children of that. Another
 * Versailles (the user's own) has its own tree and data folder, so it is
 * never counted.
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * Every process of the instance: `{ pid, kind, type, mb, cpuMs }`, where kind
 * is `host` (the app), `keys` (the `--keys-hook` helper), `guard` (taskbar
 * guard), `webview` (msedgewebview2, `type` = browser / renderer / gpu-process
 * / utility / crashpad-handler) or `other` (anything the app started); mb is
 * private memory, cpuMs the CPU time used so far.
 */
export function sampleProcesses(hostPid, dataDir) {
  return sampleAt(hostPid, dataDir).rows;
}

/** The rows and when (ms since the epoch) their CPU times were read. */
function sampleAt(hostPid, dataDir) {
  const ps = `
$root = ${Number(hostPid)}
$data = '${String(dataDir).replace(/'/g, "''")}'
# Ids as [int]: hashtable keys of different number types never match.
$all = @(Get-CimInstance Win32_Process | Select-Object @{ n = 'ProcessId'; e = { [int]$_.ProcessId } }, @{ n = 'ParentProcessId'; e = { [int]$_.ParentProcessId } }, Name, CommandLine, CreationDate)
$host0 = $all | Where-Object { $_.ProcessId -eq $root } | Select-Object -First 1
$rows = @()
$at = 0
if ($host0) {
  # Children by parent; a parent id can be stale (reused), so only processes
  # started after the app count as its descendants.
  $kids = @{}
  foreach ($p in $all) {
    if ($p.CreationDate -lt $host0.CreationDate) { continue }
    if (-not $kids.ContainsKey($p.ParentProcessId)) { $kids[$p.ParentProcessId] = New-Object System.Collections.ArrayList }
    [void]$kids[$p.ParentProcessId].Add($p)
  }
  $mine = @{ $root = $host0 }
  $queue = New-Object System.Collections.Queue
  $queue.Enqueue($root)
  while ($queue.Count) {
    $id = $queue.Dequeue()
    if ($kids.ContainsKey($id)) { foreach ($c in $kids[$id]) { if (-not $mine.ContainsKey($c.ProcessId)) { $mine[$c.ProcessId] = $c; $queue.Enqueue($c.ProcessId) } } }
  }
  foreach ($p in $all) { if ($p.Name -eq 'msedgewebview2.exe' -and $p.CommandLine -like "*$data*") { $mine[$p.ProcessId] = $p } }
  $live = @{}
  foreach ($g in @(Get-Process -Id @($mine.Keys) -ErrorAction SilentlyContinue)) { $live[[int]$g.Id] = $g }
  $at = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
  foreach ($id in @($mine.Keys)) {
    $p = $mine[$id]; $g = $live[$id]
    if (-not $g) { continue }
    $cmd = [string]$p.CommandLine
    $kind = 'other'; $type = $p.Name
    if ($id -eq $root) { $kind = 'host'; $type = 'app' }
    elseif ($p.Name -eq 'msedgewebview2.exe') { $kind = 'webview'; $type = if ($cmd -match '--type=([a-z-]+)') { $matches[1] } else { 'browser' } }
    elseif ($cmd -match '--keys-hook') { $kind = 'keys'; $type = 'keys-hook' }
    elseif ($cmd -match '--taskbar-guard') { $kind = 'guard'; $type = 'taskbar-guard' }
    $cpu = if ($g.TotalProcessorTime) { $g.TotalProcessorTime.TotalMilliseconds } else { 0 }
    $rows += [pscustomobject]@{ pid = $id; kind = $kind; type = $type; mb = [math]::Round($g.PrivateMemorySize64 / 1MB, 1); cpuMs = [math]::Round($cpu) }
  }
}
@{ at = $at; rows = @($rows) } | ConvertTo-Json -Depth 3 -Compress`;
  const out = execFileSync("powershell", ["-NoProfile", "-Command", ps], { encoding: "utf8" }).trim();
  const r = JSON.parse(out);
  return { at: r.at, rows: [r.rows || []].flat() };
}

/**
 * Totals of a sample: host and key helper MB (null when not running), the
 * WebView2 total and its split by process type, and the CPU time of the
 * processes that make up the app (host, key helper, WebView2).
 */
export function summarize(rows) {
  const sum = (list) => Math.round(list.reduce((a, r) => a + r.mb, 0));
  const of = (kind) => rows.filter((r) => r.kind === kind);
  const byType = {};
  for (const r of of("webview")) {
    byType[r.type] = byType[r.type] || { n: 0, mb: 0 };
    byType[r.type].n += 1;
    byType[r.type].mb += r.mb;
  }
  for (const v of Object.values(byType)) v.mb = Math.round(v.mb);
  return {
    host: of("host").length ? sum(of("host")) : null,
    keys: of("keys").length ? sum(of("keys")) : null,
    webviewTotal: sum(of("webview")),
    webviewCount: of("webview").length,
    byType,
    other: rows.filter((r) => r.kind === "other" || r.kind === "guard"),
    cpuMs: rows.filter((r) => ["host", "keys", "webview"].includes(r.kind)).reduce((a, r) => a + r.cpuMs, 0),
  };
}

/** "browser 1× 40, renderer 3× 120, ..." */
export function formatByType(byType) {
  return Object.entries(byType)
    .map(([t, v]) => `${t} ${v.n}× ${v.mb}`)
    .join(", ");
}

/**
 * CPU of the instance over `ms`, in percent of one core: CPU time used by the
 * host, key helper and WebView2 processes between two samples. A process that
 * starts in between counts with all its time. Returns the end sample too,
 * and the CPU ms by process type (host, keys-hook, renderer, gpu-process...).
 */
export async function idleCpu(hostPid, dataDir, ms) {
  const counted = (rows) => rows.filter((r) => ["host", "keys", "webview"].includes(r.kind));
  const cpu = (rows) => new Map(counted(rows).map((r) => [r.pid, r.cpuMs]));
  const first = sampleAt(hostPid, dataDir);
  const before = cpu(first.rows);
  await new Promise((r) => setTimeout(r, ms));
  const { at, rows } = sampleAt(hostPid, dataDir);
  const elapsed = first.at && at ? at - first.at : NaN; // NaN: the app was gone
  let used = 0;
  const byType = {};
  for (const r of counted(rows)) {
    const d = Math.max(0, r.cpuMs - (before.get(r.pid) || 0));
    const t = r.kind === "host" ? "host" : r.type;
    used += d;
    byType[t] = (byType[t] || 0) + d;
  }
  return { percent: (used / elapsed) * 100, usedMs: used, elapsedMs: elapsed, byType, rows };
}

/** The bench-mode boot snapshot the app writes to `<root>/.versailles/boot.json`. */
export function bootSnapshot(root) {
  const f = join(root, ".versailles/boot.json");
  if (!existsSync(f)) return null;
  try {
    return JSON.parse(readFileSync(f, "utf8"));
  } catch {
    return null; // written while we read
  }
}

/** Budgets: `{ hostPrivateMB, webviewTotalMB, idleCpuPercent, startupMs }`. */
export function loadBudget(file) {
  const b = JSON.parse(readFileSync(file, "utf8"));
  for (const k of ["hostPrivateMB", "webviewTotalMB", "idleCpuPercent", "startupMs"]) {
    if (typeof b[k] !== "number") throw new Error(`${file}: ${k} must be a number`);
  }
  return b;
}
