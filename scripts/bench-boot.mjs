#!/usr/bin/env node
/**
 * Release WebView2 boot probe. Times wallpaper-ready on the real binary.
 *
 *   npm run bench:boot
 *   npm run bench:boot -- --profile engine
 *   npm run bench:boot -- --exe path/to/versailles.exe --update-baseline
 */
import { spawn, spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const updateBaseline = args.includes("--update-baseline");
const profileArg = (() => {
  const i = args.indexOf("--profile");
  return i >= 0 ? args[i + 1] : "all";
})();
const exeArg = (() => {
  const i = args.indexOf("--exe");
  return i >= 0 ? args[i + 1] : null;
})();
const runs = (() => {
  const i = args.indexOf("--runs");
  return i >= 0 ? Math.max(2, Number(args[i + 1]) || 5) : 5;
})();
const inCi = process.env.CI === "true" || process.env.GITHUB_ACTIONS === "true";

function parseProfiles() {
  if (profileArg === "engine") return ["engine"];
  if (profileArg === "user") return ["user"];
  const out = ["engine"];
  if (existsSync(join(root, "tests/fixtures/user-desktop/index.html"))) out.push("user");
  return out;
}

function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const addr = s.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      s.close((err) => (err ? reject(err) : resolve(port)));
    });
    s.on("error", reject);
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function median(nums) {
  const a = [...nums].sort((x, y) => x - y);
  const mid = Math.floor(a.length / 2);
  return a.length % 2 ? a[mid] : (a[mid - 1] + a[mid]) / 2;
}

function p95(nums) {
  const a = [...nums].sort((x, y) => x - y);
  if (!a.length) return 0;
  const i = Math.min(a.length - 1, Math.ceil(a.length * 0.95) - 1);
  return a[Math.max(0, i)];
}

function resolveExe() {
  if (exeArg) return exeArg;
  const candidates = [
    join(root, "src-tauri/target/release/versailles.exe"),
    join(root, "src-tauri/target/release/versailles"),
  ];
  return candidates.find((p) => existsSync(p)) || null;
}

function ensureReleaseBuild() {
  if (exeArg) return;
  if (!args.includes("--rebuild") && resolveExe()) return;
  console.log("building release binary…");
  const r = spawnSync(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["run", "tauri:build", "--", "--no-bundle"],
    { cwd: root, stdio: "inherit", shell: true },
  );
  if (r.status !== 0) {
    console.error("tauri:build failed");
    process.exit(r.status ?? 1);
  }
}

function stageFixture(profile, dest) {
  if (profile === "engine") {
    cpSync(join(root, "templates/starter/.sdk"), join(dest, ".sdk"), { recursive: true });
    cpSync(join(root, "templates/starter/desktop"), join(dest, "desktop"), { recursive: true });
    return;
  }
    const fixture = join(root, "tests/fixtures/user-desktop");
    cpSync(fixture, join(dest, "desktop"), { recursive: true });
    const sdkSrc = join(root, "templates/starter/.sdk");
    cpSync(sdkSrc, join(dest, ".sdk"), { recursive: true });
    const barSrc = join(root, "templates/starter/desktop/bar.js");
    if (existsSync(barSrc) && !existsSync(join(dest, "desktop/bar.js"))) {
      cpSync(barSrc, join(dest, "desktop/bar.js"));
    }
}

async function pollBoot(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/debug/boot`);
      if (res.ok) {
        last = await res.json();
        if (last.ready) return last;
      }
    } catch {
      /* not up yet */
    }
    await sleep(50);
  }
  return last;
}

function killPid(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      /* gone */
    }
  }
}

async function oneRun(exe, profile) {
  const dest = mkdtempSync(join(tmpdir(), `versailles-bench-${profile}-`));
  const port = await freePort();
  try {
    stageFixture(profile, dest);
    const child = spawn(exe, ["--bench-boot"], {
      env: {
        ...process.env,
        VERSAILLES_BENCH: "1",
        VERSAILLES_ROOT: dest,
        VERSAILLES_API_PORT: String(port),
      },
      stdio: "ignore",
      windowsHide: true,
    });
    const boot = await pollBoot(port, 30_000);
    let idle = boot;
    if (boot?.ready) {
      await sleep(3_000);
      try {
        const res = await fetch(`http://127.0.0.1:${port}/debug/boot`);
        if (res.ok) idle = await res.json();
      } catch {
        /* process already gone */
      }
    }
    killPid(child.pid);
    await sleep(200);
    return { boot, idle, dest };
  } finally {
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch {
      /* still locked on Windows */
    }
  }
}

function loadBaseline() {
  const p = join(root, "scripts/bench-boot.baseline.json");
  if (!existsSync(p)) return null;
  return JSON.parse(readFileSync(p, "utf8"));
}

function saveBaseline(data) {
  writeFileSync(
    join(root, "scripts/bench-boot.baseline.json"),
    JSON.stringify(data, null, 2) + "\n",
    "utf8",
  );
}

ensureReleaseBuild();
const exe = resolveExe();
if (!exe) {
  console.error("versailles release binary not found");
  process.exit(1);
}
console.log("exe", exe);

const profiles = parseProfiles();
const baseline = loadBaseline() || {
  slack: 1.4,
  ciCeilingMs: 15000,
  engine: { medianMs: 8000, p95Ms: 12000 },
  user: { medianMs: 12000, p95Ms: 18000 },
};

let failed = false;
const summary = {};

for (const profile of profiles) {
  const samples = [];
  let lastIdle = null;
  for (let i = 0; i < runs; i++) {
    console.log(`\n${profile} run ${i + 1}/${runs}`);
    const { boot, idle } = await oneRun(exe, profile);
    lastIdle = idle;
    if (!boot?.ready) {
      console.error("  not ready", boot);
      failed = true;
      continue;
    }
    console.log(
      `  ready_ms=${boot.readyMs} nav=${boot.counters.iframeNavigations} parse=${boot.counters.parsePageCalls} html=${boot.counters.htmlReads} media=${boot.counters.mediaNowCalls}`,
    );
    samples.push(boot);
  }
  const warm = samples.slice(1);
  const use = warm.length ? warm : samples;
  if (!use.length) {
    failed = true;
    continue;
  }
  const ready = use.map((s) => s.readyMs);
  const med = median(ready);
  const p = p95(ready);
  const last = use[use.length - 1];
  const idle = lastIdle || last;
  summary[profile] = { medianMs: med, p95Ms: p, last, idle };

  const nav = last.counters.iframeNavigations;
  if (nav !== 1) {
    console.error(`FAIL ${profile}: iframe_navigations=${nav} (want 1)`);
    failed = true;
  }
  const parses = last.counters.parsePageCalls;
  if (parses < 1 || parses > 2) {
    console.error(`FAIL ${profile}: parse_page_calls=${parses} (want 1–2)`);
    failed = true;
  }
  if (idle?.ready) {
    const dParse = idle.counters.parsePageCalls - last.counters.parsePageCalls;
    const dHtml = idle.counters.htmlReads - last.counters.htmlReads;
    const media = idle.counters.mediaNowCalls;
    if (dParse > 0 || dHtml > 0) {
      console.error(
        `FAIL ${profile}: counters climbed during idle parse+${dParse} html+${dHtml}`,
      );
      failed = true;
    }
    if (profile === "user" && media > 2) {
      console.error(`FAIL ${profile}: media_now_calls=${media} after idle (want ≤2)`);
      failed = true;
    }
  }

  const base = baseline[profile];
  const slack = baseline.slack ?? 1.4;
  const ceiling = baseline.ciCeilingMs ?? 15000;
  if (base?.medianMs) {
    const limit = inCi ? Math.max(base.medianMs * slack, ceiling) : base.medianMs * slack;
    if (med > limit) {
      console.error(
        `FAIL ${profile}: median ${med}ms > ${Math.round(limit)}ms (baseline ${base.medianMs} slack ${slack})`,
      );
      failed = true;
    }
  }
  console.log(
    `${profile}  median=${Math.round(med)}ms  p95=${Math.round(p)}ms  nav=${nav} parse=${parses}`,
  );
}

if (updateBaseline) {
  const next = {
    slack: baseline.slack ?? 1.4,
    ciCeilingMs: baseline.ciCeilingMs ?? 15000,
    engine: summary.engine
      ? { medianMs: Math.round(summary.engine.medianMs), p95Ms: Math.round(summary.engine.p95Ms) }
      : baseline.engine,
    user: summary.user
      ? { medianMs: Math.round(summary.user.medianMs), p95Ms: Math.round(summary.user.p95Ms) }
      : baseline.user,
  };
  saveBaseline(next);
  console.log("updated scripts/bench-boot.baseline.json");
}

writeFileSync(
  join(root, "scripts/bench-boot.last.json"),
  JSON.stringify({ inCi, summary }, null, 2) + "\n",
  "utf8",
);

if (failed) process.exit(1);
console.log("bench:boot OK");
