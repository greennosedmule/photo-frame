import { describe, expect, it } from 'vitest';
import type { ManifestPhoto } from './api';
import { wantedUrls } from './prefetch';

const p = (hash: string, media_rotation = 0): ManifestPhoto => ({
  hash, w: 1, h: 1, effective_date: '2020-01-01T00:00:00Z', date_source: 'exif', favorite: false, tags: [], rotation: media_rotation, media_rotation,
});

describe('wantedUrls', () => {
  it('wants the display and blur variant of every photograph', () => {
    expect(wantedUrls([p('a'.repeat(64))])).toEqual([`/media/${'a'.repeat(64)}/display`, `/media/${'a'.repeat(64)}/blur`]);
  });

  it('follows a photograph to its rotated address, so the old one becomes unwanted', () => {
    const h = 'b'.repeat(64);
    expect(wantedUrls([p(h, 90)])).toEqual([`/media/${h}/display?r=90`, `/media/${h}/blur?r=90`]);
  });

  it('is empty for an empty library', () => {
    expect(wantedUrls([])).toEqual([]);
  });
});
