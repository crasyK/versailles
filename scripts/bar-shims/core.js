function callerArgs(args) {
  const caller =
    (typeof window !== "undefined" && window.__VERSAILLES_WIDGET_ID__) || undefined;
  if (!caller) return args;
  return Object.assign({ caller }, args || {});
}

export function invoke(cmd, args) {
  return window.versailles.invoke(cmd, callerArgs(args));
}
