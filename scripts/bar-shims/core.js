function callerArgs(args) {
  const caller =
    (typeof window !== "undefined" && window.__VERSAILLES_WIDGET_ID__) || undefined;
  if (!caller) return args;
  return Object.assign({ caller }, args || {});
}

function sdk() {
  const v = window.versailles;
  if (v && typeof v.invoke === "function") return v;
  return null;
}

export function invoke(cmd, args) {
  const v = sdk();
  if (v) return v.invoke(cmd, callerArgs(args));
  const t = window.__TAURI__;
  if (t?.core?.invoke) return t.core.invoke(cmd, callerArgs(args));
  return Promise.reject(new Error("Versailles API not available in this window"));
}
