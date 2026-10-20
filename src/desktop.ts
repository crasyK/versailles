import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type DesktopLayout = {
  pageUrl?: string | null;
};

const pageEl = document.querySelector<HTMLIFrameElement>("#page")!;
let pageUrl = "";
let wallpaperReported = false;
/** Bumped per navigation so a stale probe cannot steer a newer load. */
let navGen = 0;
let loadRetries = 0;
const MAX_LOAD_RETRIES = 12;
const PROBE_TIMEOUT_MS = 1500;

const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

/**
 * Is the file server answering? The iframe's error page is cross-origin, so
 * `contentWindow.location` cannot tell a refused connection from a real page.
 * A no-cors HEAD resolves (opaque) when the server answers and rejects on
 * connection-refused / timeout.
 */
async function reachable(url: string): Promise<boolean> {
  const ctl = new AbortController();
  const timer = window.setTimeout(() => ctl.abort(), PROBE_TIMEOUT_MS);
  try {
    await fetch(url, { method: "HEAD", mode: "no-cors", cache: "no-store", signal: ctl.signal });
    return true;
  } catch {
    return false;
  } finally {
    window.clearTimeout(timer);
  }
}

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

/** Point the iframe at `url` once the server answers (a few short probes, then try anyway). */
async function navigate(url: string) {
  const gen = ++navGen;
  for (let i = 0; i < 6; i++) {
    if (await reachable(url)) break;
    if (gen !== navGen) return;
    await sleep(400);
  }
  if (gen !== navGen) return;
  pageEl.src = bust(url);
  reportBoot("nav");
}

function applyPage(url?: string | null, force = false) {
  if (!url) return;
  const next = withDesktopMode(url);
  if (!force && pageUrl && samePage(pageUrl, next)) return;
  pageUrl = next;
  loadRetries = 0;
  void navigate(next);
}

function reloadPage() {
  const target = pageUrl || pageEl.src;
  if (target) void navigate(target);
}

void (async () => {
  try {
    applyPage((await invoke<DesktopLayout>("get_desktop_layout")).pageUrl);
  } catch {
    /* page optional until the file exists */
  }

  // `load` also fires for the connection-refused error page. Re-probe the
  // server: if it is down, back off and reload; only a reachable load counts.
  pageEl.addEventListener("load", () => {
    const src = pageEl.src;
    if (!src) return;
    const gen = navGen;
    void reachable(src).then((ok) => {
      if (gen !== navGen) return;
      if (!ok) {
        if (loadRetries++ < MAX_LOAD_RETRIES) {
          window.setTimeout(() => {
            if (gen === navGen) reloadPage();
          }, Math.min(800 * loadRetries, 4000));
        }
        return;
      }
      loadRetries = 0;
      if (!wallpaperReported) {
        wallpaperReported = true;
        reportBoot("loaded");
      }
    });
  });

  let retries = 0;
  const bootRetry = window.setInterval(() => {
    if (pageUrl || pageEl.src) {
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

  // The broadcast carries no monitor: ask for this window's own URL (it adds
  // `monitor` / `primary` for this wallpaper), e.g. after monitors changed.
  await listen("desktop://layout", () => {
    void invoke<DesktopLayout>("get_desktop_layout")
      .then((layout) => applyPage(layout.pageUrl))
      .catch(() => {});
  });
  await listen("desktop://reload", () => reloadPage());
  await listen("registry://changed", () => reloadPage());
})();
