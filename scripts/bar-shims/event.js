function sdk() {
  const v = window.versailles;
  if (v && typeof v.listen === "function") return v;
  return null;
}

export function listen(event, handler) {
  const v = sdk();
  if (v) return v.listen(event, (payload) => handler({ payload }));
  const t = window.__TAURI__;
  if (t?.event?.listen) return t.event.listen(event, handler);
  return Promise.reject(new Error("Versailles event API not available"));
}
