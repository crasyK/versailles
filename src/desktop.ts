import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type DesktopLayout = {
  pageUrl?: string | null;
};

const pageEl = document.querySelector<HTMLIFrameElement>("#page")!;
let pageUrl = "";

function bust(url: string) {
  const u = new URL(url);
  u.searchParams.set("_v", String(Date.now()));
  return u.toString();
}

function applyPage(url?: string | null, force = false) {
  if (!url) return;
  if (!force && pageUrl === url && pageEl.src) return;
  pageUrl = url;
  pageEl.src = bust(url);
}

function reloadPage() {
  if (pageUrl) {
    pageEl.src = bust(pageUrl);
    return;
  }
  if (pageEl.src) {
    pageEl.src = bust(pageEl.src);
  }
}

void (async () => {
  try {
    applyPage((await invoke<DesktopLayout>("get_desktop_layout")).pageUrl, true);
  } catch {
    /* page optional until the file exists */
  }
  await listen<DesktopLayout>("desktop://layout", (ev) => applyPage(ev.payload.pageUrl, true));
  await listen("desktop://reload", () => reloadPage());
  await listen("registry://changed", () => reloadPage());
})();
