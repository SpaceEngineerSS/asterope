import { describe, expect, it } from 'vitest';

import { decodeShare, encodeShare } from './share';
import type { SharedBench } from './share';

const defaults: SharedBench = {
  pointing: { raDeg: 83.822, decDeg: -5.391, rollDeg: 12 },
  camera: { width: 1024, height: 1024, fovDeg: 14, distortion: [0, 0, 0, 0, 0], scaleSpan: 0, currentEpoch: false },
  sensor: {
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
    hotPixelFraction: 0,
    hotPixelRate: 5000,
    cosmicRayMean: 0,
    strayIntensity: 0,
    strayAzimuthDeg: 0,
    strayFalloff: 0.35,
    noise: true,
    hotPixelSeed: 1,
    rateXDeg: 0,
    rateYDeg: 0,
    rateZDeg: 0,
    colorTerm: 0,
    vignetting: 0,
    prnu: 0,
    dsnu: 0,
    columnNoise: 0,
    psfGrowth: 0,
    coma: 0,
    rowTimeUs: 0,
    nonlinearity: 0,
    blooming: 0,
  },
  solver: {
    cellSize: 32,
    segmentSigma: 3,
    peakSigma: 5,
    minArea: 3,
    maxArea: 600,
    maxSharpness: 0.7,
    maxElongation: 2.5,
    saturation: 4095,
    method: 'iwc',
    maxDetections: 64,
    tolerancePx: 0.75,
    matchRadiusPx: 1.5,
    patternStars: 14,
    maxTriads: 2000,
    minMatches: 5,
    maxChanceProbability: 1e-3,
    weighted: true,
    trails: false,
  },
  seed: 42,
};

const encodeRaw = (value: unknown) => `#bench=${btoa(JSON.stringify(value))}`;

describe('share links', () => {
  it('round-trips a bench', () => {
    const bench: SharedBench = {
      ...defaults,
      pointing: { raDeg: 201.5, decDeg: 44, rollDeg: 300 },
      camera: { ...defaults.camera, fovDeg: 20, width: 512, height: 512, distortion: [-0.05, 0.01, 0, 1e-4, 0] },
      sensor: { ...defaults.sensor, readNoise: 40, rateYDeg: 0.5, noise: false },
      solver: { ...defaults.solver, method: 'cog', tolerancePx: 1.25 },
      seed: 777,
    };
    const hash = encodeShare(bench);
    expect(hash).toMatch(/^#bench=[A-Za-z0-9_-]+$/);
    expect(decodeShare(hash, defaults)).toEqual(bench);
  });

  it('ignores hashes that are not bench links', () => {
    expect(decodeShare('', defaults)).toBeNull();
    expect(decodeShare('#other', defaults)).toBeNull();
    expect(decodeShare('#bench=%%%', defaults)).toBeNull();
    expect(decodeShare(encodeRaw('text'), defaults)).toBeNull();
  });

  it('keeps defaults for wrong types, unknown keys and out-of-range values', () => {
    const decoded = decodeShare(
      encodeRaw({
        pointing: { raDeg: 'east', decDeg: 12, extra: 1 },
        camera: { fovDeg: 500, width: 777, distortion: [1, 2] },
        sensor: { readNoise: null, exposure: Infinity, noise: 'yes', gain: 2 },
        solver: { method: 'magic', tolerancePx: 2 },
        seed: -3.7,
        __proto__: { polluted: true },
      }),
      defaults,
    );
    expect(decoded?.pointing).toEqual({ raDeg: 83.822, decDeg: 12, rollDeg: 12 });
    expect(decoded?.camera).toEqual({ ...defaults.camera, fovDeg: 60 });
    expect(decoded?.sensor).toEqual({ ...defaults.sensor, gain: 2 });
    expect(decoded?.solver).toEqual({ ...defaults.solver, tolerancePx: 2 });
    expect(decoded?.seed).toBe(3);
    expect(({} as { polluted?: boolean }).polluted).toBeUndefined();
  });
});
