// Zoom is remembered as a normalised focal rectangle (0..1 of the original
// image), so it survives a client with a different aspect ratio.

import type { FocalRect } from './settings';

export const FULL: FocalRect = { x: 0, y: 0, w: 1, h: 1 };

export interface Size {
  w: number;
  h: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Keep a rectangle inside the image and at least 1% of it on each side. */
export function clampRect(r: FocalRect): FocalRect {
  const w = clamp(r.w, 0.01, 1);
  const h = clamp(r.h, 0.01, 1);
  return { x: clamp(r.x, 0, 1 - w), y: clamp(r.y, 0, 1 - h), w, h };
}

/**
 * The photograph element is viewport-sized with the image "contained" inside
 * it, so the image sits at an offset when letterboxed. Returns the CSS
 * transform (`translate(tx, ty) scale(scale)`, origin at the element's top
 * left) that centres `rect` and zooms until it just fits the viewport.
 */
export function rectToTransform(rect: FocalRect, image: Size, viewport: Size): { scale: number; tx: number; ty: number } {
  const { boxW, boxH, offX, offY } = contain(image, viewport);
  const scale = Math.min(viewport.w / (rect.w * boxW), viewport.h / (rect.h * boxH));
  const cx = offX + (rect.x + rect.w / 2) * boxW;
  const cy = offY + (rect.y + rect.h / 2) * boxH;
  return { scale, tx: viewport.w / 2 - cx * scale, ty: viewport.h / 2 - cy * scale };
}

/** Inverse: the focal rectangle a given scale and pan shows. */
export function transformToRect(t: { scale: number; tx: number; ty: number }, image: Size, viewport: Size): FocalRect {
  const { boxW, boxH, offX, offY } = contain(image, viewport);
  const cx = (viewport.w / 2 - t.tx) / t.scale - offX;
  const cy = (viewport.h / 2 - t.ty) / t.scale - offY;
  const w = Math.min(1, viewport.w / t.scale / boxW);
  const h = Math.min(1, viewport.h / t.scale / boxH);
  return clampRect({ x: cx / boxW - w / 2, y: cy / boxH - h / 2, w, h });
}

function contain(image: Size, viewport: Size) {
  const fit = Math.min(viewport.w / image.w, viewport.h / image.h);
  const boxW = image.w * fit;
  const boxH = image.h * fit;
  return { boxW, boxH, offX: (viewport.w - boxW) / 2, offY: (viewport.h - boxH) / 2 };
}
