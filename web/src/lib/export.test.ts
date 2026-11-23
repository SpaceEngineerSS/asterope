import { describe, expect, it } from 'vitest';

import type { FrameResult } from '../engine/types';
import { frameFits, matchesCsv, solutionJson, wcsCards, wcsPixelToSky, wcsText } from './export';

const DEG = Math.PI / 180;
const intrinsics = { fx: 4200, fy: 4200, cx: 511.5, cy: 383.5, k1: 0, fovXDeg: 13.9, equidistant: false };

function frame(pointing: { raDeg: number; decDeg: number; rollDeg: number } | null): FrameResult {
  return {
    width: 1024,
    height: 768,
    pixels: new Uint16Array(1024 * 768).fill(100),
    index: { stars: 0, pairs: 0, magLimit: 6, fovXDeg: 13.9, fovYDeg: 10.4, pixelScaleArcsec: 49, rebuilt: false },
    detect: {
      detections: [{ x: 600.25, y: 300.5, flux: 9000, peak: 800, snr: 90, area: 20, saturated: false, sigma: 1.2, stellarity: 1 }],
      rejected: [],
      background: 100,
      noise: 1.4,
      blobs: 1,
      noiseBlobs: 0,
      hotPixels: 0,
      cosmicRays: 0,
      tooLarge: 0,
      edge: 0,
    },
    solve: {
      attitude: pointing && {
        q: [0, 0, 0, 1],
        pointing,
        rmsArcsec: 2.5,
        lambdaMax: 1,
        questQMethodRad: 0,
        questSvdRad: 0,
        chanceProbability: 1e-12,
        sigmaArcsec: [1, 1, 9],
        pyramid: null,
        matches: [
          { detection: 0, hip: 27989, mag: 0.45, residualArcsec: 1.5, predictedX: 600.2, predictedY: 300.6, catalogIndex: 9 },
        ],
      },
      triads: 1,
      pyramids: 1,
      scalesTried: 1,
      refined: false,
      intrinsics,
    },
    field: [],
    truth: [],
    truthPointing: null,
    truthQ: null,
    error: null,
    timing: { renderMs: 0, detectMs: 1, identifyMs: 1, indexMs: 0 },
    stopped: false,
    track: null,
  stack: null,
  };
}

/** Pixel of a sky position from the conventions in docs/conventions.md, without quaternions. */
function project(pointing: { raDeg: number; decDeg: number; rollDeg: number }, raDeg: number, decDeg: number): [number, number] {
  const [a, d, r] = [pointing.raDeg * DEG, pointing.decDeg * DEG, pointing.rollDeg * DEG];
  const boresight = [Math.cos(d) * Math.cos(a), Math.cos(d) * Math.sin(a), Math.sin(d)];
  const north = [-Math.sin(d) * Math.cos(a), -Math.sin(d) * Math.sin(a), Math.cos(d)];
  const east = [-Math.sin(a), Math.cos(a), 0];
  const up = north.map((n, i) => n * Math.cos(r) + (east[i] ?? 0) * Math.sin(r));
  const yAxis = up.map((v) => -v);
  const xAxis = [
    (yAxis[1] ?? 0) * (boresight[2] ?? 0) - (yAxis[2] ?? 0) * (boresight[1] ?? 0),
    (yAxis[2] ?? 0) * (boresight[0] ?? 0) - (yAxis[0] ?? 0) * (boresight[2] ?? 0),
    (yAxis[0] ?? 0) * (boresight[1] ?? 0) - (yAxis[1] ?? 0) * (boresight[0] ?? 0),
  ];
  const [ra, dec] = [raDeg * DEG, decDeg * DEG];
  const star = [Math.cos(dec) * Math.cos(ra), Math.cos(dec) * Math.sin(ra), Math.sin(dec)];
  const dot = (axis: number[]) => axis.reduce((sum, v, i) => sum + v * (star[i] ?? 0), 0);
  return [(intrinsics.fx * dot(xAxis)) / dot(boresight) + intrinsics.cx, (intrinsics.fy * dot(yAxis)) / dot(boresight) + intrinsics.cy];
}

function parse(cards: string[]): Map<string, number> {
  return new Map(
    cards.map((c) => [c.slice(0, 8).trim(), Number(c.slice(10, 30))] as const).filter(([, v]) => Number.isFinite(v)),
  );
}

describe('wcsCards', () => {
  it.each([
    { raDeg: 83.8, decDeg: -5.4, rollDeg: 0 },
    { raDeg: 200, decDeg: 61, rollDeg: 37 },
    { raDeg: 359.5, decDeg: -70, rollDeg: 215 },
  ])('agrees with the camera model at $raDeg, $decDeg, roll $rollDeg', (pointing) => {
    const cards = wcsCards(frame(pointing));
    expect(cards).not.toBeNull();
    expect(cards?.every((c) => c.length === 80)).toBe(true);
    const k = parse(cards ?? []);
    const wcs = {
      crval: [k.get('CRVAL1') ?? NaN, k.get('CRVAL2') ?? NaN] as [number, number],
      crpix: [k.get('CRPIX1') ?? NaN, k.get('CRPIX2') ?? NaN] as [number, number],
      cd: [k.get('CD1_1') ?? NaN, k.get('CD1_2') ?? NaN, k.get('CD2_1') ?? NaN, k.get('CD2_2') ?? NaN] as [number, number, number, number],
    };
    for (const [dRa, dDec] of [[0, 0], [2.5, 1], [-3, -2], [1, -3.5]] as const) {
      const ra = pointing.raDeg + dRa / Math.cos(pointing.decDeg * DEG);
      const dec = pointing.decDeg + dDec;
      const [x, y] = project(pointing, ra, dec);
      const [raBack, decBack] = wcsPixelToSky(wcs, x, y);
      expect(((raBack - ra + 540) % 360) - 180).toBeCloseTo(0, 8);
      expect(decBack).toBeCloseTo(dec, 8);
    }
  });

  it('is absent without a solution', () => {
    expect(wcsCards(frame(null))).toBeNull();
    expect(wcsText(frame(null))).toBeNull();
  });
});

describe('text exports', () => {
  const solved = frame({ raDeg: 88.8, decDeg: 7.4, rollDeg: 10 });

  it('writes JSON with names and convention', () => {
    const data = JSON.parse(solutionJson(solved, 'orion.jpg')) as {
      solved: boolean;
      quaternion: { convention: string };
      matches: { hip: number; name: string | null; x: number }[];
    };
    expect(data.solved).toBe(true);
    expect(data.quaternion.convention).toContain('scalar last');
    expect(data.matches).toEqual([expect.objectContaining({ hip: 27989, name: 'Betelgeuse', x: 600.25 })]);
    expect((JSON.parse(solutionJson(frame(null), null)) as { solved: boolean }).solved).toBe(false);
  });

  it('writes CSV rows', () => {
    expect(matchesCsv(solved).trim().split('\n')).toEqual([
      'hip,name,magnitude,x,y,predicted_x,predicted_y,residual_arcsec',
      '27989,Betelgeuse,0.450,600.250,300.500,600.200,300.600,1.50',
    ]);
  });
});

describe('frameFits', () => {
  it('writes a block-aligned unsigned 16-bit image, bottom row first', () => {
    const source = frame({ raDeg: 10, decDeg: 20, rollDeg: 30 });
    source.pixels[0] = 65535;
    source.pixels[1024 * 767] = 7;
    const bytes = frameFits(source);
    expect(bytes.length % 2880).toBe(0);
    const header = new TextDecoder().decode(bytes.subarray(0, 2880));
    expect(header.startsWith('SIMPLE  =                    T')).toBe(true);
    expect(header).toContain('NAXIS1  =                 1024');
    expect(header).toContain("CTYPE1  =           'RA---TAN'");
    const dataStart = header.includes('END     ') ? 2880 : NaN;
    const view = new DataView(bytes.buffer);
    // First stored row is the bottom image row.
    expect(view.getInt16(dataStart, false) + 32768).toBe(7);
    expect(view.getInt16(dataStart + 1024 * 767 * 2, false) + 32768).toBe(65535);
  });
});
