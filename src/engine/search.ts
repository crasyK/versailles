import Fuse from "fuse.js";

export type SearchPreset = {
  n: string;
  d: string;
  cat: string;
  t: "web" | "folder" | "app" | "term";
  target: string;
  aliases?: string[];
};

let fuse: Fuse<SearchPreset> | null = null;
let fuseList: SearchPreset[] = [];

export function rebuildFuseIndex(list: SearchPreset[]) {
  fuseList = list;
  fuse = new Fuse(list, {
    keys: [
      { name: "n", weight: 0.45 },
      { name: "d", weight: 0.25 },
      { name: "cat", weight: 0.15 },
      { name: "aliases", weight: 0.15 },
    ],
    threshold: 0.38,
    ignoreLocation: true,
  });
}

export function fuzzyPresets(query: string, limit = 12): SearchPreset[] {
  const q = query.trim();
  if (!q) return [];
  if (!fuse) rebuildFuseIndex(fuseList);
  if (!fuse) return [];
  return fuse.search(q, { limit }).map((r) => r.item);
}

export function parseCategoryFilter(raw: string): { cat: string | null; rest: string } {
  const m = raw.match(/^\s*(web|folder|app|term|apps)\s+(.*)$/i);
  if (!m) return { cat: null, rest: raw.trim() };
  return { cat: m[1].toLowerCase(), rest: m[2].trim() };
}

export function presetMatchesCat(p: SearchPreset, cat: string | null): boolean {
  if (!cat) return true;
  if (cat === "apps") return p.cat === "apps" || p.t === "app";
  if (cat === "web") return p.t === "web";
  if (cat === "folder") return p.t === "folder";
  if (cat === "term") return p.t === "term";
  return p.cat.toLowerCase() === cat;
}

export function findPresetStrict(
  list: SearchPreset[],
  name: string,
): SearchPreset | undefined {
  const p = name.toLowerCase();
  const exact =
    list.find((x) => x.n === p) || list.find((x) => (x.aliases ?? []).includes(p));
  if (exact) return exact;
  const prefix = list.filter(
    (x) => x.n.startsWith(p) || (x.aliases ?? []).some((a) => a.startsWith(p)),
  );
  if (prefix.length === 1) return prefix[0];
  return undefined;
}

export function duplicateShortcutIds(list: SearchPreset[]): string[] {
  const seen = new Map<string, number>();
  const dups: string[] = [];
  for (const p of list) {
    const keys = [p.n, ...(p.aliases ?? [])].map((k) => k.toLowerCase());
    for (const k of keys) {
      seen.set(k, (seen.get(k) ?? 0) + 1);
    }
  }
  for (const [k, n] of seen) {
    if (n > 1) dups.push(k);
  }
  return [...new Set(dups)].sort();
}

export function validatePreset(p: SearchPreset): string | null {
  if (!p.n?.trim()) return "shortcut missing name (n)";
  if (!p.t) return `${p.n}: missing type (t)`;
  if (!p.target?.trim()) return `${p.n}: missing target`;
  if (!["web", "folder", "app", "term"].includes(p.t)) {
    return `${p.n}: unknown type '${p.t}'`;
  }
  if (p.t === "web" && !/^https?:\/\//i.test(p.target) && !p.target.includes("://")) {
    return `${p.n}: web target should be a URL`;
  }
  return null;
}

export function timeAwareBoost(cat: string, hour: number): number {
  if (cat === "personal" && hour >= 6 && hour < 11) return 3;
  if (cat === "dev" && hour >= 9 && hour < 18) return 2;
  if (cat === "media" && hour >= 17) return 2;
  return 0;
}

export function orderDefaults(
  list: SearchPreset[],
  recents: string[],
  pins: string[],
  opts: { timeAware?: boolean; limit?: number },
): SearchPreset[] {
  const limit = opts.limit ?? 12;
  const hour = new Date().getHours();
  const byName = new Map(list.map((p) => [p.n, p]));
  const out: SearchPreset[] = [];
  const used = new Set<string>();

  const push = (p: SearchPreset | undefined) => {
    if (!p || used.has(p.n) || p.cat === "apps") return;
    used.add(p.n);
    out.push(p);
  };

  for (const id of pins) push(byName.get(id));
  for (const id of recents) push(byName.get(id));

  const rest = list
    .filter((p) => p.cat !== "apps" && !used.has(p.n))
    .map((p) => ({
      p,
      score:
        timeAwareBoost(p.cat, hour) +
        (recents.indexOf(p.n) >= 0 ? 5 - recents.indexOf(p.n) : 0),
    }))
    .sort((a, b) => b.score - a.score || a.p.n.localeCompare(b.p.n));

  for (const { p } of rest) {
    push(p);
    if (out.length >= limit) break;
  }
  return out.slice(0, limit);
}

export function playLaunchTick(enabled: boolean) {
  if (!enabled) return;
  try {
    const ctx = new AudioContext();
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.connect(g);
    g.connect(ctx.destination);
    o.frequency.value = 880;
    g.gain.value = 0.04;
    o.start();
    g.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.08);
    o.stop(ctx.currentTime + 0.09);
  } catch {
    /* optional */
  }
}
