const LANCZOS = [
  676.5203681218851, -1259.1392167224028, 771.32342877765313, -176.61502916214059,
  12.507343278686905, -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7,
] as const;

/** Natural log of the gamma function for positive arguments (Lanczos, g = 7). */
export function lnGamma(x: number): number {
  const shifted = x - 1;
  let sum = 0.99999999999980993;
  LANCZOS.forEach((coefficient, i) => {
    sum += coefficient / (shifted + i + 1);
  });
  const t = shifted + 7.5;
  return 0.5 * Math.log(2 * Math.PI) + (shifted + 0.5) * Math.log(t) - t + Math.log(sum);
}

function binomialCdf(k: number, n: number, p: number): number {
  if (p <= 0 || k >= n) {
    return 1;
  }
  if (p >= 1) {
    return 0;
  }
  const lnP = Math.log(p);
  const lnQ = Math.log(1 - p);
  const lnFactorial = lnGamma(n + 1);
  let total = 0;
  for (let i = 0; i <= k; i += 1) {
    total += Math.exp(lnFactorial - lnGamma(i + 1) - lnGamma(n - i + 1) + i * lnP + (n - i) * lnQ);
  }
  return Math.min(total, 1);
}

function bisect(belowTarget: (p: number) => boolean): number {
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 60; i += 1) {
    const mid = 0.5 * (lo + hi);
    if (belowTarget(mid)) {
      lo = mid;
    } else {
      hi = mid;
    }
  }
  return 0.5 * (lo + hi);
}

/** Exact (Clopper-Pearson) two-sided 95 % interval for a binomial proportion. */
export function clopperPearson(successes: number, trials: number): [number, number] {
  const tail = 0.025;
  if (trials <= 0 || successes > trials) {
    return [0, 1];
  }
  const lower = successes === 0 ? 0 : bisect((p) => 1 - binomialCdf(successes - 1, trials, p) < tail);
  const upper = successes === trials ? 1 : bisect((p) => binomialCdf(successes, trials, p) > tail);
  return [lower, upper];
}

/** Linear-interpolated percentile of ascending-sorted samples. */
export function percentile(sorted: readonly number[], fraction: number): number {
  if (sorted.length === 0) {
    return NaN;
  }
  const position = Math.min(Math.max(fraction, 0), 1) * (sorted.length - 1);
  const lower = Math.floor(position);
  const a = sorted[lower] ?? NaN;
  const b = sorted[lower + 1] ?? a;
  return a + (b - a) * (position - lower);
}
