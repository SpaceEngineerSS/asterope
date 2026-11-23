import { useMemo, useRef, useState } from 'react';
import type { DragEvent } from 'react';

import { NumberField, Section, Select, Toggle } from '../components/controls';
import { OverlayControls } from '../components/OverlayControls';
import { SensorView } from '../components/SensorView';
import { SolutionPanel } from '../components/SolutionPanel';
import { Timeline } from '../components/Timeline';
import type { Pointing } from '../engine/types';
import { download, videoCsv } from '../lib/export';
import { fixed, integer, percent } from '../lib/format';
import { setStretch } from '../state/store';
import type { AppState } from '../state/store';
import {
  SAMPLE_RATES,
  STACK_DEPTHS,
  closeVideo,
  inspectSample,
  openLiveCamera,
  openVideo,
  setSampleRate,
  setStackDepth,
  setVideoAutoTolerance,
  setVideoFov,
  setVideoSolver,
  setVideoUnknownFov,
  startVideo,
  stopVideo,
  useVideoState,
} from '../state/video';
import { MAX_VIDEO_SIDE } from '../video/reader';

const ACCEPT = 'video/*,.mp4,.webm,.mov,.mkv';

export function VideoBench({ state }: { state: AppState }) {
  const video = useVideoState();
  const { source, camera, frame, samples, running, opening, error } = video;
  const solver = video.solver ?? state.image.solver;
  const [highlight, setHighlight] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const open = (file: File | undefined) => {
    if (file) {
      void openVideo(file, file.name);
    }
  };

  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(false);
    open(event.dataTransfer.files[0]);
  };

  const counts = useMemo(() => {
    const of = (event: string) => samples.filter((s) => s.event === event).length;
    const solved = samples.filter((s) => s.q !== null).length;
    return { solved, tracked: of('tracked'), acquired: of('acquired'), coasting: of('coasting'), lost: of('lost') };
  }, [samples]);

  const path = useMemo(
    () => samples.map((s) => s.pointing).filter((p): p is Pointing => p !== null),
    [samples],
  );

  const baseName = (source?.name ?? 'video').replace(/\.[^.]+$/, '');
  const fovAside = video.unknownFov ? (video.fovFitted ? 'fitted' : 'searched') : 'entered';

  return (
    <div
      className={dragging ? 'bench bench--video bench--drop' : 'bench bench--video'}
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
        <Section title="Video">
          <button
            type="button"
            className="dropzone"
            onClick={() => {
              inputRef.current?.click();
            }}
          >
            <span className="dropzone__title">{source?.name ?? 'Drop a video of the night sky'}</span>
            <span className="dropzone__hint">
              {source
                ? `${source.width} × ${source.height} px${source.binning > 1 ? ` · binned ${source.binning}×` : ''}${
                    source.durationS === null ? ' · live' : ` · ${fixed(source.durationS, 1)} s`
                  }`
                : 'MP4 or WebM · click to browse'}
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
          <div className="actions">
            <button
              type="button"
              className="button"
              disabled={opening}
              onClick={() => {
                void openLiveCamera();
              }}
            >
              Use camera
            </button>
            {source ? (
              <button type="button" className="button" onClick={closeVideo}>
                Close
              </button>
            ) : null}
          </div>
        </Section>

        <Section title="Run" aside={running ? 'solving' : source ? 'idle' : ''}>
          <div className="actions">
            {running ? (
              <button type="button" className="button" onClick={stopVideo}>
                Stop
              </button>
            ) : (
              <button type="button" className="button" disabled={!source || !state.ready} onClick={startVideo}>
                {source?.live ? 'Start' : 'Run from start'}
              </button>
            )}
            <button
              type="button"
              className="button"
              disabled={samples.length === 0}
              onClick={() => {
                download(`${baseName}-attitude.csv`, videoCsv(samples), 'text/csv');
              }}
            >
              Attitude log CSV
            </button>
          </div>
          {source?.live ? null : (
            <Select
              label="Frames per second"
              value={video.sampleRate}
              options={SAMPLE_RATES.map((rate) => ({ value: rate, label: `${rate}` }))}
              onChange={setSampleRate}
            />
          )}
          <Select
            label="Stack depth"
            value={video.stackDepth}
            options={STACK_DEPTHS.map((depth) => ({ value: depth, label: depth === 1 ? '1 (off)' : `${depth} frames` }))}
            onChange={setStackDepth}
          />
          <table className="data">
            <tbody>
              <tr>
                <th scope="row">Frames solved</th>
                <td>
                  {integer(counts.solved)} / {integer(samples.length)}{' '}
                  <span className="unit">{samples.length > 0 ? percent(counts.solved / samples.length, 1) : ''}</span>
                </td>
              </tr>
              <tr>
                <th scope="row">Track · acquire</th>
                <td>
                  {integer(counts.tracked)} &middot; {integer(counts.acquired)}
                </td>
              </tr>
              <tr>
                <th scope="row">Coast · lost</th>
                <td>
                  {integer(counts.coasting)} &middot; {integer(counts.lost)}
                </td>
              </tr>
              {source?.live ? (
                <tr>
                  <th scope="row">Frames skipped</th>
                  <td>{integer(video.dropped)}</td>
                </tr>
              ) : null}
              <tr>
                <th scope="row">Stars paired</th>
                <td>
                  {frame?.stack && frame.stack.pairs > 0 ? `${frame.stack.pairs}` : fixed(NaN, 0)}
                  <span className="unit">{frame?.stack && frame.stack.pairs > 0 ? `${fixed(frame.stack.shiftPx, 1)} px shift` : ''}</span>
                </td>
              </tr>
            </tbody>
          </table>
        </Section>

        <Section title="Optics" aside={fovAside}>
          <Toggle label="Field of view unknown" checked={video.unknownFov} onChange={setVideoUnknownFov} />
          <NumberField label={video.unknownFov && !video.fovFitted ? 'Starting guess' : 'Field of view'} value={camera.fovDeg} min={2} max={110} step={0.5} digits={2} unit={'°'} slider
            title="Horizontal field of view of the full video frame"
            onChange={setVideoFov} />
        </Section>

        <Section title="Detection">
          <NumberField label="Peak threshold" value={solver.peakSigma} min={3} max={60} step={0.5} digits={1} unit={'σ'} slider
            onChange={(peakSigma) => { setVideoSolver({ peakSigma, segmentSigma: Math.min(3, peakSigma) }); }} />
          <NumberField label="Min area" value={solver.minArea} min={1} max={50} step={1} digits={0} unit="px"
            onChange={(minArea) => { setVideoSolver({ minArea }); }} />
          <NumberField label="Max elongation" value={solver.maxElongation} min={1.2} max={10} step={0.1} digits={1}
            title="Rejects trailed stars and streaks above this axis ratio"
            onChange={(maxElongation) => { setVideoSolver({ maxElongation }); }} />
        </Section>

        <Section title="Solver" aside={video.autoTolerance ? 'tolerance from field of view' : 'tolerance entered'}>
          <Toggle label="Automatic tolerance" checked={video.autoTolerance} onChange={setVideoAutoTolerance} />
          <NumberField label="Pair tolerance" value={solver.tolerancePx} min={0.2} max={20} step={0.1} digits={1} unit="px"
            onChange={(tolerancePx) => { setVideoSolver({ tolerancePx, matchRadiusPx: Math.max(solver.matchRadiusPx, tolerancePx) }); }} />
          <NumberField label="Match radius" value={solver.matchRadiusPx} min={0.5} max={30} step={0.1} digits={1} unit="px"
            onChange={(matchRadiusPx) => { setVideoSolver({ matchRadiusPx }); }} />
        </Section>

        <OverlayControls bench="video" overlay={state.overlay.video} />

        <Section title="Limits">
          <ul className="notes">
            <li>A phone records about 1/30 s per frame in 8 bits, which shows only the few brightest stars. Stacking averages the last frames after aligning them on those stars and recovers fainter ones; it cannot help when fewer than three stars are visible in a single frame.</li>
            <li>The target is video from a camera on a tripod or a star tracker: exposures of tens of milliseconds or longer, focus at infinity, no automatic gain pumping.</li>
            <li>A file is sampled at the chosen rate by seeking; nothing is skipped. A live camera is solved as fast as frames can be processed and the frames in between are counted as skipped.</li>
            <li>The first lock with an unknown field of view searches 6&deg; to 100&deg; and can take a minute. After a search that finds nothing, the next waits 1, 2, 4 and then 8 seconds of video, so titles and twilight are passed over. Later frames are solved from the predicted attitude in milliseconds.</li>
            <li>Stars moving more than about 24 px between samples beyond the predicted motion break frame-to-frame alignment; raise the frame rate.</li>
            <li>Video above {MAX_VIDEO_SIDE} px per side is block-averaged before detection.</li>
          </ul>
        </Section>
      </aside>

      <SensorView
        frame={frame}
        overlay={state.overlay.video}
        stretch={state.stretch}
        distortion={camera.distortion}
        highlight={highlight}
        onStretch={setStretch}
        placeholder={opening ? 'Opening video' : 'Drop a video of the night sky anywhere on this bench, or record one on the Synthetic bench'}
      />

      <SolutionPanel
        frame={frame}
        busy={running || opening}
        error={error}
        calibration
        highlight={highlight}
        onHighlight={setHighlight}
        progress={video.progress}
        onStop={stopVideo}
        exportName={baseName}
        path={path}
      />
      <Timeline
        samples={samples}
        durationS={source?.durationS ?? null}
        selected={video.selected}
        onSelect={(index) => {
          void inspectSample(index);
        }}
      />
    </div>
  );
}
