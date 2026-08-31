import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWindow, currentMonitor } from "@tauri-apps/api/window";
import { Terminal } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import "xterm/css/xterm.css";
import {
  loadSpawnableEngineContext,
  loadEngineRuntime,
  pushRecent,
  togglePin,
  saveLastTermSeed,
  type EngineRuntimeState,
} from "./engine/spawnable-config";
import {
  rebuildFuseIndex,
  fuzzyPresets,
  parseCategoryFilter,
  presetMatchesCat,
  findPresetStrict,
  duplicateShortcutIds,
  validatePreset,
  orderDefaults,
  playLaunchTick,
} from "./engine/search";

type CliOutput = { stdout: string; stderr: string; code: number };

type Preset = {
  n: string;
  t: "web" | "folder" | "app" | "term";
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
  /** Pulsing live-session marker (background PTY). */
  live?: boolean;
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
let headDot!: HTMLElement;

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
  headDot = root.querySelector(".cli-head i") as HTMLElement;
  if (!headDot) throw new Error("Action bar missing .cli-head i");
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
/**
 * Auto-dismiss calls forceIdle at 280ms, which is inside Windows key-repeat
 * (~250ms) and double-click (~500ms). A second Enter/click then launches again.
 * This cooldown survives forceIdle and is shared across a double-bound engine.
 */
const LAUNCH_COOLDOWN_MS = 700;

type BarWindow = Window & {
  __VERSAILLES_BAR_BOUND__?: boolean;
  __VERSAILLES_LAST_LAUNCH__?: number;
};

function barWindow(): BarWindow {
  return window as BarWindow;
}

function launchBlocked(): boolean {
  const at = barWindow().__VERSAILLES_LAST_LAUNCH__ || 0;
  return Date.now() - at < LAUNCH_COOLDOWN_MS;
}

function claimLaunch(): boolean {
  if (launchBlocked()) return false;
  barWindow().__VERSAILLES_LAST_LAUNCH__ = Date.now();
  return true;
}

let mode: LauncherMode = "action";
let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let searchAddon: SearchAddon | null = null;
let termFindOpen = false;
let termFindCase = false;
let termMenuBound = false;
let ptyDataUnlisten: UnlistenFn | null = null;
let ptyExitUnlisten: UnlistenFn | null = null;
let termSeed: string | null = null;
/** Backend PTY still running (may be detached from the UI). */
let sessionAlive = false;
let ENGINE_ID = "action-bar";
let ENGINE_OPTS = {
  blurDismissMs: BLUR_DISMISS_MS,
  suggestionLimit: 12,
  compact: false,
  launchTick: false,
  searchHf: "https://huggingface.co/models?search={q}",
  timeAwareDefaults: true,
  autoDismissLaunch: true,
};
let ENGINE_RUNTIME: EngineRuntimeState = { recents: [], pins: [] };
let lastLaunchError: { preset: Preset; err: string } | null = null;
let escClearPending = false;
let termSessionLabel = "";
let hostAvailable = true;
/** Coalesce pty://data into one term.write per animation frame. */
let ptyWriteBuf = "";
let ptyRaf: number | null = null;

const WEB_TLDS = new Set([
  "com", "org", "net", "io", "dev", "app", "ai", "co", "me", "tv", "gg", "to",
  "sh", "rs", "edu", "gov", "info", "xyz", "de", "uk", "eu", "us", "ca", "au",
  "nl", "fr", "it", "es", "ch", "at", "be", "se", "no", "dk", "fi", "pl", "cz",
  "in", "jp", "kr", "cn", "ru", "br", "mx", "nz", "ie", "pt",
]);

/** Turn typed text into a browser URL, or null if it is not address-like. */
function looksLikeUrl(raw: string): string | null {
  const s = raw.trim();
  if (!s || /\s/.test(s)) return null;
  if (/^https?:\/\/.+/i.test(s)) return s;
  if (/^www\.[a-z0-9]/i.test(s)) return "https://" + s;
  if (/^localhost(?::\d{1,5})?(?:[/?#].*)?$/i.test(s)) return "http://" + s;
  if (/^\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?(?:[/?#].*)?$/.test(s)) return "http://" + s;
  const host = s.split(/[/?#]/)[0]?.split(":")[0] ?? "";
  const parts = host.split(".");
  if (parts.length < 2) return null;
  const tld = (parts[parts.length - 1] || "").toLowerCase();
  if (!WEB_TLDS.has(tld)) return null;
  if (!/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/i.test(host)) {
    return null;
  }
  return "https://" + s;
}

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

function continueRow(): Row {
  const label = termSessionLabel ? `reattach · ${termSessionLabel}` : "reattach background terminal";
  return { c: "continue", d: label, cc: "continue", live: true };
}

function syncLiveChrome() {
  const live = sessionAlive && mode === "action";
  root.classList.toggle("cli-live", live);
  headDot.classList.toggle("cli-live-dot", sessionAlive);
  modeLabel.classList.toggle("cli-live-badge", sessionAlive);
}

function showRows(list: Row[]) {
  rows = list;
  rowSel = -1;
  sug.innerHTML = "";
  rows.forEach((r) => {
    const d = document.createElement("div");
    d.className = r.live ? "cl-s cl-s-live" : "cl-s";
    d.innerHTML = `<b>${esc(r.c)}</b> <span>${esc(r.d || "")}</span>`;
    d.title = r.path || r.d || "";
    d.onmousedown = (e) => {
      e.preventDefault();
      if (e.detail > 1 || launchBlocked()) return;
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

function submitCommand(raw: string, background = false) {
  const v = raw.trim();
  if (busy || launchBlocked()) return;
  escClearPending = false;
  inp.value = "";
  syncEcho();
  rowSel = -1;
  if (v) hist.push(v);
  hi = 0;
  run(v, background);
}

/** Run, open, or complete a suggestion row (shared by Enter and click). */
function activateRow(r: Row, background = false) {
  if (r.path) {
    rowSel = -1;
    void openPath(r.path);
    return;
  }
  const cmd = commandFromRow(r);
  if (cmd) {
    submitCommand(cmd, background);
    return;
  }
  pick(r);
}

function defaults() {
  clearRes();
  const limit = ENGINE_OPTS.suggestionLimit;
  const verbs: Row[] = [
    { c: "?", d: "search the web", cc: "? " },
    { c: "!!", d: "open a terminal", cc: "!!" },
  ];
  const ordered = orderDefaults(PRESETS, ENGINE_RUNTIME.recents, ENGINE_RUNTIME.pins, {
    timeAware: ENGINE_OPTS.timeAwareDefaults,
    limit,
  });
  const rows: Row[] = [];
  const pinSet = new Set(ENGINE_RUNTIME.pins);
  const recentSet = new Set(ENGINE_RUNTIME.recents);
  for (const x of ordered) {
    let tag = x.cat;
    if (pinSet.has(x.n)) tag = `pin · ${x.cat}`;
    else if (recentSet.has(x.n)) tag = `recent · ${x.cat}`;
    rows.push({ c: x.n, d: `${tag} · ${x.d}`, cc: x.n });
  }
  if (root) root.classList.toggle("cli-compact", ENGINE_OPTS.compact);
  const prefix = sessionAlive ? [continueRow()] : [];
  showRows([...prefix, ...rows, ...verbs]);
}

function findPreset(name: string): Preset | undefined {
  return findPresetStrict(PRESETS, name);
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
  const typedUrl = looksLikeUrl(s);
  if (typedUrl || /^https?:\/\//i.test(s) || /^www\./i.test(s)) {
    return [{ c: s, d: "open in browser", cc: s }];
  }
  if (/^\?/.test(s)) {
    const q = s.replace(/^\?+\s*/, "");
    const qUrl = looksLikeUrl(q);
    if (qUrl) return [{ c: s, d: "open in browser", cc: s }];
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

  const { cat: catFilter, rest: catRest } = parseCategoryFilter(s);
  if (catFilter && !catRest) {
    PRESETS.filter((x) => presetMatchesCat(x, catFilter))
      .slice(0, ENGINE_OPTS.suggestionLimit)
      .forEach((x) => out.push({ c: x.n, d: `${x.cat} · ${x.d}`, cc: x.n }));
    return out;
  }
  const low = (catRest || s).toLowerCase();
  if (low && !s.includes(" ")) {
    if (sessionAlive && ("continue".startsWith(low) || "attach".startsWith(low))) {
      out.push(continueRow());
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
    const fuzzy = fuzzyPresets(low, ENGINE_OPTS.suggestionLimit);
    const list = fuzzy.length ? fuzzy : PRESETS.filter((x) => presetMatches(x, low));
    list
      .slice(0, ENGINE_OPTS.suggestionLimit)
      .forEach((x) =>
        out.push({
          c: commandForQuery(x, low),
          d: `${x.cat} · ${x.d} · ${esc(x.target).slice(0, 48)}`,
        }),
      );
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
    const label = termSessionLabel || "pwsh";
    titleEl.textContent = `versailles · ${label}`;
    modeLabel.textContent = "terminal";
    footL.textContent = "alt+space hide";
    footM.textContent = "ctrl+f find";
    footHint.textContent = "right-click session";
    footR.textContent = sessionAlive ? "live · background ok" : "right-click menu";
  } else {
    titleEl.textContent = "versailles";
    modeLabel.textContent = sessionAlive ? "live" : "actions";
    footL.textContent = "ctrl+l clear";
    footM.textContent = "ctrl+1-9";
    footHint.textContent = "esc";
    const n = PRESETS.filter((x) => x.cat !== "apps").length;
    footR.textContent = n
      ? `${n} shortcuts · ? web · https:// · !! term`
      : "? web · https:// · !! term · help";
  }
  syncLiveChrome();
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
      if (termSessionLabel) {
        setRes("out", "terminal ended");
        const hit = findPreset(termSessionLabel);
        if (hit) showRows([{ c: hit.n, d: `reopen · ${hit.d}`, cc: hit.n }]);
        else defaults();
      } else defaults();
    }
  });
}

const SEARCH_DECO = {
  matchBackground: "#3f3f46",
  matchBorder: "#6b7280",
  matchOverviewRuler: "#6b7280",
  activeMatchBackground: "#f9fafb",
  activeMatchBorder: "#141414",
  activeMatchColorOverviewRuler: "#f9fafb",
};

async function copyTermSelection(): Promise<boolean> {
  if (!term?.hasSelection()) return false;
  const text = term.getSelection();
  if (!text) return false;
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

async function pasteToTerm(): Promise<boolean> {
  let text = "";
  try {
    text = await navigator.clipboard.readText();
  } catch {
    return false;
  }
  if (!text) return false;
  void invoke("pty_write", { data: text }).catch(() => {});
  return true;
}

function selectAllTerm() {
  term?.selectAll();
}

function clearTermBuffer() {
  term?.clear();
}

function searchOpts(incremental = false) {
  return {
    caseSensitive: termFindCase,
    incremental,
    decorations: SEARCH_DECO,
  };
}

function runTermFind(dir: "next" | "prev", incremental = false) {
  if (!searchAddon) return;
  const input = document.getElementById("cli-term-find-in") as HTMLInputElement | null;
  const q = input?.value ?? "";
  if (!q) {
    searchAddon.clearDecorations();
    term?.clearSelection();
    return;
  }
  const opts = searchOpts(incremental);
  if (dir === "prev") searchAddon.findPrevious(q, opts);
  else searchAddon.findNext(q, opts);
}

function ensureFindBar() {
  if (document.getElementById("cli-term-find")) return;
  const bar = document.createElement("div");
  bar.id = "cli-term-find";
  bar.className = "cli-term-find";
  bar.innerHTML =
    '<input id="cli-term-find-in" type="search" spellcheck="false" autocomplete="off" placeholder="Find" aria-label="Find in terminal" />' +
    '<button type="button" id="cli-term-find-prev" title="Previous">↑</button>' +
    '<button type="button" id="cli-term-find-next" title="Next">↓</button>' +
    '<button type="button" id="cli-term-find-case" title="Match case">Aa</button>' +
    '<button type="button" id="cli-term-find-close" title="Close">×</button>';
  termWrap.appendChild(bar);
  const input = bar.querySelector("#cli-term-find-in") as HTMLInputElement;
  const prev = bar.querySelector("#cli-term-find-prev") as HTMLButtonElement;
  const next = bar.querySelector("#cli-term-find-next") as HTMLButtonElement;
  const cse = bar.querySelector("#cli-term-find-case") as HTMLButtonElement;
  const close = bar.querySelector("#cli-term-find-close") as HTMLButtonElement;
  input.addEventListener("input", () => runTermFind("next", true));
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      runTermFind(e.shiftKey ? "prev" : "next");
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hideTermFind();
    } else if (e.key.toLowerCase() === "f" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
    }
  });
  prev.addEventListener("click", () => runTermFind("prev"));
  next.addEventListener("click", () => runTermFind("next"));
  cse.addEventListener("click", () => {
    termFindCase = !termFindCase;
    cse.classList.toggle("on", termFindCase);
    runTermFind("next", true);
  });
  close.addEventListener("click", () => hideTermFind());
  bar.addEventListener("mousedown", (e) => e.stopPropagation());
}

function showTermFind() {
  if (!searchAddon) return;
  ensureFindBar();
  const bar = document.getElementById("cli-term-find");
  const input = document.getElementById("cli-term-find-in") as HTMLInputElement | null;
  if (!bar || !input) return;
  bar.classList.add("on");
  termFindOpen = true;
  input.focus();
  input.select();
}

function hideTermFind() {
  document.getElementById("cli-term-find")?.classList.remove("on");
  termFindOpen = false;
  searchAddon?.clearDecorations();
  if (mode === "terminal") term?.focus();
}

function ensureTermMenu() {
  if (document.getElementById("cli-term-menu")) return;
  const menu = document.createElement("div");
  menu.id = "cli-term-menu";
  menu.className = "cli-term-menu";
  menu.innerHTML =
    '<button type="button" data-act="copy">Copy</button>' +
    '<button type="button" data-act="paste">Paste</button>' +
    '<button type="button" data-act="select-all">Select All</button>' +
    '<button type="button" data-act="clear">Clear</button>' +
    '<button type="button" data-act="new">New session</button>' +
    '<button type="button" data-act="kill">Kill session</button>';
  document.body.appendChild(menu);
  menu.addEventListener("mousedown", (e) => e.stopPropagation());
  menu.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn) return;
    const act = btn.getAttribute("data-act");
    hideTermMenu();
    if (act === "copy") void copyTermSelection();
    else if (act === "paste") void pasteToTerm();
    else if (act === "select-all") selectAllTerm();
    else if (act === "clear") clearTermBuffer();
    else if (act === "new") {
      forceIdle("term-new");
      void enterTerminal(undefined, { fresh: true });
    } else if (act === "kill") {
      void closeTerminalFromCommand();
    }
  });
  if (!termMenuBound) {
    termMenuBound = true;
    document.addEventListener("mousedown", (e) => {
      if (!menu.classList.contains("on")) return;
      if (menu.contains(e.target as Node)) return;
      hideTermMenu();
    });
  }
}

function showTermMenu(x: number, y: number) {
  ensureTermMenu();
  const menu = document.getElementById("cli-term-menu");
  if (!menu) return;
  const copyBtn = menu.querySelector('[data-act="copy"]') as HTMLButtonElement | null;
  if (copyBtn) copyBtn.disabled = !term?.hasSelection();
  menu.classList.add("on");
  const pad = 8;
  const w = menu.offsetWidth || 148;
  const h = menu.offsetHeight || 140;
  menu.style.left = `${Math.min(x, window.innerWidth - w - pad)}px`;
  menu.style.top = `${Math.min(y, window.innerHeight - h - pad)}px`;
}

function hideTermMenu() {
  document.getElementById("cli-term-menu")?.classList.remove("on");
}

function bindTermChrome(t: Terminal) {
  t.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== "keydown") return true;
    const key = ev.key.toLowerCase();
    const mod = ev.ctrlKey || ev.metaKey;

    if (key === "escape") return true;
    if (ev.shiftKey && ev.key === "PageUp") {
      t.scrollPages(-1);
      return false;
    }
    if (ev.shiftKey && ev.key === "PageDown") {
      t.scrollPages(1);
      return false;
    }
    if (mod && key === "f") {
      showTermFind();
      return false;
    }
    if (mod && ev.shiftKey && key === "c") {
      void copyTermSelection();
      return false;
    }
    if (mod && key === "c" && t.hasSelection()) {
      void copyTermSelection();
      return false;
    }
    if (mod && key === "v") {
      void pasteToTerm();
      return false;
    }
    if (ev.shiftKey && key === "insert") {
      void pasteToTerm();
      return false;
    }
    if (ev.ctrlKey && key === "insert") {
      void copyTermSelection();
      return false;
    }
    return true;
  });

  termHost.addEventListener(
    "contextmenu",
    (e) => {
      if (mode !== "terminal") return;
      e.preventDefault();
      e.stopPropagation();
      showTermMenu(e.clientX, e.clientY);
    },
    true,
  );

  termHost.addEventListener("paste", (e) => {
    if (mode !== "terminal") return;
    const text = e.clipboardData?.getData("text");
    if (!text) return;
    e.preventDefault();
    void invoke("pty_write", { data: text }).catch(() => {});
  });
}

function ensureTerm(): Terminal {
  if (term?.options.convertEol) {
    try {
      term.dispose();
    } catch {
      /* ignore */
    }
    term = null;
    fitAddon = null;
    searchAddon = null;
  }
  if (term) return term;
  const t = new Terminal({
    // ConPTY already translates newlines — convertEol wraps TUIs onto line 1.
    convertEol: false,
    cursorBlink: true,
    cursorStyle: "bar",
    fontFamily: '"Cascadia Mono", "JetBrains Mono", Consolas, monospace',
    fontSize: 13,
    lineHeight: 1,
    scrollback: 10000,
    scrollSensitivity: 1,
    smoothScrollDuration: 0,
    fastScrollModifier: "alt",
    fastScrollSensitivity: 5,
    drawBoldTextInBrightColors: true,
    minimumContrastRatio: 4.5,
    wordSeparator: " ()[]{}',\"`\"",
    windowsPty: { backend: "conpty", buildNumber: 26100 },
    overviewRulerWidth: 0,
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
  try {
    const search = new SearchAddon();
    t.loadAddon(search);
    searchAddon = search;
  } catch {
    searchAddon = null;
  }
  try {
    t.loadAddon(
      new WebLinksAddon((_ev, uri) => {
        void invoke("cli_open", { target: uri }).catch(() => {});
      }),
    );
  } catch {
    /* addon not loaded */
  }
  t.open(termHost);
  fitAddon = fit;
  term = t;

  if (typeof ResizeObserver !== "undefined") {
    let fitTimer = 0;
    new ResizeObserver(() => {
      if (mode !== "terminal") return;
      window.clearTimeout(fitTimer);
      fitTimer = window.setTimeout(() => {
        void fitAndResizePty();
      }, 40);
    }).observe(termHost);
  }

  t.onData((data) => {
    void invoke("pty_write", { data }).catch(() => {});
  });

  bindTermChrome(t);

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
    document.body.style.width = "";
    document.body.style.height = "";

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

function fitTermExact() {
  if (!term || !fitAddon) return;
  const proposed = fitAddon.proposeDimensions();
  if (!proposed || !proposed.cols || !proposed.rows) {
    fitAddon.fit();
    return;
  }
  // FitAddon uses the host's border box, so host padding is counted as cells.
  // Those extra cols wrap a full-width TUI row and the top line walks to the bottom.
  const style = getComputedStyle(termHost);
  const padX =
    (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0);
  const cellW =
    proposed.cols > 0 && termHost.clientWidth > 0
      ? termHost.clientWidth / proposed.cols
      : 9;
  const padCols = cellW > 1 && padX > 0 ? Math.ceil(padX / cellW) : 0;
  const cols = Math.max(20, proposed.cols - padCols - 1);
  const rows = Math.max(8, proposed.rows);
  if (term.cols !== cols || term.rows !== rows) {
    term.resize(cols, rows);
  }
}

async function fitAndResizePty() {
  if (!term || !fitAddon) return;
  fitTermExact();
  const cols = term.cols;
  const rows = term.rows;
  try {
    await invoke("pty_resize", { cols, rows });
  } catch {
    /* session may not be open yet */
  }
}

async function settleTermSize() {
  fitTermExact();
  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  fitTermExact();
  await delay(60);
  await fitAndResizePty();
}

function termSizeReady(t: Terminal): boolean {
  return (
    termHost.clientWidth > 0 &&
    termHost.clientHeight > 0 &&
    (t.cols || 0) >= 20 &&
    (t.rows || 0) >= 8
  );
}

/** Wait until the canvas host has a real fit — a 0-size PTY leaves Ink TUIs blank. */
async function waitForTermSize(t: Terminal): Promise<{ cols: number; rows: number }> {
  for (let i = 0; i < 16; i++) {
    fitTermExact();
    if (termSizeReady(t)) {
      return { cols: t.cols, rows: t.rows };
    }
    await delay(40);
  }
  fitTermExact();
  return { cols: Math.max(20, t.cols || 80), rows: Math.max(8, t.rows || 24) };
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
  hideTermFind();
  hideTermMenu();
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

async function closeTerminalFromCommand() {
  forceIdle("term-kill");
  await killTerminalSession();
  termSessionLabel = "";
  try {
    await detachTerminal(false);
  } catch {
    mode = "action";
    termWrap.hidden = true;
  }
  await refreshSessionAlive();
  applyChrome("action");
  defaults();
  setRes("ok", "&rarr; background terminal closed");
  inp.focus();
}

async function enterTerminal(seedCmd?: string, opts: { fresh?: boolean } = {}) {
  const fresh = opts.fresh === true;
  termSeed = seedCmd?.trim() || null;
  clearBlurTimer();
  void invoke("arm_overlay_focus_guard", { ms: 800 }).catch(() => {});

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
        await settleTermSize();
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
      const size = await waitForTermSize(t);
      await bindPtyListeners();
      await invoke("pty_open", { cwd, cols: size.cols, rows: size.rows });
      sessionAlive = true;
      await settleTermSize();
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
  if (!claimLaunch()) return;
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
  const url = looksLikeUrl(q);
  if (url) return openTarget(url);
  openTarget("https://www.google.com/search?q=" + encodeURIComponent(q));
}

function openTarget(target: string, opts: { background?: boolean } = {}) {
  if (!claimLaunch()) return;
  void withBusy(async () => {
    try {
      if (!hostAvailable) throw new Error("host unavailable — preview mode cannot launch");
      await invoke("cli_open", { target });
      playLaunchTick(ENGINE_OPTS.launchTick);
      setRes("ok", `&rarr; opened <span class="link">${esc(target)}</span>`);
      if (ENGINE_OPTS.autoDismissLaunch && !opts.background) {
        await delay(280);
        await hideLauncherWindow("launch");
      }
    } catch (e) {
      setRes("err", esc(String(e)));
    }
  }, { focusSteals: false });
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
  if (!claimLaunch()) return;
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

async function launchPreset(hit: Preset, opts: { background?: boolean } = {}) {
  if (hit.t === "term") {
    clearBlurTimer();
    termSessionLabel = hit.n;
    void saveLastTermSeed(ENGINE_ID, hit.target);
    return void enterTerminal(hit.target, { fresh: true });
  }
  lastLaunchError = null;
  if (!claimLaunch()) return;
  await withBusy(async () => {
    try {
      if (!hostAvailable) throw new Error("host unavailable — preview mode cannot launch");
      await invoke("cli_open", { target: hit.target });
      void pushRecent(ENGINE_ID, hit.n);
      playLaunchTick(ENGINE_OPTS.launchTick);
      if (hit.t === "web") {
        setRes("ok", `&rarr; opened <span class="link">${esc(hit.d)}</span>`);
      } else if (hit.t === "folder") {
        setRes("ok", `&rarr; opening <span class="link">${esc(hit.d)}</span> in Explorer`);
      } else {
        setRes("ok", `&rarr; launching ${esc(hit.d)}`);
      }
      if (ENGINE_OPTS.autoDismissLaunch && !opts.background) {
        await delay(280);
        await hideLauncherWindow("launch");
      }
    } catch (e) {
      const msg = String(e);
      lastLaunchError = { preset: hit, err: msg };
      setRes("err", esc(msg));
      showRows([
        { c: `retry ${hit.n}`, d: "try again", cc: hit.n },
        { c: "config", d: "edit shortcuts in index.html", cc: "config" },
      ]);
    }
  }, { focusSteals: false });
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
    const inline = (window as unknown as { __VERSAILLES_BLOCK__?: { shortcuts?: Preset[] } })
      .__VERSAILLES_BLOCK__;
    if (inline && Array.isArray(inline.shortcuts)) {
      USER_SHORTCUTS = inline.shortcuts.map((s) => normalizeUserShortcut(s, home));
      return;
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
  const dups = duplicateShortcutIds(USER_SHORTCUTS);
  const bad = USER_SHORTCUTS.map((p) => validatePreset(p)).filter(Boolean) as string[];
  if (dups.length || bad.length) {
    const msg = [
      dups.length ? `duplicate: ${dups.slice(0, 4).join(", ")}` : "",
      bad.length ? bad.slice(0, 2).join(" · ") : "",
    ]
      .filter(Boolean)
      .join(" · ");
    setRes("err", esc(msg));
  }
  rebuildFuseIndex(PRESETS.length ? PRESETS : USER_SHORTCUTS);
  ENGINE_RUNTIME = await loadEngineRuntime(ENGINE_ID);
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

function run(c: string, background = false) {
  const bg = { background };
  if (!c) return;
  if (background && c.includes(" ")) {
    const parts = c.split(/\s+/);
    void (async () => {
      for (const name of parts) {
        const hit = findPreset(name);
        if (hit) await launchPreset(hit, { background: true });
      }
    })();
    return;
  }
  if (c === "cls" || c === "clear") {
    clearRes();
    return defaults();
  }
  if (c.startsWith("??")) return void fileSearch(c.slice(2).trim());
  if (c.startsWith("?")) return void webSearch(c.slice(1).trim());
  if (c.startsWith("=")) return void calcExpr(c.slice(1).trim());
  if (c.startsWith("!!") || c.startsWith("!")) return void shellExec(c);
  const typedUrl = looksLikeUrl(c);
  if (typedUrl) return openTarget(typedUrl);
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
      if (sub === "new" || sub === "fresh") {
        forceIdle("term-new");
        return void enterTerminal(undefined, { fresh: true });
      }
      if (sub === "kill" || sub === "close") {
        return void closeTerminalFromCommand();
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
      setRes("out", "type a shortcut name · or one of these");
      return showRows([
        ...(sessionAlive ? [continueRow()] : []),
        { c: "?", d: "search the web", cc: "? " },
        { c: "https://", d: "open a URL", cc: "https://" },
        { c: "??", d: "search files", cc: "?? " },
        { c: "!!", d: "open a terminal", cc: "!!" },
        { c: "!", d: "run pwsh inline", cc: "! " },
        { c: "=", d: "calculator", cc: "= " },
        { c: "start", d: "installed apps", cc: "start" },
        { c: "desk", d: "toggle the desktop page", cc: "desk" },
        { c: "config", d: "edit index.html", cc: "config" },
        { c: "lock", d: "lock workstation", cc: "lock" },
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
      return setRes("out", "alt+space hides · terminal stays running in the background");
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
      if (hit && !arg) return void launchPreset(hit, bg);
      if (hit && arg && hit.n === cmd) return void launchPreset(hit, bg);
      if (cmd.startsWith("retry ") && lastLaunchError) return void launchPreset(lastLaunchError.preset);
      if (cmd.startsWith("pin ") || cmd.startsWith("unpin ")) {
        const name = cmd.split(/\s+/)[1] || "";
        return void (async () => {
          ENGINE_RUNTIME = await togglePin(ENGINE_ID, name);
          setRes("ok", `&rarr; pins updated`);
          defaults();
        })();
      }
      if (!arg && !c.startsWith("!") && !c.startsWith("?") && !c.startsWith("=") && !/[\\/]/.test(c) && /^[\w.-]+$/i.test(cmd)) {
        const near = PRESETS.filter((x) => presetMatches(x, cmd)).slice(0, 6);
        if (near.length) {
          setRes("err", `no shortcut '${esc(cmd)}' — did you mean?`);
          showRows(near.map((x) => ({ c: x.n, d: `${x.cat} · ${x.d}`, cc: x.n })));
          return;
        }
        setRes("err", `no shortcut '${esc(cmd)}' — try 'shortcuts'`);
        return;
      }
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

function focusPrompt() {
  if (mode === "terminal") {
    term?.focus();
    return;
  }
  try {
    inp.focus();
  } catch {
    /* HWND not ready yet */
  }
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
    e.stopImmediatePropagation();
    if (e.repeat || launchBlocked()) return;
    if (rowSel >= 0 && rows[rowSel]) {
      activateRow(rows[rowSel], e.ctrlKey);
      return;
    }
    submitCommand(inp.value, e.ctrlKey);
  } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "l") {
    e.preventDefault();
    inp.value = "";
    syncEcho();
    clearRes();
    defaults();
  } else if (e.ctrlKey && !e.shiftKey && e.key === "p") {
    e.preventDefault();
    if (hist.length) {
      hi = Math.min(hi + 1, hist.length);
      inp.value = hist[hist.length - hi];
      syncEcho();
      refreshProposals();
    }
  } else if (e.ctrlKey && !e.shiftKey && e.key === "n") {
    e.preventDefault();
    if (hi > 0) {
      hi--;
      inp.value = hi ? hist[hist.length - hi] : "";
      syncEcho();
      refreshProposals();
    }
  } else if (e.altKey && /^[1-9]$/.test(e.key)) {
    const pin = ENGINE_RUNTIME.pins[Number(e.key) - 1];
    if (pin) {
      e.preventDefault();
      const hit = findPreset(pin);
      if (hit) void launchPreset(hit);
    }
  } else if (e.ctrlKey && /^[1-9]$/.test(e.key)) {
    e.preventDefault();
    const idx = Number(e.key) - 1;
    if (rows[idx]) activateRow(rows[idx], e.ctrlKey);
  }
});

document.addEventListener(
  "keydown",
  (e) => {
    if (mode === "terminal" && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      showTermFind();
      return;
    }
    const esc = e.key === "Escape" || e.code === "Escape";
    if (!esc) return;
    const termOpen = mode === "terminal" || root?.dataset.mode === "terminal" || (termWrap && !termWrap.hidden);
    if (termOpen) {
      if (termFindOpen) {
        e.preventDefault();
        e.stopPropagation();
        hideTermFind();
        return;
      }
      const menu = document.getElementById("cli-term-menu");
      if (menu?.classList.contains("on")) {
        e.preventDefault();
        e.stopPropagation();
        hideTermMenu();
        return;
      }
      return;
    }
    if (inp.value.trim()) {
      e.preventDefault();
      e.stopPropagation();
      if (escClearPending) {
        escClearPending = false;
        dismissAction("escape");
        return;
      }
      escClearPending = true;
      inp.value = "";
      syncEcho();
      clearRes();
      defaults();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    dismissAction("escape");
  },
  true,
);

document.querySelector(".cli")!.addEventListener("click", (e) => {
  e.stopPropagation();
  clearBlurTimer();
  focusPrompt();
});
termWrap.addEventListener("mousedown", (e) => {
  e.stopPropagation();
  clearBlurTimer();
  if (mode === "terminal") term?.focus();
});
root.addEventListener("mousedown", () => clearBlurTimer());

void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (focused) {
    clearBlurTimer();
    requestAnimationFrame(focusPrompt);
    return;
  }
  // Terminal mode: keep session alive on focus flicker; only Action auto-dismisses.
  if (mode === "terminal" || sessionAlive) return;
  const blurDelay = busy
    ? Math.max(ENGINE_OPTS.blurDismissMs, FOCUS_STEAL_GRACE_MS + 80)
    : ENGINE_OPTS.blurDismissMs;
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
  focusPrompt();
}

function applySeed(seed?: string) {
  if (seed == null || seed === "") return defaults();
  const trimmed = seed.trim();
  if (trimmed === "apps" || trimmed === "start") return presetRows("apps");
  inp.value = seed;
  syncEcho();
  refreshProposals();
}

const _barBoot = barWindow();
if (!_barBoot.__VERSAILLES_BAR_BOUND__) {
  _barBoot.__VERSAILLES_BAR_BOUND__ = true;
void (async () => {
  const v = (window as unknown as { versailles?: { waitForTauri?: () => Promise<unknown> } }).versailles;
  if (v?.waitForTauri) await v.waitForTauri();
  bindDom();
  bindUi();
  try {
    const ctx = await loadSpawnableEngineContext();
    ENGINE_ID = ctx.id;
    ENGINE_OPTS = ctx.opts;
    ENGINE_RUNTIME = await loadEngineRuntime(ENGINE_ID);
    hostAvailable = true;
  } catch {
    hostAvailable = typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== "undefined"
      || !!(window as unknown as { versailles?: unknown }).versailles;
  }

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
  focusPrompt();
})();
}
