import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type WidgetManifest = {
  id: string;
  title: string;
  width: number;
  height: number;
  alwaysOnTop?: boolean;
};

type RegisteredWidget = {
  manifest: WidgetManifest;
  path: string;
  entryPath: string;
  legacy?: boolean;
  embedded?: boolean;
};

type RegistrySnapshot = {
  widgets: RegisteredWidget[];
  errors: string[];
};

type OpenWidget = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  alwaysOnTop: boolean;
};

type Rect = { x: number; y: number; width: number; height: number };

type AppConfig = {
  version: string;
  autostart: boolean;
  openManagerOnStartup: boolean;
  apiEnabled: boolean;
  apiToken: string;
  apiPort: number;
  snapThreshold: number;
  launcherHotkey: string;
  sessionWidgets: unknown[];
  activeLayout: string | null;
  catalog?: { seenIds: string[]; hiddenIds: string[] };
  desktop?: {
    enabled: boolean;
    page?: string;
    anywhereBar?: boolean;
  };
};

type ApiInfo = {
  enabled: boolean;
  port: number;
  token: string;
  baseUrl: string;
};

type RuntimeStatus = {
  mediaError: string | null;
  registryErrors: string[];
};

type Tab = "widgets" | "layout" | "settings" | "api";

const app = document.querySelector<HTMLDivElement>("#app")!;
let tab: Tab = "widgets";
let registry: RegistrySnapshot = { widgets: [], errors: [] };
let openWidgets: OpenWidget[] = [];
let monitors: Rect[] = [];
let layouts: string[] = [];
let config: AppConfig | null = null;
let apiInfo: ApiInfo | null = null;
let runtime: RuntimeStatus = { mediaError: null, registryErrors: [] };
let layoutName = "Work";
let toastMessage: string | null = null;
let toastTimer: number | undefined;
let settingsSavedFlash = false;
let layoutSavedFlash = false;

const ACCENT_BY_ID: Record<string, string> = {
  clock: "#ff9f0a",
  "now-playing": "#e85d75",
  weather: "#0a84ff",
  todo: "#30d158",
};
const ACCENT_PALETTE = ["#ff9f0a", "#e85d75", "#0a84ff", "#30d158", "#9ac324", "#7b61ff", "#ffd60a", "#ff453a"];

function accentFor(id: string) {
  if (ACCENT_BY_ID[id]) return ACCENT_BY_ID[id];
  let hash = 0;
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  return ACCENT_PALETTE[hash % ACCENT_PALETTE.length];
}

function showToast(message: string) {
  toastMessage = message;
  render();
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toastMessage = null;
    render();
  }, 4500);
}

async function refresh() {
  try {
    registry = await invoke<RegistrySnapshot>("list_widgets");
    openWidgets = await invoke<OpenWidget[]>("list_open_widgets");
    monitors = await invoke<Rect[]>("get_monitors");
    layouts = await invoke<string[]>("list_layouts");
    config = await invoke<AppConfig>("get_config");
    apiInfo = await invoke<ApiInfo>("get_api_info");
    runtime = await invoke<RuntimeStatus>("get_runtime_status");
  } catch (err) {
    showToast(`Refresh failed: ${String(err)}`);
  }
  render();
}

const NAV_ICONS: Record<Tab, string> = {
  widgets:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="3" width="8" height="8" rx="2"/><rect x="13" y="3" width="8" height="8" rx="2"/><rect x="3" y="13" width="8" height="8" rx="2"/><rect x="13" y="13" width="8" height="8" rx="2"/></svg>',
  layout:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="14" rx="2"/><path d="M3 21h18"/></svg>',
  settings:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1"/></svg>',
  api:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M8 9l-4 3 4 3M16 9l4 3-4 3M13 5l-2 14"/></svg>',
};

function navButton(id: Tab, label: string) {
  return `<button class="nav-item${tab === id ? " on" : ""}" data-tab="${id}">${NAV_ICONS[id]}${label}</button>`;
}

function renderWidgets() {
  const SLIDEOUTS = new Set(["calculator", "calendar", "notes"]);
  const page = registry.widgets.filter((w) => w.embedded);
  const slide = registry.widgets.filter((w) => SLIDEOUTS.has(w.manifest.id));
  const legacy = registry.widgets.filter((w) => w.legacy && !w.embedded);
  const other = registry.widgets.filter(
    (w) => !w.embedded && !w.legacy && !SLIDEOUTS.has(w.manifest.id),
  );

  function card(w: RegisteredWidget, kind: "page" | "slide" | "legacy" | "other") {
    const floating = openWidgets.some((o) => o.id === w.manifest.id);
    const title = w.manifest.title;
    const actions =
      kind === "page"
        ? `<span class="lib-state on-page">desktop page</span>`
        : `<button class="btn sm ${floating ? "ghost-d" : "blue"}" data-open="${escapeHtml(w.manifest.id)}">${floating ? "Focus" : "Open"}</button>
          <button class="btn sm ghost-d" data-close="${escapeHtml(w.manifest.id)}" ${floating ? "" : "disabled"}>Close</button>
          ${floating ? `<span class="lib-state">${kind === "slide" ? "Slide-out" : "Floating"}</span>` : ""}`;
    return `<article class="lib-card">
        <div class="lib-top">
          <div class="lib-thumb" style="background:${accentFor(w.manifest.id)}">${escapeHtml((title[0] || "?").toUpperCase())}</div>
          <div>
            <div class="lib-name">${escapeHtml(title)}</div>
            <div class="lib-meta">${escapeHtml(w.manifest.id)} · ${w.manifest.width}×${w.manifest.height}</div>
          </div>
        </div>
        <div class="lib-actions">${actions}</div>
      </article>`;
  }

  function section(title: string, desc: string, widgets: RegisteredWidget[], kind: "page" | "slide" | "legacy" | "other") {
    if (!widgets.length) return "";
    return `<h3 class="lib-section">${title}</h3>
      <p class="lib-section-desc">${desc}</p>
      <div class="lib-grid">${widgets.map((w) => card(w, kind)).join("")}</div>`;
  }

  const runtimeErrors = [
    ...registry.errors,
    ...runtime.registryErrors,
    ...(runtime.mediaError ? [`Media: ${runtime.mediaError}`] : []),
  ];

  return `<div class="mgr-head">
      <div>
        <h2>Widgets</h2>
        <p>Desktop layout is <code>desktop\\index.html</code> · ${openWidgets.length} overlay windows</p>
      </div>
      <button class="btn ghost-d sm" data-action="refresh">Refresh</button>
    </div>
    ${section("Desktop page", "Iframed in Documents\\Widgets\\desktop\\index.html. Change the file, not a floating window.", page, "page")}
    ${section("Slide-outs", "Legacy overlay windows. Notes live on the desktop page now.", slide, "slide")}
    ${section("Legacy · floating", "Old movable windows. Parked under Documents\\Widgets\\legacy. Open still works.", legacy, "legacy")}
    ${section("Other", "Registered widgets that are not on the desktop page.", other, "other")}
    ${
      runtimeErrors.length
        ? `<pre class="errors">${escapeHtml([...new Set(runtimeErrors)].join("\n"))}</pre>`
        : ""
    }`;
}

function renderLayout() {
  const scale = 0.18;
  const offsetX = 20;
  const offsetY = 20;
  const monitorHtml = monitors
    .map((m, i) => {
      return `<div class="lay-monitor" style="left:${offsetX + m.x * scale}px;top:${offsetY + m.y * scale}px;width:${m.width * scale}px;height:${m.height * scale}px"><span>DISPLAY ${i + 1}</span></div>`;
    })
    .join("");

  const boxVariants = ["", "o", "g"];
  const widgetHtml = openWidgets
    .map((w, i) => {
      const variant = boxVariants[i % boxVariants.length];
      return `<div class="lay-box${variant ? " " + variant : ""}" data-drag="${escapeHtml(w.id)}" style="left:${offsetX + w.x * scale}px;top:${offsetY + w.y * scale}px;width:${Math.max(48, w.width * scale)}px;height:${Math.max(28, w.height * scale)}px">${escapeHtml(w.id)}</div>`;
    })
    .join("");

  const layoutOptions = layouts
    .map((n) => `<option value="${escapeHtml(n)}" ${config?.activeLayout === n ? "selected" : ""}>${escapeHtml(n)}</option>`)
    .join("");

  return `<div class="mgr-head">
      <div>
        <h2>Layout</h2>
        <p>Overlay windows only (slide-outs and legacy). The desktop is <code>desktop\\index.html</code></p>
      </div>
    </div>
    <div class="lay-bar">
      <input type="text" id="layout-name" value="${escapeHtml(layoutName)}" placeholder="Layout name" spellcheck="false" />
      <button class="btn blue sm" data-action="save-layout">${layoutSavedFlash ? "Saved" : "Save template"}</button>
      <select id="layout-select">${layoutOptions || "<option value=''>No layouts yet</option>"}</select>
      <button class="btn ghost-d sm" data-action="apply-layout">Apply</button>
      <span class="lay-hint">Hold Shift to disable snapping</span>
    </div>
    <div class="lay-canvas" id="layout-canvas">${monitorHtml}${widgetHtml}</div>`;
}

function switchRow(label: string, desc: string, id: string, checked: boolean) {
  return `<div class="set-row">
    <div><div class="lab">${label}</div><div class="desc">${desc}</div></div>
    <label class="switch"><input type="checkbox" id="${id}" ${checked ? "checked" : ""} /><i></i></label>
  </div>`;
}

function inputRow(label: string, desc: string, input: string) {
  return `<div class="set-row">
    <div><div class="lab">${label}</div><div class="desc">${desc}</div></div>
    ${input}
  </div>`;
}

function renderSettings() {
  if (!config) return "";
  return `<div class="mgr-head"><div><h2>Settings</h2><p>Startup, snapping, and hotkeys</p></div></div>
  <div class="set-panel">
    ${switchRow("Launch with Windows", "Start Deck quietly to the tray on sign-in", "autostart", config.autostart)}
    ${switchRow("Open manager on startup", "Show this window when Deck starts", "open-manager", config.openManagerOnStartup)}
    ${inputRow("Snap threshold", "How close widgets get before guides snap them", `<input class="set-input" type="number" id="snap" value="${config.snapThreshold}" style="width:80px" />`)}
    ${switchRow("Anywhere bar", "Separate always-on-top strip. Covers app title bars — leave off", "anywhere-bar", config.desktop?.anywhereBar ?? false)}
    ${switchRow("Desktop page", "Fullscreen HTML from Documents\\Widgets\\desktop\\index.html. Apps cover it. Does not hide the taskbar", "desktop-enabled", config.desktop?.enabled ?? false)}
    ${inputRow("Launcher hotkey", "Global shortcut that toggles the launcher", `<input class="set-input" type="text" id="hotkey" value="${escapeHtml(config.launcherHotkey)}" spellcheck="false" />`)}
    ${switchRow("Control API", "Localhost-only API for AI / automation. Widget files are always served; this only locks down remote commands", "api-enabled", config.apiEnabled)}
    ${inputRow("API port", "Port for the local HTTP server", `<input class="set-input" type="number" id="api-port" value="${config.apiPort}" style="width:100px" />`)}
  </div>
  <div class="set-actions">
    <button class="btn blue sm" data-action="save-settings">Save settings</button>
    <button class="btn ghost-d sm" data-action="launcher">Open launcher</button>
    <button class="btn ghost-d sm" data-action="canvas">Open canvas</button>
    <button class="btn ghost-d sm" data-action="open-logs">Open log folder</button>
    <span class="set-saved${settingsSavedFlash ? " show" : ""}" id="set-saved">Saved</span>
  </div>
  <div class="set-note">
    <h3>How to launch</h3>
    <p>Dev: <code>cd Documents\\Widgets\\app</code> then <code>npm run tauri:dev</code> (or double-click <code>dev.cmd</code>).</p>
    <p>Installed: open Deck from the Start menu / desktop shortcut. Optional: enable “Launch with Windows” above.</p>
    <p>Tray icon opens this manager. <kbd>Alt+Space</kbd> opens the launcher (configurable). The desktop is <code>Documents\\Widgets\\desktop\\index.html</code> — edit that file, then toggle <code>desk</code>.</p>
  </div>`;
}

function apiBlock(label: string, value: string) {
  return `<div class="api-block">
    <div class="lab">${label}</div>
    <div class="api-code"><span>${escapeHtml(value)}</span><button class="api-copy">Copy</button></div>
  </div>`;
}

function renderApi() {
  if (!apiInfo) return "";
  return `<div class="mgr-head"><div><h2>Local API</h2><p>Localhost-only control surface for AI / automation</p></div></div>
  ${apiBlock("Base URL", apiInfo.baseUrl)}
  ${apiBlock("Bearer token", apiInfo.token)}
  ${apiBlock("Example", `curl -H "Authorization: Bearer ${apiInfo.token}" ${apiInfo.baseUrl}/widgets`)}`;
}

function render() {
  app.innerHTML = `<div class="mgr">
    <nav class="mgr-nav">
      <div class="mgr-brand">Deck</div>
      ${navButton("widgets", "Widgets")}
      ${navButton("layout", "Layout")}
      ${navButton("settings", "Settings")}
      ${navButton("api", "API")}
    </nav>
    <main class="mgr-main">
      ${
        tab === "widgets"
          ? renderWidgets()
          : tab === "layout"
            ? renderLayout()
            : tab === "settings"
              ? renderSettings()
              : renderApi()
      }
    </main>
  </div>
  ${toastMessage ? `<div class="toast error">${escapeHtml(toastMessage)}</div>` : ""}`;

  bind();
}

function bind() {
  app.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((btn) => {
    btn.onclick = () => {
      tab = btn.dataset.tab as Tab;
      render();
    };
  });

  app.querySelectorAll<HTMLButtonElement>("[data-open]").forEach((btn) => {
    btn.onclick = async () => {
      try {
        await invoke("open_widget", { id: btn.dataset.open });
      } catch (err) {
        showToast(`Could not open widget: ${String(err)}`);
      } finally {
        await refresh();
      }
    };
  });

  app.querySelectorAll<HTMLButtonElement>("[data-close]").forEach((btn) => {
    btn.onclick = async () => {
      try {
        await invoke("close_widget", { id: btn.dataset.close });
      } catch (err) {
        showToast(`Could not close widget: ${String(err)}`);
      } finally {
        await refresh();
      }
    };
  });

  const refreshBtn = app.querySelector<HTMLButtonElement>("[data-action='refresh']");
  if (refreshBtn) refreshBtn.onclick = () => void refresh();

  const saveSettings = app.querySelector<HTMLButtonElement>("[data-action='save-settings']");
  if (saveSettings && config) {
    saveSettings.onclick = async () => {
      const next = {
        ...config!,
        autostart: (app.querySelector("#autostart") as HTMLInputElement).checked,
        openManagerOnStartup: (app.querySelector("#open-manager") as HTMLInputElement).checked,
        snapThreshold: Number((app.querySelector("#snap") as HTMLInputElement).value),
        launcherHotkey: (app.querySelector("#hotkey") as HTMLInputElement).value,
        desktop: {
          ...(config!.desktop ?? {
            enabled: false,
            page: "desktop/index.html",
            anywhereBar: false,
          }),
          enabled: (app.querySelector("#desktop-enabled") as HTMLInputElement).checked,
          anywhereBar: (app.querySelector("#anywhere-bar") as HTMLInputElement).checked,
        },
        apiEnabled: (app.querySelector("#api-enabled") as HTMLInputElement).checked,
        apiPort: Number((app.querySelector("#api-port") as HTMLInputElement).value),
      };
      config = await invoke<AppConfig>("save_config", { config: next });
      try {
        if (next.desktop.enabled) {
          await invoke("open_desktop_surface");
        } else {
          await invoke("close_desktop_surface");
          if (next.desktop.anywhereBar) await invoke("open_anywhere_bar");
          else await invoke("close_anywhere_bar");
        }
      } catch {
        /* surface / HUD optional */
      }
      settingsSavedFlash = true;
      await refresh();
      window.setTimeout(() => {
        settingsSavedFlash = false;
        render();
      }, 1600);
    };
  }

  const saveLayout = app.querySelector<HTMLButtonElement>("[data-action='save-layout']");
  if (saveLayout) {
    saveLayout.onclick = async () => {
      const name = (app.querySelector("#layout-name") as HTMLInputElement).value.trim() || "Work";
      layoutName = name;
      await invoke("save_layout", { name });
      layoutSavedFlash = true;
      await refresh();
      window.setTimeout(() => {
        layoutSavedFlash = false;
        render();
      }, 1400);
    };
  }

  const applyLayout = app.querySelector<HTMLButtonElement>("[data-action='apply-layout']");
  if (applyLayout) {
    applyLayout.onclick = async () => {
      const name = (app.querySelector("#layout-select") as HTMLSelectElement).value;
      if (!name) return;
      await invoke("apply_layout", { name });
      await refresh();
    };
  }

  const launcher = app.querySelector<HTMLButtonElement>("[data-action='launcher']");
  if (launcher) launcher.onclick = () => void invoke("toggle_launcher");

  const canvas = app.querySelector<HTMLButtonElement>("[data-action='canvas']");
  if (canvas) canvas.onclick = () => void invoke("open_canvas");

  const logs = app.querySelector<HTMLButtonElement>("[data-action='open-logs']");
  if (logs) logs.onclick = () => void invoke("open_log_folder");

  app.querySelectorAll<HTMLButtonElement>(".api-copy").forEach((btn) => {
    btn.onclick = async () => {
      try {
        await navigator.clipboard.writeText(btn.previousElementSibling?.textContent ?? "");
      } catch {
        /* clipboard unavailable */
      }
      btn.textContent = "Copied";
      window.setTimeout(() => {
        btn.textContent = "Copy";
      }, 1200);
    };
  });

  // Layout editor dragging
  const scale = 0.18;
  app.querySelectorAll<HTMLDivElement>("[data-drag]").forEach((box) => {
    let startX = 0;
    let startY = 0;
    let origLeft = 0;
    let origTop = 0;
    box.onmousedown = (ev) => {
      ev.preventDefault();
      startX = ev.clientX;
      startY = ev.clientY;
      origLeft = box.offsetLeft;
      origTop = box.offsetTop;
      box.classList.add("drag");
      const onMove = async (e: MouseEvent) => {
        const left = origLeft + (e.clientX - startX);
        const top = origTop + (e.clientY - startY);
        box.style.left = `${left}px`;
        box.style.top = `${top}px`;
        const x = Math.round((left - 20) / scale);
        const y = Math.round((top - 20) / scale);
        await invoke("move_widget_cmd", {
          id: box.dataset.drag,
          x,
          y,
          disableSnap: e.shiftKey,
        });
      };
      const onUp = async () => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        box.classList.remove("drag");
        await invoke("clear_guides");
        await refresh();
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    };
  });
}

function escapeHtml(value: string) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

void (async () => {
  await listen("layout://changed", () => void refresh());
  await listen("widget://opened", () => void refresh());
  await listen("widget://closed", () => void refresh());
  await listen("registry://changed", () => void refresh());
  await listen<string>("widget://error", (e) => {
    showToast(e.payload || "Widget error");
  });
  await refresh();
})();
