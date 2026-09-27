// Pure and DOM-free so it can be unit-tested on its own.
//
//   weight(p) = Π tagWeight(t) for t in p.tags
//             × (1 + tagAffinity × sharedTagCount(p, current)) × recencyPenalty(p)
//             × arrivalBoost(p)
//   arrivalBoost = ARRIVAL_BOOST if this frame saw p arrive within ARRIVAL_BOOST_MS, else 1.0

/** New arrivals come round more often for a while after they have been shown. */
export const ARRIVAL_BOOST = 3;
export const ARRIVAL_BOOST_MS = 24 * 60 * 60 * 1000;
//   recencyPenalty = 0.05 if shown within the last N, else 1.0;  N = min(50, eligible × 0.3)

export interface Candidate {
  hash: string;
  tags: string[];
}

export function recencyWindow(eligibleCount: number): number {
  return Math.min(50, Math.floor(eligibleCount * 0.3));
}

export function weight(
  p: Candidate,
  current: Candidate | undefined,
  recent: readonly string[],
  tagAffinity: number,
  eligibleCount: number,
  tagWeights: Readonly<Record<string, number>> = {},
  boosted: ReadonlySet<string> = new Set(),
): number {
  const base = p.tags.reduce((w, t) => w * (tagWeights[t] ?? 1), 1);
  const shared = current ? p.tags.filter((t) => current.tags.includes(t)).length : 0;
  const window = recencyWindow(eligibleCount);
  const recentlyShown = window > 0 && recent.slice(-window).includes(p.hash);
  return base * (1 + tagAffinity * shared) * (recentlyShown ? 0.05 : 1) * (boosted.has(p.hash) ? ARRIVAL_BOOST : 1);
}

/** Pick the next photograph. `rand` is injectable (returns [0,1)) for tests. */
export function pickNext(
  eligible: readonly Candidate[],
  current: Candidate | undefined,
  recent: readonly string[],
  tagAffinity: number,
  tagWeights: Readonly<Record<string, number>> = {},
  boosted: ReadonlySet<string> = new Set(),
  rand: () => number = Math.random,
): Candidate | undefined {
  if (eligible.length === 0) return undefined;
  const weights = eligible.map((p) => weight(p, current, recent, tagAffinity, eligible.length, tagWeights, boosted));
  const total = weights.reduce((a, b) => a + b, 0);
  let r = rand() * total;
  for (let i = 0; i < eligible.length; i++) {
    r -= weights[i]!;
    if (r < 0) return eligible[i];
  }
  return eligible[eligible.length - 1];
}
