// Stateful wrapper over the pure pieces: walks forward and back through what
// has been shown. DOM-free; randomness and the clock are injectable.

import type { ManifestPhoto } from './api';
import { byDate, eligible, ON_THIS_DAY_MIN, onThisDay } from './ordering';
import { ARRIVAL_BOOST_MS, pickNext } from './sequencing';
import type { ClientSettings } from './settings';

type Picking = Pick<ClientSettings, 'ordering' | 'tagAffinity' | 'tagWeights' | 'hidden' | 'tagFilter'>;

/** At most this many new arrivals jump the queue; the rest join the shuffle. */
export const ARRIVALS_MAX = 20;

export class Sequencer {
  private history: string[] = [];
  private pos = -1;
  /** New photographs waiting to be shown next, in order. */
  private arrivals: string[] = [];
  /** Taken from `arrivals` into the preloaded plan but not yet on screen. */
  private planned = new Set<string>();
  /** Shown because they arrived, not because the ordering reached them. */
  private jumped = new Set<string>();
  /** When this frame first saw each new photograph, for the arrival boost. */
  private arrivedAt = new Map<string, number>();

  get current(): string | undefined {
    return this.history[this.pos];
  }

  /** Hashes shown so far, oldest first, for the recency penalty. */
  get recent(): readonly string[] {
    return this.history;
  }

  /**
   * Photographs that appeared in a manifest refresh, in the order to show them.
   * They go next, ahead of the ordering, up to [`ARRIVALS_MAX`] waiting; all of
   * them get the arrival boost in shuffle.
   */
  addArrivals(hashes: readonly string[], now = new Date()): void {
    for (const h of hashes) {
      this.arrivedAt.set(h, now.getTime());
      if (this.arrivals.length < ARRIVALS_MAX && !this.arrivals.includes(h)) this.arrivals.push(h);
    }
  }

  /** Move forward: replay history if we went back, otherwise choose a new one. */
  next(photos: readonly ManifestPhoto[], s: Picking, now = new Date(), rand = Math.random): string | undefined {
    const pool = eligible(photos, s);
    const inPool = new Set(pool.map((p) => p.hash));
    while (this.pos < this.history.length - 1) {
      const h = this.history[++this.pos]!;
      this.planned.delete(h);
      if (inPool.has(h)) return h;
    }
    const chosen = this.choose(photos, pool, s, now, rand);
    if (chosen) {
      this.push(chosen);
      this.planned.delete(chosen);
    }
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
    // Arrivals that were only planned go back to the front of the queue.
    const unshown = this.history.slice(this.pos + 1).filter((h) => this.planned.has(h));
    this.arrivals = [...new Set([...unshown, ...this.arrivals])];
    this.planned.clear();
    this.history.length = this.pos + 1;
  }

  private push(h: string): void {
    this.history.push(h);
    this.pos = this.history.length - 1;
    // Bound memory on a frame that runs for months, keeping recent context.
    if (this.history.length > 500) {
      this.history.splice(0, 250);
      this.pos = Math.max(0, this.pos - 250);
      const kept = new Set(this.history);
      for (const h of this.jumped) if (!kept.has(h)) this.jumped.delete(h);
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
    const inPool = new Set(pool.map((p) => p.hash));
    // Arrivals first, whatever the ordering; one filtered out or deleted is dropped.
    while (this.arrivals.length > 0) {
      const h = this.arrivals.shift()!;
      if (!inPool.has(h)) continue;
      this.jumped.add(h);
      this.planned.add(h); // next() clears it once it is on screen
      return h;
    }
    if (s.ordering === 'chronological' || s.ordering === 'reverse-chronological') {
      // Position comes from the whole library, so hiding or filtering out the
      // current photograph continues from where it was instead of restarting.
      const ordered = byDate(all, s.ordering === 'chronological' ? 'asc' : 'desc');
      // Arrivals interrupt the walk; it resumes from the last photograph it chose.
      let anchor: string | undefined;
      for (let i = this.pos; i >= 0 && anchor === undefined; i--) if (!this.jumped.has(this.history[i]!)) anchor = this.history[i];
      const at = ordered.findIndex((p) => p.hash === anchor);
      for (let i = 1; i <= ordered.length; i++) {
        const h = ordered[(at + i + ordered.length) % ordered.length]!.hash;
        if (inPool.has(h)) {
          this.jumped.delete(h); // reached in order now, so it anchors the walk
          return h;
        }
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
    const since = now.getTime() - ARRIVAL_BOOST_MS;
    for (const [h, t] of this.arrivedAt) if (t < since) this.arrivedAt.delete(h);
    const boosted = new Set(this.arrivedAt.keys());
    const chosen = pickNext(candidates, cur, this.history, s.tagAffinity, s.tagWeights, boosted, rand)?.hash;
    if (chosen) this.jumped.delete(chosen);
    return chosen;
  }
}
