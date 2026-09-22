import { describe, expect, it } from 'vitest';
import { FULL, clampRect, rectToTransform, transformToRect } from './zoom';

const viewport = { w: 1194, h: 834 }; // iPad Pro 11"
const wide = { w: 4000, h: 3000 };

describe('zoom', () => {
  it('the full image is scale 1 with no offset (letterboxing is layout)', () => {
    const t = rectToTransform(FULL, wide, viewport);
    expect(t.scale).toBeCloseTo(1);
    expect(t.tx).toBeCloseTo(0);
    expect(t.ty).toBeCloseTo(0);
  });

  it('zooming into a rectangle centres it', () => {
    const rect = { x: 0.25, y: 0.25, w: 0.5, h: 0.5 };
    const t = rectToTransform(rect, wide, viewport);
    expect(t.scale).toBeGreaterThan(1.9);
    // The rectangle's centre lands on the viewport centre.
    const fit = Math.min(viewport.w / wide.w, viewport.h / wide.h);
    const boxW = wide.w * fit, boxH = wide.h * fit;
    const cx = (viewport.w - boxW) / 2 + 0.5 * boxW;
    const cy = (viewport.h - boxH) / 2 + 0.5 * boxH;
    expect(cx * t.scale + t.tx).toBeCloseTo(viewport.w / 2);
    expect(cy * t.scale + t.ty).toBeCloseTo(viewport.h / 2);
  });

  it('round-trips a rectangle that matches the viewport aspect', () => {
    const fit = Math.min(viewport.w / wide.w, viewport.h / wide.h);
    const boxAspect = (wide.w * fit) / (wide.h * fit);
    const w = 0.4;
    const h = (w * boxAspect * (wide.h * fit)) / (wide.w * fit) * (viewport.h / viewport.w) * (viewport.w / viewport.h);
    const rect = { x: 0.3, y: 0.2, w, h: Math.min(0.9, h) };
    const back = transformToRect(rectToTransform(rect, wide, viewport), wide, viewport);
    expect(back.x + back.w / 2).toBeCloseTo(rect.x + rect.w / 2, 2);
    expect(back.y + back.h / 2).toBeCloseTo(rect.y + rect.h / 2, 2);
  });

  it('survives a different aspect ratio', () => {
    const rect = { x: 0.4, y: 0.4, w: 0.2, h: 0.2 };
    const portrait = { w: 834, h: 1194 };
    const t = rectToTransform(rect, wide, portrait);
    expect(t.scale).toBeGreaterThan(1);
    const back = transformToRect(t, wide, portrait);
    expect(back.x + back.w / 2).toBeCloseTo(0.5, 2);
    expect(back.y + back.h / 2).toBeCloseTo(0.5, 2);
  });

  it('clamps rectangles into the image', () => {
    expect(clampRect({ x: -1, y: 2, w: 5, h: 0 })).toEqual({ x: 0, y: 0.99, w: 1, h: 0.01 });
  });
});
