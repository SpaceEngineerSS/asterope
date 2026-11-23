import type { Pointing, Quaternion } from '../engine/types';

const DEG = Math.PI / 180;

export type Vec3 = readonly [number, number, number];
export type Mat3 = readonly [Vec3, Vec3, Vec3];

/** Attitude matrix (ICRS -> sensor) of a scalar-last quaternion: `A = (w^2 - |v|^2) I + 2 v v^T - 2 w [v x]`. */
export function attitudeMatrix(q: Quaternion): Mat3 {
  const [x, y, z, w] = q;
  return [
    [w * w + x * x - y * y - z * z, 2 * (x * y + w * z), 2 * (x * z - w * y)],
    [2 * (x * y - w * z), w * w - x * x + y * y - z * z, 2 * (y * z + w * x)],
    [2 * (x * z + w * y), 2 * (y * z - w * x), w * w - x * x - y * y + z * z],
  ];
}

/** Angle of the rotation between two attitudes, degrees. */
export function rotationAngleDeg(a: Quaternion, b: Quaternion): number {
  const dot = Math.abs(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]);
  return (2 * Math.acos(Math.min(1, dot))) / DEG;
}

export function directionFromRaDec(raDeg: number, decDeg: number): Vec3 {
  const ra = raDeg * DEG;
  const dec = decDeg * DEG;
  return [Math.cos(dec) * Math.cos(ra), Math.cos(dec) * Math.sin(ra), Math.sin(dec)];
}

export interface Projection {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  /** k1, k2, k3, p1, p2 */
  distortion: readonly [number, number, number, number, number];
  /** Distance from the principal point follows the angle from the axis, not its tangent. */
  equidistant?: boolean;
}

/** Pixel position of an ICRS direction, or null when it is behind the sensor. */
export function projectDirection(a: Mat3, dir: Vec3, p: Projection): [number, number] | null {
  const bx = a[0][0] * dir[0] + a[0][1] * dir[1] + a[0][2] * dir[2];
  const by = a[1][0] * dir[0] + a[1][1] * dir[1] + a[1][2] * dir[2];
  const bz = a[2][0] * dir[0] + a[2][1] * dir[1] + a[2][2] * dir[2];
  let x: number;
  let y: number;
  if (p.equidistant) {
    const across = Math.hypot(bx, by);
    const theta = Math.atan2(across, bz);
    if (theta > 2.8) {
      return null;
    }
    x = across > 0 ? (bx / across) * theta : 0;
    y = across > 0 ? (by / across) * theta : 0;
  } else {
    if (bz <= 1e-6) {
      return null;
    }
    x = bx / bz;
    y = by / bz;
  }
  const [k1, k2, k3, p1, p2] = p.distortion;
  const r2 = x * x + y * y;
  const radial = 1 + r2 * (k1 + r2 * (k2 + r2 * k3));
  const xd = x * radial + 2 * p1 * x * y + p2 * (r2 + 2 * x * x);
  const yd = y * radial + p1 * (r2 + 2 * y * y) + 2 * p2 * x * y;
  return [p.fx * xd + p.cx, p.fy * yd + p.cy];
}

function wrap360(value: number): number {
  return ((value % 360) + 360) % 360;
}

type Triple = [number, number, number];

const dot = (a: Triple, b: Triple): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Triple, b: Triple): Triple => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const combine = (a: Triple, ka: number, b: Triple, kb: number): Triple => [a[0] * ka + b[0] * kb, a[1] * ka + b[1] * kb, a[2] * ka + b[2] * kb];
const unit = (a: Triple): Triple => {
  const length = Math.hypot(a[0], a[1], a[2]) || 1;
  return [a[0] / length, a[1] / length, a[2] / length];
};

/** Boresight and the sky directions of image right and image down for a pointing. */
function sensorAxes(pointing: Pointing): { boresight: Triple; right: Triple; down: Triple } {
  const [ra, dec, roll] = [pointing.raDeg * DEG, pointing.decDeg * DEG, pointing.rollDeg * DEG];
  const boresight: Triple = [Math.cos(dec) * Math.cos(ra), Math.cos(dec) * Math.sin(ra), Math.sin(dec)];
  const north: Triple = [-Math.sin(dec) * Math.cos(ra), -Math.sin(dec) * Math.sin(ra), Math.cos(dec)];
  const east: Triple = [-Math.sin(ra), Math.cos(ra), 0];
  const down = combine(north, -Math.cos(roll), east, -Math.sin(roll));
  return { boresight, right: cross(down, boresight), down };
}

/**
 * New pointing after dragging the sky by `(dx, dy)` image pixels.
 *
 * The camera is turned about its own axes, so the star under the cursor
 * stays under it wherever on the sky the camera looks. Image up is at
 * position angle `roll` (north through east) and east is to the left at
 * zero roll, so a sky shift to the right moves the boresight east. Near a
 * pole the roll changes as the camera passes by, which is what keeps the
 * image from spinning under the cursor.
 */
export function slew(pointing: Pointing, dx: number, dy: number, degPerPixel: number): Pointing {
  const { boresight, right, down } = sensorAxes(pointing);
  // One pixel subtends the tangent of the pixel scale at the image center.
  const scale = Math.tan(degPerPixel * DEG);
  const moved = unit(combine(combine(boresight, 1, right, -dx * scale), 1, down, -dy * scale));
  // Image down carried along: its part across the new boresight.
  const carried = unit(combine(down, 1, moved, -dot(down, moved)));
  const dec = Math.asin(Math.max(-1, Math.min(1, moved[2])));
  const ra = Math.atan2(moved[1], moved[0]);
  const north: Triple = [-Math.sin(dec) * Math.cos(ra), -Math.sin(dec) * Math.sin(ra), Math.cos(dec)];
  const east: Triple = [-Math.sin(ra), Math.cos(ra), 0];
  const roll = Math.atan2(-dot(carried, east), -dot(carried, north));
  return { raDeg: wrap360(ra / DEG), decDeg: dec / DEG, rollDeg: wrap360(roll / DEG) };
}

/**
 * How far, in image pixels, the sky seen at `from` is displaced when the
 * camera points at `to`: where the old image center lands in the new frame.
 * It lets a frame already drawn follow the cursor until the next is ready.
 */
export function skyShift(from: Pointing, to: Pointing, degPerPixel: number): [number, number] {
  const old = sensorAxes(from).boresight;
  const { boresight, right, down } = sensorAxes(to);
  const along = dot(old, boresight);
  if (along <= 0.2) {
    return [0, 0];
  }
  const scale = Math.tan(degPerPixel * DEG);
  return [dot(old, right) / along / scale, dot(old, down) / along / scale];
}

export function rollBy(pointing: Pointing, deltaDeg: number): Pointing {
  return { ...pointing, rollDeg: wrap360(pointing.rollDeg + deltaDeg) };
}

const GRID_STEPS = [0.25, 0.5, 1, 2, 5, 10, 15, 30] as const;

/** Grid spacing in degrees giving roughly five lines across the field. */
export function gridStep(fovDeg: number): number {
  const target = fovDeg / 5;
  return GRID_STEPS.find((step) => step >= target) ?? 30;
}

export interface GridLine {
  kind: 'ra' | 'dec';
  valueDeg: number;
  points: [number, number][][];
}

/** Right-ascension and declination lines crossing the frame, as pixel polylines. */
export function graticule(
  a: Mat3,
  projection: Projection,
  center: Pointing,
  fovDeg: number,
  width: number,
  height: number,
): GridLine[] {
  const step = gridStep(fovDeg);
  const reach = fovDeg;
  const margin = 0.25 * Math.max(width, height);
  const inside = (p: [number, number] | null): p is [number, number] =>
    p !== null && p[0] > -margin && p[1] > -margin && p[0] < width + margin && p[1] < height + margin;

  const trace = (sample: (t: number) => Vec3, from: number, to: number, increment: number): [number, number][][] => {
    const segments: [number, number][][] = [];
    let current: [number, number][] = [];
    for (let t = from; t <= to + 1e-9; t += increment) {
      const pixel = projectDirection(a, sample(t), projection);
      if (inside(pixel)) {
        current.push(pixel);
      } else if (current.length > 0) {
        segments.push(current);
        current = [];
      }
    }
    if (current.length > 0) {
      segments.push(current);
    }
    return segments.filter((segment) => segment.length > 1);
  };

  const lines: GridLine[] = [];
  const decLo = Math.max(-90, Math.floor((center.decDeg - reach) / step) * step);
  const decHi = Math.min(90, Math.ceil((center.decDeg + reach) / step) * step);
  const nearPole = Math.abs(center.decDeg) + reach >= 88;
  const cosDec = Math.max(Math.cos(center.decDeg * DEG), 0.02);
  const raReach = nearPole ? 180 : Math.min(180, reach / cosDec);
  const raStep = nearPole ? 30 : (GRID_STEPS.find((s) => s >= step / cosDec) ?? 30);

  for (let dec = decLo; dec <= decHi + 1e-9; dec += step) {
    if (Math.abs(dec) >= 90) {
      continue;
    }
    const points = trace(
      (ra) => directionFromRaDec(ra, dec),
      center.raDeg - raReach,
      center.raDeg + raReach,
      (2 * raReach) / 240,
    );
    if (points.length > 0) {
      lines.push({ kind: 'dec', valueDeg: dec, points });
    }
  }
  const raLo = Math.floor((center.raDeg - raReach) / raStep) * raStep;
  for (let ra = raLo; ra <= center.raDeg + raReach + 1e-9; ra += raStep) {
    if (nearPole && ra - raLo >= 360 - 1e-9) {
      break;
    }
    const points = trace((dec) => directionFromRaDec(ra, dec), Math.max(-89.5, decLo), Math.min(89.5, decHi), step / 24);
    if (points.length > 0) {
      lines.push({ kind: 'ra', valueDeg: wrap360(ra), points });
    }
  }
  return lines;
}
