/** Pure parsing helpers for the example command bar (no DOM, no IPC). */

/** `pin github` / `unpin gh` → verb + shortcut name. Bare `pin` (no name) is not a pin command. */
export function parsePinCommand(raw: string): { verb: "pin" | "unpin"; name: string } | null {
  const m = raw.trim().match(/^(pin|unpin)\s+(\S+)/i);
  if (!m) return null;
  return { verb: m[1].toLowerCase() as "pin" | "unpin", name: m[2].toLowerCase() };
}

/**
 * overlay://shown / overlay://hidden payloads.
 * Legacy host: shown = seed string, hidden = `true` (no id).
 * Newer host: an object carrying the overlay id (and the seed for shown).
 */
export function overlayEventId(payload: unknown): { id: string | null; seed: string } {
  if (typeof payload === "string") return { id: null, seed: payload };
  if (payload && typeof payload === "object") {
    const o = payload as Record<string, unknown>;
    const rawId = o.id ?? o.overlayId ?? o.overlay ?? o.spawnId ?? o.piece;
    const id = typeof rawId === "string" && rawId.trim() ? rawId.trim() : null;
    const seed = typeof o.seed === "string" ? o.seed : "";
    return { id, seed };
  }
  return { id: null, seed: "" };
}

/** Same piece? Case-insensitive, `calc` ↔ `calculator`. */
export function sameOverlayId(a: string, b: string): boolean {
  const norm = (s: string) => {
    const t = s.trim().toLowerCase();
    return t === "calc" ? "calculator" : t;
  };
  return norm(a) === norm(b);
}

/** pty://data / pty://exit session id, or null for the legacy (session-less) shape. */
export function ptyEventSession(payload: unknown): number | null {
  if (payload && typeof payload === "object") {
    const s = (payload as { session?: unknown }).session;
    if (typeof s === "number" && Number.isFinite(s)) return s;
  }
  return null;
}

/** pty://data base64 chunk: bare string (legacy) or `{ data }`. */
export function ptyEventData(payload: unknown): string | null {
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    const d = (payload as { data?: unknown }).data;
    if (typeof d === "string") return d;
  }
  return null;
}

/** Tauri accelerator → footer label: `CommandOrControl+Shift+K` → `ctrl+shift+k`. */
export function hotkeyLabel(accel: string): string {
  return accel
    .split("+")
    .map((k) => k.trim())
    .filter(Boolean)
    .map((k) => {
      const low = k.toLowerCase();
      if (low === "commandorcontrol" || low === "cmdorctrl" || low === "control") return "ctrl";
      if (low === "option") return "alt";
      return low;
    })
    .join("+");
}
