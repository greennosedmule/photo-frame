// Pure, DOM-free selection and ordering. Sequencing decides what comes next;
// this decides what is eligible and how the non-random modes walk the library.

import type { ManifestPhoto } from './api';
import type { ClientSettings } from './settings';

/** Manifest minus hidden, filtered by tag (any selected tag matches). */
export function eligible(photos: readonly ManifestPhoto[], s: Pick<ClientSettings, 'hidden' | 'tagFilter'>): ManifestPhoto[] {
  const hidden = new Set(s.hidden);
  const filter = new Set(s.tagFilter);
  return photos.filter(
    (p) => !hidden.has(p.hash) && (filter.size === 0 || p.tags.some((t) => filter.has(t))),
  );
}

export function byDate(photos: readonly ManifestPhoto[], dir: 'asc' | 'desc'): ManifestPhoto[] {
  const sign = dir === 'asc' ? 1 : -1;
  // ISO 8601 UTC strings sort lexicographically; hash breaks ties for stability.
  return [...photos].sort(
    (a, b) => sign * a.effective_date.localeCompare(b.effective_date) || a.hash.localeCompare(b.hash),
  );
}

const DAY = 86_400_000;

/** Photographs within ±`windowDays` of `now`'s month and day, in any year. */
export function onThisDay(photos: readonly ManifestPhoto[], now: Date, windowDays = 3): ManifestPhoto[] {
  return photos.filter((p) => {
    const parsed = new Date(p.effective_date);
    if (Number.isNaN(parsed.getTime())) return false;
    // Whole days: the time of day does not move a photograph in or out.
    const d = new Date(Date.UTC(parsed.getUTCFullYear(), parsed.getUTCMonth(), parsed.getUTCDate()));
    // Compare against today's month/day placed in the photograph's own year and
    // its neighbours, so a window spanning New Year still matches.
    return [-1, 0, 1].some((off) => {
      const anchor = Date.UTC(d.getUTCFullYear() + off, now.getMonth(), now.getDate());
      return Math.abs(d.getTime() - anchor) <= windowDays * DAY;
    });
  });
}

export const ON_THIS_DAY_MIN = 5;
