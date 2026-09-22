import { describe, expect, it } from 'vitest';
import { pickNext, recencyWindow, weight, type Candidate } from './sequencing';

const c = (hash: string, ...tags: string[]): Candidate => ({ hash, tags });

describe('sequencing', () => {
  it('caps the recency window at 50 and scales with small libraries', () => {
    expect(recencyWindow(10)).toBe(3);
    expect(recencyWindow(1000)).toBe(50);
  });

  it('boosts shared tags by tagAffinity without guaranteeing them', () => {
    const cur = c('a', 'dogs', 'holidays');
    expect(weight(c('b', 'dogs', 'holidays'), cur, [], 0.5, 100)).toBe(2);
    expect(weight(c('c', 'cats'), cur, [], 0.5, 100)).toBe(1);
  });

  it('penalises recently shown photographs but never excludes them', () => {
    expect(weight(c('b'), undefined, ['b'], 0.5, 100)).toBe(0.05);
  });

  it('never deadlocks on a single-photograph library', () => {
    const only = c('a');
    expect(pickNext([only], only, ['a'], 0.5)?.hash).toBe('a');
  });

  it('returns undefined for an empty eligible set', () => {
    expect(pickNext([], undefined, [], 0.5)).toBeUndefined();
  });

  it('selects proportionally to weight', () => {
    const cur = c('cur', 'x');
    const pool = [c('a', 'x'), c('b')]; // weights 1.5 and 1
    expect(pickNext(pool, cur, [], 0.5, () => 0)?.hash).toBe('a');
    expect(pickNext(pool, cur, [], 0.5, () => 0.99)?.hash).toBe('b');
  });
});
