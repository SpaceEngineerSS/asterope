import { describe, expect, it } from 'vitest';

import { attitudeMatrix, directionFromRaDec, graticule, gridStep, projectDirection, rollBy, skyShift, slew } from './sky';

const projection = {
  fx: 4000,
  fy: 4000,
  cx: 511.5,
  cy: 511.5,
  distortion: [0, 0, 0, 0, 0] as const,
};

describe('attitudeMatrix', () => {
  it('is the identity for the identity quaternion', () => {
    expect(attitudeMatrix([0, 0, 0, 1])).toEqual([
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1],
    ]);
  });

  it('follows the scalar-last passive convention', () => {
    const half = Math.PI / 4;
    const a = attitudeMatrix([0, 0, Math.sin(half), Math.cos(half)]);
    const rotated = [a[0][0], a[1][0], a[2][0]];
    expect(rotated[0]).toBeCloseTo(0, 12);
    expect(rotated[1]).toBeCloseTo(-1, 12);
    expect(rotated[2]).toBeCloseTo(0, 12);
  });
});

describe('projectDirection', () => {
  it('puts the boresight on the principal point and rejects the back hemisphere', () => {
    const identity = attitudeMatrix([0, 0, 0, 1]);
    expect(projectDirection(identity, [0, 0, 1], projection)).toEqual([511.5, 511.5]);
    expect(projectDirection(identity, [0, 0, -1], projection)).toBeNull();
  });
});

describe('slew', () => {
  const start = { raDeg: 100, decDeg: 0, rollDeg: 0 };

  it('moves the boresight east when the sky is dragged right at zero roll', () => {
    const next = slew(start, 10, 0, 0.01);
    expect(next.raDeg).toBeCloseTo(100.1, 6);
    expect(next.decDeg).toBeCloseTo(0, 9);
  });

  it('moves the boresight north when the sky is dragged down at zero roll', () => {
    expect(slew(start, 0, 10, 0.01).decDeg).toBeCloseTo(0.1, 6);
  });

  it('wraps right ascension', () => {
    expect(slew({ raDeg: 359.95, decDeg: 0, rollDeg: 0 }, 10, 0, 0.01).raDeg).toBeCloseTo(0.05, 6);
    expect(rollBy(start, -10).rollDeg).toBe(350);
  });

  it('carries the camera over the pole instead of stopping at it', () => {
    const over = slew({ raDeg: 0, decDeg: 89.9, rollDeg: 0 }, 0, 100, 0.01);
    // 1 degree toward north from 0.1 degrees short of the pole: 0.9 beyond it.
    expect(over.decDeg).toBeCloseTo(89.1, 2);
    expect(over.raDeg).toBeCloseTo(180, 6);
    expect(over.rollDeg).toBeCloseTo(180, 6);
  });

  it('keeps the star under the cursor under it, anywhere and at any roll', () => {
    for (const from of [
      { raDeg: 100, decDeg: 0, rollDeg: 0 },
      { raDeg: 83.8, decDeg: -5.4, rollDeg: 12 },
      { raDeg: 250, decDeg: 62, rollDeg: 200 },
      { raDeg: 10, decDeg: 88.5, rollDeg: 75 },
      { raDeg: 300, decDeg: -89.2, rollDeg: 310 },
    ]) {
      for (const [dx, dy] of [[40, 0], [0, -25], [-30, 55]] as const) {
        const to = slew(from, dx, dy, 0.0137);
        const [sx, sy] = skyShift(from, to, 0.0137);
        // To a few thousandths of a pixel: the image axes turn slightly
        // as the camera moves, which the first-order shift does not carry.
        expect(sx).toBeCloseTo(dx, 2);
        expect(sy).toBeCloseTo(dy, 2);
        // The opposite drag comes back to where it started.
        const back = slew(to, -dx, -dy, 0.0137);
        expect(skyShift(from, back, 0.0137)[0]).toBeCloseTo(0, 2);
        expect(skyShift(from, back, 0.0137)[1]).toBeCloseTo(0, 2);
      }
    }
  });
});

describe('graticule', () => {
  it('chooses a readable spacing', () => {
    expect(gridStep(14)).toBe(5);
    expect(gridStep(2)).toBe(0.5);
  });

  it('draws both families of lines through a frame', () => {
    // Pointing RA 0, Dec 0, roll 0: sensor x is west, y is south, z is the boresight.
    const a = attitudeMatrix([-0.5, 0.5, -0.5, 0.5]);
    const center = { raDeg: 0, decDeg: 0, rollDeg: 0 };
    expect(projectDirection(a, directionFromRaDec(0, 0), projection)).toEqual([511.5, 511.5]);
    const east = projectDirection(a, directionFromRaDec(1, 0), projection);
    const north = projectDirection(a, directionFromRaDec(0, 1), projection);
    expect(east?.[0]).toBeLessThan(511.5);
    expect(north?.[1]).toBeLessThan(511.5);
    const lines = graticule(a, projection, center, 14, 1024, 1024);
    expect(lines.some((line) => line.kind === 'ra')).toBe(true);
    expect(lines.some((line) => line.kind === 'dec')).toBe(true);
  });
});
