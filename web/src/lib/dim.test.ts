import { describe, expect, it } from 'vitest';
import { dimOpacity } from './dim';

const sch = { start: '22:00', end: '07:00', opacity: 0.8, blackout: false };
const at = (h: number, m = 0, s = 0) => new Date(2026, 0, 1, h, m, s);

describe('dimOpacity', () => {
  it('is off in the daytime and full inside the window', () => {
    expect(dimOpacity(at(12), sch)).toBe(0);
    expect(dimOpacity(at(23), sch)).toBeCloseTo(0.8);
    expect(dimOpacity(at(3), sch)).toBeCloseTo(0.8); // wraps midnight
  });
  it('ramps in over 60 seconds from the start time', () => {
    expect(dimOpacity(at(22, 0, 0), sch)).toBe(0);
    expect(dimOpacity(at(22, 0, 30), sch)).toBeCloseTo(0.4);
    expect(dimOpacity(at(22, 1, 0), sch)).toBeCloseTo(0.8);
  });
  it('ramps out over 60 seconds from the end time', () => {
    expect(dimOpacity(at(7, 0, 0), sch)).toBeCloseTo(0.8);
    expect(dimOpacity(at(7, 0, 30), sch)).toBeCloseTo(0.4);
    expect(dimOpacity(at(7, 1, 0), sch)).toBe(0);
  });
  it('blackout goes fully opaque', () => {
    expect(dimOpacity(at(23), { ...sch, blackout: true })).toBe(1);
  });
  it('a same-day window works too', () => {
    const day = { ...sch, start: '09:00', end: '17:00' };
    expect(dimOpacity(at(12), day)).toBeCloseTo(0.8);
    expect(dimOpacity(at(20), day)).toBe(0);
  });
  it('a touch lifts the dim until the deadline', () => {
    const now = at(23);
    expect(dimOpacity(now, sch, now.getTime() + 1000)).toBe(0);
    expect(dimOpacity(now, sch, now.getTime() - 1)).toBeCloseTo(0.8);
  });
  it('bad or empty schedules never dim', () => {
    expect(dimOpacity(at(23), { ...sch, start: 'x' })).toBe(0);
    expect(dimOpacity(at(23), { ...sch, start: '22:00', end: '22:00' })).toBe(0);
  });
});
