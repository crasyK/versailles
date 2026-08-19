function invoke(cmd, args) {
  const v = window.versailles;
  if (v && typeof v.invoke === "function") return v.invoke(cmd, args);
  const t = window.__TAURI__;
  if (t?.core?.invoke) return t.core.invoke(cmd, args);
  return Promise.reject(new Error("Versailles API not available"));
}

function label() {
  return window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label || "widget-action-bar";
}

function ipcSize(size) {
  const kind = size?.type === "Physical" ? "Physical" : "Logical";
  return { [kind]: { width: size.width, height: size.height } };
}

function ipcPos(pos) {
  const kind = pos?.type === "Physical" ? "Physical" : "Logical";
  return { [kind]: { x: pos.x, y: pos.y } };
}

export function getCurrentWindow() {
  const l = label();
  return {
    label: l,
    async setSize(size) {
      return invoke("plugin:window|set_size", { label: l, value: ipcSize(size) });
    },
    async setPosition(position) {
      return invoke("plugin:window|set_position", { label: l, value: ipcPos(position) });
    },
    async currentMonitor() {
      return invoke("plugin:window|current_monitor");
    },
    async isFocused() {
      return invoke("plugin:window|is_focused", { label: l });
    },
    async onFocusChanged(handler) {
      const v = window.versailles;
      if (!v || typeof v.listen !== "function") return () => {};
      const offFocus = await v.listen("tauri://focus", () => handler({ payload: true }));
      const offBlur = await v.listen("tauri://blur", () => handler({ payload: false }));
      return () => {
        try {
          offFocus();
        } catch {
          /* ignore */
        }
        try {
          offBlur();
        } catch {
          /* ignore */
        }
      };
    },
  };
}

export async function currentMonitor() {
  return getCurrentWindow().currentMonitor();
}
