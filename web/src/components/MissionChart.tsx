import { useEffect, useRef } from 'react';
import type { MouseEvent } from 'react';

import type { Blinder, MissionReport, MissionSpec } from '../engine/types';

interface Props {
  report: MissionReport | null;
  spec: MissionSpec;
  selected: number | null;
  onSelect: (index: number) => void;
}

const INK = { sun: '#e0a030', earth: '#c8cdc7', moon: '#7f8884', lock: '#7bd88f', fault: '#c8553d', grid: '#262a29', rule: '#3a403e' };
const STATUS: Record<Blinder, string> = { none: INK.lock, sun: INK.sun, earth: INK.fault, moon: INK.moon };
const FONT = '10px "IBM Plex Mono", ui-monospace, monospace';
const MARGIN = { left: 38, right: 10, top: 8, bottom: 44 };

function prepare(canvas: HTMLCanvasElement, context: CanvasRenderingContext2D): { width: number; height: number } {
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, width, height);
  return { width, height };
}

function useCanvas(draw: (canvas: HTMLCanvasElement, context: CanvasRenderingContext2D) => void, inputs: unknown[]) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return undefined;
    }
    const render = () => {
      draw(canvas, context);
    };
    render();
    const observer = new ResizeObserver(render);
    observer.observe(canvas);
    return () => {
      observer.disconnect();
    };
    // The drawing closes over the inputs listed by the caller.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, inputs);
  return ref;
}

/**
 * Angles from the boresight to the Sun, the Earth's limb and the Moon
 * through the mission, with each exclusion angle as a dashed line and a
 * strip underneath that shows what, if anything, blinds the camera.
 */
export function MissionAngles({ report, spec, selected, onSelect }: Props) {
  const ref = useCanvas(
    (canvas, context) => {
      const { width, height } = prepare(canvas, context);
      const plot = { x: MARGIN.left, y: MARGIN.top, w: width - MARGIN.left - MARGIN.right, h: height - MARGIN.top - MARGIN.bottom };
      const y = (angle: number) => plot.y + plot.h * (1 - Math.max(0, Math.min(180, angle)) / 180);
      context.font = FONT;
      context.lineWidth = 1;
      context.strokeStyle = INK.grid;
      context.fillStyle = INK.moon;
      context.textAlign = 'right';
      context.textBaseline = 'middle';
      context.beginPath();
      for (let angle = 0; angle <= 180; angle += 30) {
        const line = Math.round(y(angle)) + 0.5;
        context.moveTo(plot.x, line);
        context.lineTo(plot.x + plot.w, line);
        context.fillText(`${angle}°`, plot.x - 6, line);
      }
      context.stroke();
      const samples = report?.samples ?? [];
      const last = samples[samples.length - 1];
      if (!last) {
        return;
      }
      const span = last.seconds || 1;
      const x = (seconds: number) => plot.x + (seconds / span) * plot.w;

      const limits: [number, string][] = [
        [spec.sunExclusionDeg, INK.sun],
        [spec.earthLimbExclusionDeg, INK.earth],
        [spec.moonExclusionDeg, INK.moon],
      ];
      context.setLineDash([2, 5]);
      for (const [limit, ink] of limits) {
        context.strokeStyle = ink;
        context.beginPath();
        context.moveTo(plot.x, Math.round(y(limit)) + 0.5);
        context.lineTo(plot.x + plot.w, Math.round(y(limit)) + 0.5);
        context.stroke();
      }
      context.setLineDash([]);

      const curves: [(index: number) => number, string][] = [
        [(index) => samples[index]?.moonDeg ?? 0, INK.moon],
        [(index) => samples[index]?.earthLimbDeg ?? 0, INK.earth],
        [(index) => samples[index]?.sunDeg ?? 0, INK.sun],
      ];
      for (const [value, ink] of curves) {
        context.strokeStyle = ink;
        context.beginPath();
        samples.forEach((sample, index) => {
          const px = x(sample.seconds);
          const py = y(value(index));
          if (index === 0) {
            context.moveTo(px, py);
          } else {
            context.lineTo(px, py);
          }
        });
        context.stroke();
      }

      // What blinds the camera, and when it is in the Earth's shadow.
      const strip = plot.y + plot.h + 8;
      const step = plot.w / samples.length;
      samples.forEach((sample, index) => {
        context.fillStyle = STATUS[sample.blinder];
        context.fillRect(plot.x + index * step, strip, Math.ceil(step), 8);
        if (sample.eclipse) {
          context.fillStyle = INK.rule;
          context.fillRect(plot.x + index * step, strip + 11, Math.ceil(step), 3);
        }
      });
      context.fillStyle = INK.moon;
      context.textAlign = 'left';
      context.textBaseline = 'top';
      const minutes = span / 60;
      const tick = minutes > 400 ? 120 : minutes > 150 ? 60 : minutes > 60 ? 20 : 10;
      for (let minute = 0; minute <= minutes; minute += tick) {
        context.fillText(`${minute} min`, x(minute * 60), strip + 20);
      }
      const picked = selected === null ? undefined : samples[selected];
      if (picked) {
        const px = Math.round(x(picked.seconds)) + 0.5;
        context.strokeStyle = INK.earth;
        context.beginPath();
        context.moveTo(px, plot.y);
        context.lineTo(px, strip + 14);
        context.stroke();
      }
    },
    [report, spec, selected],
  );

  const pick = (event: MouseEvent<HTMLCanvasElement>) => {
    const count = report?.samples.length ?? 0;
    if (count === 0) {
      return;
    }
    const box = event.currentTarget.getBoundingClientRect();
    const fraction = (event.clientX - box.left - MARGIN.left) / (box.width - MARGIN.left - MARGIN.right);
    onSelect(Math.max(0, Math.min(count - 1, Math.round(fraction * (count - 1)))));
  };

  return (
    <figure className="mission-chart">
      <canvas ref={ref} className="mission-chart__angles" onClick={pick} aria-label="Angles from the boresight through the mission" />
      <figcaption>
        <span><i style={{ background: INK.sun }} />Sun</span>
        <span><i style={{ background: INK.earth }} />Earth&rsquo;s limb</span>
        <span><i style={{ background: INK.moon }} />Moon</span>
        <span>dashed: exclusion angle</span>
        <span><i style={{ background: INK.lock }} />clear</span>
        <span><i style={{ background: INK.fault }} />blinded by the Earth</span>
        <span><i style={{ background: INK.rule }} />in shadow</span>
      </figcaption>
    </figure>
  );
}

/** Where the boresight goes on the sky, coloured by what blinds it. */
export function MissionTrack({ report, selected }: Pick<Props, 'report' | 'selected'>) {
  const ref = useCanvas(
    (canvas, context) => {
      const { width, height } = prepare(canvas, context);
      context.lineWidth = 1;
      context.strokeStyle = INK.grid;
      context.beginPath();
      for (let ra = 0; ra <= 360; ra += 60) {
        const px = Math.round(width - (ra / 360) * width) + 0.5;
        context.moveTo(px, 0);
        context.lineTo(px, height);
      }
      for (let dec = -60; dec <= 60; dec += 30) {
        const py = Math.round(height / 2 - (dec / 180) * height) + 0.5;
        context.moveTo(0, py);
        context.lineTo(width, py);
      }
      context.stroke();
      const samples = report?.samples ?? [];
      samples.forEach((sample, index) => {
        const px = width - (sample.pointing.raDeg / 360) * width;
        const py = height / 2 - (sample.pointing.decDeg / 180) * height;
        context.fillStyle = STATUS[sample.blinder];
        const size = index === selected ? 7 : 2;
        context.fillRect(px - size / 2, py - size / 2, size, size);
      });
    },
    [report, selected],
  );
  return (
    <figure className="mission-chart">
      <canvas ref={ref} className="mission-chart__track" aria-label="Track of the boresight on the sky" />
      <figcaption>
        <span>RA 360&deg;</span>
        <span>boresight on the sky &middot; Dec &plusmn;90&deg;</span>
        <span>RA 0&deg;</span>
      </figcaption>
    </figure>
  );
}
