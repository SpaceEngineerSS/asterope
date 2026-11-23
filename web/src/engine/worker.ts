/// <reference lib="webworker" />

import init, {
  Engine,
  advancePointing,
  attitudeErrorArcsec,
  centroidValidation,
  decodeFits,
  defaultSensor,
  defaultSolver,
  pointingQuaternion,
  stressSensor,
  stressValues,
  wahbaValidation,
} from '../wasm/asterope_wasm.js';
import {
  decodeCentroidRow,
  decodeDesign,
  decodeDetect,
  decodeField,
  decodeIndexInfo,
  decodeMission,
  decodeProgress,
  decodeSensor,
  decodeSolve,
  decodeSolver,
  decodeStack,
  decodeTrackEvent,
  decodeTrials,
  decodeTruth,
  decodeWahba,
  encodeDesign,
  encodeMission,
  encodeSensor,
  encodeSolver,
} from './decode';
import type { Job, Request, Response } from './protocol';
import type {
  AttitudeError,
  CameraSpec,
  FrameResult,
  IndexInfo,
  Pointing,
  Quaternion,
  SolverParams,
  StackInfo,
  TrackEvent,
  TrialClass,
  TrialOutcome,
} from './types';

declare const self: DedicatedWorkerGlobalScope;

const FIELD_MAG_LIMIT = 6.5;
const TRIALS_PER_BATCH = 5;

let engine: Engine | null = null;
const cancelled = new Set<number>();

function post(message: Response, transfer: Transferable[] = []): void {
  self.postMessage(message, transfer);
}

function requireEngine(): Engine {
  if (engine === null) {
    throw new Error('engine is not initialized');
  }
  return engine;
}

function yieldToMessages(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}

function configure(camera: CameraSpec): { info: IndexInfo; indexMs: number } {
  const start = performance.now();
  const info = decodeIndexInfo(
    requireEngine().configure(
      camera.width,
      camera.height,
      camera.fovDeg,
      Float64Array.of(...camera.distortion, camera.equidistant ? 1 : 0),
      camera.scaleSpan,
      camera.currentEpoch,
    ),
  );
  return { info, indexMs: info.rebuilt ? performance.now() - start : 0 };
}

function toQuaternion(values: ArrayLike<number>): Quaternion {
  return [values[0] ?? 0, values[1] ?? 0, values[2] ?? 0, values[3] ?? 1];
}

interface Truth {
  pointing: Pointing;
}

interface SearchOptions {
  /** Job id, used to report progress and to honour cancellation. */
  id: number;
  unknownFov: boolean;
}

interface TrackOptions {
  trackDt: number;
  /** Unsolved frames coasted over before a new search. */
  maxMisses: number;
}

const SCALES_PER_STEP = 6;
const SEQUENCE_MAX_MISSES = 3;

async function solveCurrentFrame(
  camera: CameraSpec,
  solver: SolverParams,
  info: IndexInfo,
  renderMs: number,
  indexMs: number,
  truth: Truth | null,
  search: SearchOptions | TrackOptions | null,
  stack: StackInfo | null = null,
): Promise<FrameResult> {
  const active = requireEngine();
  const solverBuffer = encodeSolver(solver);

  let start = performance.now();
  const detect = decodeDetect(active.detect(solverBuffer));
  const detectMs = performance.now() - start;

  start = performance.now();
  let stopped = false;
  let track: TrackEvent | null = null;
  let solve;
  if (search === null) {
    solve = decodeSolve(active.identify(solverBuffer));
  } else if ('trackDt' in search) {
    const buffer = active.trackStep(solverBuffer, search.trackDt, search.maxMisses);
    solve = decodeSolve(buffer);
    track = decodeTrackEvent(buffer);
  } else {
    active.beginSearch(solverBuffer, search.unknownFov);
    for (;;) {
      const { done, progress } = decodeProgress(active.stepSearch(SCALES_PER_STEP));
      if (done) {
        break;
      }
      post({ type: 'progress', id: search.id, progress });
      await yieldToMessages();
      if (cancelled.delete(search.id)) {
        stopped = true;
        break;
      }
    }
    solve = decodeSolve(active.searchResult());
  }
  const identifyMs = performance.now() - start;

  const truthQ = truth
    ? toQuaternion(pointingQuaternion(truth.pointing.raDeg, truth.pointing.decDeg, truth.pointing.rollDeg))
    : null;
  let error: AttitudeError | null = null;
  if (solve.attitude && truthQ) {
    const e = attitudeErrorArcsec(Float64Array.from(solve.attitude.q), Float64Array.from(truthQ));
    error = { crossArcsec: e[0] ?? NaN, rollArcsec: e[1] ?? NaN, totalArcsec: e[2] ?? NaN };
  }

  const overlayQ = solve.attitude?.q ?? truthQ;
  const k = solve.intrinsics;
  const field = overlayQ
    ? decodeField(
        active.field(Float64Array.from(overlayQ), Float64Array.of(k.fx, k.fy, k.cx, k.cy, k.k1, k.equidistant ? 1 : 0), FIELD_MAG_LIMIT),
      )
    : [];

  return {
    width: camera.width,
    height: camera.height,
    pixels: active.frame(),
    index: info,
    detect,
    solve,
    field,
    truth: decodeTruth(active.truth()),
    truthPointing: truth?.pointing ?? null,
    truthQ,
    error,
    timing: { renderMs, detectMs, identifyMs, indexMs },
    stopped,
    track,
    stack,
  };
}

async function run(id: number, job: Job): Promise<void> {
  const active = requireEngine();
  switch (job.kind) {
    case 'synthetic': {
      const { info, indexMs } = configure(job.camera);
      let pointing = job.pointing;
      if (job.sequence?.reset) {
        active.trackReset();
      } else if (job.sequence) {
        const { rateXDeg, rateYDeg, rateZDeg } = job.sensor;
        const next = advancePointing(
          Float64Array.of(pointing.raDeg, pointing.decDeg, pointing.rollDeg),
          Float64Array.of(rateXDeg, rateYDeg, rateZDeg),
          job.sequence.dt,
        );
        pointing = { raDeg: next[0] ?? 0, decDeg: next[1] ?? 0, rollDeg: next[2] ?? 0 };
      }
      const start = performance.now();
      active.render(pointing.raDeg, pointing.decDeg, pointing.rollDeg, encodeSensor(job.sensor), job.seed);
      const renderMs = performance.now() - start;
      const frame = await solveCurrentFrame(
        job.camera,
        job.solver,
        info,
        renderMs,
        indexMs,
        { pointing },
        job.sequence ? { trackDt: job.sequence.dt, maxMisses: SEQUENCE_MAX_MISSES } : null,
      );
      post({ type: 'frame', id, frame }, [frame.pixels.buffer]);
      return;
    }
    case 'trial': {
      const { info, indexMs } = configure(job.camera);
      const start = performance.now();
      const setup = active.renderTrial(job.masterSeed, job.trial, encodeSensor(job.sensor), job.disturbances);
      const renderMs = performance.now() - start;
      const pointing = { raDeg: setup[0] ?? 0, decDeg: setup[1] ?? 0, rollDeg: setup[2] ?? 0 };
      const frame = await solveCurrentFrame(job.camera, job.solver, info, renderMs, indexMs, { pointing }, null);
      post({ type: 'replay', id, replay: { frame, sensor: decodeSensor(setup, 3) } }, [frame.pixels.buffer]);
      return;
    }
    case 'image': {
      const { info, indexMs } = configure(job.camera);
      if (job.pixels) {
        active.loadFrame(job.pixels);
      }
      const frame = await solveCurrentFrame(job.camera, job.solver, info, 0, indexMs, null, {
        id,
        unknownFov: job.unknownFov,
      });
      post({ type: 'frame', id, frame }, [frame.pixels.buffer]);
      return;
    }
    case 'video': {
      const { info, indexMs } = configure(job.camera);
      if (job.reset) {
        active.trackReset();
        active.stackReset();
      }
      const start = performance.now();
      const stack = decodeStack(active.stackFrame(job.pixels, job.stackDepth, encodeSolver(job.solver)));
      const stackMs = performance.now() - start;
      const frame = await solveCurrentFrame(
        job.camera,
        job.solver,
        info,
        stackMs,
        indexMs,
        null,
        // Video frames can repeat or jump, so a miss is searched at once instead of coasted over.
        job.unknownFov ? { id, unknownFov: true } : { trackDt: job.dt, maxMisses: 0 },
        stack,
      );
      post({ type: 'frame', id, frame }, [frame.pixels.buffer]);
      return;
    }
    case 'monteCarlo': {
      configure(job.camera);
      const sensor = encodeSensor(job.sensor);
      const solver = encodeSolver(job.solver);
      const end = job.firstTrial + job.trials;
      for (let first = job.firstTrial; first < end; first += TRIALS_PER_BATCH) {
        if (cancelled.delete(id)) {
          post({ type: 'trials', id, outcomes: [], done: true });
          return;
        }
        const count = Math.min(TRIALS_PER_BATCH, end - first);
        const rows = active.monteCarlo(job.masterSeed, first, count, sensor, solver, job.disturbances);
        const done = first + count >= end;
        post({ type: 'trials', id, outcomes: decodeTrials(rows, first), done });
        await yieldToMessages();
      }
      return;
    }
    case 'stress': {
      configure(job.camera);
      const base = encodeSensor(job.sensor);
      const solver = encodeSolver(job.solver);
      const values = stressValues(job.parameter);
      for (const [index, value] of values.entries()) {
        const sensor = stressSensor(base, job.parameter, value);
        const outcomes: TrialOutcome[] = [];
        for (let first = 0; first < job.trialsPerStep; first += TRIALS_PER_BATCH) {
          if (cancelled.delete(id)) {
            post({ type: 'stress', id, step: null, steps: values.length, done: true });
            return;
          }
          const count = Math.min(TRIALS_PER_BATCH, job.trialsPerStep - first);
          outcomes.push(...decodeTrials(active.monteCarlo(job.masterSeed, first, count, sensor, solver, false), first));
          await yieldToMessages();
        }
        const solved = outcomes
          .filter((o) => o.class === 'success' || o.class === 'inaccurate')
          .map((o) => o.crossArcsec)
          .sort((a, b) => a - b);
        const tally = (wanted: TrialClass) => outcomes.filter((o) => o.class === wanted).length;
        post({
          type: 'stress',
          id,
          steps: values.length,
          done: index === values.length - 1,
          step: {
            value,
            trials: outcomes.length,
            success: tally('success'),
            falsePositive: tally('falsePositive'),
            noSolution: tally('noSolution'),
            inaccurate: tally('inaccurate'),
            crossMedianArcsec: solved[Math.floor(solved.length / 2)] ?? NaN,
          },
        });
      }
      return;
    }
    case 'centroid': {
      const total = job.psfSigmas.length * job.peakSnrs.length;
      let finished = 0;
      for (const peakSnr of job.peakSnrs) {
        for (const psfSigma of job.psfSigmas) {
          if (cancelled.delete(id)) {
            return;
          }
          const seeds = peakSnr === null ? 1 : job.seeds;
          const values = centroidValidation(
            psfSigma,
            peakSnr ?? -1,
            job.phases,
            seeds,
            job.method === 'cog' ? 0 : 1,
            job.seed,
          );
          finished += 1;
          post({
            type: 'centroid',
            id,
            row: decodeCentroidRow(values, psfSigma, peakSnr),
            done: finished === total,
          });
          await yieldToMessages();
        }
      }
      return;
    }
    case 'design': {
      const report = decodeDesign(active.design(encodeDesign(job.spec)));
      post({ type: 'design', id, report }, [report.counts.buffer]);
      return;
    }
    case 'mission': {
      post({ type: 'mission', id, report: decodeMission(active.mission(encodeMission(job.spec))) });
      return;
    }
    case 'wahba': {
      post({ type: 'wahba', id, result: decodeWahba(wahbaValidation(job.cases, job.seed)) });
      return;
    }
    case 'fits': {
      const decoded = decodeFits(new Uint8Array(job.bytes));
      const pixels = decoded.slice(2);
      post({ type: 'fits', id, width: decoded[0] ?? 0, height: decoded[1] ?? 0, pixels }, [pixels.buffer]);
      return;
    }
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

let queue: Promise<void> = Promise.resolve();

self.onmessage = (event: MessageEvent<Request>) => {
  const request = event.data;
  switch (request.type) {
    case 'init':
      queue = queue.then(async () => {
        try {
          await init();
          engine = new Engine(request.epochYear);
          post({
            type: 'ready',
            defaults: {
              catalogSize: engine.catalogSize(),
              sensor: decodeSensor(defaultSensor()),
              solver: decodeSolver(defaultSolver()),
            },
          });
        } catch (error) {
          post({ type: 'fatal', message: describe(error) });
        }
      });
      return;
    case 'cancel':
      cancelled.add(request.id);
      return;
    case 'job':
      queue = queue.then(async () => {
        try {
          await run(request.id, request.job);
        } catch (error) {
          post({ type: 'error', id: request.id, message: describe(error) });
        } finally {
          cancelled.delete(request.id);
        }
      });
  }
};
