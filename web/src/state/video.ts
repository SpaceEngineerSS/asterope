import { useSyncExternalStore } from 'react';

import type { VideoJob } from '../engine/protocol';
import type {
  CameraSpec,
  FrameResult,
  Pointing,
  Quaternion,
  SearchProgress,
  SolverParams,
  TrackEvent,
} from '../engine/types';
import { rotationAngleDeg } from '../lib/sky';
import { openCamera, openVideoFile } from '../video/reader';
import type { VideoSource } from '../video/reader';
import { appState, autoTolerances, claimEngine } from './store';

export interface VideoSample {
  timeS: number;
  event: TrackEvent;
  pointing: Pointing | null;
  q: Quaternion | null;
  matches: number;
  detections: number;
  rmsArcsec: number;
  /** Angular rate since the previous solved sample, degrees per second. */
  rateDegS: number | null;
  /** Frames averaged for this sample. */
  stackFrames: number;
}

export interface VideoSourceInfo {
  name: string;
  width: number;
  height: number;
  binning: number;
  durationS: number | null;
  live: boolean;
}

export interface VideoState {
  source: VideoSourceInfo | null;
  opening: boolean;
  camera: CameraSpec;
  solver: SolverParams | null;
  autoTolerance: boolean;
  /** The entered field of view is only a starting guess. */
  unknownFov: boolean;
  /** A search has measured the field of view of this source. */
  fovFitted: boolean;
  /** Frames per second of video time taken from a file. */
  sampleRate: number;
  stackDepth: number;
  running: boolean;
  frame: FrameResult | null;
  samples: VideoSample[];
  selected: number | null;
  /** Camera frames that arrived while an earlier one was being solved. */
  dropped: number;
  progress: SearchProgress | null;
  error: string | null;
}

export const SAMPLE_RATES = [2, 5, 10, 15, 30] as const;
export const STACK_DEPTHS = [1, 2, 4, 8, 16] as const;
/**
 * After a field-of-view search that found nothing, the next one waits this
 * many seconds of video, doubling each time up to the limit. Titles, twilight
 * and clouds are skipped cheaply instead of being searched frame by frame.
 */
const SEARCH_BACKOFF_S = 1;
const SEARCH_BACKOFF_LIMIT_S = 8;
const MAX_LIVE_SAMPLES = 3000;

let state: VideoState = {
  source: null,
  opening: false,
  camera: { width: 0, height: 0, fovDeg: 60, distortion: [0, 0, 0, 0, 0], scaleSpan: 0, currentEpoch: true },
  solver: null,
  autoTolerance: true,
  unknownFov: true,
  fovFitted: false,
  sampleRate: 10,
  stackDepth: 1,
  running: false,
  frame: null,
  samples: [],
  selected: null,
  dropped: 0,
  progress: null,
  error: null,
};

const listeners = new Set<() => void>();
let source: VideoSource | null = null;
/** Incremented to abandon the loop that is running. */
let runToken = 0;
let activeJob: number | null = null;

function update(patch: Partial<VideoState>): void {
  state = { ...state, ...patch };
  listeners.forEach((listener) => {
    listener();
  });
}

export function useVideoState(): VideoState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => state,
  );
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function solverFor(camera: CameraSpec, solver: SolverParams, auto: boolean): SolverParams {
  return auto && camera.width > 0 ? { ...solver, ...autoTolerances(camera) } : solver;
}

function currentSolver(): SolverParams {
  return state.solver ?? appState().image.solver;
}

function solve(job: Omit<VideoJob, 'kind'>): Promise<FrameResult> {
  return new Promise((resolve, reject) => {
    const engine = claimEngine('video');
    activeJob = engine.submit({ kind: 'video', ...job }, (response) => {
      if (response.type === 'progress') {
        update({ progress: response.progress });
      } else if (response.type === 'frame') {
        activeJob = null;
        resolve(response.frame);
      } else if (response.type === 'error') {
        activeJob = null;
        reject(new Error(response.message));
      }
    });
  });
}

function sampleOf(frame: FrameResult, timeS: number, previous: VideoSample | undefined): VideoSample {
  const attitude = frame.solve.attitude;
  const reference = previous?.q && attitude && timeS > previous.timeS ? previous : null;
  return {
    timeS,
    event: frame.track ?? (attitude ? 'acquired' : 'lost'),
    pointing: attitude?.pointing ?? null,
    q: attitude?.q ?? null,
    matches: attitude?.matches.length ?? 0,
    detections: frame.detect.detections.length,
    rmsArcsec: attitude?.rmsArcsec ?? NaN,
    rateDegS:
      reference?.q && attitude ? rotationAngleDeg(reference.q, attitude.q) / (timeS - reference.timeS) : null,
    stackFrames: frame.stack?.frames ?? 1,
  };
}

function lastSolved(samples: readonly VideoSample[]): VideoSample | undefined {
  for (let i = samples.length - 1; i >= 0; i -= 1) {
    if (samples[i]?.q) {
      return samples[i];
    }
  }
  return undefined;
}

/** Adopts the camera a field-of-view search has fitted to the frame. */
function adoptFittedCamera(frame: FrameResult): void {
  const { fovXDeg, k1, equidistant } = frame.solve.intrinsics;
  const [, k2, k3, p1, p2] = state.camera.distortion;
  const camera: CameraSpec = {
    ...state.camera,
    fovDeg: Math.round(fovXDeg * 100) / 100,
    distortion: [k1, k2, k3, p1, p2],
    equidistant,
  };
  update({ camera, fovFitted: true, solver: solverFor(camera, currentSolver(), state.autoTolerance) });
}

async function run(token: number): Promise<void> {
  const active = source;
  if (!active) {
    return;
  }
  const step = 1 / state.sampleRate;
  let previousTime: number | null = null;
  let presented: number | null = null;
  let nextSearchAt = 0;
  let backoff = SEARCH_BACKOFF_S;
  try {
    for (let index = 0; token === runToken; index += 1) {
      let timeS = index * step;
      if (active.live) {
        const next = await active.nextFrame();
        timeS = next.timeS;
        if (presented !== null && next.presented !== null) {
          update({ dropped: state.dropped + Math.max(0, next.presented - presented - 1) });
        }
        presented = next.presented;
      } else {
        if (timeS > (active.durationS ?? 0)) {
          break;
        }
        await active.seek(timeS);
      }
      if (token !== runToken) {
        break;
      }
      const searching = state.unknownFov && !state.fovFitted && timeS >= nextSearchAt;
      const frame = await solve({
        camera: state.camera,
        pixels: active.grab(),
        solver: currentSolver(),
        dt: Math.max(previousTime === null ? step : timeS - previousTime, 1e-3),
        reset: previousTime === null,
        stackDepth: state.stackDepth,
        unknownFov: searching,
      });
      previousTime = timeS;
      const samples = [...state.samples, sampleOf(frame, timeS, lastSolved(state.samples))].slice(-MAX_LIVE_SAMPLES);
      update({ frame, samples, selected: samples.length - 1, progress: null });
      if (searching && frame.solve.attitude) {
        adoptFittedCamera(frame);
      } else if (searching && !frame.stopped) {
        nextSearchAt = timeS + backoff;
        backoff = Math.min(2 * backoff, SEARCH_BACKOFF_LIMIT_S);
      }
    }
    if (token === runToken && state.unknownFov && !state.fovFitted && state.samples.length > 0) {
      update({
        error:
          'No frame searched could be explained by a field of view from 6° to 100°. Enter the field of view, raise the stack depth, or check that stars are visible as points.',
      });
    }
  } catch (error) {
    if (token === runToken) {
      update({ error: describe(error) });
    }
  }
  if (token === runToken) {
    update({ running: false, progress: null });
  }
}

export function startVideo(): void {
  if (!source || state.running || !appState().ready) {
    return;
  }
  runToken += 1;
  update({
    running: true,
    samples: [],
    selected: null,
    dropped: 0,
    error: null,
    solver: solverFor(state.camera, currentSolver(), state.autoTolerance),
  });
  void run(runToken);
}

export function stopVideo(): void {
  runToken += 1;
  if (activeJob !== null) {
    claimEngine('video').cancel(activeJob);
  }
  update({ running: false, progress: null });
}

/**
 * Shows the frame behind one sample again. The frames that were stacked
 * into it are replayed first so the view matches what the run solved.
 */
export async function inspectSample(index: number): Promise<void> {
  const active = source;
  const sample = state.samples[index];
  if (!active || active.live || state.running || !sample) {
    return;
  }
  runToken += 1;
  const token = runToken;
  update({ selected: index });
  const first = Math.max(0, index - state.stackDepth + 1);
  try {
    for (let i = first; i <= index && token === runToken; i += 1) {
      const timeS = state.samples[i]?.timeS ?? sample.timeS;
      await active.seek(timeS);
      const frame = await solve({
        camera: state.camera,
        pixels: active.grab(),
        solver: currentSolver(),
        dt: 1 / state.sampleRate,
        reset: i === first,
        stackDepth: state.stackDepth,
        unknownFov: false,
      });
      if (token === runToken && i === index) {
        update({ frame });
      }
    }
  } catch (error) {
    if (token === runToken) {
      update({ error: describe(error) });
    }
  }
}

function closeSource(): void {
  runToken += 1;
  source?.close();
  source = null;
}

async function adopt(opened: Promise<VideoSource>, name: string, fovDeg: number | null): Promise<void> {
  closeSource();
  const token = runToken;
  update({ opening: true, running: false, error: null, source: null, frame: null, samples: [], selected: null });
  try {
    const next = await opened;
    if (token !== runToken) {
      next.close();
      return;
    }
    source = next;
    const camera: CameraSpec = {
      ...state.camera,
      width: next.width,
      height: next.height,
      fovDeg: fovDeg ?? state.camera.fovDeg,
      distortion: [0, 0, 0, 0, 0],
      equidistant: false,
    };
    update({
      opening: false,
      camera,
      unknownFov: fovDeg === null ? state.unknownFov : false,
      fovFitted: false,
      solver: solverFor(camera, currentSolver(), state.autoTolerance),
      source: {
        name,
        width: next.width,
        height: next.height,
        binning: next.binning,
        durationS: next.durationS,
        live: next.live,
      },
    });
    startVideo();
  } catch (error) {
    update({ opening: false, error: describe(error) });
  }
}

/** Opens a video and starts solving it. `fovDeg` is given when the field of view is known. */
export function openVideo(file: Blob, name: string, fovDeg: number | null = null): Promise<void> {
  return adopt(openVideoFile(file), name, fovDeg);
}

export function openLiveCamera(): Promise<void> {
  return adopt(openCamera(), 'Camera', null);
}

export function closeVideo(): void {
  closeSource();
  update({ source: null, running: false, frame: null, samples: [], selected: null, progress: null });
}

export function setVideoFov(fovDeg: number): void {
  const camera: CameraSpec = { ...state.camera, fovDeg, distortion: [0, 0, 0, 0, 0], equidistant: false };
  update({ camera, fovFitted: false, solver: solverFor(camera, currentSolver(), state.autoTolerance) });
}

export function setVideoUnknownFov(unknownFov: boolean): void {
  update({ unknownFov, fovFitted: false });
}

export function setVideoSolver(patch: Partial<SolverParams>): void {
  const manual = patch.tolerancePx !== undefined || patch.matchRadiusPx !== undefined;
  update({ solver: { ...currentSolver(), ...patch }, autoTolerance: manual ? false : state.autoTolerance });
}

export function setVideoAutoTolerance(autoTolerance: boolean): void {
  update({ autoTolerance, solver: solverFor(state.camera, currentSolver(), autoTolerance) });
}

export function setSampleRate(sampleRate: number): void {
  update({ sampleRate });
}

export function setStackDepth(stackDepth: number): void {
  update({ stackDepth });
}
