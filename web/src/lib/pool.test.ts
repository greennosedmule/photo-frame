import { describe, expect, it } from 'vitest';
import { pool } from './pool';

describe('pool', () => {
  it('never exceeds the concurrency limit and visits every item', async () => {
    let active = 0;
    let peak = 0;
    const seen: number[] = [];
    await pool([1, 2, 3, 4, 5, 6, 7, 8], 3, async (n) => {
      active++;
      peak = Math.max(peak, active);
      await new Promise((r) => setTimeout(r, 5));
      seen.push(n);
      active--;
    });
    expect(peak).toBe(3);
    expect(seen.sort()).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('keeps going after failures and reports them', async () => {
    const r = await pool([1, 2, 3, 4], 2, async (n) => {
      if (n % 2 === 0) throw new Error(`bad ${n}`);
    });
    expect(r.failed.sort()).toEqual([2, 4]);
    expect((r.error as Error).message).toMatch(/bad/);
  });

  it('handles an empty list and reports progress', async () => {
    expect((await pool([], 4, async () => {})).failed).toEqual([]);
    const marks: number[] = [];
    await pool([1, 2, 3], 1, async () => {}, (d) => marks.push(d));
    expect(marks).toEqual([1, 2, 3]);
  });
});
