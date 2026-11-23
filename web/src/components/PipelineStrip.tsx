import type { FrameResult } from '../engine/types';
import { fixed, integer } from '../lib/format';
import type { OverlayOptions } from '../state/store';

/** A pipeline stage whose overlay layer can be shown on its own. */
export type SoloLayer = 'rejected' | 'centroids' | 'pyramid' | 'matches';

/** Overlay with only the chosen stage's layer switched on. */
export function soloOverlay(overlay: OverlayOptions, solo: SoloLayer | null): OverlayOptions {
  if (solo === null) {
    return overlay;
  }
  return {
    ...overlay,
    centroids: solo === 'centroids',
    matches: solo === 'matches',
    labels: solo === 'matches',
    rejected: solo === 'rejected',
    pyramid: solo === 'pyramid',
    residuals: false,
    catalog: false,
    graticule: false,
    reticle: false,
  };
}

interface CellProps {
  label: string;
  value: string;
  detail?: string;
  layer?: SoloLayer;
  solo?: SoloLayer | null;
  onSolo?: (layer: SoloLayer | null) => void;
}

function Cell({ label, value, detail, layer, solo, onSolo }: CellProps) {
  const body = (
    <>
      <span className="strip__label">{label}</span>
      <span className="strip__value">{value}</span>
      {detail ? <span className="strip__detail">{detail}</span> : null}
    </>
  );
  if (!layer || !onSolo) {
    return <div className="strip__cell">{body}</div>;
  }
  const active = solo === layer;
  return (
    <button
      type="button"
      className={active ? 'strip__cell strip__cell--button is-active' : 'strip__cell strip__cell--button'}
      aria-pressed={active}
      title={active ? 'Show all layers' : `Show only the ${label.toLowerCase()} layer`}
      onClick={() => {
        onSolo(active ? null : layer);
      }}
    >
      {body}
    </button>
  );
}

interface Props {
  frame: FrameResult | null;
  solo: SoloLayer | null;
  onSolo: (layer: SoloLayer | null) => void;
}

export function PipelineStrip({ frame, solo, onSolo }: Props) {
  const dash = fixed(NaN, 0);
  if (!frame) {
    return (
      <footer className="strip">
        <Cell label="Pipeline" value={dash} />
      </footer>
    );
  }
  const { detect, solve, timing, index } = frame;
  const rejected = detect.hotPixels + detect.cosmicRays + detect.tooLarge + detect.edge;
  return (
    <footer className="strip">
      <Cell label="Background" value={fixed(detect.background, 1)} detail={`σ ${fixed(detect.noise, 2)} DN`} />
      <Cell label="Blobs" value={integer(detect.blobs)} detail={`${integer(detect.noiseBlobs)} below peak threshold`} />
      <Cell
        label="Rejected"
        value={integer(rejected)}
        detail={`hot ${detect.hotPixels} · cosmic ${detect.cosmicRays} · large ${detect.tooLarge} · edge ${detect.edge}`}
        layer="rejected"
        solo={solo}
        onSolo={onSolo}
      />
      <Cell label="Centroids" value={integer(detect.detections.length)} layer="centroids" solo={solo} onSolo={onSolo} />
      <Cell
        label="Triads"
        value={integer(solve.triads)}
        detail={`${integer(solve.pyramids)} pyramids verified`}
        layer="pyramid"
        solo={solo}
        onSolo={onSolo}
      />
      <Cell
        label="Matched"
        value={integer(solve.attitude?.matches.length ?? 0)}
        layer="matches"
        solo={solo}
        onSolo={onSolo}
      />
      <Cell
        label="Time"
        value={`${fixed(timing.detectMs + timing.identifyMs, 1)} ms`}
        detail={`render ${fixed(timing.renderMs, 1)} · detect ${fixed(timing.detectMs, 1)} · identify ${fixed(timing.identifyMs, 1)}`}
      />
      <Cell
        label="Index"
        value={`${integer(index.stars)} stars`}
        detail={`${integer(index.pairs)} pairs · V ≤ ${fixed(index.magLimit, 2)} · ${fixed(index.pixelScaleArcsec, 1)}″/px`}
      />
    </footer>
  );
}
