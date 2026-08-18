export type Position = { x: number; y: number };

export type MediaInfo = {
  title: string;
  artist: string;
  album: string;
  status: string;
  positionMs: number;
  durationMs: number;
  thumbnailDataUrl?: string | null;
  source: string;
  hasSession?: boolean;
};

export type SnapGuide = {
  orientation: "vertical" | "horizontal";
  position: number;
};

export type SnapResult = {
  x: number;
  y: number;
  guides: SnapGuide[];
};

type Unlisten = () => void;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<T>(event, (e) => handler(e.payload));
  return () => {
    void un();
  };
}

function widgetIdFromLabel(): string | null {
  try {
    const w = window as unknown as { __DECK_WIDGET_ID__?: string };
    if (typeof w.__DECK_WIDGET_ID__ === "string" && w.__DECK_WIDGET_ID__) {
      return w.__DECK_WIDGET_ID__;
    }
    const fromQuery = new URLSearchParams(window.location.search).get("deckWidgetId");
    if (fromQuery) return fromQuery;
    const label = (
      window as unknown as {
        __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } };
      }
    ).__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
    if (label?.startsWith("widget-")) return label.slice("widget-".length);
  } catch {
    /* ignore */
  }
  return null;
}

export const deck = {
  async listWidgets() {
    return invoke("list_widgets");
  },
  async openWidget(id: string, position?: Position, alwaysOnTop?: boolean) {
    return invoke("open_widget", { id, position, alwaysOnTop });
  },
  async closeWidget(id: string) {
    return invoke("close_widget", { id });
  },
  async listOpen() {
    return invoke("list_open_widgets");
  },
  async moveTo(id: string, x: number, y: number, disableSnap = false): Promise<SnapResult> {
    return invoke("move_widget_cmd", { id, x, y, disableSnap });
  },
  async setAlwaysOnTop(id: string, value: boolean) {
    return invoke("set_widget_always_on_top", { id, value });
  },
  async popupMenu(id?: string) {
    const widgetId = id || widgetIdFromLabel();
    if (!widgetId) throw new Error("Not inside a widget window");
    return invoke("popup_widget_menu", { id: widgetId });
  },
  media: {
    async nowPlaying() {
      return invoke<MediaInfo>("media_now");
    },
    async playPause() {
      return invoke("media_play_pause_cmd");
    },
    async next() {
      return invoke("media_next_cmd");
    },
    async previous() {
      return invoke("media_previous_cmd");
    },
    async onUpdate(handler: (info: MediaInfo) => void) {
      return listen<MediaInfo>("media://update", handler);
    },
  },
  async on<T = unknown>(event: string, handler: (payload: T) => void) {
    return listen<T>(event, handler);
  },
  async clearGuides() {
    return invoke("clear_guides");
  },
  /** Left-drag via startDragging + right-click native menu (no CSS drag-region). */
  enableNativeChrome(el: HTMLElement) {
    // Signal host fallback injector before any listeners so it won't double-handle.
    (window as Window & { __DECK_NATIVE_CHROME__?: boolean }).__DECK_NATIVE_CHROME__ = true;
    el.style.cursor = el.style.cursor || "grab";
    el.removeAttribute("data-tauri-drag-region");
    const interactive = "button, a, input, select, textarea, label, [data-no-drag]";

    const eventElement = (e: Event): Element | null => {
      const t = e.target;
      if (!t) return null;
      if (t instanceof Element) return t;
      return (t as Node).parentElement;
    };

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      const elTarget = eventElement(e);
      if (elTarget?.closest(interactive)) return;
      e.preventDefault();
      void (async () => {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().startDragging();
      })().catch(console.error);
    };

    const onContext = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      void this.popupMenu();
    };

    el.addEventListener("pointerdown", onPointerDown);
    el.addEventListener("contextmenu", onContext);
    return () => {
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("contextmenu", onContext);
    };
  },
};

export type Deck = typeof deck;
export default deck;
