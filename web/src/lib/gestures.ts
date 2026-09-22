// Touch and pointer gestures for the frame surface, over @use-gesture/vanilla.
//
//   tap: info overlay          drag left/right: photo follows the finger, then next / previous
//   pinch: zoom, pan when zoomed   double tap: reset to remembered zoom
//   long press: control sheet  swipe up: share
//   two-finger swipe down: hide this photograph on this client
//
// Everything visual lives in the component; this only reports intent.

import { Gesture } from '@use-gesture/vanilla';

export interface GestureHandlers {
  /** Any pointer down. The frame resets its dwell timer and lifts the dim. */
  touch(): void;
  tap(): void;
  doubleTap(): void;
  longPress(): void;
  swipe(dir: 'left' | 'right' | 'up'): void;
  /** Live horizontal finger travel in px while dragging an unzoomed photograph. */
  drag(dx: number): void;
  /** The drag ended without a swipe: settle or spring back. */
  dragEnd(): void;
  twoFingerSwipeDown(): void;
  /** `ratio` is relative to the start of the pinch; `origin` is in client pixels. */
  pinch(ratio: number, origin: [number, number]): void;
  pinchEnd(): void;
  pan(dx: number, dy: number): void;
  panEnd(): void;
}

const DOUBLE_TAP_MS = 300;
const LONG_PRESS_MS = 600;
const LONG_PRESS_SLOP_PX = 10;

export function attachGestures(el: HTMLElement, h: GestureHandlers, isZoomed: () => boolean): () => void {
  // ---- long press and touch notification, on raw pointer events --------------
  let pressTimer: ReturnType<typeof setTimeout> | undefined;
  let pressStart: [number, number] | undefined;
  const pointers = new Set<number>();
  let longPressFired = false;

  const cancelPress = () => {
    clearTimeout(pressTimer);
    pressTimer = undefined;
  };
  const onDown = (e: PointerEvent) => {
    pointers.add(e.pointerId);
    h.touch();
    // A mouse has a right button for this, and a held-down click is a drag: the
    // hold gesture is for touch and pen only.
    if (pointers.size === 1 && e.pointerType !== 'mouse') {
      longPressFired = false;
      pressStart = [e.clientX, e.clientY];
      pressTimer = setTimeout(() => {
        longPressFired = true;
        h.longPress();
      }, LONG_PRESS_MS);
    } else {
      cancelPress(); // a second finger is a pinch, not a press
    }
  };
  const onMove = (e: PointerEvent) => {
    if (!pressStart) return;
    if (Math.hypot(e.clientX - pressStart[0], e.clientY - pressStart[1]) > LONG_PRESS_SLOP_PX) cancelPress();
  };
  const onUp = (e: PointerEvent) => {
    pointers.delete(e.pointerId);
    cancelPress();
  };
  el.addEventListener('pointerdown', onDown);
  el.addEventListener('pointermove', onMove);
  el.addEventListener('pointerup', onUp);
  el.addEventListener('pointercancel', onUp);

  // ---- taps -------------------------------------------------------------------
  let lastTap = 0;
  let tapTimer: ReturnType<typeof setTimeout> | undefined;
  const onTap = () => {
    if (longPressFired) return;
    const now = Date.now();
    if (now - lastTap < DOUBLE_TAP_MS) {
      clearTimeout(tapTimer);
      lastTap = 0;
      h.doubleTap();
    } else {
      lastTap = now;
      // Wait out the double-tap window before treating it as a single tap.
      tapTimer = setTimeout(h.tap, DOUBLE_TAP_MS);
    }
  };

  // ---- drag, swipe and pinch -----------------------------------------------------
  let maxTouches = 0;
  let pinchRatio = 1;
  let pinching = false;

  const gesture = new Gesture(
    el,
    {
      onDrag: (s) => {
        // A tap arrives as one event on release with `tap` set and `last`
        // unset, so it must be handled before the in-progress branch below.
        if (s.tap) return onTap();
        maxTouches = Math.max(maxTouches, s.touches);
        if (s.first) maxTouches = s.touches;
        if (!s.last) {
          if (s.touches === 1 && !pinching) {
            if (isZoomed()) h.pan(s.delta[0], s.delta[1]);
            else if (Math.abs(s.movement[0]) > Math.abs(s.movement[1])) h.drag(s.movement[0]);
          }
          return;
        }
        const wasPinch = pinching || Math.abs(pinchRatio - 1) > 0.15;
        if (maxTouches >= 2) {
          h.dragEnd();
          if (!wasPinch && s.movement[1] > 80 && Math.abs(s.movement[0]) < s.movement[1]) h.twoFingerSwipeDown();
        } else if (isZoomed()) {
          h.panEnd();
        } else {
          const [sx, sy] = s.swipe;
          if (sx === -1) h.swipe('left');
          else if (sx === 1) h.swipe('right');
          else if (sy === -1) {
            h.dragEnd();
            h.swipe('up');
          } else h.dragEnd();
        }
        pinchRatio = 1;
      },
      onPinchStart: () => {
        pinching = true;
        pinchRatio = 1;
      },
      onPinch: (s) => {
        pinchRatio = s.offset[0];
        h.pinch(s.offset[0], s.origin as [number, number]);
      },
      onPinchEnd: () => {
        pinching = false;
        h.pinchEnd();
      },
    },
    {
      drag: { filterTaps: true, threshold: 6, swipe: { distance: [50, 50], velocity: [0.3, 0.3] }, pointer: { touch: true } },
      // Offset starts at 1 each pinch, so `ratio` is relative to the pinch start.
      pinch: { scaleBounds: { min: 0.3, max: 10 }, from: () => [1, 0], pointer: { touch: true } },
    },
  );

  return () => {
    gesture.destroy();
    cancelPress();
    clearTimeout(tapTimer);
    el.removeEventListener('pointerdown', onDown);
    el.removeEventListener('pointermove', onMove);
    el.removeEventListener('pointerup', onUp);
    el.removeEventListener('pointercancel', onUp);
  };
}
