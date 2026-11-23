import { useRef, useState } from 'react';
import type { DragEvent } from 'react';

import { NumberField, Section, Toggle } from '../components/controls';
import { OverlayControls } from '../components/OverlayControls';
import { PipelineStrip, soloOverlay } from '../components/PipelineStrip';
import type { SoloLayer } from '../components/PipelineStrip';
import { SensorView } from '../components/SensorView';
import { SolutionPanel } from '../components/SolutionPanel';
import { MAX_SIDE } from '../image/decode';
import {
  SAMPLES,
  loadImageFile,
  loadSample,
  setAutoTolerance,
  setImageCamera,
  setImageMirrored,
  setImageSolver,
  setStretch,
  setUnknownFov,
  stopImageSearch,
} from '../state/store';
import type { AppState } from '../state/store';

const ACCEPT = '.png,.jpg,.jpeg,.fits,.fit,.fts,image/png,image/jpeg';

export function ImageBench({ state }: { state: AppState }) {
  const { camera, solver, frame, busy, error, fileName, binning, fovSource, autoTolerance, unknownFov, mirrored, progress } =
    state.image;
  const [highlight, setHighlight] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const [solo, setSolo] = useState<SoloLayer | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const open = (file: File | undefined) => {
    if (file) {
      void loadImageFile(file);
    }
  };

  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(false);
    open(event.dataTransfer.files[0]);
  };

  return (
    <div
      className={dragging ? 'bench bench--drop' : 'bench'}
      onDragOver={(event) => {
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => {
        setDragging(false);
      }}
      onDrop={onDrop}
    >
      <aside className="panel panel--left">
        <Section title="Frame">
          <button
            type="button"
            className="dropzone"
            onClick={() => {
              inputRef.current?.click();
            }}
          >
            <span className="dropzone__title">{fileName ?? 'Drop a night-sky image'}</span>
            <span className="dropzone__hint">
              {fileName && camera.width > 0
                ? `${camera.width} × ${camera.height} px${binning > 1 ? ` · binned ${binning}×` : ''}`
                : 'PNG, JPEG or FITS · click to browse'}
            </span>
          </button>
          <input
            ref={inputRef}
            type="file"
            accept={ACCEPT}
            hidden
            onChange={(event) => {
              open(event.currentTarget.files?.[0]);
              event.currentTarget.value = '';
            }}
          />
          <ul className="samples">
            {SAMPLES.map((sample) => (
              <li key={sample.file}>
                <button
                  type="button"
                  className="link"
                  title={sample.credit}
                  onClick={() => {
                    void loadSample(sample);
                  }}
                >
                  {sample.label}
                </button>
                <span>{sample.credit}</span>
              </li>
            ))}
          </ul>
        </Section>

        <Section title="Optics" aside={unknownFov ? 'searched' : fovSource === 'exif' ? 'from EXIF' : 'entered'}>
          <Toggle label="Field of view unknown" checked={unknownFov} onChange={setUnknownFov} />
          <Toggle label="Mirrored image" checked={mirrored} onChange={setImageMirrored} />
          <NumberField label={unknownFov ? 'Starting guess' : 'Field of view'} value={camera.fovDeg} min={2} max={110} step={0.5} digits={2} unit={'°'} slider
            title={unknownFov
              ? 'Fields from 6° to 100° are searched, starting nearest to this value'
              : 'Horizontal field of view of the full image'}
            onChange={(fovDeg) => { setImageCamera({ fovDeg }); }} />
          {unknownFov ? null : (
            <Toggle label="Wide-angle (equidistant) lens" checked={camera.equidistant ?? false}
              onChange={(equidistant) => { setImageCamera({ equidistant }); }} />
          )}
          {unknownFov ? null : (
            <NumberField label="Scale search" value={camera.scaleSpan * 100} min={0} max={30} step={1} digits={0} unit={'±%'}
              title="How far the solver may deviate from the entered field of view"
              onChange={(percent) => { setImageCamera({ scaleSpan: percent / 100 }); }} />
          )}
        </Section>

        <Section title="Detection">
          <NumberField label="Peak threshold" value={solver.peakSigma} min={3} max={60} step={0.5} digits={1} unit={'σ'} slider
            onChange={(peakSigma) => { setImageSolver({ peakSigma, segmentSigma: Math.min(3, peakSigma) }); }} />
          <NumberField label="Min area" value={solver.minArea} min={1} max={50} step={1} digits={0} unit="px"
            onChange={(minArea) => { setImageSolver({ minArea }); }} />
          <NumberField label="Max area" value={solver.maxArea} min={50} max={20000} step={50} digits={0} unit="px"
            onChange={(maxArea) => { setImageSolver({ maxArea }); }} />
          <NumberField label="Max elongation" value={solver.maxElongation} min={1.2} max={10} step={0.1} digits={1}
            title="Rejects trailed stars and streaks above this axis ratio"
            onChange={(maxElongation) => { setImageSolver({ maxElongation }); }} />
        </Section>

        <Section title="Solver" aside={autoTolerance ? 'tolerance from field of view' : 'tolerance entered'}>
          <Toggle label="Automatic tolerance" checked={autoTolerance} onChange={setAutoTolerance} />
          <NumberField label="Pair tolerance" value={solver.tolerancePx} min={0.2} max={20} step={0.1} digits={1} unit="px"
            title="Allowed mismatch between measured and catalog star separations; wide lenses need more"
            onChange={(tolerancePx) => { setImageSolver({ tolerancePx, matchRadiusPx: Math.max(solver.matchRadiusPx, tolerancePx) }); }} />
          <NumberField label="Match radius" value={solver.matchRadiusPx} min={0.5} max={30} step={0.1} digits={1} unit="px"
            onChange={(matchRadiusPx) => { setImageSolver({ matchRadiusPx }); }} />
          <NumberField label="Pattern stars" value={solver.patternStars} min={4} max={30} step={1} digits={0}
            onChange={(patternStars) => { setImageSolver({ patternStars }); }} />
        </Section>

        <OverlayControls bench="image" overlay={state.overlay.image} />

        <Section title="Limits">
          <ul className="notes">
            <li>With a known field of view a solve takes milliseconds; an unknown one is searched from 6&deg; to 100&deg; and can take a minute.</li>
            <li>The catalog stops at V 6.5; fields narrower than about 8&deg; often hold too few stars.</li>
            <li>Bright nebulosity, captions and watermarks are detected as blobs and can crowd out real stars.</li>
            <li>Trailed stars, heavy JPEG compression and strong lens distortion reduce the match count.</li>
            <li>The search has a budget of candidate patterns per field and reports no lock when it runs out.</li>
            <li>Images above {MAX_SIDE} px per side are block-averaged before detection.</li>
            <li>Star positions use Hipparcos proper motion propagated to today. Planets and satellites stay unmatched.</li>
          </ul>
        </Section>
      </aside>

      <SensorView
        frame={frame}
        overlay={soloOverlay(state.overlay.image, solo)}
        stretch={state.stretch}
        distortion={camera.distortion}
        highlight={highlight}
        onStretch={setStretch}
        placeholder={
          busy ? 'Decoding image' : 'Drop a PNG, JPEG or FITS image of the night sky anywhere on this bench'
        }
      />

      <SolutionPanel
        frame={frame}
        busy={busy}
        error={error}
        calibration
        highlight={highlight}
        onHighlight={setHighlight}
        progress={progress}
        onStop={stopImageSearch}
        exportName={(fileName ?? 'frame').replace(/\.[^.]+$/, '')}
      />
      <PipelineStrip frame={frame} solo={solo} onSolo={setSolo} />
    </div>
  );
}
