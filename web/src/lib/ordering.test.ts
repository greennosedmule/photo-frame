import { describe, expect, it } from 'vitest';
import type { ManifestPhoto } from './api';
import { byDate, eligible, onThisDay } from './ordering';

const p = (hash: string, date: string, ...tags: string[]): ManifestPhoto => ({
  hash, w: 1, h: 1, effective_date: date, date_source: 'exif', favorite: false, tags, rotation: 0, media_rotation: 0,
});

describe('eligible', () => {
  const photos = [p('a', '2020-01-01T00:00:00Z', 'dogs'), p('b', '2021-01-01T00:00:00Z', 'cats'), p('c', '2022-01-01T00:00:00Z')];
  it('empty filter keeps everything not hidden', () => {
    expect(eligible(photos, { hidden: ['b'], tagFilter: [] }).map((x) => x.hash)).toEqual(['a', 'c']);
  });
  it('tag filter is any-of', () => {
    expect(eligible(photos, { hidden: [], tagFilter: ['dogs', 'cats'] }).map((x) => x.hash)).toEqual(['a', 'b']);
  });
  it('hidden wins over the filter', () => {
    expect(eligible(photos, { hidden: ['a'], tagFilter: ['dogs'] })).toEqual([]);
  });
});

describe('byDate', () => {
  const photos = [p('b', '2021-01-01T00:00:00Z'), p('a', '2020-01-01T00:00:00Z'), p('c', '2021-01-01T00:00:00Z')];
  it('sorts ascending and descending, ties by hash', () => {
    expect(byDate(photos, 'asc').map((x) => x.hash)).toEqual(['a', 'b', 'c']);
    expect(byDate(photos, 'desc').map((x) => x.hash)).toEqual(['b', 'c', 'a']);
  });
  it('does not mutate its input', () => {
    byDate(photos, 'asc');
    expect(photos[0]!.hash).toBe('b');
  });
});

describe('onThisDay', () => {
  const now = new Date(2026, 6, 4); // 4 July
  it('matches within three days in any year', () => {
    const ps = [p('exact', '2015-07-04T10:00:00Z'), p('edge', '2019-07-07T12:00:00Z'), p('early', '2010-07-01T12:00:00Z'), p('far', '2015-07-20T00:00:00Z')];
    expect(onThisDay(ps, now).map((x) => x.hash).sort()).toEqual(['early', 'edge', 'exact']);
  });
  it('wraps across New Year', () => {
    const ps = [p('nye', '2018-12-31T12:00:00Z'), p('jan2', '2019-01-02T12:00:00Z'), p('mid', '2019-06-01T00:00:00Z')];
    const ny = new Date(2027, 0, 1);
    expect(onThisDay(ps, ny).map((x) => x.hash).sort()).toEqual(['jan2', 'nye']);
  });
  it('ignores unparsable dates', () => {
    expect(onThisDay([p('bad', 'nope')], now)).toEqual([]);
  });
});
