"use strict";
(() => {
  // scripts/bar-shims/core.js
  function callerArgs(args) {
    const caller = typeof window !== "undefined" && window.__VERSAILLES_WIDGET_ID__ || void 0;
    if (!caller) return args;
    return Object.assign({ caller }, args || {});
  }
  function invoke(cmd, args) {
    return window.versailles.invoke(cmd, callerArgs(args));
  }

  // scripts/bar-shims/event.js
  function listen(event, handler) {
    return window.versailles.listen(event, (payload) => handler({ payload }));
  }

  // scripts/bar-shims/dpi.js
  var LogicalSize = class {
    constructor(width, height) {
      this.width = width;
      this.height = height;
      this.type = "Logical";
    }
  };
  var PhysicalPosition = class {
    constructor(x, y) {
      this.x = x;
      this.y = y;
      this.type = "Physical";
    }
  };

  // scripts/bar-shims/window.js
  function getCurrentWindow() {
    return window.__TAURI__.window.getCurrentWindow();
  }
  async function currentMonitor() {
    return getCurrentWindow().currentMonitor();
  }

  // scripts/bar-shims/xterm.js
  var Terminal = window.Terminal;

  // scripts/bar-shims/fit.js
  var FitAddon = window.FitAddon?.FitAddon || window.FitAddon;

  // src/launcher.ts
  var root;
  var titleEl;
  var modeLabel;
  var termWrap;
  var termHost;
  var footL;
  var footM;
  var footHint;
  var footR;
  var inp;
  var psEl;
  var echoEl;
  var sug;
  var res;
  function mustEl(id) {
    const el = document.getElementById(id);
    if (!el) throw new Error(`Action bar missing #${id}`);
    return el;
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
  var HOME = "";
  var cwd = "";
  var PRESETS = [];
  var USER_SHORTCUTS = [];
  var hist = [];
  var hi = 0;
  var rows = [];
  var rowSel = -1;
  var busy = false;
  var busyGen = 0;
  var blurTimer = null;
  var BLUR_DISMISS_MS = 280;
  var FOCUS_STEAL_GRACE_MS = 320;
  var BUSY_WATCHDOG_MS = 4500;
  var mode = "action";
  var term = null;
  var fitAddon = null;
  var ptyDataUnlisten = null;
  var ptyExitUnlisten = null;
  var termSeed = null;
  var sessionAlive = false;
  var ptyWriteBuf = "";
  var ptyRaf = null;
  var esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  var joinPath = (a, b) => a.endsWith("\\") ? a + b : a + "\\" + b;
  var psQuote = (s) => `'${s.replace(/'/g, "''")}'`;
  var delay = (ms) => new Promise((r) => setTimeout(r, ms));
  function clearBlurTimer() {
    if (blurTimer) {
      clearTimeout(blurTimer);
      blurTimer = null;
    }
  }
  function forceIdle(_reason) {
    busyGen += 1;
    busy = false;
    clearBlurTimer();
  }
  function scheduleDismiss(ms, reason) {
    clearBlurTimer();
    const genAtSchedule = busyGen;
    blurTimer = setTimeout(() => {
      blurTimer = null;
      if (mode === "terminal") return;
      if (busy || genAtSchedule !== busyGen) {
        scheduleDismiss(ms, reason);
        return;
      }
      void getCurrentWindow().isFocused().then((still) => {
        if (!still && mode === "action" && !busy) dismissAction(reason);
      }).catch(() => {
      });
    }, ms);
  }
  function setPrompt() {
    psEl.textContent = `PS ${cwd}>`;
  }
  function syncEcho() {
    echoEl.textContent = inp.value;
  }
  function setRes(cls, html) {
    res.className = `cli-res on ${cls}`;
    res.innerHTML = html;
  }
  function clearRes() {
    res.className = "cli-res";
    res.innerHTML = "";
  }
  async function withBusy(fn, opts = {}) {
    if (busy) {
      return void 0;
    }
    const focusSteals = opts.focusSteals ?? true;
    const gen = ++busyGen;
    busy = true;
    clearBlurTimer();
    const watchdog = setTimeout(() => {
      if (gen === busyGen && busy) {
        forceIdle("watchdog");
        setRes("err", "command timed out \u2014 dismissed");
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
  function showRows(list) {
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
  function pick(r) {
    if (r.cat && isProfileName(r.cat)) {
      presetRows(r.cat);
      return;
    }
    if (r.cc !== void 0) {
      inp.value = r.cc;
      syncEcho();
      inp.focus();
      refreshProposals();
    } else if (r.path) {
      void openPath(r.path);
    }
  }
  function commandFromRow(r) {
    if (r.path) return null;
    if (r.cat && isProfileName(r.cat)) return null;
    if (r.cc !== void 0 && (r.cc.endsWith(" ") || /<[^>]+>/.test(r.c))) {
      return null;
    }
    return (r.cc ?? r.c).trim() || null;
  }
  function submitCommand(raw) {
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
  function activateRow(r) {
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
      { c: "profiles", d: "personal \xB7 work \xB7 dev \xB7 ai \xB7 fun \xB7 folders \xB7 apps", cat: "profiles" },
      { c: "?  <query>", d: "Google search", cc: "? " },
      { c: "hf <q>", d: "Hugging Face \xB7 gh github \xB7 yt youtube \xB7 w wiki", cc: "hf " },
      { c: "?? <query>", d: "search files", cc: "?? " }
    ]);
  }
  function findPreset(name) {
    const p = name.toLowerCase();
    return PRESETS.find((x) => x.n === p) || PRESETS.find((x) => (x.aliases ?? []).includes(p)) || PRESETS.find((x) => x.n.startsWith(p)) || PRESETS.find((x) => (x.aliases ?? []).some((a) => a.startsWith(p)));
  }
  function presetMatches(x, low) {
    if (!low) return true;
    if (x.n.startsWith(low) || x.n.includes(low) || x.d.toLowerCase().includes(low)) return true;
    return (x.aliases ?? []).some((a) => a.startsWith(low) || a.includes(low));
  }
  function commandForQuery(x, low) {
    if (x.n.startsWith(low)) return x.n;
    const alias = (x.aliases ?? []).find((a) => a.startsWith(low));
    return alias ?? x.n;
  }
  function profileNames() {
    return [...new Set(PRESETS.map((x) => x.cat))].sort((a, b) => b.length - a.length);
  }
  function isProfileName(name) {
    const p = name.toLowerCase();
    return profileNames().some((cat) => cat === p);
  }
  function findInProfile(cat, name) {
    const c = cat.toLowerCase();
    const n = name.toLowerCase().trim();
    if (!n) return void 0;
    const scoped = PRESETS.filter((x) => x.cat.toLowerCase() === c);
    return scoped.find((x) => x.n === n) || scoped.find((x) => (x.aliases ?? []).includes(n)) || scoped.find((x) => x.n.startsWith(n)) || scoped.find((x) => (x.aliases ?? []).some((a) => a.startsWith(n)));
  }
  function suggestions(raw) {
    const s = raw.trim();
    const out = [];
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
        out.push({ c: "hf ", d: "search models \xB7 spaces \xB7 papers" });
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
      PRESETS.filter((x) => !p || presetMatches(x, p) || x.cat.toLowerCase().startsWith(p)).slice(0, 8).forEach((x) => out.push({ c: commandForQuery(x, p), d: `${x.cat} \xB7 ${x.d}`, cc: x.n }));
      return out;
    }
    const parts = s.split(/\s+/);
    if (parts.length >= 2 && isProfileName(parts[0].toLowerCase())) {
      const cat = parts[0].toLowerCase();
      const rest = parts.slice(1).join(" ").toLowerCase();
      PRESETS.filter((x) => x.cat.toLowerCase() === cat && (!rest || presetMatches(x, rest))).slice(0, 10).forEach((x) => out.push({ c: commandForQuery(x, rest), d: `${x.cat} \xB7 ${x.d}`, cc: x.n }));
      if (!out.length) out.push({ c: cat, d: `no match in ${cat} \xB7 tab to browse`, cat });
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
          d: sessionAlive ? "new terminal (kills background)" : "open embedded terminal"
        });
      }
      if ("lock".startsWith(low)) out.push({ c: "lock", d: "lock workstation" });
      if ("start".startsWith(low)) out.push({ c: "start", d: "Start menu \xB7 installed apps" });
      if ("showdesk".startsWith(low) || "peek".startsWith(low)) {
        out.push({ c: "showdesk", d: "show desktop" });
      }
      if ("desk".startsWith(low)) out.push({ c: "desk", d: "toggle the HTML desktop page" });
      if ("hide".startsWith(low)) out.push({ c: "hide ", d: "hide an auto-added app" });
      profileNames().filter((cat) => cat.startsWith(low)).slice(0, 4).forEach(
        (cat) => out.push({ c: cat, d: `profile \xB7 ${PRESETS.filter((x) => x.cat === cat).length} shortcuts`, cat })
      );
      PRESETS.filter((x) => presetMatches(x, low)).slice(0, 8).forEach((x) => out.push({ c: commandForQuery(x, low), d: `${x.cat} \xB7 ${x.d}` }));
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
    if (list.length) showRows(list.map((x) => ({ ...x, cc: x.cc ?? x.c })));
    else {
      const hint = needsTerminal(raw) ? "\u21B5 open terminal" : "\u21B5 run inline (pwsh)";
      showRows([{ c: raw, d: hint }]);
    }
  }
  function firstLine(s, max = 120) {
    const line = s.trim().split(/\r?\n/, 1)[0] ?? "";
    return line.length > max ? line.slice(0, max) + "\u2026" : line;
  }
  function formatBlock(text, maxLines = 14, maxChars = 2400) {
    const lines = text.trim().split(/\r?\n/).filter(Boolean);
    if (!lines.length) return "";
    const slice = lines.slice(0, maxLines);
    let html = slice.map((l) => esc(l)).join("<br>");
    if (lines.length > maxLines) html += "<br>\u2026";
    if (html.length > maxChars) html = html.slice(0, maxChars) + "\u2026";
    return html;
  }
  function needsTerminal(cmd) {
    const raw = cmd.trim();
    if (/^!!/.test(raw)) return true;
    const low = raw.toLowerCase();
    return /\|\s*(iex|invoke-expression)\b/.test(low) || /\.ps1\b/.test(low) || /\b(read-host|install-module|install-package|winget\s+install|choco\s+install|scoop\s+install)\b/.test(
      low
    );
  }
  function stripTerminalBang(cmd) {
    return cmd.replace(/^!!\s?/, "").trim();
  }
  function stripInlineBang(cmd) {
    return cmd.replace(/^!\s?/, "").trim();
  }
  function b64ToUtf8(b64) {
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
  function enqueuePtyData(text) {
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
  function applyChrome(next) {
    root.dataset.mode = next;
    if (next === "terminal") {
      titleEl.textContent = "versailles \xB7 pwsh";
      modeLabel.textContent = "terminal";
      footL.textContent = "esc detach";
      footM.textContent = "ctrl+c";
      footHint.textContent = "paste ok";
      footR.textContent = sessionAlive ? "background live" : "";
    } else {
      modeLabel.textContent = sessionAlive ? "actions \xB7 live" : "actions";
    }
  }
  async function refreshSessionAlive() {
    try {
      sessionAlive = await invoke("pty_is_alive");
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
    ptyDataUnlisten = await listen("pty://data", (ev) => {
      try {
        enqueuePtyData(b64ToUtf8(ev.payload));
      } catch {
      }
    });
    ptyExitUnlisten = await listen("pty://exit", () => {
      sessionAlive = false;
      term?.writeln("\r\n\x1B[90m[session ended \u2014 esc returns to actions]\x1B[0m");
      if (mode === "action") {
        applyChrome("action");
        defaults();
      }
    });
  }
  function ensureTerm() {
    if (term) return term;
    const t = new Terminal({
      convertEol: true,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: '"Cascadia Mono", "JetBrains Mono", Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.25,
      scrollback: 5e3,
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
        brightWhite: "#ffffff"
      }
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(termHost);
    fitAddon = fit;
    term = t;
    t.onData((data) => {
      void invoke("pty_write", { data }).catch(() => {
      });
    });
    termHost.addEventListener("paste", (e) => {
      if (mode !== "terminal") return;
      const text = e.clipboardData?.getData("text");
      if (!text) return;
      e.preventDefault();
      void invoke("pty_write", { data: text }).catch(() => {
      });
    });
    return t;
  }
  async function resizeLauncher(next) {
    const win = getCurrentWindow();
    try {
      const monitor = await currentMonitor();
      const scale = monitor?.scaleFactor ?? 1;
      const monW = monitor ? monitor.size.width / scale : 1920;
      const monH = monitor ? monitor.size.height / scale : 1080;
      let w;
      let h;
      if (next === "terminal") {
        w = Math.round(Math.min(1280, Math.max(960, monW * 0.78)));
        h = Math.round(Math.min(860, Math.max(640, monH * 0.78)));
      } else {
        w = 640;
        h = 420;
      }
      await win.setSize(new LogicalSize(w, h));
      if (monitor) {
        const x = monitor.position.x + Math.round((monitor.size.width - w * scale) / 2);
        const y = next === "terminal" ? monitor.position.y + Math.round((monitor.size.height - h * scale) / 2) : monitor.position.y + Math.round(120 * scale);
        await win.setPosition(new PhysicalPosition(x, y));
      }
    } catch {
      await win.setSize(
        next === "terminal" ? new LogicalSize(1100, 720) : new LogicalSize(640, 420)
      );
    }
  }
  async function fitAndResizePty() {
    if (!term || !fitAddon) return;
    fitAddon.fit();
    const cols = term.cols;
    const rows2 = term.rows;
    try {
      await invoke("pty_resize", { cols, rows: rows2 });
    } catch {
    }
  }
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
    }
    await refreshSessionAlive();
    if (focusAction) {
      defaults();
      inp.focus();
    }
  }
  async function killTerminalSession() {
    try {
      await invoke("pty_close");
    } catch {
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
  async function enterTerminal(seedCmd, opts = {}) {
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
        const rows2 = t.rows || 24;
        await bindPtyListeners();
        await invoke("pty_open", { cwd, cols, rows: rows2 });
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
  async function openPath(path) {
    await withBusy(async () => {
      try {
        await invoke("cli_open", { target: path });
        setRes("ok", `&rarr; opened <span class="link">${esc(path)}</span>`);
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    });
  }
  function webSearch(q) {
    if (!q) return setRes("err", "? : missing query \u2014 usage: ? &lt;query&gt;");
    openTarget("https://www.google.com/search?q=" + encodeURIComponent(q));
  }
  function openTarget(target) {
    void withBusy(async () => {
      try {
        await invoke("cli_open", { target });
        setRes("ok", `&rarr; opened <span class="link">${esc(target)}</span>`);
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    });
  }
  function calcExpr(expr) {
    if (!expr) return setRes("err", "= : missing expression \u2014 usage: = 1+2*3");
    const safe = expr.replace(/,/g, ".");
    if (!/^[0-9+\-*/().\s%^]*$/.test(safe)) {
      return setRes("err", "= : only numbers and + - * / % ^ ( ) allowed");
    }
    void withBusy(async () => {
      try {
        const ps = safe.replace(/\^/g, "**");
        const out = await invoke("cli_exec", {
          cmd: `[math]::Round((${ps}), 6)`,
          cwd
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
  function wikiSearch(q) {
    if (!q) return setRes("err", "w : missing query \u2014 usage: w &lt;topic&gt;");
    openTarget("https://en.wikipedia.org/wiki/Special:Search?search=" + encodeURIComponent(q));
  }
  function ytSearch(q) {
    if (!q) return setRes("err", "yt : missing query \u2014 usage: yt &lt;query&gt;");
    openTarget("https://www.youtube.com/results?search_query=" + encodeURIComponent(q));
  }
  function ghSearch(q) {
    if (!q) return openTarget("https://github.com/search");
    openTarget("https://github.com/search?q=" + encodeURIComponent(q));
  }
  function hfSearch(q) {
    const raw = q.trim();
    const tpl = root?.getAttribute("data-search-hf") || "https://huggingface.co/models?search={q}";
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
  async function fileSearch(q) {
    if (!q) return setRes("err", "?? : missing query \u2014 usage: ?? &lt;query&gt;");
    setRes("out", `searching files for "${esc(q)}"\u2026`);
    await withBusy(async () => {
      try {
        const matches = await invoke("cli_search_files", { query: q });
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
  function presetRows(filter) {
    const f = (filter || "").toLowerCase();
    const list = f ? PRESETS.filter((x) => presetMatches(x, f) || x.cat.toLowerCase().includes(f)) : PRESETS;
    if (!list.length) {
      setRes("err", `presets : nothing matches '${esc(filter || "")}'`);
      return defaults();
    }
    const cats = [...new Set(list.map((x) => x.cat))];
    setRes("out", f ? `${list.length} in \u201C${esc(f)}\u201D` : `${list.length} shortcuts \xB7 ${cats.join(" \xB7 ")}`);
    showRows(list.map((x) => ({ c: x.n, d: `${x.cat} \xB7 ${x.d}`, cc: x.n })));
  }
  async function openFileTarget(target, label) {
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
  async function openPreset(arg) {
    const p = arg.toLowerCase().trim();
    if (!p) return presetRows();
    if (p === "config") return void openConfigFile();
    if (p === "desktopfile") return void openDesktopFile();
    const hit = findPreset(p);
    if (!hit) {
      const inCat = PRESETS.filter((x) => x.cat.toLowerCase() === p || x.cat.toLowerCase().startsWith(p));
      if (inCat.length > 1) return presetRows(p);
      if (inCat.length === 1) {
        return void launchPreset(inCat[0]);
      }
      return setRes("err", `no shortcut '${esc(arg)}' \u2014 try 'presets'`);
    }
    return void launchPreset(hit);
  }
  function widgetsRoot(home) {
    return joinPath(home, "Documents\\Widgets");
  }
  async function openConfigFile() {
    return openDesktopFile();
  }
  async function openDesktopFile() {
    return openFileTarget(joinPath(widgetsRoot(HOME || "C:\\"), "desktop\\index.html"), "desktop/index.html");
  }
  async function launchPreset(hit) {
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
  function expandHome(target, home) {
    return target.replace(/^~([\\/]|$)/, home + "$1").replace(/%HOME%/gi, home).replace(/\$HOME/g, home);
  }
  function normalizeUserShortcut(raw, home) {
    return {
      ...raw,
      target: raw.t === "folder" ? expandHome(raw.target, home) : raw.target
    };
  }
  function mergePresetsInto(base, extras) {
    const result = [...base];
    const byId = new Map(result.map((p, i) => [p.n, i]));
    for (const p of extras) {
      const idx = byId.get(p.n);
      if (idx !== void 0) {
        result[idx] = p;
      } else {
        byId.set(p.n, result.length);
        result.push(p);
      }
    }
    return result;
  }
  function parseVersaillesBlock(html) {
    try {
      const doc = new DOMParser().parseFromString(html, "text/html");
      const el = doc.getElementById("versailles");
      if (!el) return null;
      return JSON.parse(el.textContent || "");
    } catch {
      return null;
    }
  }
  async function loadUserShortcuts() {
    USER_SHORTCUTS = [];
    const home = HOME || "C:\\";
    try {
      let html = "";
      try {
        html = await invoke("get_desktop_html");
      } catch {
        const api2 = await invoke("get_api_info");
        const res3 = await fetch(`${api2.base_url}/files/desktop/index.html`, { cache: "no-store" });
        if (res3.ok) html = await res3.text();
      }
      const block = html ? parseVersaillesBlock(html) : null;
      if (block && Array.isArray(block.shortcuts)) {
        USER_SHORTCUTS = block.shortcuts.map((s) => normalizeUserShortcut(s, home));
        return;
      }
      const api = await invoke("get_api_info");
      const base = api.base_url;
      let shortcutsPath = "shortcuts.json";
      try {
        const cfgRes = await fetch(`${base}/files/versailles.json`, { cache: "no-store" });
        if (cfgRes.ok) {
          const cfg = await cfgRes.json();
          if (Array.isArray(cfg.shortcuts)) {
            USER_SHORTCUTS = cfg.shortcuts.map((s) => normalizeUserShortcut(s, home));
            return;
          }
          if (typeof cfg.shortcuts === "string" && cfg.shortcuts.trim()) {
            shortcutsPath = cfg.shortcuts.replace(/\\/g, "/").replace(/^\//, "");
          }
        }
      } catch {
      }
      const res2 = await fetch(`${base}/files/${shortcutsPath}`, { cache: "no-store" });
      if (!res2.ok) return;
      const data = await res2.json();
      if (!Array.isArray(data.shortcuts)) return;
      USER_SHORTCUTS = data.shortcuts.map((s) => normalizeUserShortcut(s, home));
    } catch {
    }
  }
  function builtinPresets(home) {
    return [
      { n: "mail", t: "web", d: "Gmail", target: "https://mail.google.com/", cat: "personal" },
      { n: "github", t: "web", d: "GitHub", target: "https://github.com/", cat: "dev" },
      { n: "downloads", t: "folder", d: "Downloads", target: joinPath(home, "Downloads"), cat: "folders" },
      { n: "documents", t: "folder", d: "Documents", target: joinPath(home, "Documents"), cat: "folders" },
      { n: "desktop", t: "folder", d: "Desktop", target: joinPath(home, "Desktop"), cat: "folders" }
    ];
  }
  function applyCatalog(entries) {
    const builtins = mergePresetsInto(builtinPresets(HOME || "C:\\"), USER_SHORTCUTS);
    const byId = new Map(builtins.map((p, i) => [p.n, i]));
    for (const e of entries) {
      const preset = {
        n: e.id,
        t: "app",
        d: e.fresh ? `new \xB7 ${e.name}` : e.name,
        target: e.target,
        cat: "apps",
        aliases: (e.aliases || []).filter((a) => a !== e.id)
      };
      const idx = byId.get(e.id);
      if (idx !== void 0 && builtins[idx].t === "app") {
        builtins[idx] = preset;
      } else if (idx === void 0) {
        builtins.push(preset);
        byId.set(e.id, builtins.length - 1);
      }
    }
    PRESETS = builtins;
  }
  async function loadCatalog() {
    try {
      const entries = await invoke("list_catalog");
      applyCatalog(entries);
      void invoke("ack_catalog").catch(() => {
      });
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
  async function cdCmd(arg) {
    if (!arg || arg === "~") {
      cwd = HOME;
      setPrompt();
      return setRes("out", esc(cwd));
    }
    const p = arg.replace(/\//g, "\\");
    let target;
    if (/^[A-Za-z]:[\\/]/.test(p)) {
      target = p;
    } else {
      const stack = [];
      (cwd + "\\" + p).split("\\").forEach((seg) => {
        if (seg === "..") stack.pop();
        else if (seg && seg !== ".") stack.push(seg);
      });
      target = stack.join("\\");
    }
    target = target.replace(/\\+$/, "");
    await withBusy(async () => {
      try {
        const out = await invoke("cli_exec", {
          cmd: `if (Test-Path -LiteralPath ${psQuote(target)} -PathType Container) { Write-Output 'OK' }`,
          cwd
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
        const out = await invoke("cli_exec", {
          cmd: "Get-ChildItem -Force | ForEach-Object { if ($_.PSIsContainer) { 'D|' + $_.Name } else { 'F|' + $_.Name } }",
          cwd
        });
        if (out.code !== 0) return setRes("err", esc(firstLine(out.stderr) || "ls failed"));
        const entries = out.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
        const dirs = entries.filter((e) => e.startsWith("D|")).map((e) => e.slice(2));
        const files = entries.filter((e) => e.startsWith("F|")).map((e) => e.slice(2));
        const n = dirs.length + files.length;
        if (!n) {
          setRes("out", "(empty) \u2014 " + esc(cwd));
          return defaults();
        }
        setRes("out", `${n} entr${n === 1 ? "y" : "ies"} \u2014 ${esc(cwd)}`);
        showRows(
          [
            ...dirs.map((d) => ({ c: d + "/", d: "dir", cc: "cd " + d })),
            ...files.map((f) => ({ c: f, d: "file", path: joinPath(cwd, f) }))
          ].slice(0, 6)
        );
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  async function shellExec(cmd) {
    const trimmed = cmd.trim();
    if (/^!!/.test(trimmed)) {
      const run2 = stripTerminalBang(trimmed);
      return void enterTerminal(run2 || void 0);
    }
    const inline = /^!\s?/.test(trimmed) ? stripInlineBang(trimmed) : trimmed;
    if (!inline) return setRes("err", "! : missing command \u2014 usage: ! Get-Date");
    if (needsTerminal(trimmed) && !/^!\s?/.test(trimmed)) {
      return void enterTerminal(inline);
    }
    setRes("out", "\u2026");
    await withBusy(async () => {
      try {
        const out = await invoke("cli_exec", { cmd: inline, cwd });
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
  function run(c) {
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
        if (sub === "new" || sub === "fresh") return void enterTerminal(void 0, { fresh: true });
        if (sub === "kill" || sub === "close") {
          return void (async () => {
            await killTerminalSession();
            setRes("ok", "&rarr; background terminal closed");
            applyChrome("action");
            defaults();
          })();
        }
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
            const on = await invoke("toggle_desktop_surface");
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
            const entries = await invoke("hide_catalog_entry", { id: targetId });
            applyCatalog(entries);
            setRes("ok", `&rarr; hid ${esc(targetId)} from apps`);
            presetRows("apps");
          } catch (e) {
            setRes("err", esc(String(e)));
          }
        }, { focusSteals: false });
      }
      case "help":
        setRes("out", "grammar \u2014 ? web \xB7 ?? files \xB7 = calc \xB7 term \xB7 profile names (personal, work, dev, fun, apps\u2026)");
        return showRows([
          ...sessionAlive ? [{ c: "continue", d: "reattach background terminal", cc: "continue" }] : [],
          { c: "term", d: "open / reattach terminal", cc: "term" },
          { c: "config", d: "open desktop/index.html", cc: "config" },
          { c: "desktopfile", d: "open desktop/index.html", cc: "desktopfile" },
          { c: "? <query>", d: "Google search", cc: "? " },
          { c: "?? <query>", d: "search files", cc: "?? " },
          { c: "= <expr>", d: "quick calculator", cc: "= " },
          { c: "! <cmd>", d: "run inline pwsh", cc: "! " },
          { c: "!!", d: "open detachable terminal", cc: "!!" },
          { c: "hf <q>", d: "Hugging Face models (hf models|datasets|spaces)", cc: "hf " },
          { c: "mail \xB7 github", d: "Gmail \xB7 GitHub (extend via #versailles in index.html)", cc: "mail" },
          { c: "start", d: "Start menu \xB7 installed apps", cc: "start" },
          { c: "showdesk", d: "Show desktop (taskbar Win+D)", cc: "showdesk" },
          { c: "desk", d: "toggle the HTML desktop page", cc: "desk" },
          { c: "hide <app>", d: "remove an auto-added app from the bar", cc: "hide " },
          { c: "presets <cat>", d: "browse personal \xB7 work \xB7 dev \xB7 fun \xB7 apps \u2026", cc: "presets " },
          { c: "lock", d: "lock workstation", cc: "lock" },
          { c: "cls", d: "reset the bar", cc: "cls" }
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
        return setRes("out", "alt+space hides \xB7 esc detaches terminal (keeps it running)");
      default: {
        if (isProfileName(cmd)) {
          if (arg) {
            const scoped = findInProfile(cmd, arg);
            if (scoped) return void launchPreset(scoped);
            return setRes("err", `no \u201C${esc(arg)}\u201D in ${esc(cmd)} \xB7 tab to browse`);
          }
          return presetRows(cmd);
        }
        const hit = findPreset(cmd);
        if (hit && !arg) return void launchPreset(hit);
        if (hit && arg && hit.n === cmd) return void launchPreset(hit);
        return void shellExec(c);
      }
    }
  }
  async function hideLauncherWindow(reason) {
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
    void invoke("dismiss_launcher");
  }
  function dismissAction(reason) {
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
        return;
      } else if (e.key === "Tab") {
        e.preventDefault();
        const r = rowSel >= 0 && rows[rowSel] || rows.find((x) => x.cc !== void 0);
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
      true
    );
    document.querySelector(".cli").addEventListener("click", () => {
      if (mode === "action") inp.focus();
      else term?.focus();
    });
    void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        clearBlurTimer();
        return;
      }
      if (mode === "terminal") return;
      const blurDelay = busy ? Math.max(BLUR_DISMISS_MS, FOCUS_STEAL_GRACE_MS + 80) : BLUR_DISMISS_MS;
      scheduleDismiss(blurDelay, "outside-blur");
    });
    window.addEventListener("resize", () => {
      if (mode === "terminal") void fitAndResizePty();
    });
  }
  async function resetBar(seed) {
    forceIdle("resetBar");
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
  function applySeed(seed) {
    if (seed == null || seed === "") return defaults();
    const trimmed = seed.trim();
    if (trimmed === "apps" || trimmed === "start") return presetRows("apps");
    inp.value = seed;
    syncEcho();
    refreshProposals();
  }
  void (async () => {
    const v = window.versailles;
    if (v?.waitForTauri) await v.waitForTauri();
    bindDom();
    bindUi();
    const onShown = (ev) => {
      const seed = typeof ev === "string" ? ev : typeof ev?.payload === "string" ? ev.payload : "";
      void resetBar(seed);
    };
    await listen("overlay://shown", onShown);
    await listen("launcher://shown", onShown);
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
      HOME = await invoke("cli_home");
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
})();
