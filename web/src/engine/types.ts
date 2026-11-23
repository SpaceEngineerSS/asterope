export interface Pointing {
  raDeg: number;
  decDeg: number;
  rollDeg: number;
}

export interface CameraSpec {
  width: number;
  height: number;
  fovDeg: number;
  /** Brown-Conrady coefficients k1, k2, k3, p1, p2. */
  distortion: readonly [number, number, number, number, number];
  /** Half-width of the focal-length search, as a fraction. 0 disables it. */
  scaleSpan: number;
  /** Propagate catalog proper motion to the current epoch (real photographs). */
  currentEpoch: boolean;
  /** An equidistant (wide-angle) lens instead of a pinhole. */
  equidistant?: boolean;
}

export interface SensorParams {
  zeroPoint: number;
  exposure: number;
  psfSigma: number;
  skyBackground: number;
  darkCurrent: number;
  readNoise: number;
  gain: number;
  bias: number;
  fullWell: number;
  bitDepth: number;
  hotPixelFraction: number;
  hotPixelRate: number;
  cosmicRayMean: number;
  strayIntensity: number;
  strayAzimuthDeg: number;
  strayFalloff: number;
  noise: boolean;
  hotPixelSeed: number;
  /** Angular rate about the sensor x, y and boresight axes, degrees per second. */
  rateXDeg: number;
  rateYDeg: number;
  rateZDeg: number;
  /** Instrumental magnitude = V + colorTerm * (B - V). */
  colorTerm: number;
  /** Share of the cos^4 illumination falloff present, 0 to 1. */
  vignetting: number;
  /** Fixed gain pattern, fractional RMS. */
  prnu: number;
  /** Fixed dark-current pattern, electrons per second RMS. */
  dsnu: number;
  /** Per-column offset drawn each frame, electrons RMS. */
  columnNoise: number;
  /** PSF width at the corner is 1 + psfGrowth times the central one. */
  psfGrowth: number;
  /** Share of a corner star's light in an outward flare. */
  coma: number;
  /** Rolling-shutter row time, microseconds; 0 for a global shutter. */
  rowTimeUs: number;
  /** Shortfall of the response at full well, as a fraction. */
  nonlinearity: number;
  /** Share of the charge above full well that spills along the column. */
  blooming: number;
}

export type CentroidMethod = 'cog' | 'iwc';

export interface SolverParams {
  cellSize: number;
  segmentSigma: number;
  peakSigma: number;
  minArea: number;
  maxArea: number;
  maxSharpness: number;
  maxElongation: number;
  saturation: number;
  method: CentroidMethod;
  maxDetections: number;
  tolerancePx: number;
  matchRadiusPx: number;
  patternStars: number;
  maxTriads: number;
  minMatches: number;
  maxChanceProbability: number;
  /** Weigh stars by centroid accuracy in the attitude fit. */
  weighted: boolean;
  /** Keep stars drawn into trails by rotation instead of rejecting them. */
  trails: boolean;
}

export interface IndexInfo {
  stars: number;
  pairs: number;
  magLimit: number;
  fovXDeg: number;
  fovYDeg: number;
  pixelScaleArcsec: number;
  rebuilt: boolean;
}

export interface Detection {
  x: number;
  y: number;
  flux: number;
  peak: number;
  snr: number;
  area: number;
  saturated: boolean;
  sigma: number;
  /** 0 to 1; how star-like the blob is. */
  stellarity: number;
}

/** State of a running identification. */
export interface SearchProgress {
  /** 1-based index of the field-of-view rung being searched. */
  rung: number;
  rungs: number;
  rungFovDeg: number;
  scalesTried: number;
  scalesTotal: number;
  pyramids: number;
  triads: number;
}

export type RejectReason = 'hot' | 'cosmic' | 'large' | 'edge';

export interface Rejected {
  x: number;
  y: number;
  area: number;
  reason: RejectReason;
}

export interface DetectResult {
  detections: Detection[];
  rejected: Rejected[];
  background: number;
  noise: number;
  blobs: number;
  noiseBlobs: number;
  hotPixels: number;
  cosmicRays: number;
  tooLarge: number;
  edge: number;
}

export interface StarMatch {
  detection: number;
  hip: number;
  mag: number;
  residualArcsec: number;
  predictedX: number;
  predictedY: number;
  catalogIndex: number;
}

export type Quaternion = readonly [number, number, number, number];

export interface Intrinsics {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  k1: number;
  fovXDeg: number;
  /** The lens maps the angle from the axis, not its tangent, to distance. */
  equidistant: boolean;
}

export interface Attitude {
  q: Quaternion;
  pointing: Pointing;
  rmsArcsec: number;
  lambdaMax: number;
  questQMethodRad: number;
  questSvdRad: number;
  chanceProbability: number;
  /** One-sigma uncertainty about the sensor x, y and boresight axes, arcseconds. */
  sigmaArcsec: readonly [number, number, number] | null;
  /** Detection indices of the confirming pyramid; null for a tracked solution. */
  pyramid: readonly [number, number, number, number] | null;
  matches: StarMatch[];
}

/** How a frame of a sequence was solved. */
export type TrackEvent = 'acquired' | 'tracked' | 'coasting' | 'lost';

export interface SolveResult {
  attitude: Attitude | null;
  triads: number;
  pyramids: number;
  scalesTried: number;
  refined: boolean;
  intrinsics: Intrinsics;
}

export interface TruthStar {
  hip: number;
  x: number;
  y: number;
  mag: number;
  electrons: number;
}

export interface FieldStar {
  hip: number;
  x: number;
  y: number;
  mag: number;
}

export interface AttitudeError {
  crossArcsec: number;
  rollArcsec: number;
  totalArcsec: number;
}

export interface Timing {
  renderMs: number;
  detectMs: number;
  identifyMs: number;
  indexMs: number;
}

/** How a video frame was aligned with the frames before it. */
export interface StackInfo {
  /** Frames averaged into the stored frame. */
  frames: number;
  /** Stars paired with the previous frame; 0 when registration failed. */
  pairs: number;
  rmsPx: number;
  /** Displacement of the frame center since the previous frame, pixels. */
  shiftPx: number;
  /** The frame's pixel values are this many times the mean of the stacked frames. */
  scale: number;
}

export interface FrameResult {
  width: number;
  height: number;
  pixels: Uint16Array;
  index: IndexInfo;
  detect: DetectResult;
  solve: SolveResult;
  /** Catalog stars predicted inside the frame for the solved (or true) attitude. */
  field: FieldStar[];
  truth: TruthStar[];
  truthPointing: Pointing | null;
  truthQ: Quaternion | null;
  error: AttitudeError | null;
  timing: Timing;
  /** The search was stopped before it finished. */
  stopped: boolean;
  /** Set for frames solved as part of a sequence. */
  track: TrackEvent | null;
  /** Set for video frames. */
  stack: StackInfo | null;
}

/** A camera described by its lens, detector and exposure. */
export interface DesignSpec {
  focalLengthMm: number;
  apertureMm: number;
  pixelUm: number;
  width: number;
  height: number;
  transmission: number;
  quantumEfficiency: number;
  bandwidthNm: number;
  exposure: number;
  psfSigma: number;
  readNoise: number;
  darkCurrent: number;
  skyBackground: number;
  /** Rotation across the boresight during the exposure, degrees per second. */
  rateDeg: number;
  peakSigma: number;
}

/** A camera carried round a circular orbit. */
export interface MissionSpec {
  altitudeKm: number;
  inclinationDeg: number;
  nodeDeg: number;
  /** Julian date, terrestrial time, of the start. */
  julianDate: number;
  /** Boresight in the orbit's frame: along track, orbit normal, up. */
  boresight: [number, number, number];
  /** Tumbling rate about the sensor axes, degrees per second; zeros keep the camera fixed to the orbit. */
  spinDeg: [number, number, number];
  sunExclusionDeg: number;
  earthLimbExclusionDeg: number;
  moonExclusionDeg: number;
  orbits: number;
  stepSeconds: number;
}

export type Blinder = 'none' | 'sun' | 'earth' | 'moon';

export interface MissionSample {
  seconds: number;
  sunDeg: number;
  earthLimbDeg: number;
  moonDeg: number;
  eclipse: boolean;
  blinder: Blinder;
  pointing: Pointing;
}

export interface MissionReport {
  periodSeconds: number;
  /** Shares of the samples, 0 to 1. */
  blindedBySun: number;
  blindedByEarth: number;
  blindedByMoon: number;
  inEclipse: number;
  longestBlindSeconds: number;
  bodyRateDeg: number;
  samples: MissionSample[];
}

export const COVERAGE_COLUMNS = 72;
export const COVERAGE_ROWS = 36;

/** What the engine predicts for a design. */
export interface DesignReport {
  fovXDeg: number;
  fovYDeg: number;
  pixelScaleArcsec: number;
  /** Electrons per second from a V = 0 star. */
  zeroPoint: number;
  noise: number;
  trailPx: number;
  limitingMagnitude: number;
  /** The catalog ends before the limiting magnitude. */
  catalogLimited: boolean;
  starsPerField: number;
  sigmaCrossArcsec: number;
  sigmaRollArcsec: number;
  medianCrossArcsec: number;
  medianRollArcsec: number;
  streakRateDeg: number;
  /** Sky fraction with at least 4, 5 and 8 detectable stars in the field. */
  coverage: readonly [number, number, number];
  fewestStars: number;
  medianStars: number;
  focalRatio: number;
  /** Stars per field on a 72 x 36 grid, declination +90 first, right ascension 0 to 360. */
  counts: Uint16Array;
}

export type TrialClass = 'success' | 'inaccurate' | 'falsePositive' | 'noSolution';

export interface TrialOutcome {
  trial: number;
  class: TrialClass;
  crossArcsec: number;
  rollArcsec: number;
  totalArcsec: number;
  truthStars: number;
  detections: number;
  rejected: number;
  matches: number;
  triads: number;
  pointing: Pointing;
}

/** Sensor quantities a stress sweep can vary; the order matches `StressParameter::ALL`. */
export const STRESS_PARAMETERS = [
  { id: 'readNoise', label: 'Read noise', unit: 'e⁻' },
  { id: 'strayLight', label: 'Stray light', unit: 'e⁻/px/s' },
  { id: 'hotPixels', label: 'Hot pixels', unit: '%' },
  { id: 'cosmicRays', label: 'Cosmic rays', unit: 'per frame' },
  { id: 'psfSigma', label: 'PSF σ', unit: 'px' },
  { id: 'exposure', label: 'Exposure', unit: 's' },
  { id: 'bodyRate', label: 'Body rate', unit: '°/s' },
] as const;

/** Outcome of all trials at one value of the swept parameter. */
export interface StressStep {
  value: number;
  trials: number;
  success: number;
  falsePositive: number;
  noSolution: number;
  inaccurate: number;
  /** Median cross-boresight error of solved trials, arcseconds (NaN when none solved). */
  crossMedianArcsec: number;
}

export interface CentroidRow {
  psfSigma: number;
  /** null for the noiseless case. */
  peakSnr: number | null;
  samples: number;
  missed: number;
  meanError: number;
  maxError: number;
  electrons: number;
  bound: number;
}

export interface WahbaResult {
  cases: number;
  worstQuestRad: number;
  worstQMethodRad: number;
  failures: number;
}
