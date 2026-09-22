import { describe, expect, it } from 'vitest';
import { defaultSettings, mergeSettings } from './settings';

describe('mergeSettings', () => {
  it('nothing stored gives the defaults', () => {
    expect(mergeSettings(undefined)).toEqual(defaultSettings);
    expect(mergeSettings(null)).toEqual(defaultSettings);
    expect(mergeSettings('garbage')).toEqual(defaultSettings);
  });

  it('keeps valid stored values', () => {
    const stored = { ...defaultSettings, dwellSeconds: 90, ordering: 'chronological', hidden: ['a'], tagFilter: ['dogs'], zoom: { a: { x: 0.1, y: 0.2, w: 0.3, h: 0.4 } } };
    expect(mergeSettings(stored)).toEqual(stored);
  });

  it('repairs invalid fields individually', () => {
    const m = mergeSettings({ dwellSeconds: -5, ordering: 'sideways', tagAffinity: 9, hidden: ['a', 3, null], zoom: { a: { x: 'no' }, b: { x: 0, y: 0, w: 1, h: 1 } }, dimSchedule: { opacity: 'x', blackout: 1 } });
    expect(m.dwellSeconds).toBe(3);
    expect(m.ordering).toBe('shuffle');
    expect(m.tagAffinity).toBe(1);
    expect(m.hidden).toEqual(['a']);
    expect(Object.keys(m.zoom)).toEqual(['b']);
    expect(m.dimSchedule).toEqual(defaultSettings.dimSchedule);
  });

  it('an older record missing newer fields still loads', () => {
    expect(mergeSettings({ dwellSeconds: 10 }).fillMode).toBe('blur');
  });
});
