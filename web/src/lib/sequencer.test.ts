import { describe, expect, it } from 'vitest';
import type { ManifestPhoto } from './api';
import { Sequencer } from './sequencer';

const p = (hash: string, date: string, ...tags: string[]): ManifestPhoto => ({
  hash, w: 1, h: 1, effective_date: date, date_source: 'exif', favorite: false, tags, rotation: 0, media_rotation: 0,
});
const base = { ordering: 'chronological' as const, tagAffinity: 0.5, hidden: [] as string[], tagFilter: [] as string[] };
const lib = [p('c', '2022-01-01T00:00:00Z'), p('a', '2020-01-01T00:00:00Z'), p('b', '2021-01-01T00:00:00Z')];

describe('Sequencer', () => {
  it('walks chronologically and wraps', () => {
    const s = new Sequencer();
    const seq = [1, 2, 3, 4].map(() => s.next(lib, base));
    expect(seq).toEqual(['a', 'b', 'c', 'a']);
  });

  it('walks reverse-chronologically', () => {
    const s = new Sequencer();
    const cfg = { ...base, ordering: 'reverse-chronological' as const };
    expect([1, 2, 3].map(() => s.next(lib, cfg))).toEqual(['c', 'b', 'a']);
  });

  it('goes back and replays history forward before choosing anew', () => {
    const s = new Sequencer();
    s.next(lib, base); s.next(lib, base); s.next(lib, base); // a b c
    expect(s.prev(lib, base)).toBe('b');
    expect(s.prev(lib, base)).toBe('a');
    expect(s.prev(lib, base)).toBeUndefined();
    expect(s.next(lib, base)).toBe('b');
    expect(s.next(lib, base)).toBe('c');
    expect(s.next(lib, base)).toBe('a'); // new choice, wraps
  });

  it('skips photographs hidden since they were shown', () => {
    const s = new Sequencer();
    s.next(lib, base); s.next(lib, base); s.next(lib, base); // a b c
    const hidden = { ...base, hidden: ['b'] };
    expect(s.prev(lib, hidden)).toBe('a');
  });

  it('a hidden current photograph does not stall chronological order', () => {
    const s = new Sequencer();
    s.next(lib, base); // a
    s.next(lib, base); // b
    const hidden = { ...base, hidden: ['b'] };
    expect(s.next(lib, hidden)).toBe('c');
  });

  it('shuffle rarely repeats the current photograph, but is allowed to', () => {
    // The recency penalty is a multiplier, not an exclusion, so small libraries
    // never deadlock. Seeded so the test is deterministic.
    let seed = 42;
    const rand = () => {
      seed = (seed + 0x6d2b79f5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const s = new Sequencer();
    const many = Array.from({ length: 20 }, (_, i) => p(`h${i}`, '2020-01-01T00:00:00Z'));
    const cfg = { ...base, ordering: 'shuffle' as const };
    let last = s.next(many, cfg, new Date(), rand);
    let repeats = 0;
    for (let i = 0; i < 400; i++) {
      const n = s.next(many, cfg, new Date(), rand);
      if (n === last) repeats++;
      last = n;
    }
    expect(repeats).toBeLessThan(400 * 0.03);
  });

  it('on-this-day falls back to shuffle with fewer than five matches', () => {
    const s = new Sequencer();
    const now = new Date(2026, 6, 4);
    const ps = [p('t1', '2015-07-04T00:00:00Z'), p('o1', '2015-01-01T00:00:00Z'), p('o2', '2016-01-01T00:00:00Z')];
    const cfg = { ...base, ordering: 'on-this-day' as const };
    const seen = new Set<string | undefined>();
    for (let i = 0; i < 60; i++) seen.add(s.next(ps, cfg, now, Math.random));
    expect(seen.has('o1') || seen.has('o2')).toBe(true);
  });

  it('on-this-day restricts to anniversaries once there are five', () => {
    const s = new Sequencer();
    const now = new Date(2026, 6, 4);
    const ps = [
      ...Array.from({ length: 5 }, (_, i) => p(`t${i}`, `${2010 + i}-07-04T00:00:00Z`)),
      p('other', '2015-01-01T00:00:00Z'),
    ];
    const cfg = { ...base, ordering: 'on-this-day' as const };
    for (let i = 0; i < 50; i++) expect(s.next(ps, cfg, now, Math.random)).toMatch(/^t/);
  });

  it('returns undefined for an empty library and recovers when it fills', () => {
    const s = new Sequencer();
    expect(s.next([], base)).toBeUndefined();
    expect(s.next(lib, base)).toBe('a');
  });

  it('plans ahead so next() returns exactly what was preloaded', () => {
    const s = new Sequencer();
    const many = Array.from({ length: 30 }, (_, i) => p(`h${i}`, '2020-01-01T00:00:00Z'));
    const cfg = { ...base, ordering: 'shuffle' as const };
    const first = s.next(many, cfg);
    const planned = s.ahead(many, cfg, 2);
    expect(planned).toHaveLength(2);
    expect(s.current).toBe(first);
    expect(s.ahead(many, cfg, 2)).toEqual(planned); // idempotent
    expect(s.next(many, cfg)).toBe(planned[0]);
    expect(s.ahead(many, cfg, 2)[0]).toBe(planned[1]);
    expect(s.next(many, cfg)).toBe(planned[1]);
  });

  it('dropAhead discards a stale plan when settings change', () => {
    const s = new Sequencer();
    s.next(lib, base); // a
    expect(s.ahead(lib, base, 2)).toEqual(['b', 'c']);
    s.dropAhead();
    const rev = { ...base, ordering: 'reverse-chronological' as const };
    expect(s.ahead(lib, rev, 1)).toEqual(['c']); // continues from a, descending wraps to c
  });

  it('plans through a whole library without stalling on tiny sets', () => {
    const s = new Sequencer();
    const one = [p('only', '2020-01-01T00:00:00Z')];
    const cfg = { ...base, ordering: 'shuffle' as const };
    s.next(one, cfg);
    expect(s.ahead(one, cfg, 2)).toEqual(['only', 'only']);
  });
});
