// Dim schedule as a pure function of time, so it can be tested without a DOM.

import type { ClientSettings } from './settings';

export const RAMP_SECONDS = 60;
export const TOUCH_LIFT_MS = 30_000;

function secondsOfDay(hhmm: string): number | undefined {
  const m = /^(\d{1,2}):(\d{2})$/.exec(hhmm);
  if (!m) return undefined;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? h * 3600 + min * 60 : undefined;
}

/** Seconds elapsed since `from`, wrapping past midnight. */
const since = (now: number, from: number) => (((now - from) % 86400) + 86400) % 86400;

/**
 * Overlay opacity 0..1. The dim ramps in over 60 s from the start time and back
 * out over 60 s from the end time. A blackout schedule dims to fully opaque.
 * `liftedUntil` (epoch ms, from a recent touch) suspends dimming.
 */
export function dimOpacity(
  now: Date,
  schedule: ClientSettings['dimSchedule'],
  liftedUntil = 0,
): number {
  if (now.getTime() < liftedUntil) return 0;
  const start = secondsOfDay(schedule.start);
  const end = secondsOfDay(schedule.end);
  if (start === undefined || end === undefined || start === end) return 0;
  const target = schedule.blackout ? 1 : Math.min(1, Math.max(0, schedule.opacity));
  const t = now.getHours() * 3600 + now.getMinutes() * 60 + now.getSeconds() + now.getMilliseconds() / 1000;

  const windowLen = since(end, start);
  const sinceStart = since(t, start);
  if (sinceStart < windowLen) {
    return target * Math.min(1, sinceStart / RAMP_SECONDS);
  }
  const sinceEnd = since(t, end);
  return sinceEnd < RAMP_SECONDS ? target * (1 - sinceEnd / RAMP_SECONDS) : 0;
}
