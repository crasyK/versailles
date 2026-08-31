import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type DesktopLayout = {
  pageUrl?: string | null;
};

const pageEl = document.querySelector<HTMLIFrameElement>("#page")!;
let pageUrl = "";
let wallpaperReported = false;

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

function samePage(a: string, b: string) {
  try {
    const ua = new URL(a);
    const ub = new URL(b);
    ua.searchParams.delete("_v");
    ub.searchParams.delete("_v");
    return ua.toString() === ub.toString();
  } catch {
    return a === b;
  }
}

function bootPort(): string | null {
  try {
    if (!pageUrl) return null;
    return new URL(pageUrl).port || null;
  } catch {
    return null;
  }
}

function reportBoot(path: string) {
  const port = bootPort();
  if (!port) return;
  void fetch(`http://127.0.0.1:${port}/debug/boot/${path}`, { method: "POST" }).catch(
    () => {},
  );
}

function applyPage(url?: string | null, force = false) {
  if (!url) return;
  const next = withDesktopMode(url);
  if (!force && pageUrl && pageEl.src && samePage(pageUrl, next)) return;
  pageUrl = next;
  pageEl.src = bust(next);
  reportBoot("nav");
}

function reloadPage() {
  if (pageUrl) {
    pageEl.src = bust(pageUrl);
    reportBoot("nav");
    return;
  }
  if (pageEl.src) {
    pageEl.src = bust(pageEl.src);
    reportBoot("nav");
  }
}

function isChromeErrorHref(href: string) {
  return /chrome-error:|error-page|ERR_/i.test(href);
}

void (async () => {
  try {
    applyPage((await invoke<DesktopLayout>("get_desktop_layout")).pageUrl);
  } catch {
    /* page optional until the file exists */
  }

  // If the file server raced boot, iframe shows connection-refused — retry a few times.
  pageEl.addEventListener("load", () => {
    try {
      const href = pageEl.contentWindow?.location?.href ?? "";
      if (isChromeErrorHref(href)) {
        window.setTimeout(() => reloadPage(), 800);
        return;
      }
    } catch {
      /* cross-origin ok — real page loaded from file server */
    }
    if (!wallpaperReported && pageEl.src) {
      wallpaperReported = true;
      reportBoot("loaded");
    }
  });

  let retries = 0;
  const bootRetry = window.setInterval(() => {
    if (pageEl.src) {
      window.clearInterval(bootRetry);
      return;
    }
    if (retries++ > 6) {
      window.clearInterval(bootRetry);
      return;
    }
    void invoke<DesktopLayout>("get_desktop_layout")
      .then((layout) => {
        if (layout.pageUrl) {
          applyPage(layout.pageUrl);
          window.clearInterval(bootRetry);
        }
      })
      .catch(() => {});
  }, 700);

  await listen<DesktopLayout>("desktop://layout", (ev) =>
    applyPage(ev.payload.pageUrl),
  );
  await listen("desktop://reload", () => reloadPage());
  await listen("registry://changed", () => reloadPage());
})();
