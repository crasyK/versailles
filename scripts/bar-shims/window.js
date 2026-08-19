export function getCurrentWindow() {
  return window.__TAURI__.window.getCurrentWindow();
}

export async function currentMonitor() {
  return getCurrentWindow().currentMonitor();
}
