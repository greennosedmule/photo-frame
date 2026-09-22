// Client settings live in IndexedDB and are never sent to the server.
// Do not use localStorage: it is subject to Safari's eviction; IndexedDB in a
// home-screen web app is not.

export interface FocalRect {
  x: number;
  y: number;
  w: number;
  h: number;
} // each 0..1 of the original image, so it survives a different aspect ratio

export interface ClientSettings {
  dwellSeconds: number;
  ordering: 'shuffle' | 'chronological' | 'reverse-chronological' | 'on-this-day';
  tagFilter: string[];
  tagAffinity: number;
  transition: 'crossfade' | 'cut';
  fillMode: 'blur' | 'letterbox' | 'crop';
  hidden: string[];
  zoom: Record<string, FocalRect>;
  dimSchedule: { start: string; end: string; opacity: number; blackout: boolean };
}

export const defaultSettings: ClientSettings = {
  dwellSeconds: 30,
  ordering: 'shuffle',
  tagFilter: [],
  tagAffinity: 0.5,
  transition: 'crossfade',
  fillMode: 'blur',
  hidden: [],
  zoom: {},
  dimSchedule: { start: '22:00', end: '07:00', opacity: 0.8, blackout: false },
};

const orderings = ['shuffle', 'chronological', 'reverse-chronological', 'on-this-day'] as const;
const transitions = ['crossfade', 'cut'] as const;
const fills = ['blur', 'letterbox', 'crop'] as const;

const num = (v: unknown, lo: number, hi: number, fallback: number) =>
  typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : fallback;
const oneOf = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T =>
  allowed.includes(v as T) ? (v as T) : fallback;
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

/**
 * Whatever is in storage, produce valid settings: unknown or malformed fields
 * fall back to defaults, so an older or damaged record never breaks the frame.
 */
export function mergeSettings(stored: unknown): ClientSettings {
  const s = (typeof stored === 'object' && stored !== null ? stored : {}) as Record<string, unknown>;
  const d = defaultSettings;
  const dim = (typeof s.dimSchedule === 'object' && s.dimSchedule !== null ? s.dimSchedule : {}) as Record<string, unknown>;
  const zoom: Record<string, FocalRect> = {};
  if (typeof s.zoom === 'object' && s.zoom !== null) {
    for (const [hash, r] of Object.entries(s.zoom as Record<string, Partial<FocalRect>>)) {
      if (r && [r.x, r.y, r.w, r.h].every((v) => typeof v === 'number' && Number.isFinite(v))) {
        zoom[hash] = { x: r.x!, y: r.y!, w: r.w!, h: r.h! };
      }
    }
  }
  return {
    dwellSeconds: num(s.dwellSeconds, 3, 3600, d.dwellSeconds),
    ordering: oneOf(s.ordering, orderings, d.ordering),
    tagFilter: strings(s.tagFilter),
    tagAffinity: num(s.tagAffinity, 0, 1, d.tagAffinity),
    transition: oneOf(s.transition, transitions, d.transition),
    fillMode: oneOf(s.fillMode, fills, d.fillMode),
    hidden: strings(s.hidden),
    zoom,
    dimSchedule: {
      start: typeof dim.start === 'string' ? dim.start : d.dimSchedule.start,
      end: typeof dim.end === 'string' ? dim.end : d.dimSchedule.end,
      opacity: num(dim.opacity, 0, 1, d.dimSchedule.opacity),
      blackout: typeof dim.blackout === 'boolean' ? dim.blackout : d.dimSchedule.blackout,
    },
  };
}

// ---- IndexedDB ---------------------------------------------------------------

const DB_NAME = 'photoframe';
const STORE = 'kv';
const KEY = 'settings';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return open().then(
    (db) =>
      new Promise<T>((resolve, reject) => {
        const req = run(db.transaction(STORE, mode).objectStore(STORE));
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      }),
  );
}

/** Never throws: with IndexedDB unavailable (private mode) the frame still runs on defaults. */
export async function loadSettings(): Promise<ClientSettings> {
  try {
    return mergeSettings(await tx('readonly', (s) => s.get(KEY)));
  } catch {
    return mergeSettings(undefined);
  }
}

export async function saveSettings(settings: ClientSettings): Promise<void> {
  try {
    await tx('readwrite', (s) => s.put(settings, KEY));
  } catch {
    /* best effort */
  }
}
