/**
 * Versailles widget SDK — named hooks, native chrome, browser stubs.
 *
 * Hooks are declared on the piece (`data-hooks="media,spawn"`). The host never
 * grants more than that list. Unscoped `versailles.media` infers the caller;
 * use `versailles.for("now-playing")` when several pieces share the page.
 */
(function (global) {
  "use strict";

  function inIframe() {
    try {
      return global.parent && global.parent !== global;
    } catch {
      return true;
    }
  }

  function tauri() {
    try {
      if (global.__TAURI__?.core?.invoke) return global.__TAURI__;
      if (inIframe() && global.parent.__TAURI__?.core?.invoke) {
        return global.parent.__TAURI__;
      }
    } catch {
      /* cross-origin parent */
    }
    return null;
  }

  async function invoke(cmd, args) {
    const t = tauri();
    if (!t?.core?.invoke) throw new Error("Versailles API not available in this window");
    return t.core.invoke(cmd, args);
  }

  async function listen(event, handler) {
    const t = tauri();
    if (!t?.event?.listen) throw new Error("Versailles event API not available");
    return t.event.listen(event, (e) => handler(e.payload));
  }

  function widgetIdFromLabel() {
    try {
      if (typeof global.__VERSAILLES_WIDGET_ID__ === "string" && global.__VERSAILLES_WIDGET_ID__) {
        return global.__VERSAILLES_WIDGET_ID__;
      }
      if (typeof global.__DECK_WIDGET_ID__ === "string" && global.__DECK_WIDGET_ID__) {
        return global.__DECK_WIDGET_ID__;
      }
      const params = new URLSearchParams(global.location?.search || "");
      const fromQuery = params.get("versaillesWidgetId") || params.get("deckWidgetId") || params.get("id");
      if (fromQuery) return fromQuery;
      const label =
        global.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ||
        global.__TAURI__?.window?.getCurrentWindow?.()?.label;
      if (typeof label === "string" && label.startsWith("widget-")) {
        return label.slice("widget-".length);
      }
    } catch {
      /* ignore */
    }
    return null;
  }

  async function waitForTauri(attempts = 50, delayMs = 100) {
    for (let i = 0; i < attempts; i++) {
      if (tauri()?.core?.invoke) return tauri();
      await new Promise((r) => setTimeout(r, delayMs));
    }
    return null;
  }

  async function startDragging() {
    const t = tauri();
    try {
      const win = t?.window?.getCurrentWindow?.();
      if (win?.startDragging) {
        await win.startDragging();
        return;
      }
    } catch {
      /* fall through */
    }
    await invoke("plugin:webview|start_dragging").catch(() =>
      invoke("plugin:window|start_dragging")
    );
  }

  /**
   * Native drag (left-button) + right-click native OS menu.
   * Avoids data-tauri-drag-region so contextmenu events still fire in WebView2.
   */
  function enableNativeChrome(el) {
    if (inIframe()) return () => {};
    global.__VERSAILLES_NATIVE_CHROME__ = true;
    global.__DECK_NATIVE_CHROME__ = true;
    if (!el) return () => {};
    el.style.cursor = el.style.cursor || "grab";
    el.removeAttribute("data-tauri-drag-region");
    el.querySelectorAll("[data-tauri-drag-region]").forEach((node) => {
      node.removeAttribute("data-tauri-drag-region");
    });

    const interactive = "button, a, input, select, textarea, label, [data-no-drag]";

    function eventElement(e) {
      const t = e.target;
      if (!t) return null;
      if (t.nodeType === 1) return t;
      return t.parentElement || null;
    }

    const onPointerDown = (e) => {
      if (e.button !== 0) return;
      const elTarget = eventElement(e);
      if (elTarget && elTarget.closest(interactive)) return;
      e.preventDefault();
      startDragging().catch((err) => console.error(err));
    };

    const onContext = async (e) => {
      e.preventDefault();
      e.stopPropagation();
      const id = widgetIdFromLabel();
      if (!id) {
        console.error("Versailles: cannot resolve widget id for context menu");
        return;
      }
      try {
        await invoke("popup_widget_menu", { id });
      } catch (err) {
        console.error(err);
      }
    };

    el.addEventListener("pointerdown", onPointerDown);
    el.addEventListener("contextmenu", onContext);
    return () => {
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("contextmenu", onContext);
    };
  }

  function parseHooks(el) {
    if (!el || !el.getAttribute) return new Set();
    return new Set(
      String(el.getAttribute("data-hooks") || "")
        .split(",")
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean)
    );
  }

  function findPiece(id) {
    if (!id) return null;
    const nodes = global.document ? global.document.querySelectorAll("[data-id]") : [];
    for (const el of nodes) {
      if (String(el.getAttribute("data-id") || "") === id) return el;
    }
    return null;
  }

  function inferCaller(hook) {
    const fromLabel = widgetIdFromLabel();
    if (fromLabel) return fromLabel;
    if (!global.document) return null;
    const matches = [];
    global.document.querySelectorAll("[data-hooks][data-id]").forEach((el) => {
      if (parseHooks(el).has(hook)) matches.push(el.getAttribute("data-id"));
    });
    const uniq = [...new Set(matches.filter(Boolean))];
    return uniq.length === 1 ? uniq[0] : null;
  }

  function undeclared(hook, id) {
    const err = new Error(
      id
        ? "hook '" + hook + "' not declared on '" + id + "'"
        : "hook '" + hook + "' requires versailles.for(id) or data-hooks on the piece"
    );
    err.code = "hook-undeclared";
    return err;
  }

  function makeApi(fixedId) {
    async function gate(hook) {
      const id = fixedId || inferCaller(hook);
      if (!id) throw undeclared(hook, null);
      const el = findPiece(id);
      if (el && !parseHooks(el).has(hook)) throw undeclared(hook, id);
      if (!el && !fixedId) throw undeclared(hook, id);
      // Template not in the live tree (desktop mode): still pass caller; host enforces.
      return id;
    }

    async function hostInvoke(hook, cmd, args) {
      const id = await gate(hook);
      if (!tauri()?.core?.invoke) return { host: false };
      return invoke(cmd, Object.assign({ caller: id }, args || {}));
    }

    return {
      media: {
        async now() {
          return hostInvoke("media", "media_now");
        },
        async nowPlaying() {
          return hostInvoke("media", "media_now");
        },
        async playPause() {
          return hostInvoke("media", "media_play_pause_cmd");
        },
        async next() {
          return hostInvoke("media", "media_next_cmd");
        },
        async previous() {
          return hostInvoke("media", "media_previous_cmd");
        },
        async onUpdate(handler) {
          await gate("media");
          if (!tauri()?.event?.listen) return () => {};
          return listen("media://update", handler);
        },
      },
      mouse: {
        async position() {
          return hostInvoke("mouse", "get_mouse_position_cmd");
        },
      },
      layout: {
        async move(x, y) {
          const id = await gate("layout");
          if (!tauri()?.core?.invoke) return { host: false };
          return invoke("move_widget_cmd", { id, x, y, caller: id });
        },
      },
      async spawn(spawnId) {
        const id = await gate("spawn");
        if (!tauri()?.core?.invoke) return { host: false, ok: false };
        return invoke("toggle_slideout", { id: spawnId, caller: id });
      },
      shell: {
        async open(target) {
          return hostInvoke("shell", "cli_open", { target });
        },
      },
    };
  }

  const scoped = makeApi(null);
  const versailles = {
    invoke,
    listen,
    waitForTauri,
    widgetId: widgetIdFromLabel,
    enableNativeChrome,
    drag: {
      enable: enableNativeChrome,
    },
    for(id) {
      return makeApi(String(id || ""));
    },
    media: scoped.media,
    mouse: scoped.mouse,
    layout: scoped.layout,
    spawn: scoped.spawn,
    shell: scoped.shell,
    async closeWidget(id) {
      return invoke("close_widget", { id: id || widgetIdFromLabel() });
    },
  };

  global.versailles = versailles;
  global.deck = versailles;
})(typeof window !== "undefined" ? window : globalThis);
