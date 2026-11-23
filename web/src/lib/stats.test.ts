import { describe, expect, it } from 'vitest';

import { clopperPearson, lnGamma, percentile } from './stats';

describe('lnGamma', () => {
  it('matches factorials', () => {
    expect(lnGamma(1)).toBeCloseTo(0, 12);
    expect(lnGamma(6)).toBeCloseTo(Math.log(120), 10);
    expect(lnGamma(101)).toBeCloseTo(363.73937555556347, 8);
  });
});

describe('clopperPearson', () => {
  it('uses closed forms at the extremes', () => {
    const [lo, hi] = clopperPearson(0, 1000);
    expect(lo).toBe(0);
    expect(hi).toBeCloseTo(1 - 0.025 ** (1 / 1000), 9);
    const [lo2, hi2] = clopperPearson(10_000, 10_000);
    expect(hi2).toBe(1);
    expect(lo2).toBeCloseTo(0.025 ** (1 / 10_000), 9);
  });

  it('matches beta-quantile reference values', () => {
    const [lo, hi] = clopperPearson(990, 1000);
    expect(lo).toBeCloseTo(0.98168, 3);
    expect(hi).toBeCloseTo(0.9952, 3);
    const [lo2, hi2] = clopperPearson(5, 10);
    expect(lo2).toBeCloseTo(0.18709, 4);
    expect(hi2).toBeCloseTo(0.81291, 4);
  });
});

describe('percentile', () => {
  it('interpolates', () => {
    expect(percentile([1, 2, 4], 0.75)).toBe(3);
    expect(percentile([], 0.5)).toBeNaN();
  });
});
