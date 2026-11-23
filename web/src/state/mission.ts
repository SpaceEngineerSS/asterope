import { useSyncExternalStore } from 'react';

import type { MissionReport } from '../engine/types';
import { missionSpec } from '../lib/mission';
import type { MissionSetup } from '../lib/mission';
import { appState, claimEngine, setPointing, setTab } from './store';

export interface MissionState {
  setup: MissionSetup;
  report: MissionReport | null;
  error: string | null;
  /** Sample picked on the chart. */
  selected: number | null;
}

let state: MissionState = {
  setup: {
    altitudeKm: 550,
    inclinationDeg: 53,
    nodeDeg: 40,
    year: 2026,
    month: 3,
    day: 20,
    tiltDeg: 0,
    azimuthDeg: 90,
    tumbleDeg: 0,
    sunExclusionDeg: 35,
    earthLimbExclusionDeg: 25,
    moonExclusionDeg: 10,
    orbits: 1,
  },
  report: null,
  error: null,
  selected: null,
};

const listeners = new Set<() => void>();
let evaluating = false;
let stale = false;

function update(patch: Partial<MissionState>): void {
  state = { ...state, ...patch };
  listeners.forEach((listener) => {
    listener();
  });
}

export function useMissionState(): MissionState {
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

/** Evaluates the current mission; a change made meanwhile is evaluated next. */
export function evaluateMission(): void {
  if (!appState().ready) {
    return;
  }
  if (evaluating) {
    stale = true;
    return;
  }
  evaluating = true;
  stale = false;
  // Geometry only: the frame held by another bench stays valid.
  claimEngine('mission').submit({ kind: 'mission', spec: missionSpec(state.setup) }, (response) => {
    evaluating = false;
    if (response.type === 'mission') {
      update({ report: response.report, error: null });
    } else if (response.type === 'error') {
      update({ report: null, error: response.message });
    }
    if (stale) {
      evaluateMission();
    }
  });
}

export function setMission(patch: Partial<MissionSetup>): void {
  update({ setup: { ...state.setup, ...patch }, selected: null });
  evaluateMission();
}

export function selectSample(index: number | null): void {
  update({ selected: index });
}

/** Points the Synthetic bench where the camera looks at the selected moment. */
export function openSelectedOnSyntheticBench(): void {
  const sample = state.selected === null ? undefined : state.report?.samples[state.selected];
  if (sample) {
    setTab('synthetic');
    setPointing(sample.pointing);
  }
}
