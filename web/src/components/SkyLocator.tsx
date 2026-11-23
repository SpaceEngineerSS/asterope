import { useEffect, useRef } from 'react';

import type { Intrinsics, Pointing, Quaternion } from '../engine/types';
import { attitudeMatrix } from '../lib/sky';

interface Props {
  /** Attitude to show: the solution, or the truth when there is no lock. */
  q: Quaternion | null;
  pointing: Pointing | null;
  intrinsics: Intrinsics | null;
  width: number;
  height: number;
  locked: boolean;
  /** Earlier boresight positions, oldest first, drawn as a track. */
  path?: readonly Pointing[];
}

const RAD = 180 / Math.PI;

/** RA / Dec (degrees) of a pixel, ignoring lens distortion. */
function pixelToSky(q: Quaternion, k: Intrinsics, u: number, v: number): [number, number] {
  const a = attitudeMatrix(q);
  const b = [(u - k.cx) / k.fx, (v - k.cy) / k.fy, 1] as const;
  // The attitude matrix is orthonormal, so its transpose maps sensor to ICRS.
  const x = a[0][0] * b[0] + a[1][0] * b[1] + a[2][0] * b[2];
  const y = a[0][1] * b[0] + a[1][1] * b[1] + a[2][1] * b[2];
  const z = a[0][2] * b[0] + a[1][2] * b[1] + a[2][2] * b[2];
  const norm = Math.hypot(x, y, z);
  return [((Math.atan2(y, x) * RAD) % 360 + 360) % 360, Math.asin(z / norm) * RAD];
}

/**
 * All-sky locator in a plate carree projection: RA increases to the left, as
 * on the sky seen from inside, and the outline is the sensor footprint.
 */
export function SkyLocator({ q, pointing, intrinsics, width, height, locked, path }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return;
    }
    const ratio = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    canvas.width = Math.round(w * ratio);
    canvas.height = Math.round(h * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, w, h);
    const sx = (raDeg: number) => w - (raDeg / 360) * w;
    const sy = (decDeg: number) => h / 2 - (decDeg / 180) * h;

    context.lineWidth = 1;
    context.strokeStyle = '#262a29';
    context.beginPath();
    for (let ra = 0; ra <= 360; ra += 60) {
      const x = Math.round(sx(ra)) + (ra === 0 ? -0.5 : 0.5);
      context.moveTo(x, 0);
      context.lineTo(x, h);
    }
    for (let dec = -90; dec <= 90; dec += 30) {
      const y = Math.min(h - 0.5, Math.round(sy(dec)) + 0.5);
      context.moveTo(0, y);
      context.lineTo(w, y);
    }
    context.stroke();

    if (path && path.length > 1) {
      context.strokeStyle = 'rgba(123, 216, 143, 0.55)';
      context.beginPath();
      path.forEach((point, i) => {
        const previous = path[i - 1];
        if (!previous || Math.abs(point.raDeg - previous.raDeg) > 180) {
          context.moveTo(sx(point.raDeg), sy(point.decDeg));
        } else {
          context.lineTo(sx(point.raDeg), sy(point.decDeg));
        }
      });
      context.stroke();
    }

    if (!q || !pointing || !intrinsics) {
      return;
    }
    const color = locked ? '#7bd88f' : '#e0a030';
    const edge: [number, number][] = [];
    const steps = 12;
    const corners: [number, number][] = [
      [-0.5, -0.5],
      [width - 0.5, -0.5],
      [width - 0.5, height - 0.5],
      [-0.5, height - 0.5],
    ];
    corners.forEach(([u0, v0], i) => {
      const [u1, v1] = corners[(i + 1) % 4] ?? [u0, v0];
      for (let s = 0; s < steps; s += 1) {
        const t = s / steps;
        edge.push(pixelToSky(q, intrinsics, u0 + (u1 - u0) * t, v0 + (v1 - v0) * t));
      }
    });
    context.strokeStyle = color;
    context.beginPath();
    edge.forEach(([ra, dec], i) => {
      const [previousRa] = edge[(i + edge.length - 1) % edge.length] ?? [ra];
      // Break the outline where it wraps around RA 0 / 360.
      if (i === 0 || Math.abs(ra - previousRa) > 180) {
        context.moveTo(sx(ra), sy(dec));
      } else {
        context.lineTo(sx(ra), sy(dec));
      }
    });
    context.stroke();
    context.fillStyle = color;
    context.fillRect(Math.round(sx(pointing.raDeg)) - 1.5, Math.round(sy(pointing.decDeg)) - 1.5, 3, 3);
  }, [q, pointing, intrinsics, width, height, locked, path]);

  return (
    <figure className="locator">
      <canvas ref={ref} role="img" aria-label="Position of the field of view on the whole sky" />
      <figcaption>
        <span>RA 360&deg;</span>
        <span>all sky &middot; Dec &plusmn;90&deg;</span>
        <span>0&deg;</span>
      </figcaption>
    </figure>
  );
}
