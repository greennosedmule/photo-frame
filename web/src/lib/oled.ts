// Persistent elements shift a few pixels every ten minutes so nothing sits on
// the same OLED pixels for hours. Deterministic per ten-minute bucket.

export const SHIFT_INTERVAL_MS = 10 * 60 * 1000;
export const MAX_SHIFT_PX = 8;

export function oledOffset(nowMs: number): { x: number; y: number } {
  const bucket = Math.floor(nowMs / SHIFT_INTERVAL_MS);
  // Two cheap integer hashes; only needs to look unrelated between buckets.
  const h1 = Math.imul(bucket ^ 0x9e3779b9, 0x85ebca6b) >>> 0;
  const h2 = Math.imul(bucket ^ 0x7f4a7c15, 0xc2b2ae35) >>> 0;
  const span = MAX_SHIFT_PX * 2 + 1;
  return { x: (h1 % span) - MAX_SHIFT_PX, y: (h2 % span) - MAX_SHIFT_PX };
}
