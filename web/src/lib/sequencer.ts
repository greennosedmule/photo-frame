// Stateful wrapper over the pure pieces: walks forward and back through what
// has been shown. DOM-free; randomness and the clock are injectable.

import type { ManifestPhoto } from './api';
import { byDate, eligible, ON_THIS_DAY_MIN, onThisDay } from './ordering';
import { pickNext } from './sequencing';
import type { ClientSettings } from './settings';

type Picking = Pick<ClientSettings, 'ordering' | 'tagAffinity' | 'hidden' | 'tagFilter'>;

export class Sequencer {
  private history: string[] = [];
  private pos = -1;

  get current(): string | undefined {
    return this.history[this.pos];
  }

  /** Hashes shown so far, oldest first, for the recency penalty. */
  get recent(): readonly string[] {
    return this.history;
  }

  /** Move forward: replay history if we went back, otherwise choose a new one. */
  next(photos: readonly ManifestPhoto[], s: Picking, now = new Date(), rand = Math.random): string | undefined {
    const pool = eligible(photos, s);
    const inPool = new Set(pool.map((p) => p.hash));
    while (this.pos < this.history.length - 1) {
      const h = this.history[++this.pos]!;
      if (inPool.has(h)) return h;
    }
    const chosen = this.choose(photos, pool, s, now, rand);
    if (chosen) this.push(chosen);
    return chosen;
  }

  /**
   * Decide the next `count` photographs now, without moving, so they can be
   * preloaded. They are committed to history, so `next` returns exactly them.
   * Call [`dropAhead`] when settings change and the plan is stale.
   */
  ahead(photos: readonly ManifestPhoto[], s: Picking, count: number, now = new Date(), rand = Math.random): string[] {
    const pool = eligible(photos, s);
    const inPool = new Set(pool.map((p) => p.hash));
    const here = this.pos;
    // Plan from the end of what is already planned.
    this.pos = this.history.length - 1;
    while (this.history.length - 1 - here < count) {
      const chosen = this.choose(photos, pool, s, now, rand);
      if (!chosen) break;
      this.history.push(chosen);
      this.pos = this.history.length - 1;
    }
    this.pos = here;
    return this.history.slice(here + 1).filter((h) => inPool.has(h)).slice(0, count);
  }

  /** Forget anything planned beyond the current photograph. */
  dropAhead(): void {
    this.history.length = this.pos + 1;
  }

  private push(h: string): void {
    this.history.push(h);
    this.pos = this.history.length - 1;
    // Bound memory on a frame that runs for months, keeping recent context.
    if (this.history.length > 500) {
      this.history.splice(0, 250);
      this.pos = Math.max(0, this.pos - 250);
    }
  }

  /** Step back to the previous photograph that is still eligible. */
  prev(photos: readonly ManifestPhoto[], s: Picking): string | undefined {
    const inPool = new Set(eligible(photos, s).map((p) => p.hash));
    for (let i = this.pos - 1; i >= 0; i--) {
      if (inPool.has(this.history[i]!)) {
        this.pos = i;
        return this.history[i];
      }
    }
    return undefined;
  }

  /** The photograph `prev` would step to, without moving. */
  peekPrev(photos: readonly ManifestPhoto[], s: Picking): string | undefined {
    const inPool = new Set(eligible(photos, s).map((p) => p.hash));
    for (let i = this.pos - 1; i >= 0; i--) if (inPool.has(this.history[i]!)) return this.history[i];
    return undefined;
  }

  private choose(all: readonly ManifestPhoto[], pool: ManifestPhoto[], s: Picking, now: Date, rand: () => number): string | undefined {
    if (pool.length === 0) return undefined;
    if (s.ordering === 'chronological' || s.ordering === 'reverse-chronological') {
      // Position comes from the whole library, so hiding or filtering out the
      // current photograph continues from where it was instead of restarting.
      const ordered = byDate(all, s.ordering === 'chronological' ? 'asc' : 'desc');
      const inPool = new Set(pool.map((p) => p.hash));
      const at = ordered.findIndex((p) => p.hash === this.current);
      for (let i = 1; i <= ordered.length; i++) {
        const h = ordered[(at + i + ordered.length) % ordered.length]!.hash;
        if (inPool.has(h)) return h;
      }
      return undefined;
    }
    let candidates = pool;
    if (s.ordering === 'on-this-day') {
      const today = onThisDay(pool, now);
      // Too few anniversaries makes a dull loop; fall back to plain shuffle.
      if (today.length >= ON_THIS_DAY_MIN) candidates = today;
    }
    const cur = pool.find((p) => p.hash === this.current);
    return pickNext(candidates, cur, this.history, s.tagAffinity, rand)?.hash;
  }
}
