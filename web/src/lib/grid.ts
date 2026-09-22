// Geometry for the virtualised thumbnail grid. Pure, so selection and windowing
// can be tested without a DOM: cells are laid out row-major on a fixed lattice.

export const CELL = 168; // thumbnail edge, px
export const GAP = 8;
export const PITCH = CELL + GAP;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function columnsFor(width: number): number {
  return Math.max(1, Math.floor((width + GAP) / PITCH));
}

export function rowCount(items: number, cols: number): number {
  return Math.ceil(items / cols);
}

export function cellRect(index: number, cols: number): Rect {
  return { x: (index % cols) * PITCH, y: Math.floor(index / cols) * PITCH, w: CELL, h: CELL };
}

/** Rows to render for a scroll position, with `overscan` rows either side. */
export function visibleRows(scrollTop: number, viewHeight: number, rows: number, overscan = 2): [number, number] {
  const first = Math.max(0, Math.floor(scrollTop / PITCH) - overscan);
  const last = Math.min(rows - 1, Math.floor((scrollTop + viewHeight) / PITCH) + overscan);
  return [first, last];
}

/** Indices of cells the rectangle touches (rubber-band selection). */
export function indicesInRect(r: Rect, cols: number, items: number): number[] {
  if (items === 0) return [];
  const x0 = Math.max(0, Math.floor((r.x - CELL) / PITCH) + 1);
  const x1 = Math.min(cols - 1, Math.floor((r.x + r.w) / PITCH));
  const y0 = Math.max(0, Math.floor((r.y - CELL) / PITCH) + 1);
  const y1 = Math.floor((r.y + r.h) / PITCH);
  const out: number[] = [];
  for (let row = y0; row <= y1; row++) {
    for (let col = x0; col <= x1; col++) {
      const i = row * cols + col;
      if (i >= items) break;
      // The lattice bounds are generous by up to a gap; confirm a real overlap.
      const c = cellRect(i, cols);
      if (r.x < c.x + c.w && r.x + r.w > c.x && r.y < c.y + c.h && r.y + r.h > c.y) out.push(i);
    }
  }
  return out;
}

/** Inclusive index range between two cells, in either order. */
export function indexRange(a: number, b: number): number[] {
  const [lo, hi] = a <= b ? [a, b] : [b, a];
  return Array.from({ length: hi - lo + 1 }, (_, i) => lo + i);
}

export function normalizeRect(x0: number, y0: number, x1: number, y1: number): Rect {
  return { x: Math.min(x0, x1), y: Math.min(y0, y1), w: Math.abs(x1 - x0), h: Math.abs(y1 - y0) };
}
