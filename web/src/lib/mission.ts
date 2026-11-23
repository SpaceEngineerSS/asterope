import type { MissionSpec } from '../engine/types';

/** How the camera is carried, as the bench asks for it. */
export interface MissionSetup {
  altitudeKm: number;
  inclinationDeg: number;
  nodeDeg: number;
  year: number;
  month: number;
  day: number;
  /** Angle of the boresight from the direction away from the Earth. */
  tiltDeg: number;
  /** Direction of the tilt, from along the track toward the orbit normal. */
  azimuthDeg: number;
  /** Tumbling rate; zero keeps the camera fixed to the orbit. */
  tumbleDeg: number;
  sunExclusionDeg: number;
  earthLimbExclusionDeg: number;
  moonExclusionDeg: number;
  orbits: number;
}

const DEG = Math.PI / 180;
/** Moments a mission is sampled at. */
const SAMPLES = 720;

/** Julian date at noon of a Gregorian calendar date (Fliegel and Van Flandern 1968). */
export function julianDate(year: number, month: number, day: number): number {
  const a = Math.trunc((month - 14) / 12);
  return (
    day -
    32075 +
    Math.trunc((1461 * (year + 4800 + a)) / 4) +
    Math.trunc((367 * (month - 2 - 12 * a)) / 12) -
    Math.trunc((3 * Math.trunc((year + 4900 + a) / 100)) / 4)
  );
}

/** Seconds of one revolution on a circular orbit at this altitude. */
export function orbitalPeriod(altitudeKm: number): number {
  const radius = 6378.137 + altitudeKm;
  return 2 * Math.PI * Math.sqrt(radius ** 3 / 398600.4418);
}

export function missionSpec(setup: MissionSetup): MissionSpec {
  const tilt = setup.tiltDeg * DEG;
  const azimuth = setup.azimuthDeg * DEG;
  return {
    altitudeKm: setup.altitudeKm,
    inclinationDeg: setup.inclinationDeg,
    nodeDeg: setup.nodeDeg,
    julianDate: julianDate(setup.year, setup.month, setup.day),
    boresight: [Math.sin(tilt) * Math.cos(azimuth), Math.sin(tilt) * Math.sin(azimuth), Math.cos(tilt)],
    spinDeg: [0.8 * setup.tumbleDeg, 0.6 * setup.tumbleDeg, 0],
    sunExclusionDeg: setup.sunExclusionDeg,
    earthLimbExclusionDeg: setup.earthLimbExclusionDeg,
    moonExclusionDeg: setup.moonExclusionDeg,
    orbits: setup.orbits,
    stepSeconds: (orbitalPeriod(setup.altitudeKm) * setup.orbits) / SAMPLES,
  };
}
