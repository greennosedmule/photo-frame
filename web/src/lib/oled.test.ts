import { describe, expect, it } from 'vitest';
import { MAX_SHIFT_PX, SHIFT_INTERVAL_MS, oledOffset } from './oled';

describe('oledOffset', () => {
  it('is stable within a ten-minute bucket', () => {
    expect(oledOffset(5 * SHIFT_INTERVAL_MS + 1)).toEqual(oledOffset(6 * SHIFT_INTERVAL_MS - 1));
  });
  it('stays within a few pixels and moves between buckets', () => {
    const seen = new Set<string>();
    for (let b = 0; b < 200; b++) {
      const { x, y } = oledOffset(b * SHIFT_INTERVAL_MS);
      expect(Math.abs(x)).toBeLessThanOrEqual(MAX_SHIFT_PX);
      expect(Math.abs(y)).toBeLessThanOrEqual(MAX_SHIFT_PX);
      seen.add(`${x},${y}`);
    }
    expect(seen.size).toBeGreaterThan(50);
  });
});
