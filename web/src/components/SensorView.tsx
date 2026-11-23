import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent, ReactNode } from 'react';

import { STAR_NAMES, starLabel } from '../data/star-names';
import type { FrameResult } from '../engine/types';
import { fixed } from '../lib/format';
import { buildLut, maximumOf, paint } from '../lib/stretch';
import type { Stretch } from '../lib/stretch';
import { drawOverlay, toImage } from '../overlay/draw';
import type { Viewport } from '../overlay/draw';
import type { OverlayOptions } from '../state/store';

interface Props {
  frame: FrameResult | null;
  overlay: OverlayOptions;
  stretch: Stretch;
  distortion: readonly [number, number, number, number, number];
  highlight: number | null;
  /** Dragging slews the pointing instead of panning the view. */
  onSlew?: (dxPixels: number, dyPixels: number) => void;
  /**
   * Image pixels by which the sky has moved since this frame was drawn. The
   * frame is shown displaced by it, so that a drag is followed at once and
   * the next frame only has to fill in the edge.
   */
  shift?: readonly [number, number];
  onRoll?: (deltaDeg: number) => void;
  onStretch: (stretch: Stretch) => void;
  placeholder?: ReactNode;
}

interface Size {
  width: number;
  height: number;
}

interface View {
  centerX: number;
  centerY: number;
  zoom: number;
}

interface Hover {
  x: number;
  y: number;
  value: number;
  detection: number | null;
}

const STRETCHES: readonly Stretch[] = ['linear', 'asinh', 'zscale'];
const ROLL_DEG_PER_PIXEL = 0.2;
const PICK_RADIUS_PX = 8;

function fitView(frame: FrameResult, size: Size): View {
  const zoom = Math.min(size.width / frame.width, size.height / frame.height);
  return { centerX: (frame.width - 1) / 2, centerY: (frame.height - 1) / 2, zoom: zoom > 0 ? zoom : 1 };
}

export function SensorView({
  frame,
  overlay,
  stretch,
  distortion,
  highlight,
  onSlew,
  shift,
  onRoll,
  onStretch,
  placeholder,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const baseRef = useRef<HTMLCanvasElement | null>(null);
  const dragRef = useRef<{ x: number; y: number } | null>(null);
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  const [view, setView] = useState<View | null>(null);
  const [hover, setHover] = useState<Hover | null>(null);
  const [baseVersion, setBaseVersion] = useState(0);

  const frameWidth = frame?.width ?? 0;
  const frameHeight = frame?.height ?? 0;

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const observer = new ResizeObserver(([entry]) => {
      if (entry) {
        setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
      }
    });
    observer.observe(container);
    return () => {
      observer.disconnect();
    };
  }, []);

  const fitted = useMemo(
    () => (frame && size.width > 0 ? fitView(frame, size) : null),
    // Refit only when the frame geometry or the viewport changes, not on every new frame.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [frameWidth, frameHeight, size.width, size.height],
  );

  useEffect(() => {
    setView(fitted);
  }, [fitted]);

  useEffect(() => {
    if (!frame) {
      baseRef.current = null;
      return;
    }
    const base = baseRef.current ?? document.createElement('canvas');
    if (base.width !== frame.width || base.height !== frame.height) {
      base.width = frame.width;
      base.height = frame.height;
    }
    const context = base.getContext('2d');
    if (!context) {
      return;
    }
    const image = context.createImageData(frame.width, frame.height);
    const lut = buildLut(stretch, {
      background: frame.detect.background,
      noise: frame.detect.noise,
      maximum: maximumOf(frame.pixels),
    });
    paint(frame.pixels, lut, image.data);
    context.putImageData(image, 0, 0);
    baseRef.current = base;
    setBaseVersion((version) => version + 1);
  }, [frame, stretch]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context || size.width === 0) {
      return;
    }
    const ratio = window.devicePixelRatio || 1;
    const width = Math.round(size.width * ratio);
    const height = Math.round(size.height * ratio);
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.fillStyle = '#050606';
    context.fillRect(0, 0, size.width, size.height);
    const base = baseRef.current;
    if (!frame || !view || !base) {
      return;
    }
    const [shiftX, shiftY] = shift ?? [0, 0];
    const shown = { ...view, centerX: view.centerX - shiftX, centerY: view.centerY - shiftY };
    const viewport: Viewport = { ...shown, width: size.width, height: size.height };
    context.imageSmoothingEnabled = view.zoom < 1;
    const left = (-0.5 - shown.centerX) * view.zoom + size.width / 2;
    const top = (-0.5 - shown.centerY) * view.zoom + size.height / 2;
    context.drawImage(base, left, top, frame.width * view.zoom, frame.height * view.zoom);
    drawOverlay(context, viewport, frame, overlay, { distortion, highlight });
  }, [frame, view, size, overlay, distortion, highlight, baseVersion, shift]);

  const zoomAbout = useCallback(
    (factor: number, sx: number, sy: number) => {
      setView((current) => {
        if (!current || !fitted) {
          return current;
        }
        const zoom = Math.min(Math.max(current.zoom * factor, fitted.zoom * 0.5), 48);
        const viewport = { ...current, width: size.width, height: size.height };
        const [ix, iy] = toImage(viewport, sx, sy);
        return {
          zoom,
          centerX: ix - (sx - size.width / 2) / zoom,
          centerY: iy - (sy - size.height / 2) / zoom,
        };
      });
    },
    [fitted, size.width, size.height],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const bounds = canvas.getBoundingClientRect();
      zoomAbout(Math.exp(-event.deltaY * 0.0015), event.clientX - bounds.left, event.clientY - bounds.top);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });
    return () => {
      canvas.removeEventListener('wheel', onWheel);
    };
  }, [zoomAbout]);

  const onPointerDown = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = { x: event.clientX, y: event.clientY };
  };

  const onPointerUp = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.releasePointerCapture(event.pointerId);
    dragRef.current = null;
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!frame || !view) {
      return;
    }
    const drag = dragRef.current;
    if (drag) {
      const dx = event.clientX - drag.x;
      const dy = event.clientY - drag.y;
      dragRef.current = { x: event.clientX, y: event.clientY };
      if (onRoll && event.shiftKey) {
        onRoll(dx * ROLL_DEG_PER_PIXEL);
      } else if (onSlew && !event.altKey) {
        onSlew(dx / view.zoom, dy / view.zoom);
      } else {
        setView({ ...view, centerX: view.centerX - dx / view.zoom, centerY: view.centerY - dy / view.zoom });
      }
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    const viewport = { ...view, width: size.width, height: size.height };
    const [x, y] = toImage(viewport, event.clientX - bounds.left, event.clientY - bounds.top);
    const px = Math.round(x);
    const py = Math.round(y);
    if (px < 0 || py < 0 || px >= frame.width || py >= frame.height) {
      setHover(null);
      return;
    }
    let detection: number | null = null;
    let best = PICK_RADIUS_PX / view.zoom;
    frame.detect.detections.forEach((d, index) => {
      const distance = Math.hypot(d.x - x, d.y - y);
      if (distance < best) {
        best = distance;
        detection = index;
      }
    });
    setHover({ x, y, value: frame.pixels[py * frame.width + px] ?? 0, detection });
  };

  const hovered = hover && hover.detection !== null && frame ? frame.detect.detections[hover.detection] : undefined;
  const hoveredMatch =
    hover && hover.detection !== null
      ? frame?.solve.attitude?.matches.find((m) => m.detection === hover.detection)
      : undefined;

  return (
    <section className="sensor">
      <div className="sensor__stage" ref={containerRef}>
        <canvas
          ref={canvasRef}
          className={onSlew ? 'sensor__canvas sensor__canvas--slew' : 'sensor__canvas'}
          onPointerDown={onPointerDown}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onPointerMove={onPointerMove}
          onPointerLeave={() => {
            setHover(null);
          }}
          onDoubleClick={() => {
            setView(fitted);
          }}
        />
        {!frame && placeholder ? <div className="sensor__placeholder">{placeholder}</div> : null}
      </div>
      <footer className="sensor__bar">
        <span className="readout">
          {hover ? (
            <>
              <b>x</b> {fixed(hover.x, 2)} <b>y</b> {fixed(hover.y, 2)} <b>DN</b> {hover.value}
              {hovered ? (
                <>
                  {' '}
                  <b>SNR</b> {fixed(hovered.snr, 0)}
                </>
              ) : null}
              {hoveredMatch ? (
                <span className="readout__match">
                  {' '}
                  {STAR_NAMES.has(hoveredMatch.hip) ? `${starLabel(hoveredMatch.hip)} · ` : ''}HIP {hoveredMatch.hip} <b>m</b> {fixed(hoveredMatch.mag, 2)} <b>res</b>{' '}
                  {fixed(hoveredMatch.residualArcsec, 1)}&Prime;
                </span>
              ) : null}
            </>
          ) : (
            <span className="readout__hint">
              {onSlew ? 'drag: slew · shift+drag: roll · alt+drag: pan · wheel: zoom' : 'drag: pan · wheel: zoom · double-click: fit'}
            </span>
          )}
        </span>
        <span className="sensor__tools">
          <span className="segmented" role="group" aria-label="Display stretch">
            {STRETCHES.map((option) => (
              <button
                key={option}
                type="button"
                className={option === stretch ? 'is-active' : undefined}
                aria-pressed={option === stretch}
                onClick={() => {
                  onStretch(option);
                }}
              >
                {option}
              </button>
            ))}
          </span>
          <span className="readout">
            <b>zoom</b> {view ? fixed(view.zoom, 2) : fixed(NaN, 2)}&times;
          </span>
        </span>
      </footer>
    </section>
  );
}
