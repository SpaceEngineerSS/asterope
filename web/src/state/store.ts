import { useSyncExternalStore } from 'react';

import { EngineClient, LatestWins } from '../engine/client';
import type { JobResponse } from '../engine/client';
import type { Job } from '../engine/protocol';
import type {
  CameraSpec,
  CentroidRow,
  FrameResult,
  Pointing,
  SearchProgress,
  SensorParams,
  SolverParams,
  StressStep,
  TrialOutcome,
  WahbaResult,
} from '../engine/types';
import { decodeRaster, fromFits, isFits } from '../image/decode';
import { mirrorRows } from '../lib/mirror';
import { decodeShare, encodeShare } from '../lib/share';
import type { SharedBench } from '../lib/share';
import type { Stretch } from '../lib/stretch';
import { FrameRecorder } from '../video/record';

export type Tab = 'synthetic' | 'image' | 'video' | 'design' | 'mission' | 'validation' | 'manual';
export type Bench = 'synthetic' | 'image' | 'video';

export interface OverlayOptions {
  centroids: boolean;
  matches: boolean;
  labels: boolean;
  rejected: boolean;
  pyramid: boolean;
  residuals: boolean;
  catalog: boolean;
  graticule: boolean;
  reticle: boolean;
  residualScale: number;
}

export interface SyntheticState {
  camera: CameraSpec;
  pointing: Pointing;
  sensor: SensorParams;
  solver: SolverParams;
  seed: number;
  frame: FrameResult | null;
  busy: boolean;
  error: string | null;
  /** Set while the bench shows a replayed Monte Carlo trial. */
  replay: { masterSeed: number; trial: number } | null;
  sequence: SequenceState;
}

/** A run of consecutive frames solved by the tracker while the camera rotates at the body rate. */
export interface SequenceState {
  running: boolean;
  /** Seconds between frames. */
  framePeriod: number;
  frames: number;
  tracked: number;
  searches: number;
  coasting: number;
  lost: number;
  /** Frames needed for the first solution, or null before it. */
  firstFix: number | null;
  /** The running sequence is being recorded as video. */
  recording: boolean;
  recorded: Recording | null;
}

/** A sequence saved as WebM, playable on the video bench. */
export interface Recording {
  blob: Blob;
  frames: number;
  fovDeg: number;
}

export interface ImageState {
  camera: CameraSpec;
  solver: SolverParams;
  frame: FrameResult | null;
  busy: boolean;
  error: string | null;
  fileName: string | null;
  binning: number;
  fovSource: 'manual' | 'exif';
  /** Pair tolerance and match radius follow the field of view until edited by hand. */
  autoTolerance: boolean;
  /** The entered field of view is only a hint; a ladder of fields is searched. */
  unknownFov: boolean;
  /** The image is flipped left to right before it is solved and shown. */
  mirrored: boolean;
  progress: SearchProgress | null;
}

/**
 * Matching tolerances for a photograph. Lens distortion grows roughly with
 * the square of the field angle, so wide fields need a wider window; the
 * floor covers centroid error on compressed images.
 */
export function autoTolerances(camera: CameraSpec): Pick<SolverParams, 'tolerancePx' | 'matchRadiusPx'> {
  const halfField = Math.tan((camera.fovDeg * Math.PI) / 360);
  const longSide = Math.max(camera.width, camera.height);
  // An equidistant camera already follows a wide lens; see `auto_tolerances` in the core.
  const tolerancePx = camera.equidistant
    ? Math.min(6, Math.max(2, 0.003 * longSide))
    : Math.min(12, Math.max(2, 0.021 * halfField * halfField * longSide));
  const rounded = Math.round(tolerancePx * 10) / 10;
  return { tolerancePx: rounded, matchRadiusPx: Math.round(rounded * 14) / 10 };
}

export interface LisRun {
  running: boolean;
  jobId: number | null;
  masterSeed: number;
  trials: number;
  outcomes: TrialOutcome[];
  startedAt: number;
  elapsedMs: number;
  error: string | null;
}

export interface ValidationState {
  centroid: { running: boolean; rows: CentroidRow[]; total: number; error: string | null };
  wahba: { running: boolean; result: WahbaResult | null; error: string | null };
  lis: LisRun;
  stress: StressRun;
}

export interface StressRun {
  running: boolean;
  jobId: number | null;
  /** Index into `STRESS_PARAMETERS`. */
  parameter: number;
  trialsPerStep: number;
  steps: StressStep[];
  total: number;
  error: string | null;
}

export interface AppState {
  ready: boolean;
  fatal: string | null;
  catalogSize: number;
  tab: Tab;
  overlay: Record<Bench, OverlayOptions>;
  stretch: Stretch;
  synthetic: SyntheticState;
  image: ImageState;
  validation: ValidationState;
}

export const VALIDATION_CAMERA: CameraSpec = {
  width: 1024,
  height: 1024,
  fovDeg: 14,
  distortion: [0, 0, 0, 0, 0],
  scaleSpan: 0,
  currentEpoch: false,
};

export const CENTROID_SIGMAS = [0.8, 1.0, 1.5, 2.0] as const;
export const CENTROID_SNRS = [null, 30, 60] as const;
export const WAHBA_CASES = 10_000;

const PLACEHOLDER_SENSOR: SensorParams = {
  zeroPoint: 0,
  exposure: 1,
  psfSigma: 1,
  skyBackground: 0,
  darkCurrent: 0,
  readNoise: 0,
  gain: 1,
  bias: 0,
  fullWell: 1,
  bitDepth: 12,
  hotPixelFraction: 0,
  hotPixelRate: 0,
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
};

const PLACEHOLDER_SOLVER: SolverParams = {
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
};

let state: AppState = {
  ready: false,
  fatal: null,
  catalogSize: 0,
  tab: 'synthetic',
  overlay: {
    synthetic: {
      centroids: true,
      matches: true,
      labels: true,
      rejected: true,
      pyramid: true,
      residuals: true,
      catalog: false,
      graticule: true,
      reticle: true,
      residualScale: 50,
    },
    // A photograph holds thousands of rejected blobs and pixel-scale residuals.
    image: {
      centroids: true,
      matches: true,
      labels: true,
      rejected: false,
      pyramid: true,
      residuals: true,
      catalog: false,
      graticule: true,
      reticle: false,
      residualScale: 5,
    },
    video: {
      centroids: true,
      matches: true,
      labels: true,
      rejected: false,
      pyramid: false,
      residuals: false,
      catalog: false,
      graticule: true,
      reticle: true,
      residualScale: 5,
    },
  },
  stretch: 'asinh',
  synthetic: {
    camera: { ...VALIDATION_CAMERA },
    pointing: { raDeg: 83.822, decDeg: -5.391, rollDeg: 12 },
    sensor: PLACEHOLDER_SENSOR,
    solver: PLACEHOLDER_SOLVER,
    seed: 42,
    frame: null,
    busy: false,
    error: null,
    replay: null,
    sequence: {
      running: false,
      framePeriod: 0.25,
      frames: 0,
      tracked: 0,
      searches: 0,
      coasting: 0,
      lost: 0,
      firstFix: null,
      recording: false,
      recorded: null,
    },
  },
  image: {
    camera: { width: 0, height: 0, fovDeg: 20, distortion: [0, 0, 0, 0, 0], scaleSpan: 0.1, currentEpoch: true },
    solver: PLACEHOLDER_SOLVER,
    frame: null,
    busy: false,
    error: null,
    fileName: null,
    binning: 1,
    fovSource: 'manual',
    autoTolerance: true,
    unknownFov: false,
    mirrored: false,
    progress: null,
  },
  validation: {
    centroid: { running: false, rows: [], total: 0, error: null },
    wahba: { running: false, result: null, error: null },
    lis: {
      running: false,
      jobId: null,
      masterSeed: 1,
      trials: 1000,
      outcomes: [],
      startedAt: 0,
      elapsedMs: 0,
      error: null,
    },
    stress: { running: false, jobId: null, parameter: 0, trialsPerStep: 100, steps: [], total: 10, error: null },
  },
};

const listeners = new Set<() => void>();

function update(patch: (current: AppState) => AppState): void {
  state = patch(state);
  listeners.forEach((listener) => {
    listener();
  });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useAppState(): AppState {
  return useSyncExternalStore(subscribe, () => state);
}

export function appState(): AppState {
  return state;
}

function currentEpochYear(): number {
  const now = new Date();
  const start = Date.UTC(now.getUTCFullYear(), 0, 1);
  const end = Date.UTC(now.getUTCFullYear() + 1, 0, 1);
  return now.getUTCFullYear() + (now.getTime() - start) / (end - start);
}

const client = new EngineClient(currentEpochYear());

/** Which bench last put a frame into the single engine context. */
let engineHolds: Tab | null = null;

/** The shared engine, for a bench that is about to load its own frame into it. */
export function claimEngine(tab: Tab): EngineClient {
  stopValidationRuns();
  engineHolds = tab;
  return client;
}

const syntheticChannel = new LatestWins(
  client,
  (response) => {
    engineHolds = 'synthetic';
    update((s) => {
      if (response.type === 'frame') {
        const { frame } = response;
        const sequence = s.synthetic.sequence;
        if (!frame.track || !sequence.running) {
          return { ...s, synthetic: { ...s.synthetic, frame, error: null } };
        }
        const frames = sequence.frames + 1;
        const solved = frame.solve.attitude !== null;
        return {
          ...s,
          synthetic: {
            ...s.synthetic,
            frame,
            error: null,
            pointing: frame.truthPointing ?? s.synthetic.pointing,
            sequence: {
              ...sequence,
              frames,
              tracked: sequence.tracked + (frame.track === 'tracked' ? 1 : 0),
              searches: sequence.searches + (frame.track === 'acquired' || frame.track === 'lost' ? 1 : 0),
              coasting: sequence.coasting + (frame.track === 'coasting' ? 1 : 0),
              lost: sequence.lost + (frame.track === 'lost' ? 1 : 0),
              firstFix: sequence.firstFix ?? (solved ? frames : null),
            },
          },
        };
      }
      if (response.type === 'replay') {
        const { frame, sensor } = response.replay;
        return {
          ...s,
          synthetic: {
            ...s.synthetic,
            frame,
            error: null,
            sensor: { ...sensor, hotPixelSeed: s.synthetic.sensor.hotPixelSeed },
            pointing: frame.truthPointing ?? s.synthetic.pointing,
          },
        };
      }
      if (response.type === 'error') {
        return {
          ...s,
          synthetic: {
            ...s.synthetic,
            error: response.message,
            sequence: { ...s.synthetic.sequence, running: false },
          },
        };
      }
      return s;
    });
    if (response.type === 'frame' && response.frame.track && state.synthetic.sequence.running) {
      const { frame } = response;
      let wait = 0;
      if (recorder?.matches(frame.width, frame.height)) {
        recorder.add(frame.pixels, 2 ** state.synthetic.sensor.bitDepth - 1);
        // Recorded frames are stamped with wall-clock time, so they are paced at the frame period.
        wait = Math.max(0, state.synthetic.sequence.framePeriod * 1000 - (performance.now() - sequencePushedAt));
      } else if (recorder) {
        stopSequence();
        return;
      }
      setTimeout(pushSequenceFrame, wait, false);
    }
  },
  () => {
    update((s) => ({ ...s, synthetic: { ...s.synthetic, busy: false } }));
  },
);

let recorder: FrameRecorder | null = null;
let sequencePushedAt = 0;

function pushSequenceFrame(reset: boolean): void {
  const { camera, pointing, sensor, solver, seed, sequence } = state.synthetic;
  if (!state.ready || !sequence.running) {
    return;
  }
  sequencePushedAt = performance.now();
  update((s) => ({ ...s, synthetic: { ...s.synthetic, busy: true, replay: null } }));
  syntheticChannel.push({
    kind: 'synthetic',
    camera,
    pointing,
    sensor,
    solver,
    seed: seed + sequence.frames,
    sequence: { dt: sequence.framePeriod, reset },
  });
}

/** Runs a sequence; with `record` its frames are also saved as a WebM video. */
export function startSequence(record = false): void {
  if (!state.ready) {
    return;
  }
  stopValidationRuns();
  if (record) {
    try {
      recorder = new FrameRecorder(state.synthetic.camera.width, state.synthetic.camera.height);
    } catch (error) {
      update((s) => ({ ...s, synthetic: { ...s.synthetic, error: describe(error) } }));
      return;
    }
  }
  update((s) => ({
    ...s,
    synthetic: {
      ...s.synthetic,
      sequence: {
        ...s.synthetic.sequence,
        running: true,
        frames: 0,
        tracked: 0,
        searches: 0,
        coasting: 0,
        lost: 0,
        firstFix: null,
        recording: record,
        recorded: record ? null : s.synthetic.sequence.recorded,
      },
    },
  }));
  pushSequenceFrame(true);
}

export function stopSequence(): void {
  const finished = recorder;
  recorder = null;
  update((s) => ({
    ...s,
    synthetic: { ...s.synthetic, sequence: { ...s.synthetic.sequence, running: false, recording: false } },
  }));
  if (finished && finished.frames > 0) {
    const fovDeg = state.synthetic.camera.fovDeg;
    void finished.finish().then((blob) => {
      update((s) => ({
        ...s,
        synthetic: {
          ...s.synthetic,
          sequence: { ...s.synthetic.sequence, recorded: { blob, frames: finished.frames, fovDeg } },
        },
      }));
    });
  }
}

export function setFramePeriod(framePeriod: number): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, sequence: { ...s.synthetic.sequence, framePeriod } } }));
}

const imageChannel = new LatestWins(
  client,
  (response) => {
    engineHolds = 'image';
    update((s) => {
      if (response.type === 'progress') {
        return { ...s, image: { ...s.image, progress: response.progress } };
      }
      if (response.type === 'frame') {
        // A ladder lock replaces the hint, so the next solve starts from the right field.
        const found = s.image.unknownFov && response.frame.solve.attitude !== null;
        const camera = found
          ? {
              ...s.image.camera,
              fovDeg: Math.round(response.frame.solve.intrinsics.fovXDeg * 100) / 100,
              equidistant: response.frame.solve.intrinsics.equidistant,
            }
          : s.image.camera;
        return { ...s, image: withAutoTolerance({ ...s.image, frame: response.frame, error: null, camera }) };
      }
      if (response.type === 'error') {
        return { ...s, image: { ...s.image, error: response.message } };
      }
      return s;
    });
  },
  () => {
    update((s) => ({ ...s, image: { ...s.image, busy: false, progress: null } }));
  },
);

function stopValidationRuns(): void {
  cancelLisJobs();
  const stress = state.validation.stress;
  if (stress.running && stress.jobId !== null) {
    client.cancel(stress.jobId);
  }
}

function requestSynthetic(): void {
  // A running sequence picks up changed settings on its next frame.
  if (!state.ready || state.synthetic.sequence.running) {
    return;
  }
  stopValidationRuns();
  const { camera, pointing, sensor, solver, seed } = state.synthetic;
  update((s) => ({ ...s, synthetic: { ...s.synthetic, busy: true, replay: null } }));
  syntheticChannel.push({ kind: 'synthetic', camera, pointing, sensor, solver, seed });
}

function requestImage(freshPixels?: Uint16Array): void {
  const { camera, solver, frame, unknownFov } = state.image;
  if (!state.ready || (frame === null && !freshPixels)) {
    return;
  }
  stopValidationRuns();
  const pixels = freshPixels ?? (engineHolds === 'image' ? undefined : frame?.pixels.slice());
  update((s) => ({ ...s, image: { ...s.image, busy: true, progress: null } }));
  const job: Job = pixels
    ? { kind: 'image', camera, solver, unknownFov, pixels }
    : { kind: 'image', camera, solver, unknownFov };
  imageChannel.push(job);
}

export function setUnknownFov(unknownFov: boolean): void {
  update((s) => ({ ...s, image: { ...s.image, unknownFov } }));
  requestImage();
}

/** Flips the image left to right, for optics that show the sky reversed. */
export function setImageMirrored(mirrored: boolean): void {
  const { frame, camera } = state.image;
  if (mirrored === state.image.mirrored) {
    return;
  }
  update((s) => ({ ...s, image: { ...s.image, mirrored } }));
  if (frame) {
    requestImage(mirrorRows(frame.pixels, camera.width));
  }
}

export function stopImageSearch(): void {
  imageChannel.stop();
}

/** Engine defaults; the validation suites always run with these so results are reproducible. */
let baseline: { sensor: SensorParams; solver: SolverParams } = {
  sensor: PLACEHOLDER_SENSOR,
  solver: PLACEHOLDER_SOLVER,
};

export function engineDefaults(): { sensor: SensorParams; solver: SolverParams } {
  return baseline;
}

/** Puts a camera and sensor on the synthetic bench and shows it. */
export function openOnSyntheticBench(camera: Partial<CameraSpec>, sensor: Partial<SensorParams>): void {
  stopSequence();
  update((s) => ({
    ...s,
    tab: 'synthetic',
    synthetic: {
      ...s.synthetic,
      camera: { ...s.synthetic.camera, ...camera },
      sensor: { ...s.synthetic.sensor, ...sensor },
    },
  }));
  requestSynthetic();
}

void client.ready.then(
  (defaults) => {
    baseline = { sensor: defaults.sensor, solver: defaults.solver };
    update((s) => ({
      ...s,
      ready: true,
      catalogSize: defaults.catalogSize,
      synthetic: { ...s.synthetic, sensor: defaults.sensor, solver: defaults.solver },
      image: {
        ...s.image,
        solver: {
          ...defaults.solver,
          cellSize: 64,
          peakSigma: 8,
          minArea: 5,
          maxArea: 3000,
          maxElongation: 3,
          saturation: 65535,
          maxDetections: 80,
          tolerancePx: 2,
          matchRadiusPx: 2.8,
          patternStars: 18,
          weighted: false,
        },
      },
    }));
    // A bench link in the URL replaces the defaults; the engine still validates every value.
    const shared = decodeShare(window.location.hash, sharedBench());
    if (shared) {
      update((s) => ({ ...s, synthetic: { ...s.synthetic, ...shared } }));
    }
    requestSynthetic();
  },
  (error: unknown) => {
    update((s) => ({ ...s, fatal: error instanceof Error ? error.message : String(error) }));
  },
);

function sharedBench(): SharedBench {
  const { pointing, camera, sensor, solver, seed } = state.synthetic;
  return { pointing, camera, sensor, solver, seed };
}

/** A link that opens the synthetic bench with the current settings. */
export function syntheticShareLink(): string {
  const url = new URL(window.location.href);
  url.hash = encodeShare(sharedBench());
  return url.toString();
}

export function setTab(tab: Tab): void {
  update((s) => ({ ...s, tab }));
}

export function setOverlay(bench: Bench, patch: Partial<OverlayOptions>): void {
  update((s) => ({ ...s, overlay: { ...s.overlay, [bench]: { ...s.overlay[bench], ...patch } } }));
}

export function setStretch(stretch: Stretch): void {
  update((s) => ({ ...s, stretch }));
}

export function setPointing(pointing: Pointing): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, pointing } }));
  requestSynthetic();
}

export function setSyntheticCamera(patch: Partial<CameraSpec>): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, camera: { ...s.synthetic.camera, ...patch } } }));
  requestSynthetic();
}

export function setSensor(patch: Partial<SensorParams>): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, sensor: { ...s.synthetic.sensor, ...patch } } }));
  requestSynthetic();
}

export function setSyntheticSolver(patch: Partial<SolverParams>): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, solver: { ...s.synthetic.solver, ...patch } } }));
  requestSynthetic();
}

export function setSeed(seed: number): void {
  update((s) => ({ ...s, synthetic: { ...s.synthetic, seed } }));
  requestSynthetic();
}

export function rerollSeed(): void {
  setSeed(Math.floor(Math.random() * 1_000_000));
}

function withAutoTolerance(image: ImageState): ImageState {
  if (!image.autoTolerance || image.camera.width === 0) {
    return image;
  }
  return { ...image, solver: { ...image.solver, ...autoTolerances(image.camera) } };
}

export function setImageCamera(patch: Partial<CameraSpec>): void {
  update((s) => ({
    ...s,
    image: withAutoTolerance({
      ...s.image,
      camera: { ...s.image.camera, ...patch },
      fovSource: patch.fovDeg === undefined ? s.image.fovSource : 'manual',
    }),
  }));
  requestImage();
}

export function setImageSolver(patch: Partial<SolverParams>): void {
  const manual = patch.tolerancePx !== undefined || patch.matchRadiusPx !== undefined;
  update((s) => ({
    ...s,
    image: {
      ...s.image,
      solver: { ...s.image.solver, ...patch },
      autoTolerance: manual ? false : s.image.autoTolerance,
    },
  }));
  requestImage();
}

export function setAutoTolerance(autoTolerance: boolean): void {
  update((s) => ({ ...s, image: withAutoTolerance({ ...s.image, autoTolerance }) }));
  requestImage();
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function decodeFitsFile(bytes: ArrayBuffer): Promise<{ width: number; height: number; pixels: Uint16Array }> {
  return new Promise((resolve, reject) => {
    client.submit({ kind: 'fits', bytes }, (response) => {
      if (response.type === 'fits') {
        resolve(response);
      } else if (response.type === 'error') {
        reject(new Error(response.message));
      }
    });
  });
}

export async function loadImageFile(file: File): Promise<void> {
  update((s) => ({ ...s, image: { ...s.image, busy: true, error: null, fileName: file.name } }));
  try {
    const bytes = await file.arrayBuffer();
    const decoded = isFits(file.name, new Uint8Array(bytes, 0, Math.min(bytes.byteLength, 8)))
      ? await decodeFitsFile(bytes.slice(0)).then((f) => fromFits(f.width, f.height, f.pixels))
      : await decodeRaster(file, bytes);
    const longSide = Math.max(decoded.width, decoded.height);
    update((s) => {
      const fovLong = decoded.exifFovDeg ?? null;
      const fovDeg = fovLong === null ? s.image.camera.fovDeg : (fovLong * decoded.width) / longSide;
      return {
        ...s,
        image: withAutoTolerance({
          ...s.image,
          frame: null,
          binning: decoded.binning,
          fovSource: fovLong === null ? 'manual' : 'exif',
          camera: { ...s.image.camera, width: decoded.width, height: decoded.height, fovDeg },
        }),
      };
    });
    requestImage(state.image.mirrored ? mirrorRows(decoded.pixels, decoded.width) : decoded.pixels);
  } catch (error) {
    update((s) => ({ ...s, image: { ...s.image, busy: false, error: describe(error) } }));
  }
}

export interface Sample {
  file: string;
  label: string;
  credit: string;
  /** Horizontal field of view to start from, degrees. */
  fovDeg: number;
  /** Lens model and distortion, when they differ from an undistorted pinhole. */
  lens?: Pick<CameraSpec, 'distortion' | 'equidistant'>;
}

export const SAMPLES: readonly Sample[] = [
  {
    file: 'orion-winter-triangle.jpg',
    label: 'Orion and the Winter Triangle',
    credit: 'Photograph: Akira Fujii, via ESA/Hubble (CC BY 4.0)',
    fovDeg: 52,
  },
  {
    file: 'meteor-camera-usl00b.png',
    label: 'Meteor camera, Arizona, 27 April 2023',
    credit: 'Global Meteor Network camera USL00B, mean of 256 video frames (Zenodo 7904716, CC BY 4.0)',
    fovDeg: 81.7,
    lens: { distortion: [-0.13, 0, 0, 0, 0], equidistant: true },
  },
  {
    file: 'synthetic-cygnus.fits',
    label: 'Synthetic tracker frame, Cygnus',
    credit: '16-bit FITS written by the Asterope simulator',
    fovDeg: 16,
  },
];

/** Loads one of the bundled example frames into the image bench. */
export async function loadSample(sample: Sample): Promise<void> {
  update((s) => ({
    ...s,
    image: {
      ...s.image,
      busy: true,
      error: null,
      unknownFov: false,
      mirrored: false,
      camera: {
        ...s.image.camera,
        fovDeg: sample.fovDeg,
        distortion: [0, 0, 0, 0, 0],
        equidistant: false,
        ...sample.lens,
      },
      fovSource: 'manual',
    },
  }));
  try {
    const response = await fetch(`${import.meta.env.BASE_URL}samples/${sample.file}`);
    if (!response.ok) {
      throw new Error(`could not load ${sample.file} (HTTP ${response.status})`);
    }
    await loadImageFile(new File([await response.blob()], sample.file));
  } catch (error) {
    update((s) => ({ ...s, image: { ...s.image, busy: false, error: describe(error) } }));
  }
}

export function runCentroidSuite(): void {
  const total = CENTROID_SIGMAS.length * CENTROID_SNRS.length;
  update((s) => ({ ...s, validation: { ...s.validation, centroid: { running: true, rows: [], total, error: null } } }));
  engineHolds = 'validation';
  client.submit(
    {
      kind: 'centroid',
      psfSigmas: CENTROID_SIGMAS,
      peakSnrs: CENTROID_SNRS,
      phases: 20,
      seeds: 25,
      method: baseline.solver.method,
      seed: 1,
    },
    (response) => {
      update((s) => {
        const centroid = s.validation.centroid;
        if (response.type === 'centroid') {
          return {
            ...s,
            validation: {
              ...s.validation,
              centroid: { ...centroid, rows: [...centroid.rows, response.row], running: !response.done },
            },
          };
        }
        if (response.type === 'error') {
          return { ...s, validation: { ...s.validation, centroid: { ...centroid, running: false, error: response.message } } };
        }
        return s;
      });
    },
  );
}

export function runWahbaSuite(): void {
  update((s) => ({ ...s, validation: { ...s.validation, wahba: { running: true, result: null, error: null } } }));
  client.submit({ kind: 'wahba', cases: WAHBA_CASES, seed: 20_240_229 }, (response) => {
    update((s) => {
      if (response.type === 'wahba') {
        return { ...s, validation: { ...s.validation, wahba: { running: false, result: response.result, error: null } } };
      }
      if (response.type === 'error') {
        return { ...s, validation: { ...s.validation, wahba: { running: false, result: null, error: response.message } } };
      }
      return s;
    });
  });
}

export function setLisOptions(patch: Partial<Pick<LisRun, 'masterSeed' | 'trials'>>): void {
  update((s) => ({ ...s, validation: { ...s.validation, lis: { ...s.validation.lis, ...patch } } }));
}

/** Extra engines used only to spread Monte Carlo trials over CPU cores. */
const helpers: EngineClient[] = [];
const MAX_MONTE_CARLO_WORKERS = 4;

/** Trial seeds depend only on the trial index, so any split gives the same outcomes. */
function monteCarloClients(): EngineClient[] {
  const cores = typeof navigator === 'undefined' ? 2 : navigator.hardwareConcurrency || 2;
  const wanted = Math.max(1, Math.min(MAX_MONTE_CARLO_WORKERS, cores - 1));
  while (helpers.length < wanted - 1) {
    helpers.push(new EngineClient(currentEpochYear()));
  }
  return [client, ...helpers.slice(0, wanted - 1)];
}

let lisJobs: { engine: EngineClient; id: number }[] = [];

function cancelLisJobs(): void {
  for (const job of lisJobs) {
    job.engine.cancel(job.id);
  }
}

function onLisResponse(response: JobResponse): void {
  if (response.type === 'error') {
    cancelLisJobs();
    lisJobs = [];
  }
  update((s) => {
    const lis = s.validation.lis;
    if (response.type === 'trials') {
      return {
        ...s,
        validation: {
          ...s.validation,
          lis: {
            ...lis,
            outcomes: [...lis.outcomes, ...response.outcomes],
            running: lisJobs.length > 0,
            elapsedMs: performance.now() - lis.startedAt,
          },
        },
      };
    }
    if (response.type === 'error') {
      return { ...s, validation: { ...s.validation, lis: { ...lis, running: false, error: response.message } } };
    }
    return s;
  });
}

export function runLisSuite(): void {
  if (!state.ready || state.validation.lis.running) {
    return;
  }
  const { masterSeed, trials } = state.validation.lis;
  engineHolds = 'validation';
  const engines = monteCarloClients();
  const share = Math.ceil(trials / engines.length);
  lisJobs = [];
  engines.forEach((engine, index) => {
    const firstTrial = index * share;
    const count = Math.min(share, trials - firstTrial);
    if (count <= 0) {
      return;
    }
    // Each engine numbers its own jobs, so a job is identified by engine and id together.
    const job = { engine, id: -1 };
    job.id = engine.submit(
      {
        kind: 'monteCarlo',
        camera: VALIDATION_CAMERA,
        sensor: baseline.sensor,
        solver: baseline.solver,
        masterSeed,
        firstTrial,
        trials: count,
        disturbances: true,
      },
      (response) => {
        if (response.type === 'trials' && response.done) {
          lisJobs = lisJobs.filter((other) => other !== job);
        }
        onLisResponse(response);
      },
    );
    lisJobs.push(job);
  });
  update((s) => ({
    ...s,
    validation: {
      ...s.validation,
      lis: { ...s.validation.lis, running: true, jobId: null, outcomes: [], startedAt: performance.now(), elapsedMs: 0, error: null },
    },
  }));
}

export function cancelLisSuite(): void {
  stopValidationRuns();
}

export function setStressOptions(patch: Partial<Pick<StressRun, 'parameter' | 'trialsPerStep'>>): void {
  update((s) => ({ ...s, validation: { ...s.validation, stress: { ...s.validation.stress, ...patch } } }));
}

export function runStressSuite(): void {
  if (!state.ready || state.validation.stress.running || state.validation.lis.running) {
    return;
  }
  const { parameter, trialsPerStep } = state.validation.stress;
  engineHolds = 'validation';
  const jobId = client.submit(
    {
      kind: 'stress',
      camera: VALIDATION_CAMERA,
      sensor: baseline.sensor,
      solver: baseline.solver,
      parameter,
      trialsPerStep,
      masterSeed: state.validation.lis.masterSeed,
    },
    (response) => {
      update((s) => {
        const stress = s.validation.stress;
        if (response.type === 'stress') {
          return {
            ...s,
            validation: {
              ...s.validation,
              stress: {
                ...stress,
                steps: response.step ? [...stress.steps, response.step] : stress.steps,
                total: response.steps,
                running: !response.done,
              },
            },
          };
        }
        if (response.type === 'error') {
          return { ...s, validation: { ...s.validation, stress: { ...stress, running: false, error: response.message } } };
        }
        return s;
      });
    },
  );
  update((s) => ({
    ...s,
    validation: { ...s.validation, stress: { ...s.validation.stress, running: true, jobId, steps: [], error: null } },
  }));
}

export function cancelStressSuite(): void {
  stopValidationRuns();
}

/** Opens one Monte Carlo trial on the synthetic bench. */
export function replayTrial(trial: number): void {
  stopValidationRuns();
  const { masterSeed } = state.validation.lis;
  update((s) => ({
    ...s,
    tab: 'synthetic',
    synthetic: {
      ...s.synthetic,
      camera: { ...VALIDATION_CAMERA },
      solver: baseline.solver,
      busy: true,
      replay: { masterSeed, trial },
    },
  }));
  syntheticChannel.push({
    kind: 'trial',
    camera: VALIDATION_CAMERA,
    sensor: baseline.sensor,
    solver: baseline.solver,
    masterSeed,
    trial,
    disturbances: true,
  });
}
