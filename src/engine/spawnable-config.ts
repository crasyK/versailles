import { invoke } from "@tauri-apps/api/core";

export type PresetType = "web" | "folder" | "app" | "term";

export type SpawnableEngineOpts = {
  blurDismissMs?: number;
  suggestionLimit?: number;
  compact?: boolean;
  launchTick?: boolean;
  searchHf?: string;
  timeAwareDefaults?: boolean;
  autoDismissLaunch?: boolean;
};

export type SpawnableEngineContext = {
  id: string;
  dismissOnBlur: boolean;
  opts: SpawnableEngineOpts;
};

const DEFAULT_OPTS: Required<SpawnableEngineOpts> = {
  blurDismissMs: 280,
  suggestionLimit: 12,
  compact: false,
  launchTick: false,
  searchHf: "https://huggingface.co/models?search={q}",
  timeAwareDefaults: true,
  autoDismissLaunch: true,
};

export function spawnableId(): string {
  const w = window as unknown as {
    __VERSAILLES_WIDGET_ID__?: string;
    __VERSAILLES_SPAWNABLE__?: { id?: string };
  };
  return (
    w.__VERSAILLES_SPAWNABLE__?.id?.trim() ||
    w.__VERSAILLES_WIDGET_ID__?.trim() ||
    "action-bar"
  );
}

export function blockSpawnableOpts(id: string): SpawnableEngineOpts {
  const block = (window as unknown as { __VERSAILLES_BLOCK__?: Record<string, unknown> })
    .__VERSAILLES_BLOCK__;
  const spawnables = block?.spawnables as Record<string, { opts?: SpawnableEngineOpts }> | undefined;
  if (!spawnables) return {};
  const key = id.trim().toLowerCase();
  return spawnables[key]?.opts ?? spawnables[id]?.opts ?? {};
}

export function mergeOpts(
  host: SpawnableEngineOpts | undefined,
  block: SpawnableEngineOpts,
): Required<SpawnableEngineOpts> {
  return {
    blurDismissMs: host?.blurDismissMs ?? block.blurDismissMs ?? DEFAULT_OPTS.blurDismissMs,
    suggestionLimit:
      host?.suggestionLimit ?? block.suggestionLimit ?? DEFAULT_OPTS.suggestionLimit,
    compact: host?.compact ?? block.compact ?? DEFAULT_OPTS.compact,
    launchTick: host?.launchTick ?? block.launchTick ?? DEFAULT_OPTS.launchTick,
    searchHf: host?.searchHf ?? block.searchHf ?? DEFAULT_OPTS.searchHf,
    timeAwareDefaults:
      host?.timeAwareDefaults ?? block.timeAwareDefaults ?? DEFAULT_OPTS.timeAwareDefaults,
    autoDismissLaunch:
      host?.autoDismissLaunch ?? block.autoDismissLaunch ?? DEFAULT_OPTS.autoDismissLaunch,
  };
}

export async function loadSpawnableEngineContext(): Promise<{
  id: string;
  dismissOnBlur: boolean;
  opts: Required<SpawnableEngineOpts>;
}> {
  const id = spawnableId();
  const blockOpts = blockSpawnableOpts(id);
  try {
    const ctx = await invoke<SpawnableEngineContext>("get_spawnable_engine_context", { id });
    return {
      id: ctx.id || id,
      dismissOnBlur: ctx.dismissOnBlur,
      opts: mergeOpts(ctx.opts, blockOpts),
    };
  } catch {
    return {
      id,
      dismissOnBlur: false,
      opts: mergeOpts(undefined, blockOpts),
    };
  }
}

export type EngineRuntimeState = {
  recents: string[];
  pins: string[];
  lastTermSeed?: string | null;
};

export async function loadEngineRuntime(engineId: string): Promise<EngineRuntimeState> {
  try {
    return await invoke<EngineRuntimeState>("get_engine_runtime", { engineId });
  } catch {
    return { recents: [], pins: [] };
  }
}

export async function pushRecent(engineId: string, presetId: string): Promise<void> {
  if (!presetId.trim()) return;
  try {
    await invoke("patch_engine_runtime", {
      engineId,
      patch: { pushRecent: presetId.trim().toLowerCase() },
    });
  } catch {
    /* preview / offline */
  }
}

export async function togglePin(engineId: string, presetId: string): Promise<EngineRuntimeState> {
  try {
    return await invoke<EngineRuntimeState>("patch_engine_runtime", {
      engineId,
      patch: { togglePin: presetId.trim().toLowerCase() },
    });
  } catch {
    return loadEngineRuntime(engineId);
  }
}

export async function saveLastTermSeed(engineId: string, seed: string | null): Promise<void> {
  try {
    await invoke("patch_engine_runtime", {
      engineId,
      patch: { lastTermSeed: seed },
    });
  } catch {
    /* ignore */
  }
}
