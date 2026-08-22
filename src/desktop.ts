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

function withDesktopMode(url: string) {
  const u = new URL(url);
  if (!u.searchParams.get("m")) u.searchParams.set("m", "desktop");
  return u.toString();
}

function applyPage(url?: string | null, force = false) {
  if (!url) return;
  const next = withDesktopMode(url);
  if (!force && pageUrl === next && pageEl.src) return;
  pageUrl = next;
  pageEl.src = bust(next);
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

  // If the file server raced boot, iframe shows connection-refused — retry a few times.
  pageEl.addEventListener("load", () => {
    try {
      const href = pageEl.contentWindow?.location?.href ?? "";
      if (/chrome-error:|error-page|ERR_/i.test(href)) {
        window.setTimeout(() => reloadPage(), 800);
      }
    } catch {
      /* cross-origin ok — real page loaded from file server */
    }
  });

  let retries = 0;
  const bootRetry = window.setInterval(() => {
    if (!pageEl.src || retries++ > 6) {
      window.clearInterval(bootRetry);
      return;
    }
    // Empty iframe or still no layout — ask host again.
    void invoke<DesktopLayout>("get_desktop_layout")
      .then((layout) => {
        if (layout.pageUrl) {
          applyPage(layout.pageUrl, true);
          window.clearInterval(bootRetry);
        }
      })
      .catch(() => {});
  }, 700);

  await listen<DesktopLayout>("desktop://layout", (ev) => applyPage(ev.payload.pageUrl, true));
  await listen("desktop://reload", () => reloadPage());
  await listen("registry://changed", () => reloadPage());
})();
