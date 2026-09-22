// Shared reactive state: client settings and the library.

import { getManifest, getStatus, type Manifest, type ManifestPhoto } from './api';
import { defaultSettings, loadSettings, saveSettings, type ClientSettings } from './settings';

// ---- settings (IndexedDB only; never sent to the server) ----------------------

export const settings: ClientSettings = $state(structuredClone(defaultSettings));
let saveTimer: ReturnType<typeof setTimeout> | undefined;

export async function initSettings(): Promise<void> {
  Object.assign(settings, await loadSettings());
}

/** Apply a change and persist it shortly after (dragging a slider is many changes). */
export function updateSettings(change: (s: ClientSettings) => void): void {
  change(settings);
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveSettings($state.snapshot(settings)), 400);
}

// ---- library --------------------------------------------------------------------

export const library = $state<{ manifest: Manifest | undefined; error: string | undefined; loaded: boolean }>({
  manifest: undefined,
  error: undefined,
  loaded: false,
});

/** Poll interval when idle; the spec's MANIFEST_POLL_INTERVAL default. */
export const POLL_IDLE_MS = 300_000;
/** While the server reports indexing, look more often so a new library appears promptly. */
export const POLL_BUSY_MS = 15_000;

export async function refreshManifest(): Promise<void> {
  try {
    library.manifest = await getManifest();
    library.error = undefined;
  } catch (e) {
    // Keep showing what we have: a frame must survive the backend being down.
    library.error = e instanceof Error ? e.message : String(e);
  } finally {
    library.loaded = true;
  }
}

/** Poll `/api/status` and refetch the manifest only when the generation moves. */
export function startPolling(
  onChange?: () => void,
  intervals: { idle: number; busy: number } = { idle: POLL_IDLE_MS, busy: POLL_BUSY_MS },
): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const tick = async () => {
    let busy = library.manifest?.indexing ?? false;
    try {
      const status = await getStatus();
      busy = status.indexing;
      if (!library.manifest || status.generation !== library.manifest.generation) {
        await refreshManifest();
        onChange?.();
      }
    } catch (e) {
      library.error = e instanceof Error ? e.message : String(e);
    }
    if (!stopped) timer = setTimeout(tick, busy ? intervals.busy : intervals.idle);
  };
  void refreshManifest().then(() => {
    onChange?.();
    if (!stopped) timer = setTimeout(tick, library.manifest?.indexing ? intervals.busy : intervals.idle);
  });
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}

/** Patch one photograph in place after a local edit, ahead of the next poll. */
export function patchPhoto(hash: string, change: (p: ManifestPhoto) => void): void {
  const p = library.manifest?.photos.find((x) => x.hash === hash);
  if (p) change(p);
}

/**
 * After a rotation, poll until the derivatives have caught up (each named photo
 * shows the rotation that was asked for), or a timeout. Resolves true on success.
 */
export async function waitForRotations(hashes: readonly string[], timeoutMs = 90_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await refreshManifest();
    const byHash = new Map((library.manifest?.photos ?? []).map((p) => [p.hash, p]));
    if (hashes.every((h) => !byHash.has(h) || byHash.get(h)!.media_rotation === byHash.get(h)!.rotation)) return true;
    await new Promise((r) => setTimeout(r, 1500));
  }
  return false;
}
