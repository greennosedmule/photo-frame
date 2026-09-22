// Pure and DOM-free so it can be unit-tested on its own.
//
//   weight(p) = 1 × (1 + tagAffinity × sharedTagCount(p, current)) × recencyPenalty(p)
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
): number {
  const shared = current ? p.tags.filter((t) => current.tags.includes(t)).length : 0;
  const window = recencyWindow(eligibleCount);
  const recentlyShown = window > 0 && recent.slice(-window).includes(p.hash);
  return (1 + tagAffinity * shared) * (recentlyShown ? 0.05 : 1);
}

/** Pick the next photograph. `rand` is injectable (returns [0,1)) for tests. */
export function pickNext(
  eligible: readonly Candidate[],
  current: Candidate | undefined,
  recent: readonly string[],
  tagAffinity: number,
  rand: () => number = Math.random,
): Candidate | undefined {
  if (eligible.length === 0) return undefined;
  const weights = eligible.map((p) => weight(p, current, recent, tagAffinity, eligible.length));
  const total = weights.reduce((a, b) => a + b, 0);
  let r = rand() * total;
  for (let i = 0; i < eligible.length; i++) {
    r -= weights[i]!;
    if (r < 0) return eligible[i];
  }
  return eligible[eligible.length - 1];
}
