/*! versailles-sdk 0.10.0 */
/**
 * Versailles widget SDK — named hooks, native chrome, browser stubs.
 *
 * Hooks are declared on the piece (`data-hooks="media,spawn"`). The host never
 * grants more than that list. Unscoped `versailles.media` infers the caller;
 * use `versailles.for("now-playing")` when several pieces share the page.
 *
 * The host ships this file and updates an unchanged older copy in
 * `Documents\Widgets\.sdk\` on start (keeping `versailles.js.bak`). Once you
 * edit your copy, the host leaves it alone.
 */
(function (global) {
  "use strict";

  /** Keep in step with the marker on the first line. */
  const SDK_VERSION = "0.10.0";

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

  /** Without a host (browser / file:) resolve a no-op unlisten so page boots finish. */
  async function listen(event, handler) {
    const t = tauri();
    if (!t?.event?.listen) return function () {};
    return t.event.listen(event, (e) => handler(e.payload));
  }

  /** `calc` is an alias of `calculator` everywhere ids are accepted. */
  function canonicalId(id) {
    const s = String(id || "").trim();
    return s.toLowerCase() === "calc" ? "calculator" : s;
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
      if (fromQuery) return canonicalId(fromQuery);
      const label =
        global.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ||
        global.__TAURI__?.window?.getCurrentWindow?.()?.label;
      if (typeof label === "string" && label.startsWith("widget-")) {
        // `widget-<id>:<n>` is a bar's copy on display n: still piece <id>.
        return label.slice("widget-".length).replace(/:\d+$/, "");
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
    const want = canonicalId(id).toLowerCase();
    const nodes = global.document ? global.document.querySelectorAll("[data-id]") : [];
    for (const el of nodes) {
      if (canonicalId(el.getAttribute("data-id")).toLowerCase() === want) return el;
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

    async function power(action) {
      const activation = global.navigator && global.navigator.userActivation;
      if (activation && !activation.isActive) {
        throw new Error("power." + action + " needs a click or key press");
      }
      return hostInvoke("power", "power_action", { action });
    }

    /**
     * Change a quick setting (`radios`, `audio-control`, `display`,
     * `power-mode`). Only inside a click or key press, and only from a bar or
     * overlay: the host refuses the wallpaper. Answers the hook's new state.
     */
    async function quick(hook, action) {
      const activation = global.navigator && global.navigator.userActivation;
      if (activation && !activation.isActive) {
        throw new Error(hook + ": changing a setting needs a click or key press");
      }
      return hostInvoke(hook, "quick_set", { action });
    }

    function sensor(hook) {
      return {
        async now() {
          return hostInvoke(hook, "sense_now", { hook });
        },
        /** `fn(value)` now and on every change. Returns an unsubscribe function. */
        async onChange(fn) {
          return on(hook, fn);
        },
      };
    }

    /**
     * `fn(data)` on each push of `name`: a hook (`media`, `battery`,
     * `network`, `focus`, `audio`, `clipboard`, `notify`, `tray`, `radios`,
     * `audio-control`, `display`, `power-mode`, `wifi`, `bluetooth`, `dnd`, `vpn`, `hotspot`, `sharing`,
     * `accessibility`; must be declared) or a piece event
     * (`shown`, `hidden`, `idle`, `active`, `theme-changed`). Sensor hooks
     * also call `fn` with the current value right away. Returns a function
     * that unsubscribes.
     */
    async function on(name, fn) {
      name = String(name);
      if (PUSH_HOOKS.has(name)) {
        const own = await gate(name);
        const off = pushSubscribe(own, name, fn);
        let live = true;
        if (SENSOR_HOOKS.has(name) && tauri()?.core?.invoke) {
          hostInvoke(name, "sense_now", { hook: name }).then(
            (value) => {
              if (live) fn(value, { name, data: value, initial: true });
            },
            () => {}
          );
        }
        return () => {
          live = false;
          off();
        };
      }
      if (PIECE_EVENTS.has(name)) {
        return pushSubscribe(fixedId || widgetIdFromLabel(), name, fn);
      }
      throw new Error("unknown push '" + name + "'");
    }

    function windowControl(id, action, to) {
      const args = { id: Number(id), action };
      if (to) args.to = to;
      return hostInvoke("windows-control", "windows_control", args);
    }

    /**
     * App windows (`windows` hook to read, `windows-control` to act). A
     * window: { id, title, app, exe, pid, monitor, minimized, maximized,
     * focused, rect, icon? }. `rect` and `move` use physical screen pixels.
     */
    const windows = {
      async list(opts) {
        const icons = !!(opts && opts.icons);
        const list = await hostInvoke("windows", "windows_list", { icons });
        return Array.isArray(list) ? list : [];
      },
      /** 32×32 PNG data URL of one window's icon, or null. */
      async icon(id) {
        return hostInvoke("windows", "windows_icon", { id: Number(id) });
      },
      /** `fn({ type: "opened"|"closed"|"focused"|"changed", id, window })` per change. */
      async onChange(fn) {
        return on("windows", fn);
      },
      /**
       * `fn(list)` now and after every change: the current windows in a
       * stable order (new ones last). `{ icons: true }` fills `icon`. Returns
       * a function that stops tracking.
       */
      async track(fn, opts) {
        const icons = !!(opts && opts.icons);
        const iconOf = new Map();
        const pending = [];
        let list = null;
        const emit = () => {
          try {
            fn(list.slice());
          } catch (err) {
            console.error(err);
          }
        };
        const fetchIcon = (id) => {
          windows.icon(id).then(
            (url) => {
              iconOf.set(id, url || null);
              const k = list ? list.findIndex((w) => w.id === id) : -1;
              if (k >= 0 && url) {
                list[k] = Object.assign({}, list[k], { icon: url });
                emit();
              }
            },
            () => {}
          );
        };
        const apply = (ev) => {
          if (!ev || ev.id == null) return;
          const i = list.findIndex((w) => w.id === ev.id);
          if (ev.type === "closed") {
            if (i >= 0) list.splice(i, 1);
            iconOf.delete(ev.id);
            return;
          }
          if (!ev.window) return;
          if (ev.type === "focused") list.forEach((w) => (w.focused = false));
          const next = Object.assign({}, ev.window);
          if (iconOf.get(ev.id)) next.icon = iconOf.get(ev.id);
          if (i >= 0) list[i] = next;
          else list.push(next);
          if (icons && !iconOf.has(ev.id)) fetchIcon(ev.id);
        };
        const off = await on("windows", (ev) => {
          if (!list) {
            pending.push(ev);
            return;
          }
          apply(ev);
          emit();
        });
        list = await windows.list({ icons });
        list.forEach((w) => iconOf.set(w.id, w.icon || null));
        pending.splice(0).forEach(apply);
        emit();
        return off;
      },
      focus(id) {
        return windowControl(id, "focus");
      },
      minimize(id) {
        return windowControl(id, "minimize");
      },
      restore(id) {
        return windowControl(id, "restore");
      },
      maximize(id) {
        return windowControl(id, "maximize");
      },
      /** Asks the app to close (it may still prompt to save). */
      close(id) {
        return windowControl(id, "close");
      },
      /** `{ x, y }`, `{ width, height }` or all four, physical px. */
      move(id, to) {
        return windowControl(id, "move", to || {});
      },
    };

    /**
     * Notification-area icons (`tray` hook). Versailles holds them only
     * while it hides the Windows taskbar (`shell.hideTaskbar`); otherwise
     * Explorer's tray has them and `active` is false. An icon: { id, tip,
     * showTip, app, exe, pid, guid, system, clickable, icon }.
     */
    const tray = {
      /** `{ active, icons }`. */
      async state() {
        const s = await hostInvoke("tray", "tray_list", {});
        return s && Array.isArray(s.icons) ? s : { active: false, icons: [] };
      },
      async list() {
        return (await tray.state()).icons;
      },
      /**
       * `fn({ type: "added"|"changed"|"removed", id, icon })` per change, `{ type: "active", active }` when Versailles starts or stops holding them,
       * `{ type: "balloon", id, title, text, kind: "info"|"warning"|"error"|"none"|"user" }` when an app shows a balloon (Windows shows it as a notification too).
       */
      async onChange(fn) {
        return on("tray", fn);
      },
      /**
       * `fn(icons, active)` now and after every change, oldest icon first.
       * Returns a function that stops tracking.
       */
      async track(fn) {
        let list = null;
        let active = false;
        const pending = [];
        const emit = () => {
          try {
            fn(list.slice(), active);
          } catch (err) {
            console.error(err);
          }
        };
        const apply = (ev) => {
          if (!ev) return;
          if (ev.type === "active") {
            active = !!ev.active;
            if (!active) list = [];
            return;
          }
          const i = list.findIndex((t) => t.id === ev.id);
          if (ev.type === "removed") {
            if (i >= 0) list.splice(i, 1);
          } else if (ev.icon) {
            if (i >= 0) list[i] = ev.icon;
            else list.push(ev.icon);
          }
        };
        const off = await on("tray", (ev) => {
          // Balloons change no icon (their `icon` is a kind, not an icon).
          if (!ev || ev.type === "balloon") return;
          if (!list) {
            pending.push(ev);
            return;
          }
          apply(ev);
          emit();
        });
        const s = await tray.state();
        list = s.icons.slice();
        active = !!s.active;
        pending.splice(0).forEach(apply);
        emit();
        return off;
      },
      /**
       * Click an icon as Explorer would: `"left"` (click), `"right"`
       * (contextmenu), `"double"` (dblclick: the second half of a double
       * click, so skip the second `click`, whose `detail` is 2) or `"middle"`;
       * from the keyboard `"key"` (Enter / Space) or `"menu"` (menu key /
       * Shift+F10). The app opens its menu at `x`, `y` (physical screen px),
       * by default where the cursor is (for keys, where the icon was last
       * clicked or hovered). Only inside a click or key press.
       */
      async click(id, button, x, y) {
        const activation = global.navigator && global.navigator.userActivation;
        if (activation && !activation.isActive) {
          throw new Error("tray.click needs a click or key press");
        }
        const args = { id: Number(id), button: String(button || "left") };
        if (Number.isFinite(x) && Number.isFinite(y)) Object.assign(args, { x: Math.round(x), y: Math.round(y) });
        return hostInvoke("tray", "tray_click", args);
      },
      /**
       * The pointer entered (`over` true) or left an icon: the app may show
       * its own pop-up (version 4 icons) or update its tooltip. `x`, `y` as
       * for `click`. No click needed.
       */
      async hover(id, over, x, y) {
        const args = { id: Number(id), over: !!over };
        if (Number.isFinite(x) && Number.isFinite(y)) Object.assign(args, { x: Math.round(x), y: Math.round(y) });
        return hostInvoke("tray", "tray_hover", args);
      },
    };

    return {
      on,
      windows,
      tray,
      battery: sensor("battery"),
      network: sensor("network"),
      focus: sensor("focus"),
      audio: sensor("audio"),
      /** Wi-Fi / Bluetooth radios and airplane mode: `{ wifi, bluetooth, airplane }` (true, false or null). */
      radios: Object.assign(sensor("radios"), {
        /** `set("wifi" | "bluetooth", on)`. */
        async set(radio, on) {
          return quick("radios", { kind: "radio", radio: String(radio), on: !!on });
        },
        /** Airplane mode on (all radios off) or off. Rejects when `airplane` reads null. */
        async setAirplane(on) {
          return quick("radios", { kind: "airplane", on: !!on });
        },
      }),
      /** Volume, mute and the default output. `now()`: `{ outputs: [{ id, name, default }] }`. */
      audioControl: Object.assign(sensor("audio-control"), {
        async setVolume(level) {
          return quick("audio-control", { kind: "volume", level: Number(level) });
        },
        async setMute(on) {
          return quick("audio-control", { kind: "mute", on: !!on });
        },
        /** Make an output from `now().outputs` the default. */
        async setOutput(id) {
          return quick("audio-control", { kind: "output", id: String(id) });
        },
      }),
      /** `{ projection, brightness: [{ id, device, kind, level }], nightLight: { supported, on } }`. */
      display: Object.assign(sensor("display"), {
        /** Night light on or off. Rejects when `nightLight.supported` is false. */
        async setNightLight(on) {
          return quick("display", { kind: "nightLight", on: !!on });
        },
        async setBrightness(id, level) {
          return quick("display", { kind: "brightness", id: String(id), level: Number(level) });
        },
        /** Win+P: `"pc"`, `"duplicate"`, `"extend"` or `"second"`. */
        async project(mode) {
          return quick("display", { kind: "project", mode: String(mode) });
        },
      }),
      /**
       * Wi-Fi networks in range: `{ access, connected, networks: [{ ssid, signal, secure, known, connected }] }`.
       * `access: "location-denied"` when Windows' location permission is off.
       */
      wifi: Object.assign(sensor("wifi"), {
        /** Look for networks now; the list updates a few seconds later. No click needed. */
        async scan() {
          return hostInvoke("wifi", "quick_set", { action: { kind: "wifiScan" } });
        },
        /** Join `ssid`; `password` for a new secured network (known ones need none). */
        async connect(ssid, password) {
          const action = { kind: "wifiConnect", ssid: String(ssid) };
          if (password != null && password !== "") action.password = String(password);
          return quick("wifi", action);
        },
        async disconnect() {
          return quick("wifi", { kind: "wifiDisconnect" });
        },
      }),
      /** Paired Bluetooth devices: `{ devices: [{ id, name, connected, audio }] }`. */
      bluetooth: Object.assign(sensor("bluetooth"), {
        /** Audio devices (`audio: true`) only. */
        async connect(id) {
          return quick("bluetooth", { kind: "bluetoothConnect", id: String(id) });
        },
        async disconnect(id) {
          return quick("bluetooth", { kind: "bluetoothDisconnect", id: String(id) });
        },
      }),
      /** Do Not Disturb: `{ supported, mode }`; `mode`: `"off"`, `"priority"`, `"alarms"`. */
      dnd: Object.assign(sensor("dnd"), {
        async set(mode) {
          return quick("dnd", { kind: "dnd", mode: String(mode) });
        },
      }),
      /**
       * Act on Windows notifications (read them with `notify`). Each answers
       * the notifications as `notify.now()` would.
       */
      notifyControl: {
        /** Remove one notification by its `id`. */
        async dismiss(id) {
          return quick("notify-control", { kind: "notifyDismiss", id: Number(id) });
        },
        /** Remove all of them. */
        async clear() {
          return quick("notify-control", { kind: "notifyClear" });
        },
        /** Open the app that raised a notification (its `aumid`). */
        async open(aumid) {
          return quick("notify-control", { kind: "notifyOpen", aumid: String(aumid) });
        },
        /** Turn an app's own Windows pop-ups on or off; its notifications still arrive. */
        async banners(aumid, on) {
          return quick("notify-control", { kind: "notifyBanners", aumid: String(aumid), on: !!on });
        },
      },
      /** `{ supported, mode, energySaver }`; `mode`: `"efficiency"`, `"balanced"`, `"performance"`. */
      powerMode: Object.assign(sensor("power-mode"), {
        async set(mode) {
          return quick("power-mode", { kind: "powerMode", mode: String(mode) });
        },
      }),
      // ── connections and accessibility (#63–#66) ──
      /**
       * Windows' own VPN profiles: `{ profiles: [{ name, connected, connecting }] }`.
       * VPN apps (Tailscale, ProtonVPN...) are not listed.
       */
      vpn: Object.assign(sensor("vpn"), {
        /**
         * Connect with the saved sign-in. Answers the new state, or
         * `{ ok: true, dialog: true }` when Windows' own dialog opened to ask
         * for credentials (`settings: true`: Settings opened, for a VPN app's profile).
         */
        async connect(name) {
          return quick("vpn", { kind: "vpnConnect", name: String(name) });
        },
        async disconnect(name) {
          return quick("vpn", { kind: "vpnDisconnect", name: String(name) });
        },
      }),
      /** Mobile hotspot: `{ supported, on, busy, clients, maxClients, reason }`. */
      hotspot: Object.assign(sensor("hotspot"), {
        async set(on) {
          return quick("hotspot", { kind: "hotspot", on: !!on });
        },
      }),
      /**
       * Cast and nearby sharing: `{ casting, nearby }`; `nearby`: `"off"`,
       * `"my-devices"`, `"everyone"` or null.
       */
      sharing: Object.assign(sensor("sharing"), {
        /** Open Windows' Cast panel (as Win+K): pick a screen there, or disconnect. */
        async openCast() {
          return quick("sharing", { kind: "castPanel" });
        },
        async setNearby(mode) {
          return quick("sharing", { kind: "nearby", mode: String(mode) });
        },
      }),
      /**
       * `{ magnifier, contrast, captions, colorFilter, stickyKeys }` (true, false or null).
       * `set(name, on)` answers the new state, or `{ ok: true, settings: true }`
       * when Settings opened instead (turning a contrast theme off).
       */
      accessibility: Object.assign(sensor("accessibility"), {
        async set(name, on) {
          return quick("accessibility", { kind: "accessibility", name: String(name), on: !!on });
        },
      }),
      /** Windows toast notifications, read-only. See docs/HOOKS.md. */
      notify: {
        /** `{ access, items }`: the recent toasts, newest first. */
        async now() {
          return hostInvoke("notify", "sense_now", { hook: "notify" });
        },
        /** `fn(toast)` for each new toast. Returns an unsubscribe function. */
        async onToast(fn) {
          return on("notify", fn);
        },
      },
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
        /** Change the piece's own window in one call. See docs/HOOKS.md. */
        async update(patch) {
          return hostInvoke("layout", "layout_update", { patch: patch || {} });
        },
        async resize(width, height) {
          return hostInvoke("layout", "layout_update", { patch: { width, height } });
        },
        /** Grid anchor (`tl` … `br`, `c`, `cursor`) + optional `[x, y]` inset. */
        async setAnchor(anchor, offset) {
          const patch = { anchor };
          if (offset) patch.offset = [offset[0] | 0, offset[1] | 0];
          return hostInvoke("layout", "layout_update", { patch });
        },
        async setAlwaysOnTop(on) {
          return hostInvoke("layout", "layout_update", { patch: { alwaysOnTop: !!on } });
        },
        /**
         * Only these rects ({ x, y, width, height }, CSS px in the window) take
         * clicks; the rest of the window passes them through. `null` = off.
         */
        async setHitRegions(rects) {
          return hostInvoke("layout", "layout_update", { patch: { hitRegions: rects } });
        },
        /**
         * Keep hit regions in sync with elements matching `selector`
         * (default `[data-hit]`). Returns a function that stops tracking and
         * makes the whole window take clicks again.
         */
        trackHits(selector) {
          const sel = selector || "[data-hit]";
          const doc = global.document;
          if (!doc) return () => {};
          const setHits = (rects) =>
            hostInvoke("layout", "layout_update", { patch: { hitRegions: rects } });
          let last = "";
          let frame = 0;
          let stopped = false;
          const send = () => {
            frame = 0;
            if (stopped) return;
            const rects = Array.from(doc.querySelectorAll(sel))
              .map((el) => el.getBoundingClientRect())
              .filter((r) => r.width > 0 && r.height > 0)
              .map((r) => ({
                x: Math.floor(r.left),
                y: Math.floor(r.top),
                width: Math.ceil(r.width),
                height: Math.ceil(r.height),
              }));
            const key = JSON.stringify(rects);
            if (key === last) return;
            last = key;
            setHits(rects).catch(() => {});
          };
          const schedule = () => {
            if (!frame && !stopped) frame = global.requestAnimationFrame(send);
          };
          const mo = new MutationObserver(schedule);
          mo.observe(doc.documentElement, { subtree: true, childList: true, attributes: true });
          const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(schedule) : null;
          if (ro) ro.observe(doc.documentElement);
          global.addEventListener("resize", schedule);
          doc.addEventListener("transitionend", schedule, true);
          doc.addEventListener("animationend", schedule, true);
          // Transforms mid-animation change no attribute: sample a few times a second.
          const tick = global.setInterval(schedule, 250);
          schedule();
          return () => {
            stopped = true;
            mo.disconnect();
            if (ro) ro.disconnect();
            global.removeEventListener("resize", schedule);
            doc.removeEventListener("transitionend", schedule, true);
            doc.removeEventListener("animationend", schedule, true);
            global.clearInterval(tick);
            if (frame) global.cancelAnimationFrame(frame);
            setHits(null).catch(() => {});
          };
        },
      },
      /**
       * Toggle a spawnable. With `data`, it opens (or stays open) and receives
       * the data: `versailles.spawnData()` / `versailles.onSpawn(fn)` there.
       */
      async spawn(spawnId, data) {
        const id = await gate("spawn");
        if (!tauri()?.core?.invoke) return { host: false, ok: false };
        const args = { id: spawnId, caller: id };
        if (data !== undefined) args.data = data;
        return invoke("toggle_slideout", args);
      },
      shell: {
        async open(target) {
          return hostInvoke("shell", "cli_open", { target });
        },
      },
      /** The piece's own key-value store (JSON values, ~1 MB per piece). */
      storage: {
        async get(key) {
          return hostInvoke("storage", "storage_op", { op: { op: "get", key } });
        },
        async set(key, value) {
          return hostInvoke("storage", "storage_op", { op: { op: "set", key, value } });
        },
        async delete(key) {
          return hostInvoke("storage", "storage_op", { op: { op: "delete", key } });
        },
        async keys() {
          return hostInvoke("storage", "storage_op", { op: { op: "keys" } });
        },
        async clear() {
          return hostInvoke("storage", "storage_op", { op: { op: "clear" } });
        },
      },
      clipboard: {
        async readText() {
          return hostInvoke("clipboard", "clipboard_read");
        },
        async writeText(text) {
          return hostInvoke("clipboard", "clipboard_write", { text: String(text) });
        },
        /** `fn()` whenever the system clipboard changes (read it if you need it). */
        async onChange(fn) {
          return on("clipboard", () => fn());
        },
      },
      /**
       * Lock / sleep / sign out / restart / shut down. Only runs inside a
       * click or key press, so a page cannot do it on its own.
       */
      power: {
        async lock() {
          return power("lock");
        },
        async sleep() {
          return power("sleep");
        },
        async signOut() {
          return power("signOut");
        },
        async restart() {
          return power("restart");
        },
        async shutdown() {
          return power("shutdown");
        },
      },
      /** Messages between pieces that declare `bus`. */
      bus: {
        async publish(topic, data) {
          return hostInvoke("bus", "bus_publish", { topic: String(topic), data: data === undefined ? null : data });
        },
        /**
         * `fn(data, message)` for each message on `topic` (`"*"` = all) from
         * another piece. Returns a function that unsubscribes.
         */
        async subscribe(topic, fn) {
          const own = await gate("bus");
          return busSubscribe(own, String(topic), fn);
        },
      },
    };

  }

  // The host delivers bus messages by calling this in each receiving window.
  const busSubscribers = new Set();
  global.__VERSAILLES_BUS__ = function (message) {
    if (!message || typeof message.topic !== "string") return;
    busSubscribers.forEach((sub) => {
      if (sub.own && message.from && message.from.toLowerCase() === sub.own.toLowerCase()) return;
      if (sub.topic !== "*" && sub.topic !== message.topic) return;
      try {
        sub.fn(message.data, message);
      } catch (err) {
        console.error(err);
      }
    });
  };
  function busSubscribe(own, topic, fn) {
    const sub = { own, topic, fn };
    busSubscribers.add(sub);
    return () => busSubscribers.delete(sub);
  }

  // The host pushes hook data and piece events by calling this in each
  // receiving window: { name, data, id? }. `id` = meant for that piece only.
  const pushSubscribers = new Set();
  global.__VERSAILLES_PUSH__ = function (push) {
    if (!push || typeof push.name !== "string") return;
    pushSubscribers.forEach((sub) => {
      if (sub.name !== push.name) return;
      if (push.id && sub.own && push.id.toLowerCase() !== sub.own.toLowerCase()) return;
      try {
        sub.fn(push.data, push);
      } catch (err) {
        console.error(err);
      }
    });
  };
  function pushSubscribe(own, name, fn) {
    const sub = { own, name, fn };
    pushSubscribers.add(sub);
    return () => pushSubscribers.delete(sub);
  }
  /** Hooks whose changes the host pushes; `on()` needs them declared. */
  const QUICK_HOOKS = ["radios", "audio-control", "display", "power-mode", "wifi", "bluetooth", "dnd",
    "vpn", "hotspot", "sharing", "accessibility"];
  const PUSH_HOOKS = new Set(["media", "battery", "network", "focus", "audio", "clipboard", "windows", "notify", "tray"].concat(QUICK_HOOKS));
  const SENSOR_HOOKS = new Set(["battery", "network", "focus", "audio"].concat(QUICK_HOOKS));
  // On the wallpaper this page runs in an iframe of the host shell, which
  // forwards bus messages and pushes here. Only the parent may send them.
  if (global.parent && global.parent !== global && typeof global.addEventListener === "function") {
    global.addEventListener("message", (e) => {
      const msg = e.source === global.parent && e.data && e.data.__versailles;
      if (!msg) return;
      if (msg.kind === "push") global.__VERSAILLES_PUSH__(msg.payload);
      else if (msg.kind === "bus") global.__VERSAILLES_BUS__(msg.payload);
    });
  }
  /** Piece events: for every piece, no hook needed. */
  const PIECE_EVENTS = new Set(["shown", "hiding", "hidden", "idle", "active", "theme-changed", "monitor-changed"]);

  // Wallpaper: each monitor's wallpaper loads this page with `monitor=<n>`
  // and `primary=0|1`. Mark <html> with them (`data-monitor`,
  // `data-monitor-primary`) for page CSS, and show each `.widget` or other
  // `[data-monitor]` element only where its `data-monitor` says: `all`,
  // `primary` (the default for a widget without one) or a display number.
  (function applyMonitorFilter() {
    const doc = global.document;
    if (!doc || !global.location) return;
    const query = new URLSearchParams(global.location.search);
    const here = query.get("monitor");
    if (query.get("m") !== "desktop" || !here) return;
    const onPrimary = query.get("primary") === "1";
    doc.documentElement.setAttribute("data-monitor", here);
    doc.documentElement.setAttribute("data-monitor-primary", onPrimary ? "1" : "0");
    function shownHere(raw) {
      const sel = String(raw || "").trim().toLowerCase();
      if (!sel || sel === "primary" || sel === "main" || sel === "cursor") return onPrimary;
      if (sel === "all") return true;
      return sel === here;
    }
    function apply() {
      doc.querySelectorAll(".widget, body [data-monitor]").forEach((el) => {
        if (!shownHere(el.getAttribute("data-monitor"))) el.style.setProperty("display", "none", "important");
      });
    }
    if (doc.readyState === "loading") doc.addEventListener("DOMContentLoaded", apply, { once: true });
    else apply();
  })();

  /** Every monitor: `{ number, primary, name, scale, bounds, work }` (logical sizes). */
  async function monitors() {
    if (!tauri()?.core?.invoke) return [];
    const report = await invoke("monitor_info");
    return (report && report.monitors) || [];
  }

  /**
   * The monitor this window is on (`null` outside the host). A wallpaper and
   * a docked bar (each copy of a `data-monitor="all"` bar) load with
   * `monitor=<n>`, so this is their own monitor even while they move.
   */
  async function monitor() {
    if (!tauri()?.core?.invoke) return null;
    const report = await invoke("monitor_info");
    const list = (report && report.monitors) || [];
    const fromUrl = Number(new URLSearchParams(global.location ? global.location.search : "").get("monitor"));
    const number = fromUrl || (report && report.current);
    return list.find((m) => m.number === number) || null;
  }

  const scoped = makeApi(null);
  const versailles = {
    version: SDK_VERSION,
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
    storage: scoped.storage,
    clipboard: scoped.clipboard,
    power: scoped.power,
    bus: scoped.bus,
    on: scoped.on,
    battery: scoped.battery,
    network: scoped.network,
    focus: scoped.focus,
    audio: scoped.audio,
    radios: scoped.radios,
    audioControl: scoped.audioControl,
    display: scoped.display,
    powerMode: scoped.powerMode,
    wifi: scoped.wifi,
    bluetooth: scoped.bluetooth,
    dnd: scoped.dnd,
    notifyControl: scoped.notifyControl,
    windows: scoped.windows,
    tray: scoped.tray,
    notify: scoped.notify,
    /** Data the last `spawn(id, data)` handed this spawnable (or undefined). */
    monitors,
    monitor,
    spawnData() {
      return global.__VERSAILLES_SPAWN_DATA__;
    },
    /** `fn(data)` each time `spawn(id, data)` hands this spawnable data. */
    onSpawn(fn) {
      const handler = (e) => fn(e.detail);
      global.addEventListener("versailles:spawn", handler);
      return () => global.removeEventListener("versailles:spawn", handler);
    },
    async closeWidget(id) {
      return invoke("close_widget", { id: id || widgetIdFromLabel() });
    },
  };

  global.versailles = versailles;
  global.deck = versailles;
})(typeof window !== "undefined" ? window : globalThis);
