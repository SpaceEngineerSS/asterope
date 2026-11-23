import { useEffect, useRef } from 'react';
import type { MouseEvent } from 'react';

import type { TrackEvent } from '../engine/types';
import { angle360, angle90, fixed } from '../lib/format';
import type { VideoSample } from '../state/video';

interface Props {
  samples: readonly VideoSample[];
  /** Length of the time axis in seconds; null follows the newest sample. */
  durationS: number | null;
  selected: number | null;
  onSelect: (index: number) => void;
}

const ROW = 34;
const STATE_ROW = 14;
const AXIS = 16;

const EVENT_COLOR: Record<TrackEvent, string> = {
  acquired: '#c8cdc7',
  tracked: '#7bd88f',
  coasting: '#e0a030',
  lost: '#c8553d',
};

const EVENT_LABEL: Record<TrackEvent, string> = {
  acquired: 'ACQUIRE',
  tracked: 'TRACK',
  coasting: 'COAST',
  lost: 'LOST',
};

interface Trace {
  label: string;
  unit: string;
  /** Values wrap at 360 and the line is broken there. */
  wraps: boolean;
  value: (sample: VideoSample) => number | null;
  format: (value: number) => string;
}

const TRACES: readonly Trace[] = [
  { label: 'RA', unit: '°', wraps: true, value: (s) => s.pointing?.raDeg ?? null, format: (v) => angle360(v, 3) },
  { label: 'DEC', unit: '°', wraps: false, value: (s) => s.pointing?.decDeg ?? null, format: (v) => angle90(v, 3) },
  { label: 'ROLL', unit: '°', wraps: true, value: (s) => s.pointing?.rollDeg ?? null, format: (v) => angle360(v, 3) },
  { label: 'RATE', unit: '°/s', wraps: false, value: (s) => s.rateDegS, format: (v) => fixed(v, 3) },
];

const HEIGHT = STATE_ROW + TRACES.length * ROW + AXIS;

function axisStep(span: number): number {
  const steps = [0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300, 600];
  return steps.find((step) => span / step <= 10) ?? 1200;
}

function draw(
  context: CanvasRenderingContext2D,
  width: number,
  samples: readonly VideoSample[],
  span: number,
  selected: number | null,
): void {
  context.clearRect(0, 0, width, HEIGHT);
  const x = (timeS: number) => (timeS / span) * (width - 1);
  const period = samples.length > 1 ? ((samples.at(-1)?.timeS ?? 0) - (samples[0]?.timeS ?? 0)) / (samples.length - 1) : span;

  samples.forEach((sample) => {
    context.fillStyle = EVENT_COLOR[sample.event];
    const left = Math.floor(x(sample.timeS));
    context.fillRect(left, 2, Math.max(1, Math.ceil(x(period)) - 1), STATE_ROW - 5);
  });

  context.lineWidth = 1;
  TRACES.forEach((trace, row) => {
    const top = STATE_ROW + row * ROW;
    context.strokeStyle = '#262a29';
    context.beginPath();
    context.moveTo(0, top + 0.5);
    context.lineTo(width, top + 0.5);
    context.stroke();

    const points = samples.map((sample) => ({ timeS: sample.timeS, value: trace.value(sample) }));
    const values = points.map((p) => p.value).filter((v): v is number => v !== null && Number.isFinite(v));
    if (values.length === 0) {
      return;
    }
    const low = Math.min(...values);
    const high = Math.max(...values);
    const range = Math.max(high - low, 1e-6);
    const y = (value: number) => top + ROW - 5 - ((value - low) / range) * (ROW - 10);
    context.strokeStyle = '#c8cdc7';
    context.beginPath();
    let previous: number | null = null;
    for (const point of points) {
      if (point.value === null || !Number.isFinite(point.value)) {
        previous = null;
        continue;
      }
      const broken = previous === null || (trace.wraps && Math.abs(point.value - previous) > 180);
      if (broken) {
        context.moveTo(x(point.timeS), y(point.value));
        // A lone solved sample still leaves a mark.
        context.lineTo(x(point.timeS) + 1, y(point.value));
      } else {
        context.lineTo(x(point.timeS), y(point.value));
      }
      previous = point.value;
    }
    context.stroke();
  });

  const axisTop = STATE_ROW + TRACES.length * ROW;
  context.strokeStyle = '#3a403e';
  context.fillStyle = '#7f8884';
  context.font = '10px "IBM Plex Mono", ui-monospace, monospace';
  context.textBaseline = 'top';
  context.beginPath();
  context.moveTo(0, axisTop + 0.5);
  context.lineTo(width, axisTop + 0.5);
  const step = axisStep(span);
  for (let t = 0; t <= span + 1e-9; t += step) {
    const tick = Math.min(width - 1, Math.round(x(t))) + 0.5;
    context.moveTo(tick, axisTop);
    context.lineTo(tick, axisTop + 4);
    const label = `${Number.isInteger(step) ? t.toFixed(0) : t.toFixed(1)} s`;
    const textWidth = context.measureText(label).width;
    context.fillText(label, Math.min(Math.max(0, tick + 3), width - textWidth), axisTop + 4);
  }
  context.stroke();

  const cursor = selected === null ? undefined : samples[selected];
  if (cursor) {
    context.strokeStyle = '#e0a030';
    context.beginPath();
    const cx = Math.round(x(cursor.timeS)) + 0.5;
    context.moveTo(cx, 0);
    context.lineTo(cx, axisTop);
    context.stroke();
  }
}

/** Attitude and tracker state of every solved video frame against time. */
export function Timeline({ samples, durationS, selected, onSelect }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const newest = samples.at(-1);
  const span = Math.max(durationS ?? newest?.timeS ?? 0, 1e-3);
  const shown = selected === null ? newest : samples[selected];

  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return undefined;
    }
    const render = () => {
      const ratio = window.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(HEIGHT * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      draw(context, width, samples, span, selected);
    };
    render();
    const observer = new ResizeObserver(render);
    observer.observe(canvas);
    return () => {
      observer.disconnect();
    };
  }, [samples, span, selected]);

  const pick = (event: MouseEvent<HTMLCanvasElement>) => {
    if (samples.length === 0) {
      return;
    }
    const box = event.currentTarget.getBoundingClientRect();
    const timeS = ((event.clientX - box.left) / Math.max(box.width - 1, 1)) * span;
    let best = 0;
    samples.forEach((sample, index) => {
      if (Math.abs(sample.timeS - timeS) < Math.abs((samples[best]?.timeS ?? 0) - timeS)) {
        best = index;
      }
    });
    onSelect(best);
  };

  return (
    <section className="timeline" aria-label="Attitude against time">
      <ul className="timeline__labels" style={{ gridTemplateRows: `${STATE_ROW}px repeat(${TRACES.length}, ${ROW}px)` }}>
        <li>STATE</li>
        {TRACES.map((trace) => (
          <li key={trace.label}>{trace.label}</li>
        ))}
      </ul>
      <canvas ref={ref} style={{ height: HEIGHT }} onClick={pick} role="img" aria-label="Tracker state, right ascension, declination, roll and angular rate against video time" />
      <ul className="timeline__values" style={{ gridTemplateRows: `${STATE_ROW}px repeat(${TRACES.length}, ${ROW}px)` }}>
        <li className={shown ? `timeline__state timeline__state--${shown.event}` : 'timeline__state'}>
          {shown ? EVENT_LABEL[shown.event] : 'STANDBY'}
        </li>
        {TRACES.map((trace) => {
          const value = shown ? trace.value(shown) : null;
          return (
            <li key={trace.label}>
              {value === null ? fixed(NaN, 3) : trace.format(value)}
              <span className="unit">{trace.unit}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
