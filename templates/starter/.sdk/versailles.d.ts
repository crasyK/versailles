/*! versailles-sdk types (kept in step with versailles.js) */
/**
 * Types for the Versailles page SDK (`.sdk/versailles.js`), for editors and
 * agents. Reference it from a page script with
 * `/// <reference path="../.sdk/versailles.d.ts" />`, or add it to a
 * `jsconfig.json` / `tsconfig.json`.
 *
 * Contract (docs/HOOKS.md, docs/API-REVIEW-1.0.md):
 * - A hook call works only when the piece lists the hook in `data-hooks`.
 * - Without the host (a plain browser, `file:`) hook calls resolve
 *   `{ host: false }` instead of throwing.
 * - Every rejection is a {@link VersaillesError}: an `Error` with `code`.
 * - `now()` reads once; `onChange(fn)` / `on(hook, fn)` subscribe (state
 *   hooks call `fn` with the current value first); `track(fn)` keeps a list
 *   current. Subscriptions resolve to a function that unsubscribes.
 *
 * Names marked `@deprecated` keep working through 1.x.
 */

export {};

declare global {
  /** The SDK. Also `window.versailles`. */
  var versailles: Versailles;
  /** @deprecated The pre-rename global; the same object as `versailles`. */
  var deck: Versailles;
  /** Set by the host in a spawn window: the piece id. */
  var __VERSAILLES_WIDGET_ID__: string | undefined;
  /** Set by the host for `data-dim="page"` overlays: the piece's box (logical px). */
  var __VERSAILLES_STAGE__: Versailles.Rect | undefined;
  /** Data the last `spawn(id, data)` handed this spawnable. */
  var __VERSAILLES_SPAWN_DATA__: unknown;

  interface WindowEventMap {
    /** `spawn(id, data)` handed this spawnable data (`detail`). */
    "versailles:spawn": CustomEvent<unknown>;
    /** A `data-dim="page"` overlay's piece box changed (`detail`). */
    "versailles:stage": CustomEvent<Versailles.Rect>;
  }

  /** The whole SDK object: the hook namespaces plus page-level helpers. */
  interface Versailles extends Versailles.Hooks {
    /** The SDK version (`"0.11.1"`), as on the file's first line. */
    readonly version: string;
    /**
     * Raw host command (Tauri `invoke`). Hook commands need `caller` from the
     * wallpaper; prefer the namespaces. Documented commands: `sys_stats`,
     * `list_catalog`, `catalog_icon`, `dismiss_launcher`, `monitor_info`,
     * `get_spawnable_engine_context`.
     */
    invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
    /**
     * Listen to a host event (`overlay://shown`, `pty://data`, …). Without a
     * host it resolves a no-op unlisten function.
     */
    listen<T = unknown>(event: string, handler: (payload: T) => void): Promise<Versailles.Unsubscribe>;
    /** Wait until the host bridge is there (up to `attempts` × `delayMs`). */
    waitForTauri(attempts?: number, delayMs?: number): Promise<unknown | null>;
    /** The piece id of this window (spawn windows, bar copies), or null. */
    widgetId(): string | null;
    /**
     * Native drag from `el` (left button) and the host's right-click menu
     * (folder widgets). Returns a function that removes it.
     */
    enableNativeChrome(el?: Element | null): () => void;
    drag: {
      /** @deprecated Use `enableNativeChrome(el)`. */
      enable(el?: Element | null): () => void;
    };
    /** The API acting as piece `id` (the wallpaper hosts many pieces). */
    for(id: string): Versailles.Scope;
    /** Every monitor (logical sizes). `[]` without a host. */
    monitors(): Promise<Versailles.Monitor[]>;
    /** The monitor this page or window is on; null without a host. */
    monitor(): Promise<Versailles.Monitor | null>;
    /** Data the last `spawn(id, data)` handed this spawnable (or undefined). */
    spawnData(): unknown;
    /** `fn(data)` each time `spawn(id, data)` hands this spawnable data. */
    onSpawn(fn: (data: unknown) => void): Versailles.Unsubscribe;
    /** Close a floating folder widget (default: this window's). */
    closeWidget(id?: string): Promise<void>;
  }

  namespace Versailles {
    /** A function that ends a subscription. */
    type Unsubscribe = () => void;
    /** What a hook call resolves to outside the host. */
    interface NoHost {
      host: false;
    }
    /** `T`, or `{ host: false }` outside the host. */
    type Hosted<T> = T | NoHost;

    /** Codes on {@link VersaillesError}. */
    type ErrorCode =
      | "no-host"
      | "hook-undeclared"
      | "hook-refused"
      | "unknown-hook"
      | "needs-gesture"
      | "unknown-event"
      | "bad-argument"
      | "host-error";

    /** Every SDK rejection. `message` is the host's or the SDK's text. */
    interface VersaillesError extends Error {
      code: ErrorCode;
    }

    interface Rect {
      x: number;
      y: number;
      width: number;
      height: number;
    }

    // ── payloads ──

    interface Monitor {
      /** Display number as in Windows' display settings. */
      number: number;
      primary: boolean;
      name: string;
      scale: number;
      /** Logical px. */
      bounds: Rect;
      /** Work area, logical px. */
      work: Rect;
    }

    interface MediaInfo {
      title: string;
      artist: string;
      album: string;
      /** `"Playing"`, `"Paused"`, `"Stopped"`, … as Windows names it. */
      status: string;
      positionMs: number;
      durationMs: number;
      thumbnailDataUrl: string | null;
      source: string;
      hasSession: boolean;
    }

    interface Battery {
      /** `false` on a desktop PC. */
      present: boolean;
      percent: number | null;
      plugged: boolean | null;
      charging: boolean;
      secondsLeft: number | null;
      saver: boolean;
    }

    interface Network {
      online: boolean;
      level: "none" | "local" | "constrained" | "internet";
      name: string | null;
      wifi: boolean;
      signalBars: number | null;
    }

    interface Focus {
      title: string;
      app: string | null;
      exe: string | null;
      pid: number;
      /** A Versailles window. */
      self: boolean;
    }

    interface AudioDevice {
      device: string | null;
      volume: number | null;
      muted: boolean | null;
    }

    interface Audio extends AudioDevice {
      /** The default microphone; null without one. */
      input: AudioDevice | null;
    }

    interface Posture {
      /** null on a PC that reports no posture. */
      tablet: boolean | null;
      docked: boolean;
      touch: { screen: boolean; pen: boolean; maxTouches: number };
    }

    interface Radios {
      wifi: boolean | null;
      bluetooth: boolean | null;
      airplane: boolean | null;
    }

    interface Endpoint {
      id: string;
      name: string;
      default: boolean;
    }

    interface AudioSession {
      id: string;
      app: string;
      exe: string | null;
      pid: number;
      /** 32×32 PNG data URL, or null. */
      icon: string | null;
      volume: number;
      muted: boolean;
      active: boolean;
      system: boolean;
    }

    interface AudioControl {
      outputs: Endpoint[];
      inputs: Endpoint[];
      sessions: AudioSession[];
    }

    type Projection = "pc" | "duplicate" | "extend" | "second";

    interface Display {
      projection: Projection | null;
      brightness: { id: string; device: string | null; kind: "internal" | "external"; level: number }[];
      nightLight: { supported: boolean; on: boolean | null };
    }

    type PowerModeName = "efficiency" | "balanced" | "performance";

    interface PowerMode {
      supported: boolean;
      mode: PowerModeName | null;
      energySaver: boolean | null;
    }

    interface WifiNetwork {
      ssid: string;
      signal: number;
      secure: boolean;
      known: boolean;
      connected: boolean;
    }

    interface Wifi {
      access: "ok" | "location-denied" | "no-adapter";
      /** The SSID Windows is connected to, or null. */
      connected: string | null;
      networks: WifiNetwork[];
    }

    interface BluetoothDevice {
      id: string;
      name: string;
      connected: boolean;
      /** Headphones / speakers: can be connected. */
      audio: boolean;
    }

    interface Bluetooth {
      devices: BluetoothDevice[];
    }

    interface BluetoothFound {
      id: string;
      name: string;
      kind: "classic" | "le";
      signal: number | null;
    }

    interface BluetoothPairing {
      /** `confirm`: show `pin`, ask whether it matches the device; `display`: type `pin` on the device; `pin`: ask for the device's PIN; answer those (and `waiting`) with `pairReply`. */
      status: "paired" | "cancelled" | "confirm" | "display" | "pin" | "waiting";
      id: string;
      pin?: string;
    }

    type DndMode = "off" | "priority" | "alarms";

    interface Dnd {
      supported: boolean;
      mode: DndMode | null;
    }

    interface KeyboardLayout {
      /** HKL as 8 hex digits, `"04070407"`. */
      id: string;
      name: string;
      short: string;
      lang: string;
      layout: string | null;
    }

    interface Keyboard {
      layouts: KeyboardLayout[];
      current: string | null;
    }

    interface KeyboardSet extends Keyboard {
      /** false: the app in front did not switch. */
      ok: boolean;
    }

    interface Vpn {
      profiles: { name: string; connected: boolean; connecting: boolean }[];
    }

    interface Hotspot {
      supported: boolean;
      on: boolean | null;
      busy: boolean;
      clients: number | null;
      maxClients: number | null;
      reason: string | null;
    }

    type NearbyMode = "off" | "my-devices" | "everyone";

    interface Sharing {
      casting: boolean;
      nearby: NearbyMode | null;
    }

    type AccessibilityName = "magnifier" | "contrast" | "captions" | "colorFilter" | "stickyKeys";

    type Accessibility = Record<AccessibilityName, boolean | null>;

    /** What a change answers when Windows' own window opened instead. */
    interface Opened {
      ok: true;
      dialog?: true;
      settings?: true;
    }

    interface Toast {
      id: number;
      app: string | null;
      aumid: string | null;
      title: string;
      body: string;
      /** ms since the Unix epoch. */
      time: number;
    }

    interface Notifications {
      access: "allowed" | "denied" | "unspecified" | "unavailable";
      /** Newest first. */
      items: Toast[];
    }

    interface WindowProgress {
      state: "normal" | "paused" | "error" | "indeterminate";
      /** 0..1; `null` while indeterminate. */
      value: number | null;
    }

    interface WindowBadge {
      /** PNG data URL. */
      icon: string | null;
      /** The app's description of the badge, for screen readers. */
      label: string | null;
    }

    interface JumpItem {
      /** Pass to `openJump`. */
      key: string;
      title: string;
      path: string | null;
      separator?: boolean;
      /** 16x16 PNG data URL, with `{ icons: true }`. */
      icon?: string | null;
    }

    interface JumpList {
      appId: string | null;
      tasks: JumpItem[];
      categories: { name: string; items: JumpItem[] }[];
      recent: JumpItem[];
      frequent: JumpItem[];
      /** Whether Windows knows a jump list for this app at all. */
      known: boolean;
    }

    interface AppWindow {
      /** Window handle; pass it to the control calls. */
      id: number;
      title: string;
      app: string | null;
      exe: string | null;
      pid: number;
      monitor: number | null;
      minimized: boolean;
      maximized: boolean;
      focused: boolean;
      attention: boolean;
      /** Taskbar progress the app set (`ITaskbarList3`), while Versailles hides the taskbar; `null` when none. */
      progress?: WindowProgress | null;
      /** Overlay badge the app set (unread count, status), while Versailles hides the taskbar; `null` when none. */
      badge?: WindowBadge | null;
      /** Physical screen px. */
      rect: Rect;
      /** 32×32 PNG data URL, with `{ icons: true }`. */
      icon?: string | null;
    }

    interface WindowChange {
      type: "opened" | "closed" | "focused" | "changed";
      id: number;
      window: AppWindow;
    }

    interface TrayIcon {
      id: number;
      tip: string;
      showTip: boolean;
      app: string | null;
      exe: string | null;
      pid: number;
      guid: string | null;
      system: boolean;
      clickable: boolean;
      /** PNG data URL. */
      icon: string | null;
    }

    interface TrayState {
      active: boolean;
      icons: TrayIcon[];
    }

    type TrayChange =
      | { type: "added" | "changed" | "removed"; id: number; icon: TrayIcon }
      | { type: "balloon"; id: number; title: string; text: string; kind: "info" | "warning" | "error" | "none" | "user" }
      | { type: "active"; active: boolean };

    type TrayButton = "left" | "right" | "double" | "middle" | "key" | "menu";

    interface SettingsHit {
      kind: "settings";
      title: string;
      uri: string;
      en: string;
      de: string;
      score: number;
      exact: boolean;
    }

    interface FileHit {
      kind: "file" | "folder";
      path: string;
      name: string;
      folder: string;
      /** ms since 1970. */
      modified: number | null;
      match: "name" | "contents";
    }

    interface SearchResults {
      query: string;
      source: "index" | "walk" | "off";
      ms: number;
      settings: SettingsHit[];
      files: FileHit[];
    }

    interface SearchOptions {
      /** Default 40, at most 100. */
      limit?: number;
      /** Search files (default true). */
      files?: boolean;
      /** Search Settings pages (default true). */
      settings?: boolean;
      /** Walk folders when the index is off (default true). */
      fallback?: boolean;
    }

    interface RecentFile {
      kind: "file" | "folder";
      path: string;
      name: string;
      folder: string;
      /** ms since 1970. */
      opened: number;
    }

    interface SnapResult {
      x: number;
      y: number;
      guides: unknown[];
    }

    type Anchor = "tl" | "tc" | "ml" | "c" | "mr" | "bl" | "bc" | "br" | "cursor" | "center" | "overlay";

    interface LayoutPatch {
      width?: number;
      height?: number;
      anchor?: Anchor;
      /** Inset from `anchor`, logical px (needs `anchor`). */
      offset?: [number, number];
      alwaysOnTop?: boolean;
      /** CSS px inside the window; `null` = the whole window takes clicks. */
      hitRegions?: Rect[] | null;
    }

    interface BusMessage {
      topic: string;
      data: unknown;
      /** The sending piece. */
      from: string;
    }

    /** What `on(name, fn)` passes as the second argument. */
    interface Push<T = unknown> {
      name: string;
      data: T;
      /** Set when the push is meant for one piece. */
      id?: string;
      /** The current value read on subscribe (state hooks). */
      initial?: true;
    }

    /** Payload of each push name `on()` accepts. */
    interface PushMap {
      media: MediaInfo;
      battery: Battery;
      network: Network;
      focus: Focus | null;
      audio: Audio;
      posture: Posture;
      clipboard: null;
      windows: WindowChange;
      notify: Toast;
      tray: TrayChange;
      radios: Radios;
      "audio-control": AudioControl;
      display: Display;
      "power-mode": PowerMode;
      wifi: Wifi;
      bluetooth: Bluetooth;
      dnd: Dnd;
      keyboard: Keyboard;
      vpn: Vpn;
      hotspot: Hotspot;
      sharing: Sharing;
      accessibility: Accessibility;
      // piece events (no hook needed)
      shown: null;
      hiding: null;
      hidden: null;
      idle: { idleMs: number };
      active: null;
      "theme-changed": { dark: boolean };
      "monitor-changed": { monitors: Monitor[] };
    }

    type PushName = keyof PushMap;

    // ── hook namespaces ──

    /** A state hook: one read and a subscription that starts with the current value. */
    interface StateHook<T> {
      /** The current value. */
      now(): Promise<Hosted<T>>;
      /** `fn(value)` now and on every change. */
      onChange(fn: (value: T, push: Push<T>) => void): Promise<Unsubscribe>;
    }

    /** A quick-settings change answers the hook's new state. */
    type Changed<T> = Promise<Hosted<T>>;

    interface MediaHook extends StateHook<MediaInfo> {
      /** @deprecated Use `now()`. */
      nowPlaying(): Promise<Hosted<MediaInfo>>;
      playPause(): Promise<Hosted<void>>;
      next(): Promise<Hosted<void>>;
      previous(): Promise<Hosted<void>>;
      /** @deprecated Use `onChange(fn)` (pushed only to pieces with `media`, current value first). */
      onUpdate(handler: (info: MediaInfo) => void): Promise<Unsubscribe>;
    }

    interface MouseHook {
      /** Physical screen px, `[x, y]`. */
      position(): Promise<Hosted<[number, number]>>;
    }

    interface LayoutHook {
      /** Move the piece's own window (snaps to edges). */
      move(x: number, y: number): Promise<Hosted<SnapResult>>;
      /** Change several things at once. */
      update(patch: LayoutPatch): Promise<Hosted<void>>;
      /** Logical px. */
      resize(width: number, height: number): Promise<Hosted<void>>;
      setAnchor(anchor: Anchor, offset?: [number, number]): Promise<Hosted<void>>;
      setAlwaysOnTop(on: boolean): Promise<Hosted<void>>;
      /** Only these rects (CSS px in the window) take clicks; `null` = off. */
      setHitRegions(rects: Rect[] | null): Promise<Hosted<void>>;
      /** Keep hit regions on elements matching `selector` (default `[data-hit]`). Returns a stop function. */
      trackHits(selector?: string): () => void;
    }

    interface ShellHook {
      /** A URL, path or installed app name. */
      open(target: string): Promise<Hosted<void>>;
      /** Settings pages and files (bars and overlays only). */
      search(query: string, opts?: SearchOptions): Promise<Hosted<SearchResults>>;
      /** Recently opened files, newest first (bars and overlays only). Default 30, at most 100. */
      recent(limit?: number): Promise<Hosted<RecentFile[]>>;
    }

    interface StorageHook {
      get<T = unknown>(key: string): Promise<Hosted<T | null>>;
      set(key: string, value: unknown): Promise<Hosted<unknown>>;
      delete(key: string): Promise<Hosted<unknown>>;
      keys(): Promise<Hosted<string[]>>;
      clear(): Promise<Hosted<unknown>>;
    }

    interface ClipboardHook {
      /** null when the clipboard holds no text. */
      readText(): Promise<Hosted<string | null>>;
      writeText(text: string): Promise<Hosted<void>>;
      /** `fn()` on every clipboard change (no content: read it). */
      onChange(fn: () => void): Promise<Unsubscribe>;
    }

    /** Only inside a click or key press. */
    interface PowerHook {
      lock(): Promise<Hosted<void>>;
      sleep(): Promise<Hosted<void>>;
      signOut(): Promise<Hosted<void>>;
      restart(): Promise<Hosted<void>>;
      shutdown(): Promise<Hosted<void>>;
    }

    interface BusHook {
      /** Resolves how many windows it reached. */
      publish(topic: string, data?: unknown): Promise<Hosted<number>>;
      /** `topic` `"*"` = every topic. */
      subscribe(topic: string, fn: (data: unknown, message: BusMessage) => void): Promise<Unsubscribe>;
    }

    interface RadiosHook extends StateHook<Radios> {
      set(radio: "wifi" | "bluetooth", on: boolean): Changed<Radios>;
      setAirplane(on: boolean): Changed<Radios>;
    }

    interface AudioControlHook extends StateHook<AudioControl> {
      /** Default output, 0–100. */
      setVolume(level: number): Changed<AudioControl>;
      setMute(on: boolean): Changed<AudioControl>;
      /** An `id` from `outputs`. */
      setDefaultOutput(id: string): Changed<AudioControl>;
      /** @deprecated Use `setDefaultOutput(id)`. */
      setOutput(id: string): Changed<AudioControl>;
      setInputVolume(level: number): Changed<AudioControl>;
      setInputMute(on: boolean): Changed<AudioControl>;
      /** An `id` from `inputs`. */
      setDefaultInput(id: string): Changed<AudioControl>;
      setSessionVolume(id: string, level: number): Changed<AudioControl>;
      setSessionMute(id: string, on: boolean): Changed<AudioControl>;
    }

    interface DisplayHook extends StateHook<Display> {
      setNightLight(on: boolean): Changed<Display>;
      setBrightness(id: string, level: number): Changed<Display>;
      /** Win+P. */
      setProjection(mode: Projection): Changed<Display>;
      /** @deprecated Use `setProjection(mode)`. */
      project(mode: Projection): Changed<Display>;
    }

    interface WifiHook extends StateHook<Wifi> {
      /** No click needed; the list updates seconds later. */
      scan(): Changed<Wifi>;
      connect(ssid: string, password?: string): Changed<Wifi>;
      disconnect(): Changed<Wifi>;
    }

    interface BluetoothHook extends StateHook<Bluetooth> {
      connect(id: string): Changed<Bluetooth>;
      disconnect(id: string): Changed<Bluetooth>;
      /** Look for devices to pair (`seconds`, default 10, 3-30); resolves when done. Bars and overlays only. */
      scan(opts?: { seconds?: number }): Promise<Hosted<{ devices: BluetoothFound[] }>>;
      /** Pair a device from `scan()`; `{ pin }` for one with a fixed PIN. Control class. */
      pair(id: string, opts?: { pin?: string }): Changed<BluetoothPairing>;
      /** Answer a pairing step: `accept`, with `pin` when asked for one. */
      pairReply(id: string, accept: boolean, pin?: string): Changed<BluetoothPairing>;
    }

    interface DndHook extends StateHook<Dnd> {
      set(mode: DndMode): Changed<Dnd>;
    }

    interface NotifyControlHook {
      dismiss(id: number): Changed<Notifications>;
      clear(): Changed<Notifications>;
      /** Open the app that raised a notification. */
      open(aumid: string): Changed<Notifications>;
      /** An app's own Windows pop-ups on or off. */
      setBanners(aumid: string, on: boolean): Changed<Notifications>;
      /** @deprecated Use `setBanners(aumid, on)`. */
      banners(aumid: string, on: boolean): Changed<Notifications>;
    }

    interface KeyboardHook extends StateHook<Keyboard> {
      set(id: string): Promise<Hosted<KeyboardSet>>;
      next(): Promise<Hosted<KeyboardSet | Keyboard>>;
    }

    interface PowerModeHook extends StateHook<PowerMode> {
      set(mode: PowerModeName): Changed<PowerMode>;
    }

    interface VpnHook extends StateHook<Vpn> {
      connect(name: string): Changed<Vpn | Opened>;
      disconnect(name: string): Changed<Vpn>;
    }

    interface HotspotHook extends StateHook<Hotspot> {
      set(on: boolean): Changed<Hotspot>;
    }

    interface SharingHook extends StateHook<Sharing> {
      /** Windows' Cast panel (Win+K). */
      openCast(): Changed<Sharing>;
      setNearby(mode: NearbyMode): Changed<Sharing>;
    }

    interface AccessibilityHook extends StateHook<Accessibility> {
      set(name: AccessibilityName, on: boolean): Changed<Accessibility | Opened>;
    }

    interface NotifyHook {
      now(): Promise<Hosted<Notifications>>;
      /** `fn(toast)` for each new toast (not on subscribe). */
      onChange(fn: (toast: Toast, push: Push<Toast>) => void): Promise<Unsubscribe>;
      /** @deprecated Use `onChange(fn)`. */
      onToast(fn: (toast: Toast, push: Push<Toast>) => void): Promise<Unsubscribe>;
    }

    /** `windows` reads; the control calls need `windows-control`. */
    interface WindowsHook {
      list(opts?: { icons?: boolean }): Promise<AppWindow[]>;
      icon(id: number): Promise<Hosted<string | null>>;
      /** `fn(change)` per change (no current value first: use `track`). */
      onChange(fn: (change: WindowChange, push: Push<WindowChange>) => void): Promise<Unsubscribe>;
      /** Preview card next to `target` (element or CSS-px rect); `preview(null)` hides it. Bars and overlays only. */
      preview(id: number | null, target?: Element | Rect): Promise<Hosted<void>>;
      /** `fn(list)` now and after every change. */
      track(fn: (list: AppWindow[]) => void, opts?: { icons?: boolean }): Promise<Unsubscribe>;
      focus(id: number): Promise<Hosted<void>>;
      minimize(id: number): Promise<Hosted<void>>;
      restore(id: number): Promise<Hosted<void>>;
      maximize(id: number): Promise<Hosted<void>>;
      close(id: number): Promise<Hosted<void>>;
      /** Physical px. */
      move(id: number, to: Partial<Rect>): Promise<Hosted<void>>;
      /** The window's jump list, as a right click on its taskbar button shows it. Read with `windows`. */
      jumpList(id: number, opts?: { icons?: boolean }): Promise<Hosted<JumpList>>;
      /** Open a jump list entry by its `key`. `windows-control`; only inside a click or key press. */
      openJump(id: number, key: string): Promise<Hosted<void>>;
    }

    interface TrayHook {
      /** `{ active, icons }`. */
      now(): Promise<TrayState>;
      /** @deprecated Use `now()`. */
      state(): Promise<TrayState>;
      list(): Promise<TrayIcon[]>;
      /** `fn(change)` per change (no current value first: use `track`). */
      onChange(fn: (change: TrayChange, push: Push<TrayChange>) => void): Promise<Unsubscribe>;
      /** `fn(icons, active)` now and after every change. */
      track(fn: (icons: TrayIcon[], active: boolean) => void): Promise<Unsubscribe>;
      /** Only inside a click or key press. `x`, `y`: physical screen px. */
      click(id: number, button?: TrayButton, x?: number, y?: number): Promise<Hosted<void>>;
      hover(id: number, over: boolean, x?: number, y?: number): Promise<Hosted<void>>;
    }

    /** The hook namespaces: on `versailles` (caller inferred) and on `versailles.for(id)`. */
    interface Hooks {
      /**
       * `fn(data)` on each push of a declared hook, or a piece event (no hook
       * needed). State hooks call `fn` with the current value first.
       */
      on<K extends PushName>(name: K, fn: (data: PushMap[K], push: Push<PushMap[K]>) => void): Promise<Unsubscribe>;
      media: MediaHook;
      mouse: MouseHook;
      layout: LayoutHook;
      /**
       * Toggle a spawnable; with `data` it opens (or stays open) and receives
       * it. Resolves whether it is open now.
       */
      spawn(id: string, data?: unknown): Promise<boolean | { host: false; ok: false }>;
      shell: ShellHook;
      storage: StorageHook;
      clipboard: ClipboardHook;
      power: PowerHook;
      bus: BusHook;
      battery: StateHook<Battery>;
      network: StateHook<Network>;
      focus: StateHook<Focus | null>;
      audio: StateHook<Audio>;
      posture: StateHook<Posture>;
      radios: RadiosHook;
      audioControl: AudioControlHook;
      display: DisplayHook;
      powerMode: PowerModeHook;
      wifi: WifiHook;
      bluetooth: BluetoothHook;
      dnd: DndHook;
      notifyControl: NotifyControlHook;
      keyboard: KeyboardHook;
      vpn: VpnHook;
      hotspot: HotspotHook;
      sharing: SharingHook;
      accessibility: AccessibilityHook;
      windows: WindowsHook;
      tray: TrayHook;
      notify: NotifyHook;
    }

    /** `versailles.for(id)`: the namespaces acting as one piece. */
    interface Scope extends Hooks {}
  }

  type VersaillesError = Versailles.VersaillesError;
}
