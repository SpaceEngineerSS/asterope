import { useSyncExternalStore } from 'react';

import type { CameraSpec, DesignReport, DesignSpec, SensorParams, SolverParams, TrialOutcome } from '../engine/types';
import { appState, claimEngine, engineDefaults, openOnSyntheticBench } from './store';

/** What a Monte Carlo run of the designed camera measured. */
export interface Verification {
  running: boolean;
  trials: number;
  outcomes: TrialOutcome[];
  error: string | null;
}

export interface DesignState {
  spec: DesignSpec;
  report: DesignReport | null;
  error: string | null;
  verification: Verification;
}

export const VERIFY_TRIALS = [50, 200, 500] as const;
/** The verification solver keeps every detection so the count can be compared with the prediction. */
const VERIFY_MAX_DETECTIONS = 150;

let state: DesignState = {
  spec: {
    focalLengthMm: 25,
    apertureMm: 14.5,
    pixelUm: 6,
    width: 1024,
    height: 1024,
    transmission: 0.8,
    quantumEfficiency: 0.5,
    bandwidthNm: 300,
    exposure: 0.1,
    psfSigma: 1.2,
    readNoise: 5,
    darkCurrent: 10,
    skyBackground: 20,
    rateDeg: 0,
    peakSigma: 5,
  },
  report: null,
  error: null,
  verification: { running: false, trials: 200, outcomes: [], error: null },
};

const listeners = new Set<() => void>();
let evaluating = false;
let stale = false;
let verifyJob: number | null = null;

function update(patch: Partial<DesignState>): void {
  state = { ...state, ...patch };
  listeners.forEach((listener) => {
    listener();
  });
}

export function useDesignState(): DesignState {
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

/** Evaluates the current design; a change made meanwhile is evaluated next. */
export function evaluateDesign(): void {
  if (!appState().ready) {
    return;
  }
  if (evaluating) {
    stale = true;
    return;
  }
  evaluating = true;
  stale = false;
  // The prediction only reads the catalog, so the frame held by another bench stays valid.
  claimEngine('design').submit({ kind: 'design', spec: state.spec }, (response) => {
    evaluating = false;
    if (response.type === 'design') {
      update({ report: response.report, error: null });
    } else if (response.type === 'error') {
      update({ report: null, error: response.message });
    }
    if (stale) {
      evaluateDesign();
    }
  });
}

export function setDesign(patch: Partial<DesignSpec>): void {
  stopVerification();
  update({ spec: { ...state.spec, ...patch }, verification: { ...state.verification, outcomes: [], error: null } });
  evaluateDesign();
}

function simulatedCamera(report: DesignReport): CameraSpec {
  return {
    width: state.spec.width,
    height: state.spec.height,
    fovDeg: report.fovXDeg,
    distortion: [0, 0, 0, 0, 0],
    scaleSpan: 0,
    currentEpoch: false,
  };
}

function simulatedSensor(report: DesignReport): SensorParams {
  const { exposure, psfSigma, readNoise, darkCurrent, skyBackground, rateDeg } = state.spec;
  return {
    ...engineDefaults().sensor,
    zeroPoint: report.zeroPoint,
    exposure,
    psfSigma,
    readNoise,
    darkCurrent,
    skyBackground,
    rateXDeg: 0,
    rateYDeg: rateDeg,
    rateZDeg: 0,
  };
}

function verificationSolver(): SolverParams {
  const { peakSigma } = state.spec;
  return {
    ...engineDefaults().solver,
    peakSigma,
    segmentSigma: Math.min(3, peakSigma),
    maxDetections: VERIFY_MAX_DETECTIONS,
  };
}

export function setVerifyTrials(trials: number): void {
  update({ verification: { ...state.verification, trials } });
}

/** Simulates the designed camera at random attitudes and solves every frame. */
export function verifyDesign(): void {
  const report = state.report;
  if (!report || state.verification.running) {
    return;
  }
  update({ verification: { ...state.verification, running: true, outcomes: [], error: null } });
  verifyJob = claimEngine('validation').submit(
    {
      kind: 'monteCarlo',
      camera: simulatedCamera(report),
      sensor: simulatedSensor(report),
      solver: verificationSolver(),
      masterSeed: 1,
      firstTrial: 0,
      trials: state.verification.trials,
      disturbances: false,
    },
    (response) => {
      const verification = state.verification;
      if (response.type === 'trials') {
        update({
          verification: {
            ...verification,
            outcomes: [...verification.outcomes, ...response.outcomes],
            running: !response.done,
          },
        });
      } else if (response.type === 'error') {
        update({ verification: { ...verification, running: false, error: response.message } });
      }
      if (response.type !== 'trials' || response.done) {
        verifyJob = null;
      }
    },
  );
}

export function stopVerification(): void {
  if (verifyJob !== null) {
    claimEngine('validation').cancel(verifyJob);
  }
}

export function openDesignOnSyntheticBench(): void {
  const report = state.report;
  if (report) {
    stopVerification();
    openOnSyntheticBench(simulatedCamera(report), simulatedSensor(report));
  }
}
