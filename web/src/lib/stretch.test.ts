import { describe, expect, it } from 'vitest';

import { buildLut, maximumOf, paint } from './stretch';

const input = { background: 100, noise: 2, maximum: 4000 };

describe('buildLut', () => {
  it.each(['linear', 'asinh', 'zscale'] as const)('%s is monotonic and spans the range', (stretch) => {
    const lut = buildLut(stretch, input);
    for (let i = 1; i < lut.length; i += 1) {
      expect(lut[i]).toBeGreaterThanOrEqual(lut[i - 1] ?? 0);
    }
    expect(lut[0]).toBe(0);
    expect(lut[65535]).toBe(255);
  });

  it('asinh lifts faint signal above the linear stretch', () => {
    const faint = 120;
    expect(buildLut('asinh', input)[faint]).toBeGreaterThan(buildLut('linear', input)[faint] ?? 255);
  });

  it('zscale saturates 25 sigma above the background', () => {
    expect(buildLut('zscale', input)[150]).toBe(255);
  });
});

describe('paint', () => {
  it('writes opaque gray pixels', () => {
    const pixels = Uint16Array.of(0, 65535);
    const rgba = new Uint8ClampedArray(8);
    paint(pixels, buildLut('linear', { background: 0, noise: 1, maximum: 65535 }), rgba);
    expect(Array.from(rgba)).toEqual([0, 0, 0, 255, 255, 255, 255, 255]);
    expect(maximumOf(pixels)).toBe(65535);
  });
});
