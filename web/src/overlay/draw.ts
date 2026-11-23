import { starLabel } from '../data/star-names';
import type { FrameResult, Pointing, Quaternion } from '../engine/types';
import { attitudeMatrix, graticule } from '../lib/sky';
import type { Projection } from '../lib/sky';
import type { OverlayOptions } from '../state/store';

export interface Viewport {
  /** Image coordinates shown at the canvas center. */
  centerX: number;
  centerY: number;
  /** Screen pixels per image pixel. */
  zoom: number;
  width: number;
  height: number;
}

export const COLORS = {
  match: '#7bd88f',
  unmatched: '#e0a030',
  rejected: '#c8553d',
  catalog: '#7d8784',
  grid: 'rgba(200, 205, 199, 0.16)',
  reticle: 'rgba(200, 205, 199, 0.55)',
  label: '#c8cdc7',
} as const;

export function toScreen(view: Viewport, x: number, y: number): [number, number] {
  return [(x - view.centerX) * view.zoom + view.width / 2, (y - view.centerY) * view.zoom + view.height / 2];
}

export function toImage(view: Viewport, sx: number, sy: number): [number, number] {
  return [(sx - view.width / 2) / view.zoom + view.centerX, (sy - view.height / 2) / view.zoom + view.centerY];
}

function crisp(value: number): number {
  return Math.round(value) + 0.5;
}

function cross(ctx: CanvasRenderingContext2D, x: number, y: number, arm: number, gap: number): void {
  const cx = crisp(x);
  const cy = crisp(y);
  ctx.beginPath();
  ctx.moveTo(cx - arm, cy);
  ctx.lineTo(cx - gap, cy);
  ctx.moveTo(cx + gap, cy);
  ctx.lineTo(cx + arm, cy);
  ctx.moveTo(cx, cy - arm);
  ctx.lineTo(cx, cy - gap);
  ctx.moveTo(cx, cy + gap);
  ctx.lineTo(cx, cy + arm);
  ctx.stroke();
}

function saltire(ctx: CanvasRenderingContext2D, x: number, y: number, arm: number): void {
  ctx.beginPath();
  ctx.moveTo(x - arm, y - arm);
  ctx.lineTo(x + arm, y + arm);
  ctx.moveTo(x - arm, y + arm);
  ctx.lineTo(x + arm, y - arm);
  ctx.stroke();
}

function ring(ctx: CanvasRenderingContext2D, x: number, y: number, radius: number): void {
  ctx.beginPath();
  ctx.arc(x, y, radius, 0, 2 * Math.PI);
  ctx.stroke();
}

function drawGraticule(
  ctx: CanvasRenderingContext2D,
  view: Viewport,
  frame: FrameResult,
  q: Quaternion,
  center: Pointing,
  projection: Projection,
): void {
  ctx.strokeStyle = COLORS.grid;
  ctx.lineWidth = 1;
  const lines = graticule(attitudeMatrix(q), projection, center, frame.index.fovXDeg, frame.width, frame.height);
  for (const line of lines) {
    for (const segment of line.points) {
      ctx.beginPath();
      segment.forEach(([x, y], i) => {
        const [sx, sy] = toScreen(view, x, y);
        if (i === 0) {
          ctx.moveTo(sx, sy);
        } else {
          ctx.lineTo(sx, sy);
        }
      });
      ctx.stroke();
    }
  }
}

function drawReticle(ctx: CanvasRenderingContext2D, view: Viewport, frame: FrameResult): void {
  const [x, y] = toScreen(view, (frame.width - 1) / 2, (frame.height - 1) / 2);
  ctx.strokeStyle = COLORS.reticle;
  ctx.lineWidth = 1;
  cross(ctx, x, y, 22, 7);
  ring(ctx, crisp(x), crisp(y), 14);
}

export interface OverlayContext {
  /** Lens distortion of the camera that produced the frame (k1 is taken from the solution). */
  distortion: readonly [number, number, number, number, number];
  /** Detection index emphasized by the star table, or null. */
  highlight: number | null;
}

export function drawOverlay(
  ctx: CanvasRenderingContext2D,
  view: Viewport,
  frame: FrameResult,
  options: OverlayOptions,
  context: OverlayContext,
): void {
  const { detect, solve } = frame;
  const attitude = solve.attitude;
  const k = solve.intrinsics;
  const projection: Projection = {
    fx: k.fx,
    fy: k.fy,
    cx: k.cx,
    cy: k.cy,
    distortion: [k.k1, context.distortion[1], context.distortion[2], context.distortion[3], context.distortion[4]],
    equidistant: k.equidistant,
  };
  ctx.lineWidth = 1;
  ctx.font = '11px "IBM Plex Mono", ui-monospace, monospace';
  ctx.textBaseline = 'middle';

  const gridQ = attitude?.q ?? frame.truthQ;
  const gridCenter = attitude?.pointing ?? frame.truthPointing;
  if (options.graticule && gridQ && gridCenter) {
    drawGraticule(ctx, view, frame, gridQ, gridCenter, projection);
  }
  if (options.reticle) {
    drawReticle(ctx, view, frame);
  }

  const matchByDetection = new Map(attitude?.matches.map((m) => [m.detection, m]) ?? []);

  if (options.catalog) {
    const matchedHips = new Set(attitude?.matches.map((m) => m.hip) ?? []);
    ctx.strokeStyle = COLORS.catalog;
    for (const star of frame.field) {
      if (matchedHips.has(star.hip)) {
        continue;
      }
      const [x, y] = toScreen(view, star.x, star.y);
      const half = Math.max(3, 7 - star.mag * 0.6);
      ctx.strokeRect(crisp(x) - half, crisp(y) - half, 2 * half, 2 * half);
    }
  }

  if (options.rejected) {
    ctx.strokeStyle = COLORS.rejected;
    ctx.globalAlpha = 0.7;
    for (const blob of detect.rejected) {
      const [x, y] = toScreen(view, blob.x, blob.y);
      saltire(ctx, crisp(x), crisp(y), 4);
    }
    ctx.globalAlpha = 1;
  }

  if (options.pyramid && attitude?.pyramid) {
    ctx.strokeStyle = COLORS.match;
    ctx.globalAlpha = 0.55;
    const corners = attitude.pyramid
      .map((index) => detect.detections[index])
      .filter((d) => d !== undefined)
      .map((d) => toScreen(view, d.x, d.y));
    ctx.beginPath();
    corners.forEach((a, i) => {
      corners.slice(i + 1).forEach((b) => {
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
      });
    });
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  if (options.residuals && attitude) {
    ctx.strokeStyle = COLORS.unmatched;
    for (const match of attitude.matches) {
      const detection = detect.detections[match.detection];
      if (!detection || !Number.isFinite(match.predictedX)) {
        continue;
      }
      const [x, y] = toScreen(view, detection.x, detection.y);
      const dx = (detection.x - match.predictedX) * view.zoom * options.residualScale;
      const dy = (detection.y - match.predictedY) * view.zoom * options.residualScale;
      ctx.beginPath();
      ctx.moveTo(x, y);
      ctx.lineTo(x + dx, y + dy);
      ctx.stroke();
    }
  }

  const labelBudget = view.zoom >= 1.5 ? Infinity : 28;
  let labels = 0;
  const placed: { left: number; top: number; right: number; bottom: number }[] = [];
  detect.detections.forEach((detection, index) => {
    const match = matchByDetection.get(index);
    const [x, y] = toScreen(view, detection.x, detection.y);
    if (x < -40 || y < -40 || x > view.width + 40 || y > view.height + 40) {
      return;
    }
    const color = match ? COLORS.match : COLORS.unmatched;
    ctx.strokeStyle = color;
    if (options.centroids) {
      cross(ctx, x, y, 9, 3);
    }
    if (options.matches) {
      if (match) {
        ring(ctx, x, y, 12);
      } else if (!options.centroids) {
        ring(ctx, x, y, 6);
      }
    }
    if (context.highlight === index) {
      ctx.lineWidth = 2;
      ring(ctx, x, y, 17);
      ctx.lineWidth = 1;
    }
    if (options.labels && match && labels < labelBudget) {
      // Detections come brightest first, so a label yields to a brighter star's.
      const text = starLabel(match.hip);
      const box = { left: x + 14, top: y - 17, right: x + 18 + ctx.measureText(text).width, bottom: y - 3 };
      const collides = placed.some(
        (other) => box.left < other.right && box.right > other.left && box.top < other.bottom && box.bottom > other.top,
      );
      if (!collides) {
        placed.push(box);
        labels += 1;
        ctx.fillStyle = COLORS.match;
        ctx.fillText(text, x + 16, y - 10);
      }
    }
  });
}
