import { useMemo, useState } from 'react';

import { NumberField, Section, Select, Toggle } from '../components/controls';
import { OverlayControls } from '../components/OverlayControls';
import { PipelineStrip, soloOverlay } from '../components/PipelineStrip';
import type { SoloLayer } from '../components/PipelineStrip';
import { SensorView } from '../components/SensorView';
import { SolutionPanel } from '../components/SolutionPanel';
import { downloadBlob } from '../lib/export';
import { rollBy, skyShift, slew } from '../lib/sky';
import {
  appState,
  rerollSeed,
  setFramePeriod,
  setPointing,
  setSeed,
  setSensor,
  setStretch,
  setSyntheticCamera,
  setSyntheticSolver,
  setTab,
  startSequence,
  stopSequence,
  syntheticShareLink,
} from '../state/store';
import type { AppState } from '../state/store';
import { openVideo } from '../state/video';

const RESOLUTIONS = [256, 512, 1024, 2048].map((value) => ({ value, label: `${value} × ${value}` }));
const METHODS = [
  { value: 'iwc' as const, label: 'Weighted CoG' },
  { value: 'cog' as const, label: 'Plain CoG' },
];

export function SyntheticBench({ state }: { state: AppState }) {
  const { camera, pointing, sensor, solver, seed, frame, busy, error, replay, sequence } = state.synthetic;
  const [highlight, setHighlight] = useState<number | null>(null);
  const [solo, setSolo] = useState<SoloLayer | null>(null);
  const [k1, k2, k3, p1, p2] = camera.distortion;
  const degPerPixel = frame ? frame.index.pixelScaleArcsec / 3600 : camera.fovDeg / camera.width;
  // Until the engine has drawn the sky at the pointing just asked for, the
  // frame on screen is shown moved to where that sky will be.
  const pendingShift = useMemo((): [number, number] => {
    if (!frame?.truthPointing || sequence.running) {
      return [0, 0];
    }
    const [x, y] = skyShift(frame.truthPointing, pointing, degPerPixel);
    return Math.hypot(x, y) < frame.width ? [x, y] : [0, 0];
  }, [frame, pointing, degPerPixel, sequence.running]);

  return (
    <div className="bench">
      <aside className="panel panel--left">
        {replay ? (
          <p className="notice">
            Monte Carlo trial {replay.trial} (seed {replay.masterSeed}). Changing any control returns to a live frame.
          </p>
        ) : null}
        <Section title="Attitude" aside="truth">
          <NumberField label="RA" value={pointing.raDeg} min={0} max={360} step={0.1} digits={3} unit={'°'} wrap slider
            onChange={(raDeg) => { setPointing({ ...pointing, raDeg }); }} />
          <NumberField label="Dec" value={pointing.decDeg} min={-90} max={90} step={0.1} digits={3} unit={'°'} slider
            onChange={(decDeg) => { setPointing({ ...pointing, decDeg }); }} />
          <NumberField label="Roll" value={pointing.rollDeg} min={0} max={360} step={0.5} digits={3} unit={'°'} wrap slider
            onChange={(rollDeg) => { setPointing({ ...pointing, rollDeg }); }} />
        </Section>

        <Section title="Camera">
          <NumberField label="Field of view" value={camera.fovDeg} min={2} max={60} step={0.5} digits={1} unit={'°'} slider
            onChange={(fovDeg) => { setSyntheticCamera({ fovDeg }); }} />
          <Select label="Sensor" value={camera.width} options={RESOLUTIONS}
            onChange={(side) => { setSyntheticCamera({ width: side, height: side }); }} />
          <NumberField label={'PSF σ'} value={sensor.psfSigma} min={0.5} max={4} step={0.05} digits={2} unit="px" slider
            onChange={(psfSigma) => { setSensor({ psfSigma }); }} />
          <NumberField label="Exposure" value={sensor.exposure} min={0.005} max={2} step={0.01} digits={3} unit="s" slider log
            onChange={(exposure) => { setSensor({ exposure }); }} />
          <NumberField label="Colour term" value={sensor.colorTerm} min={-1} max={1} step={0.05} digits={2}
            title="Instrumental magnitude = V + term x (B-V). 0 is the V band; unfiltered silicon is near -0.3"
            onChange={(colorTerm) => { setSensor({ colorTerm }); }} />
          <NumberField label="k1" value={k1} min={-0.5} max={0.5} step={0.005} digits={3} slider
            title="Brown-Conrady radial distortion, first coefficient"
            onChange={(value) => { setSyntheticCamera({ distortion: [value, k2, k3, p1, p2] }); }} />
          <NumberField label="k2" value={k2} min={-0.5} max={0.5} step={0.005} digits={3}
            onChange={(value) => { setSyntheticCamera({ distortion: [k1, value, k3, p1, p2] }); }} />
          <NumberField label="p1" value={p1} min={-0.01} max={0.01} step={0.0001} digits={4}
            title="Brown-Conrady tangential distortion"
            onChange={(value) => { setSyntheticCamera({ distortion: [k1, k2, k3, value, p2] }); }} />
          <NumberField label="p2" value={p2} min={-0.01} max={0.01} step={0.0001} digits={4}
            onChange={(value) => { setSyntheticCamera({ distortion: [k1, k2, k3, p1, value] }); }} />
        </Section>

        <Section title="Body rate" aside="smears the exposure">
          <NumberField label="Across x" value={sensor.rateYDeg} min={-20} max={20} step={0.1} digits={2} unit={'°/s'} slider
            title="Rotation about the sensor y axis; stars trail along image x"
            onChange={(rateYDeg) => { setSensor({ rateYDeg }); }} />
          <NumberField label="Across y" value={sensor.rateXDeg} min={-20} max={20} step={0.1} digits={2} unit={'°/s'} slider
            title="Rotation about the sensor x axis; stars trail along image y"
            onChange={(rateXDeg) => { setSensor({ rateXDeg }); }} />
          <NumberField label="About boresight" value={sensor.rateZDeg} min={-60} max={60} step={0.5} digits={1} unit={'°/s'} slider
            title="Roll rate; stars trail along arcs around the image center"
            onChange={(rateZDeg) => { setSensor({ rateZDeg }); }} />
        </Section>

        <Section title="Lens and detector flaws" aside="off at 0">
          <NumberField label="Vignetting" value={sensor.vignetting} min={0} max={1} step={0.05} digits={2} slider
            title="Share of the natural cos^4 falloff toward the corners; 1 is an uncorrected lens"
            onChange={(vignetting) => { setSensor({ vignetting }); }} />
          <NumberField label="PSF growth" value={sensor.psfGrowth} min={0} max={2} step={0.05} digits={2} slider
            title="Star images at the corner are 1 + this times as wide as at the center"
            onChange={(psfGrowth) => { setSensor({ psfGrowth }); }} />
          <NumberField label="Coma" value={sensor.coma} min={0} max={0.5} step={0.01} digits={2} slider
            title="Share of a corner star's light in a flare pointing away from the center"
            onChange={(coma) => { setSensor({ coma }); }} />
          <NumberField label="Row time" value={sensor.rowTimeUs} min={0} max={200} step={1} digits={0} unit={'µs'} slider
            title="Rolling shutter: delay between rows. 0 is a global shutter. Acts together with the body rate"
            onChange={(rowTimeUs) => { setSensor({ rowTimeUs }); }} />
          <NumberField label="Gain pattern" value={sensor.prnu} min={0} max={0.1} step={0.002} digits={3} slider
            title="Photo-response non-uniformity, RMS as a fraction of the signal"
            onChange={(prnu) => { setSensor({ prnu }); }} />
          <NumberField label="Dark pattern" value={sensor.dsnu} min={0} max={500} step={5} digits={0} unit={'e⁻/s'}
            title="Dark-signal non-uniformity, RMS"
            onChange={(dsnu) => { setSensor({ dsnu }); }} />
          <NumberField label="Column noise" value={sensor.columnNoise} min={0} max={30} step={0.5} digits={1} unit={'e⁻'}
            title="Offset shared by a column, new in every frame"
            onChange={(columnNoise) => { setSensor({ columnNoise }); }} />
          <NumberField label="Nonlinearity" value={sensor.nonlinearity} min={0} max={0.3} step={0.01} digits={2}
            title="Shortfall of the response at full well"
            onChange={(nonlinearity) => { setSensor({ nonlinearity }); }} />
          <NumberField label="Blooming" value={sensor.blooming} min={0} max={1} step={0.05} digits={2}
            title="Share of the charge above full well that spills along the column"
            onChange={(blooming) => { setSensor({ blooming }); }} />
        </Section>

        <Section title="Sequence" aside={sequence.running ? 'running' : 'tracker'}>
          <NumberField label="Frame period" value={sequence.framePeriod} min={0.02} max={5} step={0.05} digits={2} unit="s"
            title="Time between frames; the camera rotates at the body rate in between"
            onChange={setFramePeriod} />
          <div className="field field--seed">
            <span className="readout">
              <b>frames</b> {sequence.frames} <b>track</b> {sequence.tracked} <b>search</b> {sequence.searches}{' '}
              <b>lost</b> {sequence.lost}
            </span>
            <button
              type="button"
              className="button"
              onClick={() => {
                if (sequence.running) {
                  stopSequence();
                } else {
                  startSequence();
                }
              }}
            >
              {sequence.running ? 'Stop' : 'Run'}
            </button>
          </div>
          <div className="actions">
            <button
              type="button"
              className="button"
              disabled={sequence.running}
              title="Runs the sequence in real time and saves its frames as a WebM video"
              onClick={() => {
                startSequence(true);
              }}
            >
              {sequence.recording ? `Recording · ${sequence.frames}` : 'Record video'}
            </button>
            {sequence.recorded ? (
              <>
                <button
                  type="button"
                  className="button"
                  onClick={() => {
                    if (sequence.recorded) {
                      setTab('video');
                      void openVideo(sequence.recorded.blob, 'synthetic-sequence.webm', sequence.recorded.fovDeg);
                    }
                  }}
                >
                  Open on Video bench
                </button>
                <button
                  type="button"
                  className="button"
                  onClick={() => {
                    if (sequence.recorded) {
                      downloadBlob('synthetic-sequence.webm', sequence.recorded.blob);
                    }
                  }}
                >
                  Save WebM
                </button>
              </>
            ) : null}
          </div>
          <p className="hint">
            {sequence.firstFix === null
              ? 'Each frame is solved from the previous attitude; a full search runs only when that fails.'
              : `First fix after ${sequence.firstFix} frame${sequence.firstFix === 1 ? '' : 's'} (${(sequence.firstFix * sequence.framePeriod).toFixed(2)} s).`}
          </p>
        </Section>

        <Section title="Noise">
          <Toggle label="Shot and read noise" checked={sensor.noise} onChange={(noise) => { setSensor({ noise }); }} />
          <NumberField label="Read noise" value={sensor.readNoise} min={0} max={200} step={1} digits={1} unit={'e⁻'} slider
            onChange={(readNoise) => { setSensor({ readNoise }); }} />
          <NumberField label="Sky" value={sensor.skyBackground} min={0} max={20000} step={10} digits={0} unit={'e⁻/s'}
            onChange={(skyBackground) => { setSensor({ skyBackground }); }} />
          <NumberField label="Hot pixels" value={sensor.hotPixelFraction * 100} min={0} max={5} step={0.01} digits={3} unit="%" slider
            onChange={(percent) => { setSensor({ hotPixelFraction: percent / 100 }); }} />
          <NumberField label="Cosmic rays" value={sensor.cosmicRayMean} min={0} max={300} step={1} digits={0} unit="/frame" slider
            onChange={(cosmicRayMean) => { setSensor({ cosmicRayMean }); }} />
          <NumberField label="Stray light" value={sensor.strayIntensity} min={0} max={200000} step={100} digits={0} unit={'e⁻/s'} slider
            title="Brightness of the veiling gradient at the sensor edge facing the Sun or Moon"
            onChange={(strayIntensity) => { setSensor({ strayIntensity }); }} />
          <NumberField label="Stray azimuth" value={sensor.strayAzimuthDeg} min={0} max={360} step={5} digits={0} unit={'°'} wrap
            onChange={(strayAzimuthDeg) => { setSensor({ strayAzimuthDeg }); }} />
          <div className="field field--seed">
            <NumberField label="Seed" value={seed} min={0} max={999999} step={1} digits={0} onChange={setSeed} />
            <button type="button" className="button" onClick={rerollSeed}>
              Re-roll
            </button>
          </div>
        </Section>

        <Section title="Solver">
          <Select label="Centroid" value={solver.method} options={METHODS}
            onChange={(method) => { setSyntheticSolver({ method }); }} />
          <NumberField label={'Peak threshold'} value={solver.peakSigma} min={3} max={30} step={0.5} digits={1} unit={'σ'}
            onChange={(peakSigma) => { setSyntheticSolver({ peakSigma, segmentSigma: Math.min(solver.segmentSigma, peakSigma) }); }} />
          <NumberField label="Pair tolerance" value={solver.tolerancePx} min={0.1} max={5} step={0.05} digits={2} unit="px"
            title="Allowed mismatch between measured and catalog star separations"
            onChange={(tolerancePx) => { setSyntheticSolver({ tolerancePx }); }} />
          <NumberField label="Pattern stars" value={solver.patternStars} min={4} max={30} step={1} digits={0}
            title="Brightest detections used to form pyramids"
            onChange={(patternStars) => { setSyntheticSolver({ patternStars }); }} />
        </Section>

        <OverlayControls bench="synthetic" overlay={state.overlay.synthetic} />
      </aside>

      <SensorView
        frame={frame}
        overlay={soloOverlay(state.overlay.synthetic, solo)}
        stretch={state.stretch}
        distortion={camera.distortion}
        highlight={highlight}
        onStretch={setStretch}
        shift={pendingShift}
        onSlew={(dx, dy) => { setPointing(slew(appState().synthetic.pointing, dx, dy, degPerPixel)); }}
        onRoll={(delta) => { setPointing(rollBy(appState().synthetic.pointing, delta)); }}
        placeholder={state.ready ? 'Rendering first frame' : 'Loading engine'}
      />

      <SolutionPanel
        frame={frame}
        busy={busy}
        error={error}
        calibration={false}
        highlight={highlight}
        onHighlight={setHighlight}
        exportName={`asterope-seed-${seed}`}
        shareLink={syntheticShareLink}
      />
      <PipelineStrip frame={frame} solo={solo} onSolo={setSolo} />
    </div>
  );
}
