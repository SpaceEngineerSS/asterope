// Flat-buffer layouts shared with crates/asterope-wasm/src/engine.rs.

import type {
  Attitude,
  CentroidRow,
  Blinder,
  DesignReport,
  DesignSpec,
  MissionReport,
  MissionSample,
  MissionSpec,
  DetectResult,
  Detection,
  FieldStar,
  IndexInfo,
  RejectReason,
  Rejected,
  SearchProgress,
  SensorParams,
  SolveResult,
  SolverParams,
  StackInfo,
  StarMatch,
  TrackEvent,
  TrialClass,
  TrialOutcome,
  TruthStar,
  WahbaResult,
} from './types';

const DETECT_HEADER = 10;
const DETECTION_FIELDS = 9;
const REJECTED_FIELDS = 4;
const SOLUTION_HEADER = 33;
const MATCH_FIELDS = 7;
const FIELD_STAR_FIELDS = 4;
const TRUTH_FIELDS = 5;
export const TRIAL_FIELDS = 12;

const REJECT_REASONS: readonly RejectReason[] = ['hot', 'cosmic', 'large', 'edge'];
const TRIAL_CLASSES: readonly TrialClass[] = ['success', 'inaccurate', 'falsePositive', 'noSolution'];

type Buffer = ArrayLike<number>;

function at(buffer: Buffer, index: number): number {
  const value = buffer[index];
  if (value === undefined) {
    throw new RangeError(`engine buffer is shorter than expected (index ${index})`);
  }
  return value;
}

function finiteOrNull(value: number): number | null {
  return Number.isFinite(value) ? value : null;
}

export function encodeSensor(s: SensorParams): Float64Array {
  return Float64Array.of(
    s.zeroPoint,
    s.exposure,
    s.psfSigma,
    s.skyBackground,
    s.darkCurrent,
    s.readNoise,
    s.gain,
    s.bias,
    s.fullWell,
    s.bitDepth,
    s.hotPixelFraction,
    s.hotPixelRate,
    s.cosmicRayMean,
    s.strayIntensity,
    s.strayAzimuthDeg,
    s.strayFalloff,
    s.noise ? 1 : 0,
    s.hotPixelSeed,
    s.rateXDeg,
    s.rateYDeg,
    s.rateZDeg,
    s.colorTerm,
    s.vignetting,
    s.prnu,
    s.dsnu,
    s.columnNoise,
    s.psfGrowth,
    s.coma,
    s.rowTimeUs,
    s.nonlinearity,
    s.blooming,
  );
}

export function decodeSensor(b: Buffer, offset = 0): SensorParams {
  return {
    zeroPoint: at(b, offset),
    exposure: at(b, offset + 1),
    psfSigma: at(b, offset + 2),
    skyBackground: at(b, offset + 3),
    darkCurrent: at(b, offset + 4),
    readNoise: at(b, offset + 5),
    gain: at(b, offset + 6),
    bias: at(b, offset + 7),
    fullWell: at(b, offset + 8),
    bitDepth: at(b, offset + 9),
    hotPixelFraction: at(b, offset + 10),
    hotPixelRate: at(b, offset + 11),
    cosmicRayMean: at(b, offset + 12),
    strayIntensity: at(b, offset + 13),
    strayAzimuthDeg: at(b, offset + 14),
    strayFalloff: at(b, offset + 15),
    noise: at(b, offset + 16) !== 0,
    hotPixelSeed: at(b, offset + 17),
    rateXDeg: at(b, offset + 18),
    rateYDeg: at(b, offset + 19),
    rateZDeg: at(b, offset + 20),
    colorTerm: at(b, offset + 21),
    vignetting: at(b, offset + 22),
    prnu: at(b, offset + 23),
    dsnu: at(b, offset + 24),
    columnNoise: at(b, offset + 25),
    psfGrowth: at(b, offset + 26),
    coma: at(b, offset + 27),
    rowTimeUs: at(b, offset + 28),
    nonlinearity: at(b, offset + 29),
    blooming: at(b, offset + 30),
  };
}

export function encodeSolver(s: SolverParams): Float64Array {
  return Float64Array.of(
    s.cellSize,
    s.segmentSigma,
    s.peakSigma,
    s.minArea,
    s.maxArea,
    s.maxSharpness,
    s.maxElongation,
    s.saturation,
    s.method === 'cog' ? 0 : 1,
    s.maxDetections,
    s.tolerancePx,
    s.matchRadiusPx,
    s.patternStars,
    s.maxTriads,
    s.minMatches,
    s.maxChanceProbability,
    s.weighted ? 1 : 0,
    s.trails ? 1 : 0,
  );
}

export function decodeSolver(b: Buffer): SolverParams {
  return {
    cellSize: at(b, 0),
    segmentSigma: at(b, 1),
    peakSigma: at(b, 2),
    minArea: at(b, 3),
    maxArea: at(b, 4),
    maxSharpness: at(b, 5),
    maxElongation: at(b, 6),
    saturation: at(b, 7),
    method: at(b, 8) === 0 ? 'cog' : 'iwc',
    maxDetections: at(b, 9),
    tolerancePx: at(b, 10),
    matchRadiusPx: at(b, 11),
    patternStars: at(b, 12),
    maxTriads: at(b, 13),
    minMatches: at(b, 14),
    maxChanceProbability: at(b, 15),
    weighted: at(b, 16) !== 0,
    trails: at(b, 17) !== 0,
  };
}

export function decodeIndexInfo(b: Buffer): IndexInfo {
  return {
    stars: at(b, 0),
    pairs: at(b, 1),
    magLimit: at(b, 2),
    fovXDeg: at(b, 3),
    fovYDeg: at(b, 4),
    pixelScaleArcsec: at(b, 5),
    rebuilt: at(b, 6) !== 0,
  };
}

export function decodeDetect(b: Buffer): DetectResult {
  const detectionCount = at(b, 0);
  const rejectedCount = at(b, 1);
  const detections: Detection[] = [];
  for (let i = 0; i < detectionCount; i += 1) {
    const o = DETECT_HEADER + i * DETECTION_FIELDS;
    detections.push({
      x: at(b, o),
      y: at(b, o + 1),
      flux: at(b, o + 2),
      peak: at(b, o + 3),
      snr: at(b, o + 4),
      area: at(b, o + 5),
      saturated: at(b, o + 6) !== 0,
      sigma: at(b, o + 7),
      stellarity: at(b, o + 8),
    });
  }
  const rejected: Rejected[] = [];
  const rejectedStart = DETECT_HEADER + detectionCount * DETECTION_FIELDS;
  for (let i = 0; i < rejectedCount; i += 1) {
    const o = rejectedStart + i * REJECTED_FIELDS;
    rejected.push({
      x: at(b, o),
      y: at(b, o + 1),
      area: at(b, o + 2),
      reason: REJECT_REASONS[at(b, o + 3)] ?? 'hot',
    });
  }
  return {
    detections,
    rejected,
    background: at(b, 2),
    noise: at(b, 3),
    blobs: at(b, 4),
    noiseBlobs: at(b, 5),
    hotPixels: at(b, 6),
    cosmicRays: at(b, 7),
    tooLarge: at(b, 8),
    edge: at(b, 9),
  };
}

/** Layout of `Engine::step_search`; the first element is the done flag. */
export function decodeProgress(b: Buffer): { done: boolean; progress: SearchProgress } {
  return {
    done: at(b, 0) !== 0,
    progress: {
      rung: at(b, 1),
      rungs: at(b, 2),
      rungFovDeg: at(b, 3),
      scalesTried: at(b, 4),
      scalesTotal: at(b, 5),
      pyramids: at(b, 6),
      triads: at(b, 7),
    },
  };
}

export function decodeSolve(b: Buffer): SolveResult {
  const result: SolveResult = {
    attitude: null,
    triads: at(b, 1),
    pyramids: at(b, 2),
    scalesTried: at(b, 3),
    refined: at(b, 4) !== 0,
    intrinsics: {
      fx: at(b, 24),
      fy: at(b, 25),
      cx: at(b, 26),
      cy: at(b, 27),
      k1: at(b, 28),
      fovXDeg: at(b, 29),
    equidistant: at(b, 32) === 1,
    },
  };
  if (at(b, 0) === 0) {
    return result;
  }
  const matchCount = at(b, 23);
  const matches: StarMatch[] = [];
  for (let i = 0; i < matchCount; i += 1) {
    const o = SOLUTION_HEADER + i * MATCH_FIELDS;
    matches.push({
      detection: at(b, o),
      hip: at(b, o + 1),
      mag: at(b, o + 2),
      residualArcsec: at(b, o + 3),
      predictedX: at(b, o + 4),
      predictedY: at(b, o + 5),
      catalogIndex: at(b, o + 6),
    });
  }
  const sigma = [finiteOrNull(at(b, 16)), finiteOrNull(at(b, 17)), finiteOrNull(at(b, 18))] as const;
  const attitude: Attitude = {
    q: [at(b, 5), at(b, 6), at(b, 7), at(b, 8)],
    pointing: { raDeg: at(b, 9), decDeg: at(b, 10), rollDeg: at(b, 11) },
    rmsArcsec: at(b, 12),
    lambdaMax: at(b, 13),
    questQMethodRad: at(b, 14),
    questSvdRad: at(b, 30),
    chanceProbability: at(b, 15),
    sigmaArcsec:
      sigma[0] !== null && sigma[1] !== null && sigma[2] !== null
        ? [sigma[0], sigma[1], sigma[2]]
        : null,
    pyramid: Number.isFinite(at(b, 19)) ? [at(b, 19), at(b, 20), at(b, 21), at(b, 22)] : null,
    matches,
  };
  return { ...result, attitude };
}

const TRACK_EVENTS: readonly TrackEvent[] = ['acquired', 'tracked', 'coasting', 'lost'];

/** Tracking event stored in header slot 31 by `Engine::track_step`. */
const DESIGN_HEADER = 20;

const MISSION_HEADER = 8;
const MISSION_FIELDS = 9;
const BLINDERS: readonly Blinder[] = ['none', 'sun', 'earth', 'moon'];

export function encodeMission(s: MissionSpec): Float64Array {
  return Float64Array.of(
    s.altitudeKm,
    s.inclinationDeg,
    s.nodeDeg,
    s.julianDate,
    ...s.boresight,
    ...s.spinDeg,
    s.sunExclusionDeg,
    s.earthLimbExclusionDeg,
    s.moonExclusionDeg,
    s.orbits,
    s.stepSeconds,
  );
}

export function decodeMission(b: Buffer): MissionReport {
  const count = at(b, 1);
  if (b.length !== MISSION_HEADER + count * MISSION_FIELDS) {
    throw new RangeError('mission buffer has the wrong length');
  }
  const samples: MissionSample[] = [];
  for (let i = 0; i < count; i += 1) {
    const o = MISSION_HEADER + i * MISSION_FIELDS;
    samples.push({
      seconds: at(b, o),
      sunDeg: at(b, o + 1),
      earthLimbDeg: at(b, o + 2),
      moonDeg: at(b, o + 3),
      eclipse: at(b, o + 4) !== 0,
      blinder: BLINDERS[at(b, o + 5)] ?? 'none',
      pointing: { raDeg: at(b, o + 6), decDeg: at(b, o + 7), rollDeg: at(b, o + 8) },
    });
  }
  return {
    periodSeconds: at(b, 0),
    blindedBySun: at(b, 2),
    blindedByEarth: at(b, 3),
    blindedByMoon: at(b, 4),
    inEclipse: at(b, 5),
    longestBlindSeconds: at(b, 6),
    bodyRateDeg: at(b, 7),
    samples,
  };
}

export function encodeDesign(s: DesignSpec): Float64Array {
  return Float64Array.of(
    s.focalLengthMm,
    s.apertureMm,
    s.pixelUm,
    s.width,
    s.height,
    s.transmission,
    s.quantumEfficiency,
    s.bandwidthNm,
    s.exposure,
    s.psfSigma,
    s.readNoise,
    s.darkCurrent,
    s.skyBackground,
    s.rateDeg,
    s.peakSigma,
  );
}

export function decodeDesign(b: Buffer): DesignReport {
  return {
    fovXDeg: at(b, 0),
    fovYDeg: at(b, 1),
    pixelScaleArcsec: at(b, 2),
    zeroPoint: at(b, 3),
    noise: at(b, 4),
    trailPx: at(b, 5),
    limitingMagnitude: at(b, 6),
    catalogLimited: at(b, 7) !== 0,
    starsPerField: at(b, 8),
    sigmaCrossArcsec: at(b, 9),
    sigmaRollArcsec: at(b, 10),
    medianCrossArcsec: at(b, 11),
    medianRollArcsec: at(b, 12),
    streakRateDeg: at(b, 13),
    coverage: [at(b, 14), at(b, 15), at(b, 16)],
    fewestStars: at(b, 17),
    medianStars: at(b, 18),
    focalRatio: at(b, 19),
    counts: Uint16Array.from(Array.from(b).slice(DESIGN_HEADER)),
  };
}

export function decodeStack(b: Buffer): StackInfo {
  return { frames: at(b, 0), pairs: at(b, 1), rmsPx: at(b, 2), shiftPx: at(b, 3), scale: at(b, 4) };
}

export function decodeTrackEvent(b: Buffer): TrackEvent {
  return TRACK_EVENTS[at(b, 31)] ?? 'lost';
}

export function decodeTruth(b: Buffer): TruthStar[] {
  const stars: TruthStar[] = [];
  for (let o = 0; o + TRUTH_FIELDS <= b.length; o += TRUTH_FIELDS) {
    stars.push({ hip: at(b, o), x: at(b, o + 1), y: at(b, o + 2), mag: at(b, o + 3), electrons: at(b, o + 4) });
  }
  return stars;
}

export function decodeField(b: Buffer): FieldStar[] {
  const stars: FieldStar[] = [];
  for (let o = 0; o + FIELD_STAR_FIELDS <= b.length; o += FIELD_STAR_FIELDS) {
    stars.push({ hip: at(b, o), x: at(b, o + 1), y: at(b, o + 2), mag: at(b, o + 3) });
  }
  return stars;
}

export function decodeTrials(b: Buffer, firstTrial: number): TrialOutcome[] {
  const outcomes: TrialOutcome[] = [];
  for (let o = 0; o + TRIAL_FIELDS <= b.length; o += TRIAL_FIELDS) {
    outcomes.push({
      trial: firstTrial + o / TRIAL_FIELDS,
      class: TRIAL_CLASSES[at(b, o)] ?? 'noSolution',
      crossArcsec: at(b, o + 1),
      rollArcsec: at(b, o + 2),
      totalArcsec: at(b, o + 3),
      truthStars: at(b, o + 4),
      detections: at(b, o + 5),
      rejected: at(b, o + 6),
      matches: at(b, o + 7),
      triads: at(b, o + 8),
      pointing: { raDeg: at(b, o + 9), decDeg: at(b, o + 10), rollDeg: at(b, o + 11) },
    });
  }
  return outcomes;
}

export function decodeCentroidRow(b: Buffer, psfSigma: number, peakSnr: number | null): CentroidRow {
  return {
    psfSigma,
    peakSnr,
    samples: at(b, 0),
    missed: at(b, 1),
    meanError: at(b, 2),
    maxError: at(b, 3),
    electrons: at(b, 4),
    bound: at(b, 5),
  };
}

export function decodeWahba(b: Buffer): WahbaResult {
  return { cases: at(b, 0), worstQuestRad: at(b, 1), worstQMethodRad: at(b, 2), failures: at(b, 3) };
}
