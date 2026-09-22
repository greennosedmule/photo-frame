// Keep the whole `display` set (and its `blur` backdrops) in the service
// worker's media cache, so a frame with its backend down can show any photograph
// and not just the ones it happened to have shown. Media URLs are immutable, so
// a cached entry is never stale.

import { mediaUrl, type ManifestPhoto, type Variant } from './api';

export const MEDIA_CACHE = 'media';
const VARIANTS: Variant[] = ['display', 'blur'];
/** Stop filling the cache once storage is this full; eviction handles the rest. */
const QUOTA_CEILING = 0.8;

/** The URLs the frame needs for a manifest, keyed by what they name. */
export function wantedUrls(photos: readonly ManifestPhoto[]): string[] {
  return photos.flatMap((p) => VARIANTS.map((v) => mediaUrl(p.hash, v, p.media_rotation)));
}

/** True once a service worker controls this page, so fetches are cached. */
async function controlled(): Promise<boolean> {
  if (!('serviceWorker' in navigator) || !('caches' in window)) return false;
  await navigator.serviceWorker.ready;
  if (navigator.serviceWorker.controller) return true;
  return new Promise((resolve) => {
    const t = setTimeout(() => resolve(false), 10_000);
    navigator.serviceWorker.addEventListener(
      'controllerchange',
      () => {
        clearTimeout(t);
        resolve(true);
      },
      { once: true },
    );
  });
}

async function storageFull(): Promise<boolean> {
  try {
    const { usage, quota } = await navigator.storage.estimate();
    return !!usage && !!quota && usage / quota > QUOTA_CEILING;
  } catch {
    return false;
  }
}

export interface PrefetchOptions {
  /** Return true to abandon the pass (component destroyed, newer manifest). */
  cancelled: () => boolean;
  /** Pause between fetches, so the photograph on screen keeps priority. */
  gapMs?: number;
}

/**
 * Fetch every wanted URL not already cached, one at a time, through the service
 * worker's cache-first route. Returns true if the pass covered everything, in
 * which case cached entries nobody wants any more (deleted or rotated
 * photographs) are dropped so the cache does not grow without bound.
 */
export async function prefetchMedia(photos: readonly ManifestPhoto[], opts: PrefetchOptions): Promise<boolean> {
  if (!(await controlled())) return false;
  const cache = await caches.open(MEDIA_CACHE);
  const wanted = wantedUrls(photos);
  let failures = 0;
  let fetched = 0;

  for (const url of wanted) {
    if (opts.cancelled()) return false;
    if (await cache.match(url)) continue;
    if (fetched % 20 === 0 && (await storageFull())) return false;
    try {
      const res = await fetch(url, { priority: 'low' } as RequestInit);
      if (!res.ok) failures++;
      await res.arrayBuffer(); // drain, so the worker finishes writing the entry
      fetched++;
    } catch {
      // Offline or server down: stop quietly, try again on the next manifest.
      if (++failures >= 3) return false;
    }
    await new Promise((r) => setTimeout(r, opts.gapMs ?? 40));
  }

  const keep = new Set(wanted.map((u) => new URL(u, location.href).href));
  for (const req of await cache.keys()) {
    if (/\/media\/[0-9a-f]{64}\/(display|blur)/.test(req.url) && !keep.has(req.url)) await cache.delete(req);
  }
  return failures === 0;
}
