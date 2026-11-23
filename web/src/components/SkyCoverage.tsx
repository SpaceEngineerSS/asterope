import { useEffect, useRef, useState } from 'react';
import type { MouseEvent } from 'react';

import { COVERAGE_COLUMNS, COVERAGE_ROWS } from '../engine/types';

interface Props {
  /** Detectable stars per field on the 72 x 36 sky grid, declination +90 first. */
  counts: Uint16Array | null;
}

/** Star-count classes of the map, fewest first. */
export const COVERAGE_CLASSES = [
  { below: 4, color: '#8f3d2c', label: 'under 4 · no pattern possible' },
  { below: 5, color: '#b9852a', label: '4 · a pattern, one star short of acceptance' },
  { below: 8, color: '#2f5a3a', label: '5 – 7 · solvable' },
  { below: Infinity, color: '#58a86b', label: '8 or more' },
] as const;

function colorOf(count: number): string {
  return (COVERAGE_CLASSES.find((entry) => count < entry.below) ?? COVERAGE_CLASSES[3]).color;
}

interface Probe {
  raDeg: number;
  decDeg: number;
  count: number;
}

/**
 * All-sky map of how many stars the designed camera would detect, in a
 * plate carree projection with right ascension increasing to the left.
 */
export function SkyCoverage({ counts }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [probe, setProbe] = useState<Probe | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return undefined;
    }
    const render = () => {
      const ratio = window.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);
      const cellWidth = width / COVERAGE_COLUMNS;
      const cellHeight = height / COVERAGE_ROWS;
      if (counts) {
        for (let row = 0; row < COVERAGE_ROWS; row += 1) {
          for (let column = 0; column < COVERAGE_COLUMNS; column += 1) {
            context.fillStyle = colorOf(counts[row * COVERAGE_COLUMNS + column] ?? 0);
            const left = Math.floor(width - (column + 1) * cellWidth);
            const top = Math.floor(row * cellHeight);
            context.fillRect(left, top, Math.ceil(cellWidth), Math.ceil(cellHeight));
          }
        }
      }
      context.strokeStyle = 'rgba(11, 12, 12, 0.55)';
      context.lineWidth = 1;
      context.beginPath();
      for (let ra = 60; ra < 360; ra += 60) {
        const x = Math.round(width - (ra / 360) * width) + 0.5;
        context.moveTo(x, 0);
        context.lineTo(x, height);
      }
      for (let dec = -60; dec <= 60; dec += 30) {
        const y = Math.round(height / 2 - (dec / 180) * height) + 0.5;
        context.moveTo(0, y);
        context.lineTo(width, y);
      }
      context.stroke();
    };
    render();
    const observer = new ResizeObserver(render);
    observer.observe(canvas);
    return () => {
      observer.disconnect();
    };
  }, [counts]);

  const onMove = (event: MouseEvent<HTMLCanvasElement>) => {
    if (!counts) {
      return;
    }
    const box = event.currentTarget.getBoundingClientRect();
    const column = Math.min(COVERAGE_COLUMNS - 1, Math.max(0, Math.floor(((box.right - event.clientX) / box.width) * COVERAGE_COLUMNS)));
    const row = Math.min(COVERAGE_ROWS - 1, Math.max(0, Math.floor(((event.clientY - box.top) / box.height) * COVERAGE_ROWS)));
    setProbe({
      raDeg: (column + 0.5) * (360 / COVERAGE_COLUMNS),
      decDeg: 90 - (row + 0.5) * (180 / COVERAGE_ROWS),
      count: counts[row * COVERAGE_COLUMNS + column] ?? 0,
    });
  };

  return (
    <figure className="coverage">
      <canvas
        ref={ref}
        role="img"
        aria-label="Detectable stars per field over the whole sky"
        onMouseMove={onMove}
        onMouseLeave={() => {
          setProbe(null);
        }}
      />
      <figcaption>
        <span>RA 360&deg;</span>
        <span className="coverage__probe">
          {probe
            ? `RA ${probe.raDeg.toFixed(1)}° · Dec ${probe.decDeg >= 0 ? '+' : '−'}${Math.abs(probe.decDeg).toFixed(1)}° · ${probe.count} stars`
            : 'all sky · Dec ±90° · stars detectable in one field'}
        </span>
        <span>0&deg;</span>
      </figcaption>
      <ul className="coverage__legend">
        {COVERAGE_CLASSES.map((entry) => (
          <li key={entry.label}>
            <span className="coverage__swatch" style={{ background: entry.color }} />
            {entry.label}
          </li>
        ))}
      </ul>
    </figure>
  );
}
