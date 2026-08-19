import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { Terminal } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import "xterm/css/xterm.css";

type CliOutput = { stdout: string; stderr: string; code: number };

type Preset = {
  n: string;
  t: "web" | "folder" | "app";
  d: string;
  target: string;
  /** Group label for presets / help browsing. */
  cat: string;
  aliases?: string[];
};

type CatalogEntry = {
  id: string;
  name: string;
  target: string;
  source: string;
  aliases: string[];
  fresh: boolean;
  hidden: boolean;
};

type Row = {
  c: string;
  d?: string;
  cc?: string;
  path?: string;
  cat?: string;
};

type LauncherMode = "action" | "terminal";

let root!: HTMLDivElement;
let titleEl!: HTMLSpanElement;
let modeLabel!: HTMLSpanElement;
let termWrap!: HTMLDivElement;
let termHost!: HTMLDivElement;
let footL!: HTMLSpanElement;
let footM!: HTMLSpanElement;
let footHint!: HTMLSpanElement;
let footR!: HTMLSpanElement;
let inp!: HTMLInputElement;
let psEl!: HTMLSpanElement;
let echoEl!: HTMLSpanElement;
let sug!: HTMLDivElement;
let res!: HTMLDivElement;

function mustEl<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Action bar missing #${id}`);
  return el as T;
}

function bindDom() {
  root = mustEl("cli-root");
  titleEl = mustEl("cli-title");
  modeLabel = mustEl("cli-mode-label");
  termWrap = mustEl("cli-term-wrap");
  termHost = mustEl("cli-term");
  footL = mustEl("cli-foot-l");
  footM = mustEl("cli-foot-m");
  footHint = mustEl("cli-foot-r-hint");
  footR = mustEl("cli-foot-r");
  inp = mustEl("cli-in");
  psEl = mustEl("cli-ps");
  echoEl = mustEl("cli-echo");
  sug = mustEl("cli-sug");
  res = mustEl("cli-res");
}

let HOME = "";
let cwd = "";
let PRESETS: Preset[] = [];
let USER_SHORTCUTS: Preset[] = [];
let hist: string[] = [];
let hi = 0;
let rows: Row[] = [];
let rowSel = -1;
/** True while a command is in flight — blocks blur-dismiss (pwsh steals focus on Windows). */
let busy = false;
/** Bumped by forceIdle so a stale withBusy finally cannot re-lock the bar. */
let busyGen = 0;
let blurTimer: ReturnType<typeof setTimeout> | null = null;
/** Outside-click dismiss debounce. scheduleDismiss re-arms if busy flips. */
const BLUR_DISMISS_MS = 280;
/** Post-command hold when the action steals focus (browser, Explorer, editor). */
const FOCUS_STEAL_GRACE_MS = 320;
/** Failsafe — must exceed cli_exec timeout (4s). */
const BUSY_WATCHDOG_MS = 4500;

let mode: LauncherMode = "action";
let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let ptyDataUnlisten: UnlistenFn | null = null;
let ptyExitUnlisten: UnlistenFn | null = null;
let termSeed: string | null = null;
/** Backend PTY still running (may be detached from the UI). */
let sessionAlive = false;
/** Coalesce pty://data into one term.write per animation frame. */
let ptyWriteBuf = "";
let ptyRaf: number | null = null;

const esc = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const joinPath = (a: string, b: string) => (a.endsWith("\\") ? a + b : a + "\\" + b);
const psQuote = (s: string) => `'${s.replace(/'/g, "''")}'`;
const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function clearBlurTimer() {
  if (blurTimer) {
    clearTimeout(blurTimer);
    blurTimer = null;
  }
}

/** Drop the busy lock immediately (hide/show/reset). Stale async work must not re-arm it. */
function forceIdle(_reason: string) {
  busyGen += 1;
  busy = false;
  clearBlurTimer();
}

/**
 * Gen-aware dismiss scheduler — if busy flips before fire, re-arm instead of
 * dismissing mid-command or using a stale timer from a previous blur.
 */
function scheduleDismiss(ms: number, reason: string) {
  clearBlurTimer();
  const genAtSchedule = busyGen;
  blurTimer = setTimeout(() => {
    blurTimer = null;
    if (mode === "terminal") return;
    if (busy || genAtSchedule !== busyGen) {
      scheduleDismiss(ms, reason);
      return;
    }
    void getCurrentWindow()
      .isFocused()
      .then((still) => {
        if (!still && mode === "action" && !busy) dismissAction(reason);
      })
      .catch(() => {});
  }, ms);
}

function setPrompt() {
  psEl.textContent = `PS ${cwd}>`;
}
function syncEcho() {
  echoEl.textContent = inp.value;
}
function setRes(cls: "ok" | "err" | "out", html: string) {
  res.className = `cli-res on ${cls}`;
  res.innerHTML = html;
}
function clearRes() {
  res.className = "cli-res";
  res.innerHTML = "";
}

async function withBusy<T>(
  fn: () => Promise<T>,
  opts: { focusSteals?: boolean } = {},
): Promise<T | undefined> {
  if (busy) {
    return undefined;
  }
  const focusSteals = opts.focusSteals ?? true;
  const gen = ++busyGen;
  busy = true;
  clearBlurTimer();
  // Failsafe: hung invoke must not leave the bar looking alive-but-dead.
  const watchdog = setTimeout(() => {
    if (gen === busyGen && busy) {
      forceIdle("watchdog");
      setRes("err", "command timed out — dismissed");
      void invoke("dismiss_launcher");
    }
  }, BUSY_WATCHDOG_MS);
  try {
    return await fn();
  } finally {
    clearTimeout(watchdog);
    if (focusSteals) await delay(FOCUS_STEAL_GRACE_MS);
    if (gen === busyGen) {
      busy = false;
      if (mode === "action") void inp.focus();
    }
  }
}

function showRows(list: Row[]) {
  rows = list;
  rowSel = -1;
  sug.innerHTML = "";
  rows.forEach((r) => {
    const d = document.createElement("div");
    d.className = "cl-s";
    d.innerHTML = `<b>${esc(r.c)}</b><span>${esc(r.d || "")}</span>`;
    d.onmousedown = (e) => {
      e.preventDefault();
      activateRow(r);
    };
    sug.appendChild(d);
  });
}
function markSel() {
  sug.querySelectorAll(".cl-s").forEach((d, i) => d.classList.toggle("on", i === rowSel));
}
function pick(r: Row) {
  // Profile row → list that profile's shortcuts directly.
  if (r.cat && isProfileName(r.cat)) {
    presetRows(r.cat);
    return;
  }
  if (r.cc !== undefined) {
    inp.value = r.cc;
    syncEcho();
    inp.focus();
    refreshProposals();
  } else if (r.path) {
    void openPath(r.path);
  }
}

/** Resolve what Enter should execute from a highlighted suggestion. */
function commandFromRow(r: Row): string | null {
  if (r.path) return null; // handled separately
  // Profile rows and template completions (trailing space / <placeholder>) fill, don't run.
  if (r.cat && isProfileName(r.cat)) return null;
  if (r.cc !== undefined && (r.cc.endsWith(" ") || /<[^>]+>/.test(r.c))) {
    return null;
  }
  return (r.cc ?? r.c).trim() || null;
}

function submitCommand(raw: string) {
  const v = raw.trim();
  if (busy) {
    return;
  }
  inp.value = "";
  syncEcho();
  rowSel = -1;
  if (v) hist.push(v);
  hi = 0;
  run(v);
}

/** Run, open, or complete a suggestion row (shared by Enter and click). */
function activateRow(r: Row) {
  if (r.path) {
    rowSel = -1;
    void openPath(r.path);
    return;
  }
  const cmd = commandFromRow(r);
  if (cmd) {
    submitCommand(cmd);
    return;
  }
  pick(r);
}

function defaults() {
  showRows([
    { c: "profiles", d: "personal · work · dev · ai · fun · folders · apps", cat: "profiles" },
    { c: "?  <query>", d: "Google search", cc: "? " },
    { c: "hf <q>", d: "Hugging Face · gh github · yt youtube · w wiki", cc: "hf " },
    { c: "?? <query>", d: "search files", cc: "?? " },
  ]);
}

function findPreset(name: string): Preset | undefined {
  const p = name.toLowerCase();
  return (
    PRESETS.find((x) => x.n === p) ||
    PRESETS.find((x) => (x.aliases ?? []).includes(p)) ||
    PRESETS.find((x) => x.n.startsWith(p)) ||
    PRESETS.find((x) => (x.aliases ?? []).some((a) => a.startsWith(p)))
  );
}

function presetMatches(x: Preset, low: string): boolean {
  if (!low) return true;
  if (x.n.startsWith(low) || x.n.includes(low) || x.d.toLowerCase().includes(low)) return true;
  return (x.aliases ?? []).some((a) => a.startsWith(low) || a.includes(low));
}

function commandForQuery(x: Preset, low: string): string {
  if (x.n.startsWith(low)) return x.n;
  const alias = (x.aliases ?? []).find((a) => a.startsWith(low));
  return alias ?? x.n;
}

/** All profile names, longest first so "personal" wins over "per". */
function profileNames(): string[] {
  return [...new Set(PRESETS.map((x) => x.cat))].sort((a, b) => b.length - a.length);
}

function isProfileName(name: string): boolean {
  const p = name.toLowerCase();
  return profileNames().some((cat) => cat === p);
}

/** Shortcut inside a profile: `personal mail` → mail in personal. */
function findInProfile(cat: string, name: string): Preset | undefined {
  const c = cat.toLowerCase();
  const n = name.toLowerCase().trim();
  if (!n) return undefined;
  const scoped = PRESETS.filter((x) => x.cat.toLowerCase() === c);
  return (
    scoped.find((x) => x.n === n) ||
    scoped.find((x) => (x.aliases ?? []).includes(n)) ||
    scoped.find((x) => x.n.startsWith(n)) ||
    scoped.find((x) => (x.aliases ?? []).some((a) => a.startsWith(n)))
  );
}

function suggestions(raw: string): Row[] {
  const s = raw.trim();
  const out: Row[] = [];
  if (/^\?/.test(s)) {
    const q = s.replace(/^\?+\s*/, "");
    if (!s.startsWith("??")) out.push({ c: "? " + q, d: "Google search" });
    out.push({ c: "?? " + q, d: "search files" });
    return out;
  }
  const hf = raw.match(/^\s*hf(?:\s+(.*))?$/i);
  if (hf) {
    const q = (hf[1] || "").trim();
    if (!q) {
      out.push({ c: "hf", d: "open Hugging Face" });
      out.push({ c: "hf ", d: "search models · spaces · papers" });
    } else {
      out.push({ c: "hf " + q, d: "Hugging Face search" });
    }
    return out;
  }
  const m = raw.match(/^\s*(?:open|o|presets?)(?:\s+(.*))?$/i);
  if (m) {
    const p = (m[1] || "").toLowerCase();
    if (!p || "config".startsWith(p)) out.push({ c: "config", d: "open desktop/index.html" });
    if (!p || "desktopfile".startsWith(p)) out.push({ c: "desktopfile", d: "open desktop/index.html" });
    PRESETS.filter((x) => !p || presetMatches(x, p) || x.cat.toLowerCase().startsWith(p))
      .slice(0, 8)
      .forEach((x) => out.push({ c: commandForQuery(x, p), d: `${x.cat} · ${x.d}`, cc: x.n }));
    return out;
  }

  // `personal mail` / `personal m` → shortcuts inside that profile
  const parts = s.split(/\s+/);
  if (parts.length >= 2 && isProfileName(parts[0].toLowerCase())) {
    const cat = parts[0].toLowerCase();
    const rest = parts.slice(1).join(" ").toLowerCase();
    PRESETS.filter((x) => x.cat.toLowerCase() === cat && (!rest || presetMatches(x, rest)))
      .slice(0, 10)
      .forEach((x) => out.push({ c: commandForQuery(x, rest), d: `${x.cat} · ${x.d}`, cc: x.n }));
    if (!out.length) out.push({ c: cat, d: `no match in ${cat} · tab to browse`, cat });
    return out;
  }

  const low = s.toLowerCase();
  if (low && !s.includes(" ")) {
    if (sessionAlive && ("continue".startsWith(low) || "attach".startsWith(low))) {
      out.push({ c: "continue", d: "reattach background terminal" });
    }
    if ("config".startsWith(low)) out.push({ c: "config", d: "open desktop/index.html" });
    if ("desktopfile".startsWith(low)) out.push({ c: "desktopfile", d: "open desktop/index.html" });
    if ("term".startsWith(low) || "shell".startsWith(low)) {
      out.push({
        c: sessionAlive ? "term new" : "term",
        d: sessionAlive ? "new terminal (kills background)" : "open embedded terminal",
      });
    }
    if ("lock".startsWith(low)) out.push({ c: "lock", d: "lock workstation" });
    if ("start".startsWith(low)) out.push({ c: "start", d: "Start menu · installed apps" });
    if ("showdesk".startsWith(low) || "peek".startsWith(low)) {
      out.push({ c: "showdesk", d: "show desktop" });
    }
    if ("desk".startsWith(low)) out.push({ c: "desk", d: "toggle the HTML desktop page" });
    if ("hide".startsWith(low)) out.push({ c: "hide ", d: "hide an auto-added app" });
    // Profile completion: Tab/Enter into a category browses it.
    profileNames()
      .filter((cat) => cat.startsWith(low))
      .slice(0, 4)
      .forEach((cat) =>
        out.push({ c: cat, d: `profile · ${PRESETS.filter((x) => x.cat === cat).length} shortcuts`, cat }),
      );
    PRESETS.filter((x) => presetMatches(x, low))
      .slice(0, 8)
      .forEach((x) => out.push({ c: commandForQuery(x, low), d: `${x.cat} · ${x.d}` }));
  }
  return out;
}

function refreshProposals() {
  const raw = inp.value;
  if (!raw.trim()) return defaults();
  if (raw.endsWith(" ")) {
    const base = raw.trim().toLowerCase();
    const inCat = PRESETS.filter((x) => x.cat.toLowerCase() === base || x.cat.toLowerCase().startsWith(base));
    if (inCat.length) {
      presetRows(base);
      return;
    }
  }
  const list = suggestions(raw);
  // Keep explicit completion (profile rows end with a space); rows without cc run on Enter.
  if (list.length) showRows(list.map((x) => ({ ...x, cc: x.cc ?? x.c })));
  else {
    const hint = needsTerminal(raw) ? "↵ open terminal" : "↵ run inline (pwsh)";
    showRows([{ c: raw, d: hint }]);
  }
}

function firstLine(s: string, max = 120): string {
  const line = s.trim().split(/\r?\n/, 1)[0] ?? "";
  return line.length > max ? line.slice(0, max) + "…" : line;
}

function formatBlock(text: string, maxLines = 14, maxChars = 2400): string {
  const lines = text.trim().split(/\r?\n/).filter(Boolean);
  if (!lines.length) return "";
  const slice = lines.slice(0, maxLines);
  let html = slice.map((l) => esc(l)).join("<br>");
  if (lines.length > maxLines) html += "<br>…";
  if (html.length > maxChars) html = html.slice(0, maxChars) + "…";
  return html;
}

/** Installers / interactive scripts need a real terminal — not the 4s inline runner. */
function needsTerminal(cmd: string): boolean {
  const raw = cmd.trim();
  if (/^!!/.test(raw)) return true;
  const low = raw.toLowerCase();
  return (
    /\|\s*(iex|invoke-expression)\b/.test(low) ||
    /\.ps1\b/.test(low) ||
    /\b(read-host|install-module|install-package|winget\s+install|choco\s+install|scoop\s+install)\b/.test(
      low,
    )
  );
}

function stripTerminalBang(cmd: string): string {
  return cmd.replace(/^!!\s?/, "").trim();
}

function stripInlineBang(cmd: string): string {
  return cmd.replace(/^!\s?/, "").trim();
}

function b64ToUtf8(b64: string): string {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

function flushPtyWriteBuf() {
  ptyRaf = null;
  if (!ptyWriteBuf || !term) {
    ptyWriteBuf = "";
    return;
  }
  const chunk = ptyWriteBuf;
  ptyWriteBuf = "";
  term.write(chunk);
}

function enqueuePtyData(text: string) {
  ptyWriteBuf += text;
  if (ptyRaf == null) {
    ptyRaf = requestAnimationFrame(flushPtyWriteBuf);
  }
}

function clearPtyWriteBuf() {
  if (ptyRaf != null) {
    cancelAnimationFrame(ptyRaf);
    ptyRaf = null;
  }
  ptyWriteBuf = "";
}

function applyChrome(next: LauncherMode) {
  root.dataset.mode = next;
  if (next === "terminal") {
    titleEl.textContent = "versailles · pwsh";
    modeLabel.textContent = "terminal";
    footL.textContent = "esc detach";
    footM.textContent = "ctrl+c";
    footHint.textContent = "paste ok";
    footR.textContent = sessionAlive ? "background live" : "";
  } else {
    modeLabel.textContent = sessionAlive ? "actions · live" : "actions";
  }
}

async function refreshSessionAlive() {
  try {
    sessionAlive = await invoke<boolean>("pty_is_alive");
  } catch (e) {
    sessionAlive = false;
  }
}

async function bindPtyListeners() {
  if (ptyDataUnlisten) {
    ptyDataUnlisten();
    ptyDataUnlisten = null;
  }
  if (ptyExitUnlisten) {
    ptyExitUnlisten();
    ptyExitUnlisten = null;
  }
  ptyDataUnlisten = await listen<string>("pty://data", (ev) => {
    try {
      enqueuePtyData(b64ToUtf8(ev.payload));
    } catch {
      /* ignore decode glitches */
    }
  });
  ptyExitUnlisten = await listen("pty://exit", () => {
    sessionAlive = false;
    term?.writeln("\r\n\x1b[90m[session ended — esc returns to actions]\x1b[0m");
    if (mode === "action") {
      applyChrome("action");
      defaults();
    }
  });
}

function ensureTerm(): Terminal {
  if (term) return term;
  const t = new Terminal({
    convertEol: true,
    cursorBlink: true,
    cursorStyle: "bar",
    fontFamily: '"Cascadia Mono", "JetBrains Mono", Consolas, monospace',
    fontSize: 13,
    lineHeight: 1.25,
    scrollback: 5000,
    allowProposedApi: true,
    theme: {
      background: "#131212",
      foreground: "#f9fafb",
      cursor: "#f9fafb",
      cursorAccent: "#131212",
      selectionBackground: "rgba(249, 250, 251, 0.18)",
      black: "#1c1c1e",
      red: "#ff6b62",
      green: "#30d158",
      yellow: "#ffd60a",
      blue: "#64d2ff",
      magenta: "#bf5af2",
      cyan: "#64d2ff",
      white: "#f2f2f7",
      brightBlack: "#636366",
      brightRed: "#ff6961",
      brightGreen: "#9ac324",
      brightYellow: "#ffd426",
      brightBlue: "#70d7ff",
      brightMagenta: "#da8fff",
      brightCyan: "#70d7ff",
      brightWhite: "#ffffff",
    },
  });
  const fit = new FitAddon();
  t.loadAddon(fit);
  t.open(termHost);
  fitAddon = fit;
  term = t;

  t.onData((data) => {
    void invoke("pty_write", { data }).catch(() => {});
  });

  termHost.addEventListener("paste", (e) => {
    if (mode !== "terminal") return;
    const text = e.clipboardData?.getData("text");
    if (!text) return;
    e.preventDefault();
    void invoke("pty_write", { data: text }).catch(() => {});
  });

  return t;
}

async function resizeLauncher(next: LauncherMode) {
  const win = getCurrentWindow();
  // Resize from the webview side — avoids IPC→main-thread deadlock that hangs the app process.
  try {
    const monitor = await currentMonitor();
    const scale = monitor?.scaleFactor ?? 1;
    const monW = monitor ? monitor.size.width / scale : 1920;
    const monH = monitor ? monitor.size.height / scale : 1080;

    let w: number;
    let h: number;
    if (next === "terminal") {
      // Full pwsh session — large centered pane, not the compact action bar.
      w = Math.round(Math.min(1280, Math.max(960, monW * 0.78)));
      h = Math.round(Math.min(860, Math.max(640, monH * 0.78)));
    } else {
      w = 640;
      h = 420;
    }

    await win.setSize(new LogicalSize(w, h));

    if (monitor) {
      const x = monitor.position.x + Math.round((monitor.size.width - w * scale) / 2);
      const y =
        next === "terminal"
          ? monitor.position.y + Math.round((monitor.size.height - h * scale) / 2)
          : monitor.position.y + Math.round(120 * scale);
      await win.setPosition(new PhysicalPosition(x, y));
    }
  } catch {
    // Fallback sizes if monitor query fails.
    await win.setSize(
      next === "terminal" ? new LogicalSize(1100, 720) : new LogicalSize(640, 420),
    );
  }
}

async function fitAndResizePty() {
  if (!term || !fitAddon) return;
  fitAddon.fit();
  const cols = term.cols;
  const rows = term.rows;
  try {
    await invoke("pty_resize", { cols, rows });
  } catch {
    /* session may not be open yet */
  }
}

/** Hide terminal UI but keep the PTY (and xterm scrollback) alive. */
async function detachTerminal(focusAction = true) {
  if (mode !== "terminal") {
    if (focusAction) {
      await refreshSessionAlive();
      applyChrome("action");
      defaults();
      inp.focus();
    }
    return;
  }
  clearBlurTimer();
  mode = "action";
  applyChrome("action");
  termWrap.hidden = true;
  try {
    await resizeLauncher("action");
  } catch {
    /* ignore */
  }
  await refreshSessionAlive();
  if (focusAction) {
    defaults();
    inp.focus();
  }
}

/** Kill the background PTY and drop the xterm buffer. */
async function killTerminalSession() {
  try {
    await invoke("pty_close");
  } catch {
    /* ignore */
  }
  sessionAlive = false;
  if (ptyDataUnlisten) {
    ptyDataUnlisten();
    ptyDataUnlisten = null;
  }
  if (ptyExitUnlisten) {
    ptyExitUnlisten();
    ptyExitUnlisten = null;
  }
  if (term) {
    term.reset();
  }
}

async function enterTerminal(seedCmd?: string, opts: { fresh?: boolean } = {}) {
  const fresh = opts.fresh === true;
  termSeed = seedCmd?.trim() || null;

  await withBusy(async () => {
    try {
      await refreshSessionAlive();

      if (fresh && sessionAlive) {
        await killTerminalSession();
      }

      if (mode === "terminal" && sessionAlive && !fresh) {
        if (termSeed) {
          const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
          await invoke("pty_write", { data: payload });
          termSeed = null;
        }
        term?.focus();
        return;
      }

      // Attach existing background session — keep scrollback, don't reset.
      if (sessionAlive && !fresh) {
        mode = "terminal";
        applyChrome("terminal");
        termWrap.hidden = false;
        await resizeLauncher("terminal");
        await delay(80);
        ensureTerm();
        if (!ptyDataUnlisten) await bindPtyListeners();
        await fitAndResizePty();
        term?.focus();
        if (termSeed) {
          const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
          await invoke("pty_write", { data: payload });
          termSeed = null;
        }
        return;
      }

      // Fresh session
      mode = "terminal";
      applyChrome("terminal");
      termWrap.hidden = false;
      await resizeLauncher("terminal");
      await delay(80);
      const t = ensureTerm();
      t.reset();
      clearPtyWriteBuf();
      fitAddon?.fit();
      const cols = t.cols || 80;
      const rows = t.rows || 24;
      await bindPtyListeners();
      await invoke("pty_open", { cwd, cols, rows });
      sessionAlive = true;
      await fitAndResizePty();
      t.focus();

      if (termSeed) {
        await delay(180);
        const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
        await invoke("pty_write", { data: payload });
        termSeed = null;
      }
    } catch (e) {
      forceIdle("enterTerminal-error");
      setRes("err", esc(String(e)));
      sessionAlive = false;
      try {
        await detachTerminal(false);
      } catch {
        mode = "action";
        termWrap.hidden = true;
        applyChrome("action");
      }
    }
  }, { focusSteals: false });
}

async function openPath(path: string) {
  await withBusy(async () => {
    try {
      await invoke("cli_open", { target: path });
      setRes("ok", `&rarr; opened <span class="link">${esc(path)}</span>`);
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  });
}

function webSearch(q: string) {
  if (!q) return setRes("err", "? : missing query — usage: ? &lt;query&gt;");
  openTarget("https://www.google.com/search?q=" + encodeURIComponent(q));
}

function openTarget(target: string) {
  void withBusy(async () => {
    try {
      await invoke("cli_open", { target });
      setRes("ok", `&rarr; opened <span class="link">${esc(target)}</span>`);
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  });
}

function calcExpr(expr: string) {
  if (!expr) return setRes("err", "= : missing expression — usage: = 1+2*3");
  const safe = expr.replace(/,/g, ".");
  if (!/^[0-9+\-*/().\s%^]*$/.test(safe)) {
    return setRes("err", "= : only numbers and + - * / % ^ ( ) allowed");
  }
  void withBusy(async () => {
    try {
      const ps = safe.replace(/\^/g, "**");
      const out = await invoke<CliOutput>("cli_exec", {
        cmd: `[math]::Round((${ps}), 6)`,
        cwd,
      });
      if (out.code !== 0 || out.stderr.trim()) {
        setRes("err", esc(firstLine(out.stderr) || "calc failed"));
      } else {
        setRes("ok", `${esc(safe)} = <b>${esc(out.stdout.trim())}</b>`);
      }
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

function wikiSearch(q: string) {
  if (!q) return setRes("err", "w : missing query — usage: w &lt;topic&gt;");
  openTarget("https://en.wikipedia.org/wiki/Special:Search?search=" + encodeURIComponent(q));
}

function ytSearch(q: string) {
  if (!q) return setRes("err", "yt : missing query — usage: yt &lt;query&gt;");
  openTarget("https://www.youtube.com/results?search_query=" + encodeURIComponent(q));
}

function ghSearch(q: string) {
  if (!q) return openTarget("https://github.com/search");
  openTarget("https://github.com/search?q=" + encodeURIComponent(q));
}

function hfSearch(q: string) {
  const raw = q.trim();
  const tpl =
    root?.getAttribute("data-search-hf") || "https://huggingface.co/models?search={q}";
  if (!raw) return openTarget("https://huggingface.co/models");
  const parts = raw.split(/\s+/);
  const kind = (parts[0] || "").toLowerCase();
  const rest = parts.slice(1).join(" ").trim();
  if (["models", "datasets", "spaces", "papers"].includes(kind)) {
    if (!rest) {
      return openTarget(`https://huggingface.co/${kind}`);
    }
    if (kind === "papers") {
      return openTarget("https://huggingface.co/papers?q=" + encodeURIComponent(rest));
    }
    return openTarget(`https://huggingface.co/${kind}?search=` + encodeURIComponent(rest));
  }
  openTarget(tpl.replaceAll("{q}", encodeURIComponent(raw)));
}

async function fileSearch(q: string) {
  if (!q) return setRes("err", "?? : missing query — usage: ?? &lt;query&gt;");
  setRes("out", `searching files for "${esc(q)}"…`);
  await withBusy(async () => {
    try {
      const matches = await invoke<string[]>("cli_search_files", { query: q });
      if (!matches.length) {
        setRes("out", `no files match "${esc(q)}"`);
        return defaults();
      }
      setRes("out", `${matches.length} match${matches.length > 1 ? "es" : ""} for "${esc(q)}"`);
      showRows(matches.slice(0, 6).map((p) => ({ c: p, path: p })));
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

function presetRows(filter?: string) {
  const f = (filter || "").toLowerCase();
  const list = f
    ? PRESETS.filter((x) => presetMatches(x, f) || x.cat.toLowerCase().includes(f))
    : PRESETS;
  if (!list.length) {
    setRes("err", `presets : nothing matches '${esc(filter || "")}'`);
    return defaults();
  }
  const cats = [...new Set(list.map((x) => x.cat))];
  setRes("out", f ? `${list.length} in “${esc(f)}”` : `${list.length} shortcuts · ${cats.join(" · ")}`);
  showRows(list.map((x) => ({ c: x.n, d: `${x.cat} · ${x.d}`, cc: x.n })));
}

async function openFileTarget(target: string, label: string) {
  await withBusy(async () => {
    try {
      await invoke("cli_open", { target });
      setRes("ok", `&rarr; opened <span class="link">${esc(label)}</span>`);
      await hideLauncherWindow("openFileTarget");
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

async function openPreset(arg: string) {
  const p = arg.toLowerCase().trim();
  if (!p) return presetRows();
  if (p === "config") return void openConfigFile();
  if (p === "desktopfile") return void openDesktopFile();
  // Category browse: `open mail` already exact; `presets uni` / `open uni` for uni mail name.
  const hit = findPreset(p);
  if (!hit) {
    // Maybe they typed a category name
    const inCat = PRESETS.filter((x) => x.cat.toLowerCase() === p || x.cat.toLowerCase().startsWith(p));
    if (inCat.length > 1) return presetRows(p);
    if (inCat.length === 1) {
      return void launchPreset(inCat[0]);
    }
    return setRes("err", `no shortcut '${esc(arg)}' — try 'presets'`);
  }
  return void launchPreset(hit);
}

function widgetsRoot(home: string) {
  return joinPath(home, "Documents\\Widgets");
}

async function openConfigFile() {
  return openDesktopFile();
}

async function openDesktopFile() {
  return openFileTarget(joinPath(widgetsRoot(HOME || "C:\\"), "desktop\\index.html"), "desktop/index.html");
}

async function launchPreset(hit: Preset) {
  await withBusy(async () => {
    try {
      await invoke("cli_open", { target: hit.target });
      if (hit.t === "web") {
        setRes("ok", `&rarr; opened <span class="link">${esc(hit.d)}</span>`);
      } else if (hit.t === "folder") {
        setRes("ok", `&rarr; opening <span class="link">${esc(hit.d)}</span> in Explorer`);
      } else {
        setRes("ok", `&rarr; launching ${esc(hit.d)}`);
      }
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  });
}

function expandHome(target: string, home: string): string {
  return target
    .replace(/^~([\\/]|$)/, home + "$1")
    .replace(/%HOME%/gi, home)
    .replace(/\$HOME/g, home);
}

function normalizeUserShortcut(raw: Preset, home: string): Preset {
  return {
    ...raw,
    target: raw.t === "folder" ? expandHome(raw.target, home) : raw.target,
  };
}

function mergePresetsInto(base: Preset[], extras: Preset[]): Preset[] {
  const result = [...base];
  const byId = new Map(result.map((p, i) => [p.n, i]));
  for (const p of extras) {
    const idx = byId.get(p.n);
    if (idx !== undefined) {
      result[idx] = p;
    } else {
      byId.set(p.n, result.length);
      result.push(p);
    }
  }
  return result;
}

function parseVersaillesBlock(html: string): { shortcuts?: Preset[] } | null {
  try {
    const doc = new DOMParser().parseFromString(html, "text/html");
    const el = doc.getElementById("versailles");
    if (!el) return null;
    return JSON.parse(el.textContent || "");
  } catch {
    return null;
  }
}

async function loadUserShortcuts(): Promise<void> {
  USER_SHORTCUTS = [];
  const home = HOME || "C:\\";
  try {
    let html = "";
    try {
      html = await invoke<string>("get_desktop_html");
    } catch {
      const api = await invoke<{ base_url: string }>("get_api_info");
      const res = await fetch(`${api.base_url}/files/desktop/index.html`, { cache: "no-store" });
      if (res.ok) html = await res.text();
    }
    const block = html ? parseVersaillesBlock(html) : null;
    if (block && Array.isArray(block.shortcuts)) {
      USER_SHORTCUTS = block.shortcuts.map((s) => normalizeUserShortcut(s, home));
      return;
    }
    const api = await invoke<{ base_url: string }>("get_api_info");
    const base = api.base_url;
    let shortcutsPath = "shortcuts.json";
    try {
      const cfgRes = await fetch(`${base}/files/versailles.json`, { cache: "no-store" });
      if (cfgRes.ok) {
        const cfg = (await cfgRes.json()) as { shortcuts?: string | Preset[] };
        if (Array.isArray(cfg.shortcuts)) {
          USER_SHORTCUTS = cfg.shortcuts.map((s) => normalizeUserShortcut(s, home));
          return;
        }
        if (typeof cfg.shortcuts === "string" && cfg.shortcuts.trim()) {
          shortcutsPath = cfg.shortcuts.replace(/\\/g, "/").replace(/^\//, "");
        }
      }
    } catch {
      /* default shortcuts.json */
    }
    const res = await fetch(`${base}/files/${shortcutsPath}`, { cache: "no-store" });
    if (!res.ok) return;
    const data = (await res.json()) as { shortcuts?: Preset[] };
    if (!Array.isArray(data.shortcuts)) return;
    USER_SHORTCUTS = data.shortcuts.map((s) => normalizeUserShortcut(s, home));
  } catch {
    /* builtins only */
  }
}

function builtinPresets(home: string): Preset[] {
  return [
    { n: "mail", t: "web", d: "Gmail", target: "https://mail.google.com/", cat: "personal" },
    { n: "github", t: "web", d: "GitHub", target: "https://github.com/", cat: "dev" },
    { n: "downloads", t: "folder", d: "Downloads", target: joinPath(home, "Downloads"), cat: "folders" },
    { n: "documents", t: "folder", d: "Documents", target: joinPath(home, "Documents"), cat: "folders" },
    { n: "desktop", t: "folder", d: "Desktop", target: joinPath(home, "Desktop"), cat: "folders" },
  ];
}

function applyCatalog(entries: CatalogEntry[]) {
  const builtins = mergePresetsInto(builtinPresets(HOME || "C:\\"), USER_SHORTCUTS);
  const byId = new Map(builtins.map((p, i) => [p.n, i]));
  for (const e of entries) {
    const preset: Preset = {
      n: e.id,
      t: "app",
      d: e.fresh ? `new · ${e.name}` : e.name,
      target: e.target,
      cat: "apps",
      aliases: (e.aliases || []).filter((a) => a !== e.id),
    };
    const idx = byId.get(e.id);
    if (idx !== undefined && builtins[idx].t === "app") {
      builtins[idx] = preset;
    } else if (idx === undefined) {
      builtins.push(preset);
      byId.set(e.id, builtins.length - 1);
    }
  }
  PRESETS = builtins;
}

async function loadCatalog() {
  try {
    const entries = await invoke<CatalogEntry[]>("list_catalog");
    applyCatalog(entries);
    void invoke("ack_catalog").catch(() => {});
    return entries.some((e) => e.fresh);
  } catch {
    applyCatalog([]);
    return false;
  }
}

async function refreshPresets() {
  await loadUserShortcuts();
  return loadCatalog();
}

async function cdCmd(arg: string) {
  if (!arg || arg === "~") {
    cwd = HOME;
    setPrompt();
    return setRes("out", esc(cwd));
  }
  const p = arg.replace(/\//g, "\\");
  let target: string;
  if (/^[A-Za-z]:[\\/]/.test(p)) {
    target = p;
  } else {
    const stack: string[] = [];
    (cwd + "\\" + p).split("\\").forEach((seg) => {
      if (seg === "..") stack.pop();
      else if (seg && seg !== ".") stack.push(seg);
    });
    target = stack.join("\\");
  }
  target = target.replace(/\\+$/, "");
  await withBusy(async () => {
    try {
      const out = await invoke<CliOutput>("cli_exec", {
        cmd: `if (Test-Path -LiteralPath ${psQuote(target)} -PathType Container) { Write-Output 'OK' }`,
        cwd,
      });
      if (out.stdout.trim() === "OK") {
        cwd = target;
        setPrompt();
        setRes("out", esc(cwd));
      } else {
        setRes("err", `cd : Cannot find path '${esc(target)}' because it does not exist.`);
      }
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

async function lsCmd() {
  await withBusy(async () => {
    try {
      const out = await invoke<CliOutput>("cli_exec", {
        cmd: "Get-ChildItem -Force | ForEach-Object { if ($_.PSIsContainer) { 'D|' + $_.Name } else { 'F|' + $_.Name } }",
        cwd,
      });
      if (out.code !== 0) return setRes("err", esc(firstLine(out.stderr) || "ls failed"));
      const entries = out.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
      const dirs = entries.filter((e) => e.startsWith("D|")).map((e) => e.slice(2));
      const files = entries.filter((e) => e.startsWith("F|")).map((e) => e.slice(2));
      const n = dirs.length + files.length;
      if (!n) {
        setRes("out", "(empty) — " + esc(cwd));
        return defaults();
      }
      setRes("out", `${n} entr${n === 1 ? "y" : "ies"} — ${esc(cwd)}`);
      showRows(
        [
          ...dirs.map((d) => ({ c: d + "/", d: "dir", cc: "cd " + d })),
          ...files.map((f) => ({ c: f, d: "file", path: joinPath(cwd, f) })),
        ].slice(0, 6),
      );
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

async function shellExec(cmd: string) {
  const trimmed = cmd.trim();
  if (/^!!/.test(trimmed)) {
    const run = stripTerminalBang(trimmed);
    return void enterTerminal(run || undefined);
  }
  const inline = /^!\s?/.test(trimmed) ? stripInlineBang(trimmed) : trimmed;
  if (!inline) return setRes("err", "! : missing command — usage: ! Get-Date");
  if (needsTerminal(trimmed) && !/^!\s?/.test(trimmed)) {
    return void enterTerminal(inline);
  }
  setRes("out", "…");
  await withBusy(async () => {
    try {
      const out = await invoke<CliOutput>("cli_exec", { cmd: inline, cwd });
      if (out.code === 0 && !out.stderr.trim()) {
        const block = formatBlock(out.stdout);
        if (block) setRes("out", block);
        else setRes("ok", "&rarr; done (exit 0)");
      } else {
        const block = formatBlock(out.stderr || out.stdout);
        if (block) setRes("err", block);
        else setRes("err", esc(`exit ${out.code}`));
      }
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
}

function run(c: string) {
  if (!c) return;
  if (c === "cls" || c === "clear") {
    clearRes();
    return defaults();
  }
  if (c.startsWith("??")) return void fileSearch(c.slice(2).trim());
  if (c.startsWith("?")) return void webSearch(c.slice(1).trim());
  if (c.startsWith("=")) return void calcExpr(c.slice(1).trim());
  if (c.startsWith("!!") || c.startsWith("!")) return void shellExec(c);
  const sp = c.split(/\s+/);
  const cmd = sp[0].toLowerCase();
  const arg = sp.slice(1).join(" ");
  switch (cmd) {
    case "config":
      return void openConfigFile();
    case "desktopfile":
      return void openDesktopFile();
    case "open":
    case "o":
      return void openPreset(arg);
    case "presets":
    case "shortcuts":
      return presetRows(arg);
    case "continue":
    case "attach":
      return void enterTerminal();
    case "term":
    case "shell":
    case "ps": {
      const sub = arg.trim().toLowerCase();
      if (sub === "new" || sub === "fresh") return void enterTerminal(undefined, { fresh: true });
      if (sub === "kill" || sub === "close") {
        return void (async () => {
          await killTerminalSession();
          setRes("ok", "&rarr; background terminal closed");
          applyChrome("action");
          defaults();
        })();
      }
      // `term` alone reattaches if a session is live; otherwise opens fresh.
      return void enterTerminal();
    }
    case "w":
    case "wiki":
      return void wikiSearch(arg);
    case "yt":
    case "youtube":
      if (!arg) {
        const hit = findPreset(cmd);
        if (hit) return void launchPreset(hit);
      }
      return void ytSearch(arg);
    case "gh":
      if (!arg) {
        const hit = findPreset(cmd) || findPreset("github");
        if (hit) return void launchPreset(hit);
      }
      return void ghSearch(arg);
    case "hf":
      if (!arg) {
        const hit = findPreset(cmd) || findPreset("huggingface");
        if (hit) return void launchPreset(hit);
      }
      return void hfSearch(arg);
    case "lock":
      return void withBusy(async () => {
        try {
          await invoke("cli_exec", { cmd: "rundll32.exe user32.dll,LockWorkStation", cwd });
          setRes("ok", "&rarr; locked");
        } catch (e) {
          setRes("err", esc(String(e)));
        }
      }, { focusSteals: false });
    case "apps":
      return presetRows("apps");
    case "start":
      if (!arg) return presetRows("apps");
      return void openPreset(arg);
    case "showdesk":
    case "peek":
      return void withBusy(async () => {
        try {
          await invoke("shell_show_desktop");
          setRes("ok", "&rarr; show desktop");
        } catch (e) {
          setRes("err", esc(String(e)));
        }
      });
    case "desk":
      return void withBusy(async () => {
        try {
          const on = await invoke<boolean>("toggle_desktop_surface");
          setRes("ok", on ? "&rarr; desktop page" : "&rarr; closed desktop");
          if (on) await hideLauncherWindow("desk");
        } catch (e) {
          setRes("err", esc(String(e)));
        }
      }, { focusSteals: false });
    case "hide": {
      const id = arg.trim().toLowerCase();
      if (!id) return setRes("err", "hide : usage hide &lt;app&gt;");
      const hit = findPreset(id);
      const targetId = hit?.t === "app" ? hit.n : id;
      return void withBusy(async () => {
        try {
          const entries = await invoke<CatalogEntry[]>("hide_catalog_entry", { id: targetId });
          applyCatalog(entries);
          setRes("ok", `&rarr; hid ${esc(targetId)} from apps`);
          presetRows("apps");
        } catch (e) {
          setRes("err", esc(String(e)));
        }
      }, { focusSteals: false });
    }
    case "help":
      setRes("out", "grammar — ? web · ?? files · = calc · term · profile names (personal, work, dev, fun, apps…)");
      return showRows([
        ...(sessionAlive
          ? [{ c: "continue", d: "reattach background terminal", cc: "continue" }]
          : []),
        { c: "term", d: "open / reattach terminal", cc: "term" },
        { c: "config", d: "open desktop/index.html", cc: "config" },
        { c: "desktopfile", d: "open desktop/index.html", cc: "desktopfile" },
        { c: "? <query>", d: "Google search", cc: "? " },
        { c: "?? <query>", d: "search files", cc: "?? " },
        { c: "= <expr>", d: "quick calculator", cc: "= " },
        { c: "! <cmd>", d: "run inline pwsh", cc: "! " },
        { c: "!!", d: "open detachable terminal", cc: "!!" },
        { c: "hf <q>", d: "Hugging Face models (hf models|datasets|spaces)", cc: "hf " },
        { c: "mail · github", d: "Gmail · GitHub (extend via #versailles in index.html)", cc: "mail" },
        { c: "start", d: "Start menu · installed apps", cc: "start" },
        { c: "showdesk", d: "Show desktop (taskbar Win+D)", cc: "showdesk" },
        { c: "desk", d: "toggle the HTML desktop page", cc: "desk" },
        { c: "hide <app>", d: "remove an auto-added app from the bar", cc: "hide " },
        { c: "presets <cat>", d: "browse personal · work · dev · fun · apps …", cc: "presets " },
        { c: "lock", d: "lock workstation", cc: "lock" },
        { c: "cls", d: "reset the bar", cc: "cls" },
      ]);
    case "pwd":
    case "get-location":
      return setRes("out", esc(cwd));
    case "cd":
      return void cdCmd(arg);
    case "ls":
    case "dir":
    case "get-childitem":
      return void lsCmd();
    case "exit":
      return setRes("out", "alt+space hides · esc detaches terminal (keeps it running)");
    default: {
      // `personal mail` → shortcut inside profile; bare `personal` → browse.
      if (isProfileName(cmd)) {
        if (arg) {
          const scoped = findInProfile(cmd, arg);
          if (scoped) return void launchPreset(scoped);
          return setRes("err", `no “${esc(arg)}” in ${esc(cmd)} · tab to browse`);
        }
        return presetRows(cmd);
      }
      // Direct preset name — `open` is optional.
      const hit = findPreset(cmd);
      if (hit && !arg) return void launchPreset(hit);
      if (hit && arg && hit.n === cmd) return void launchPreset(hit);
      return void shellExec(c);
    }
  }
}

/** Hide via Rust dismiss — deferred hide emits launcher://hidden (forceIdle). Never await. */
async function hideLauncherWindow(reason: string) {
  forceIdle(reason);
  if (mode === "terminal") {
    try {
      await detachTerminal(false);
    } catch {
      mode = "action";
      termWrap.hidden = true;
    }
  }
  inp.value = "";
  syncEcho();
  clearRes();
  applyChrome("action");
  // Fire-and-forget: awaiting window.hide from this webview deadlocks WebView2.
  void invoke("dismiss_launcher");
}

function dismissAction(reason: string) {
  void hideLauncherWindow(reason);
}

function bindUi() {
inp.addEventListener("input", () => {
  syncEcho();
  clearRes();
  refreshProposals();
});

inp.addEventListener("keydown", (e) => {
  if (mode !== "action") {
    return;
  }
  if (e.key === "Escape") {
    // Handled by document capture listener (works even when input isn't focused).
    return;
  } else if (e.key === "Tab") {
    e.preventDefault();
    const r = (rowSel >= 0 && rows[rowSel]) || rows.find((x) => x.cc !== undefined);
    if (r) pick(r);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    if (inp.value.trim() && rows.length) {
      rowSel = Math.max(rowSel < 0 ? rows.length - 1 : rowSel - 1, 0);
      markSel();
    } else if (hist.length) {
      hi = Math.min(hi + 1, hist.length);
      inp.value = hist[hist.length - hi];
      syncEcho();
      refreshProposals();
    }
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    if (inp.value.trim() && rows.length) {
      rowSel = Math.min(rowSel + 1, rows.length - 1);
      markSel();
    } else if (hi > 0) {
      hi--;
      inp.value = hi ? hist[hist.length - hi] : "";
      syncEcho();
      refreshProposals();
    }
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (rowSel >= 0 && rows[rowSel]) {
      activateRow(rows[rowSel]);
      return;
    }
    submitCommand(inp.value);
  }
});

document.addEventListener(
  "keydown",
  (e) => {
    if (e.key !== "Escape") return;
    if (mode === "terminal") {
      e.preventDefault();
      e.stopPropagation();
      void detachTerminal(true);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    dismissAction("escape");
  },
  true,
);

document.querySelector(".cli")!.addEventListener("click", () => {
  if (mode === "action") inp.focus();
  else term?.focus();
});

void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (focused) {
    clearBlurTimer();
    return;
  }
  // Terminal mode: keep session alive on focus flicker; only Action auto-dismisses.
  if (mode === "terminal") return;
  const blurDelay = busy ? Math.max(BLUR_DISMISS_MS, FOCUS_STEAL_GRACE_MS + 80) : BLUR_DISMISS_MS;
  scheduleDismiss(blurDelay, "outside-blur");
});

window.addEventListener("resize", () => {
  if (mode === "terminal") void fitAndResizePty();
});
}

async function resetBar(seed?: string) {
  forceIdle("resetBar");
  // Reopen always lands on Action; background PTY stays alive for `continue`.
  if (mode === "terminal") {
    await detachTerminal(false);
  }
  await refreshSessionAlive();
  await refreshPresets();
  inp.value = "";
  syncEcho();
  clearRes();
  setPrompt();
  applyChrome("action");
  applySeed(seed);
  inp.focus();
}

function applySeed(seed?: string) {
  if (seed == null || seed === "") return defaults();
  const trimmed = seed.trim();
  if (trimmed === "apps" || trimmed === "start") return presetRows("apps");
  inp.value = seed;
  syncEcho();
  refreshProposals();
}

void (async () => {
  const v = (window as unknown as { versailles?: { waitForTauri?: () => Promise<unknown> } }).versailles;
  if (v?.waitForTauri) await v.waitForTauri();
  bindDom();
  bindUi();

  const onShown = (ev: { payload?: string } | string) => {
    const seed = typeof ev === "string" ? ev : typeof ev?.payload === "string" ? ev.payload : "";
    void resetBar(seed);
  };
  await listen<string>("overlay://shown", onShown);
  await listen<string>("launcher://shown", onShown);
  await listen("overlay://hidden", () => {
    forceIdle("hidden");
    void (async () => {
      if (mode === "terminal") await detachTerminal(false);
      inp.value = "";
      syncEcho();
      clearRes();
      applyChrome("action");
    })();
  });
  await listen("launcher://hidden", () => {
    forceIdle("hidden");
    void (async () => {
      if (mode === "terminal") await detachTerminal(false);
      inp.value = "";
      syncEcho();
      clearRes();
      applyChrome("action");
    })();
  });

  try {
    HOME = await invoke<string>("cli_home");
  } catch {
    HOME = "";
  }
  if (!HOME) HOME = "C:\\";
  cwd = HOME;

  PRESETS = builtinPresets(HOME);
  await refreshPresets();

  await refreshSessionAlive();
  applyChrome("action");
  setPrompt();
  defaults();
  inp.focus();
})();
