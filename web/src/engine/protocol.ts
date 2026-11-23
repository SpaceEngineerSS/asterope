import type {
  CameraSpec,
  CentroidMethod,
  CentroidRow,
  DesignReport,
  DesignSpec,
  MissionReport,
  MissionSpec,
  FrameResult,
  Pointing,
  SearchProgress,
  SensorParams,
  SolverParams,
  StressStep,
  TrialOutcome,
  WahbaResult,
} from './types';

export interface SyntheticJob {
  kind: 'synthetic';
  camera: CameraSpec;
  pointing: Pointing;
  sensor: SensorParams;
  solver: SolverParams;
  seed: number;
  /**
   * Solve as the next frame of a sequence: the pointing first advances by
   * the sensor's body rate over `dt` seconds (unless `reset`), and the frame
   * is solved by the tracker instead of a fresh search.
   */
  sequence?: { dt: number; reset: boolean };
}

/** Re-renders one Monte Carlo trial so its frame can be inspected. */
export interface TrialJob {
  kind: 'trial';
  camera: CameraSpec;
  sensor: SensorParams;
  solver: SolverParams;
  masterSeed: number;
  trial: number;
  disturbances: boolean;
}

export interface ImageJob {
  kind: 'image';
  camera: CameraSpec;
  /** Omitted to re-solve the frame already loaded in the engine. */
  pixels?: Uint16Array;
  solver: SolverParams;
  /** Treat the camera's field of view as a hint and search a ladder of fields. */
  unknownFov: boolean;
}

/** Solves the next frame of a video with the tracker. */
export interface VideoJob {
  kind: 'video';
  camera: CameraSpec;
  pixels: Uint16Array;
  solver: SolverParams;
  /** Seconds since the previous frame. */
  dt: number;
  /** First frame of a run: forget the tracked attitude and the stacked frames. */
  reset: boolean;
  /** Frames averaged before detection; 1 solves each frame alone. */
  stackDepth: number;
  /** Search a ladder of fields of view instead of tracking with the camera's. */
  unknownFov: boolean;
}

/** Predicts the performance of a camera design. */
export interface DesignJob {
  kind: 'design';
  spec: DesignSpec;
}

/** Follows a camera round an orbit. */
export interface MissionJob {
  kind: 'mission';
  spec: MissionSpec;
}

export interface MonteCarloJob {
  kind: 'monteCarlo';
  camera: CameraSpec;
  sensor: SensorParams;
  solver: SolverParams;
  masterSeed: number;
  /** Index of the first trial; lets several workers share one run. */
  firstTrial: number;
  trials: number;
  disturbances: boolean;
}

/** Sweeps one sensor parameter with every other disturbance off. */
export interface StressJob {
  kind: 'stress';
  camera: CameraSpec;
  sensor: SensorParams;
  solver: SolverParams;
  /** Index into `STRESS_PARAMETERS`. */
  parameter: number;
  trialsPerStep: number;
  masterSeed: number;
}

export interface CentroidJob {
  kind: 'centroid';
  psfSigmas: readonly number[];
  /** null entries render without noise. */
  peakSnrs: readonly (number | null)[];
  phases: number;
  seeds: number;
  method: CentroidMethod;
  seed: number;
}

export interface WahbaJob {
  kind: 'wahba';
  cases: number;
  seed: number;
}

export interface FitsJob {
  kind: 'fits';
  bytes: ArrayBuffer;
}

export type Job =
  | SyntheticJob
  | TrialJob
  | ImageJob
  | VideoJob
  | DesignJob
  | MissionJob
  | MonteCarloJob
  | StressJob
  | CentroidJob
  | WahbaJob
  | FitsJob;

export type Request =
  | { type: 'init'; epochYear: number }
  | { type: 'job'; id: number; job: Job }
  | { type: 'cancel'; id: number };

export interface EngineDefaults {
  catalogSize: number;
  sensor: SensorParams;
  solver: SolverParams;
}

export interface TrialReplay {
  frame: FrameResult;
  sensor: SensorParams;
}

export type Response =
  | { type: 'ready'; defaults: EngineDefaults }
  | { type: 'fatal'; message: string }
  | { type: 'frame'; id: number; frame: FrameResult }
  | { type: 'progress'; id: number; progress: SearchProgress }
  | { type: 'replay'; id: number; replay: TrialReplay }
  | { type: 'trials'; id: number; outcomes: TrialOutcome[]; done: boolean }
  | { type: 'centroid'; id: number; row: CentroidRow; done: boolean }
  | { type: 'stress'; id: number; step: StressStep | null; steps: number; done: boolean }
  | { type: 'wahba'; id: number; result: WahbaResult }
  | { type: 'design'; id: number; report: DesignReport }
  | { type: 'mission'; id: number; report: MissionReport }
  | { type: 'fits'; id: number; width: number; height: number; pixels: Uint16Array }
  | { type: 'error'; id: number; message: string };
