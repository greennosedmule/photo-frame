import { describe, expect, it } from 'vitest';
import { CELL, GAP, PITCH, cellRect, columnsFor, indexRange, indicesInRect, normalizeRect, rowCount, visibleRows } from './grid';

describe('grid geometry', () => {
  it('fits as many columns as the width allows, at least one', () => {
    expect(columnsFor(0)).toBe(1);
    expect(columnsFor(CELL)).toBe(1);
    expect(columnsFor(2 * CELL + GAP)).toBe(2);
    expect(columnsFor(2 * CELL + GAP - 1)).toBe(1);
    expect(columnsFor(1200)).toBe(Math.floor((1200 + GAP) / PITCH));
  });

  it('lays cells out row-major on the lattice', () => {
    expect(cellRect(0, 3)).toEqual({ x: 0, y: 0, w: CELL, h: CELL });
    expect(cellRect(4, 3)).toEqual({ x: PITCH, y: PITCH, w: CELL, h: CELL });
    expect(rowCount(7, 3)).toBe(3);
    expect(rowCount(0, 3)).toBe(0);
  });

  it('windows rows with overscan and clamps at both ends', () => {
    expect(visibleRows(0, 600, 1000)).toEqual([0, Math.floor(600 / PITCH) + 2]);
    const [first, last] = visibleRows(10 * PITCH, 600, 1000);
    expect(first).toBe(8);
    expect(last).toBe(10 + Math.floor(600 / PITCH) + 2);
    expect(visibleRows(0, 600, 2)).toEqual([0, 1]);
    expect(visibleRows(0, 600, 0)[1]).toBe(-1);
  });

  it('rubber-band selects exactly the cells it touches', () => {
    const cols = 4;
    // Entirely inside cell 0.
    expect(indicesInRect({ x: 10, y: 10, w: 20, h: 20 }, cols, 20)).toEqual([0]);
    // Spanning the gap between cells 0 and 1.
    expect(indicesInRect({ x: CELL - 2, y: 10, w: GAP + 4, h: 10 }, cols, 20)).toEqual([0, 1]);
    // Entirely within a gap selects nothing.
    expect(indicesInRect({ x: CELL + 1, y: 10, w: GAP - 2, h: 10 }, cols, 20)).toEqual([]);
    // A block across two rows and two columns.
    expect(indicesInRect({ x: PITCH + 5, y: PITCH + 5, w: PITCH, h: PITCH }, cols, 20).sort((a, b) => a - b))
      .toEqual([5, 6, 9, 10]);
  });

  it('never selects past the end of a partial last row', () => {
    expect(indicesInRect({ x: 0, y: 0, w: 10_000, h: 10_000 }, 4, 6)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(indicesInRect({ x: 0, y: 0, w: 100, h: 100 }, 4, 0)).toEqual([]);
  });

  it('builds inclusive ranges in either direction', () => {
    expect(indexRange(2, 5)).toEqual([2, 3, 4, 5]);
    expect(indexRange(5, 2)).toEqual([2, 3, 4, 5]);
    expect(indexRange(3, 3)).toEqual([3]);
  });

  it('normalises a drag rectangle from any corner', () => {
    expect(normalizeRect(50, 60, 10, 20)).toEqual({ x: 10, y: 20, w: 40, h: 40 });
  });
});
