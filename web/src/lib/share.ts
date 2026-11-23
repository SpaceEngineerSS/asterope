import type { CameraSpec, Pointing, SensorParams, SolverParams } from '../engine/types';

/** The part of the synthetic bench a link reproduces. */
export interface SharedBench {
  pointing: Pointing;
  camera: CameraSpec;
  sensor: SensorParams;
  solver: SolverParams;
  seed: number;
}

const PREFIX = '#bench=';

function toBase64Url(text: string): string {
  return btoa(text).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(text: string): string {
  return atob(text.replace(/-/g, '+').replace(/_/g, '/'));
}

export function encodeShare(bench: SharedBench): string {
  return PREFIX + toBase64Url(JSON.stringify(bench));
}

/**
 * Copies values from untrusted `source` onto `base`, keeping only keys that
 * exist in `base` and whose type matches (finite numbers, booleans, and
 * strings equal to an allowed value). Anything else keeps the default.
 */
function merge<T extends object>(base: T, source: unknown, enums: Partial<Record<keyof T, readonly string[]>> = {}): T {
  if (typeof source !== 'object' || source === null) {
    return base;
  }
  const input = source as Record<string, unknown>;
  const out = { ...base };
  for (const key of Object.keys(base) as (keyof T & string)[]) {
    const current = base[key];
    const candidate = input[key];
    if (typeof current === 'number' && typeof candidate === 'number' && Number.isFinite(candidate)) {
      out[key] = candidate as T[typeof key];
    } else if (typeof current === 'boolean' && typeof candidate === 'boolean') {
      out[key] = candidate as T[typeof key];
    } else if (typeof current === 'string' && typeof candidate === 'string' && enums[key]?.includes(candidate)) {
      out[key] = candidate as T[typeof key];
    }
  }
  return out;
}

const SIZES = [256, 512, 1024, 2048];

/** Reads a bench from a URL hash; returns null when the hash holds none. */
export function decodeShare(hash: string, defaults: SharedBench): SharedBench | null {
  if (!hash.startsWith(PREFIX)) {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(fromBase64Url(hash.slice(PREFIX.length)));
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return null;
  }
  const input = parsed as Record<string, unknown>;
  const camera = merge(defaults.camera, input.camera);
  const rawDistortion = (input.camera as { distortion?: unknown } | undefined)?.distortion;
  const distortion =
    Array.isArray(rawDistortion) && rawDistortion.length === 5 && rawDistortion.every((v) => typeof v === 'number' && Number.isFinite(v))
      ? (rawDistortion as [number, number, number, number, number])
      : defaults.camera.distortion;
  const size = SIZES.includes(camera.width) ? camera.width : defaults.camera.width;
  return {
    pointing: merge(defaults.pointing, input.pointing),
    camera: {
      ...defaults.camera,
      fovDeg: Math.min(60, Math.max(2, camera.fovDeg)),
      width: size,
      height: size,
      distortion,
    },
    sensor: merge(defaults.sensor, input.sensor),
    solver: merge(defaults.solver, input.solver, { method: ['cog', 'iwc'] }),
    seed: typeof input.seed === 'number' && Number.isFinite(input.seed) ? Math.abs(Math.trunc(input.seed)) : defaults.seed,
  };
}
