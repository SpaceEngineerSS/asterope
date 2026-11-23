import { describe, expect, it } from 'vitest';

import {
  TRIAL_FIELDS,
  decodeDetect,
  decodeSensor,
  decodeSolve,
  decodeSolver,
  decodeTrials,
  encodeSensor,
  encodeSolver,
} from './decode';
import type { SensorParams, SolverParams } from './types';

const sensor: SensorParams = {
  zeroPoint: 2e6,
  exposure: 0.1,
  psfSigma: 1.2,
  skyBackground: 20,
  darkCurrent: 10,
  readNoise: 5,
  gain: 4,
  bias: 64,
  fullWell: 16000,
  bitDepth: 12,
  hotPixelFraction: 1e-4,
  hotPixelRate: 5000,
  cosmicRayMean: 2,
  strayIntensity: 300,
  strayAzimuthDeg: 45,
  strayFalloff: 0.35,
  noise: true,
  hotPixelSeed: 7,
  rateXDeg: 0.5,
  rateYDeg: -1,
  rateZDeg: 2,
  colorTerm: -0.3,
  vignetting: 0.5,
  prnu: 0.01,
  dsnu: 30,
  columnNoise: 2,
  psfGrowth: 0.4,
  coma: 0.1,
  rowTimeUs: 20,
  nonlinearity: 0.03,
  blooming: 0.5,
};

const solver: SolverParams = {
  cellSize: 32,
  segmentSigma: 3,
  peakSigma: 5,
  minArea: 3,
  maxArea: 600,
  maxSharpness: 0.7,
  maxElongation: 2.5,
  saturation: 4095,
  method: 'cog',
  maxDetections: 64,
  tolerancePx: 0.75,
  matchRadiusPx: 1.5,
  patternStars: 14,
  maxTriads: 2000,
  minMatches: 5,
  maxChanceProbability: 1e-3,
  weighted: true,
  trails: true,
};

describe('parameter buffers', () => {
  it('round-trip', () => {
    expect(decodeSensor(encodeSensor(sensor))).toEqual(sensor);
    expect(decodeSolver(encodeSolver(solver))).toEqual(solver);
    expect(encodeSensor(sensor)).toHaveLength(31);
    expect(encodeSolver(solver)).toHaveLength(18);
  });
});

describe('decodeDetect', () => {
  it('reads detections and rejected blobs', () => {
    const buffer = [1, 2, 64.5, 1.4, 9, 6, 1, 1, 0, 0, 10.5, 20.25, 900, 80, 40, 12, 1, 1.3, 0.9, 3, 4, 1, 0, 5, 6, 7, 1];
    const result = decodeDetect(buffer);
    expect(result.background).toBe(64.5);
    expect(result.detections).toEqual([
      { x: 10.5, y: 20.25, flux: 900, peak: 80, snr: 40, area: 12, saturated: true, sigma: 1.3, stellarity: 0.9 },
    ]);
    expect(result.rejected).toEqual([
      { x: 3, y: 4, area: 1, reason: 'hot' },
      { x: 5, y: 6, area: 7, reason: 'cosmic' },
    ]);
  });

  it('rejects a truncated buffer', () => {
    expect(() => decodeDetect([1, 0, 0, 0])).toThrow(RangeError);
  });
});

describe('decodeSolve', () => {
  const header = (solved: number): number[] => {
    const h = new Array<number>(33).fill(NaN);
    h[0] = solved;
    h[1] = 3;
    h[2] = 1;
    h[3] = 1;
    h[4] = 0;
    h[23] = solved;
    h[24] = 4000;
    h[25] = 4000;
    h[26] = 511.5;
    h[27] = 511.5;
    h[28] = 0;
    h[29] = 14;
    return h;
  };

  it('returns no attitude without a lock', () => {
    const result = decodeSolve(header(0));
    expect(result.attitude).toBeNull();
    expect(result.triads).toBe(3);
    expect(result.intrinsics.fovXDeg).toBe(14);
  });

  it('reads the attitude and matches', () => {
    const h = header(1);
    [0, 0, 0, 1].forEach((value, i) => {
      h[5 + i] = value;
    });
    h[9] = 83.8;
    h[10] = -5.4;
    h[11] = 12;
    h[12] = 2.5;
    h[19] = 0;
    h[20] = 1;
    h[21] = 2;
    h[22] = 3;
    const result = decodeSolve([...h, 4, 26311, 1.69, 1.2, 500.5, 400.25, 77]);
    expect(result.attitude?.q).toEqual([0, 0, 0, 1]);
    expect(result.attitude?.pointing).toEqual({ raDeg: 83.8, decDeg: -5.4, rollDeg: 12 });
    expect(result.attitude?.sigmaArcsec).toBeNull();
    expect(result.attitude?.matches).toEqual([
      { detection: 4, hip: 26311, mag: 1.69, residualArcsec: 1.2, predictedX: 500.5, predictedY: 400.25, catalogIndex: 77 },
    ]);
  });
});

describe('decodeTrials', () => {
  it('numbers trials from the batch offset', () => {
    const row = [2, 500, 20, 501, 9, 8, 1, 6, 40, 10, -20, 30];
    const outcomes = decodeTrials([...row, ...row], 40);
    expect(row).toHaveLength(TRIAL_FIELDS);
    expect(outcomes.map((o) => o.trial)).toEqual([40, 41]);
    expect(outcomes[0]?.class).toBe('falsePositive');
    expect(outcomes[0]?.pointing).toEqual({ raDeg: 10, decDeg: -20, rollDeg: 30 });
  });
});
