function sdk() {
  const v = window.versailles;
  if (v && typeof v.listen === "function") return v;
  return null;
}

/** Preview (no host): resolve a no-op unlisten so the bar still boots. */
export function listen(event, handler) {
  const t = window.__TAURI__;
  const v = sdk();
  if (v) {
    return Promise.resolve(v.listen(event, (payload) => handler({ event, payload }))).then(
      (off) => (typeof off === "function" ? off : () => {}),
      () => () => {},
    );
  }
  if (t?.event?.listen) return t.event.listen(event, handler);
  return Promise.resolve(() => {});
}
