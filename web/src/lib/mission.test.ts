import { describe, expect, it } from 'vitest';

import { julianDate, missionSpec, orbitalPeriod } from './mission';

describe('mission setup', () => {
  it('turns calendar dates into Julian dates', () => {
    expect(julianDate(2000, 1, 1)).toBe(2451545);
    expect(julianDate(2024, 3, 31)).toBe(2460401);
    expect(julianDate(1990, 12, 31)).toBe(2448257);
  });

  it('gives the period of a low orbit', () => {
    expect(orbitalPeriod(400) / 60).toBeCloseTo(92.56, 1);
    expect(orbitalPeriod(35786) / 3600).toBeCloseTo(23.93, 1);
  });

  it('points the boresight by tilt and direction', () => {
    const base = {
      altitudeKm: 550, inclinationDeg: 53, nodeDeg: 0, year: 2026, month: 3, day: 20,
      tiltDeg: 0, azimuthDeg: 90, tumbleDeg: 0, sunExclusionDeg: 35, earthLimbExclusionDeg: 25, moonExclusionDeg: 10, orbits: 1,
    };
    expect(missionSpec(base).boresight[2]).toBeCloseTo(1, 12);
    const normal = missionSpec({ ...base, tiltDeg: 90 }).boresight;
    expect(normal[0]).toBeCloseTo(0, 12);
    expect(normal[1]).toBeCloseTo(1, 12);
    expect(missionSpec({ ...base, tiltDeg: 180 }).boresight[2]).toBeCloseTo(-1, 12);
    expect(missionSpec(base).spinDeg).toEqual([0, 0, 0]);
    expect(Math.hypot(...missionSpec({ ...base, tumbleDeg: 0.5 }).spinDeg)).toBeCloseTo(0.5, 12);
  });
});
