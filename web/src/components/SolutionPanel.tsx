import { useMemo } from 'react';

import { STAR_NAMES } from '../data/star-names';
import { ExportBar } from './ExportBar';
import { SkyLocator } from './SkyLocator';
import type { FrameResult, Pointing, SearchProgress } from '../engine/types';
import {
  angle360,
  angle90,
  angleDifference,
  fixed,
  integer,
  quaternionComponent,
  scientific,
  signed,
} from '../lib/format';

interface Props {
  frame: FrameResult | null;
  busy: boolean;
  error: string | null;
  /** Shows calibration results instead of truth errors. */
  calibration: boolean;
  highlight: number | null;
  onHighlight: (detection: number | null) => void;
  progress?: SearchProgress | null;
  onStop?: () => void;
  /** Base name of exported files. */
  exportName: string;
  shareLink?: () => string;
  /** Boresight track of a video, drawn on the sky locator. */
  path?: readonly Pointing[];
}

const FALSE_LOCK_ARCSEC = 360;

type Status = 'idle' | 'lock' | 'none' | 'false';

function statusOf(frame: FrameResult | null): { status: Status; text: string } {
  if (!frame) {
    return { status: 'idle', text: 'STANDBY' };
  }
  if (!frame.solve.attitude) {
    // A coasting tracker has no solution this frame but still holds a prediction.
    return { status: 'none', text: frame.track === 'coasting' ? 'COAST' : 'NO LOCK' };
  }
  if (frame.error && frame.error.totalArcsec > FALSE_LOCK_ARCSEC) {
    return { status: 'false', text: 'FALSE LOCK' };
  }
  const text = frame.track === 'tracked' ? 'TRACK' : frame.track === 'acquired' ? 'ACQUIRE' : 'LOCK';
  return { status: 'lock', text };
}

function Row({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <tr>
      <th scope="row">{label}</th>
      <td>
        {value}
        {unit ? <span className="unit">{unit}</span> : null}
      </td>
    </tr>
  );
}

export function SolutionPanel({
  frame,
  busy,
  error,
  calibration,
  highlight,
  onHighlight,
  progress = null,
  onStop,
  exportName,
  shareLink,
  path,
}: Props) {
  const { status, text: statusText } = statusOf(frame);
  const attitude = frame?.solve.attitude ?? null;
  const truth = frame?.truthPointing ?? null;
  const elapsed = frame ? frame.timing.renderMs + frame.timing.detectMs + frame.timing.identifyMs : NaN;

  const stars = useMemo(
    () => [...(attitude?.matches ?? [])].sort((a, b) => a.mag - b.mag),
    [attitude],
  );

  const cosDec = attitude ? Math.cos((attitude.pointing.decDeg * Math.PI) / 180) : 1;
  const errors =
    attitude && truth
      ? {
          ra: angleDifference(attitude.pointing.raDeg, truth.raDeg) * cosDec * 3600,
          dec: (attitude.pointing.decDeg - truth.decDeg) * 3600,
          roll: angleDifference(attitude.pointing.rollDeg, truth.rollDeg) * 3600,
        }
      : null;

  return (
    <aside className="panel panel--right">
      <header className={`status status--${status}`} aria-live="polite">
        <span className="status__lamp" aria-hidden="true" />
        <strong>{statusText}</strong>
        <span className="status__time">
          {busy ? <span className="status__busy">SOLVING</span> : null}
          {fixed(elapsed, 1)}
          <span className="unit">ms</span>
        </span>
      </header>
      {error ? <p className="fault">{error}</p> : null}
      {busy && progress ? (
        <div className="search">
          <div
            className="progress"
            role="progressbar"
            aria-label="Search progress"
            aria-valuemin={0}
            aria-valuemax={progress.rungs * Math.max(progress.scalesTotal, 1)}
            aria-valuenow={(progress.rung - 1) * progress.scalesTotal + progress.scalesTried}
          >
            <span
              style={{
                width: `${(100 * (progress.rung - 1 + progress.scalesTried / Math.max(progress.scalesTotal, 1))) / progress.rungs}%`,
              }}
            />
          </div>
          <p className="search__text">
            {progress.rungs > 1 ? `field ${progress.rung}/${progress.rungs} · ${fixed(progress.rungFovDeg, 0)}° · ` : ''}
            scale {progress.scalesTried}/{progress.scalesTotal} &middot; {integer(progress.pyramids)} patterns
          </p>
          {onStop ? (
            <button type="button" className="button" onClick={onStop}>
              Stop
            </button>
          ) : null}
        </div>
      ) : null}
      {!busy && frame?.stopped ? <p className="notice">Search stopped before it finished.</p> : null}
      <ExportBar frame={frame} name={exportName} {...(shareLink ? { shareLink } : {})} />

      <section className="group">
        <header className="group__head">
          <h2>Quaternion</h2>
          <span className="group__aside">ICRS &rarr; sensor, scalar last</span>
        </header>
        <table className="data data--quat">
          <tbody>
            {(['q1', 'q2', 'q3', 'q4'] as const).map((name, index) => (
              <Row key={name} label={name} value={quaternionComponent(attitude?.q[index] ?? NaN)} />
            ))}
          </tbody>
        </table>
      </section>

      <section className="group">
        <header className="group__head">
          <h2>Pointing</h2>
        </header>
        <table className="data data--pointing">
          <thead>
            <tr>
              <th />
              <th scope="col">EST (&deg;)</th>
              {calibration ? null : <th scope="col">ERR (&Prime;)</th>}
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">RA</th>
              <td>{angle360(attitude?.pointing.raDeg ?? NaN, 4)}</td>
              {calibration ? null : <td>{signed(errors?.ra ?? NaN, 1)}</td>}
            </tr>
            <tr>
              <th scope="row">DEC</th>
              <td>{angle90(attitude?.pointing.decDeg ?? NaN, 4)}</td>
              {calibration ? null : <td>{signed(errors?.dec ?? NaN, 1)}</td>}
            </tr>
            <tr>
              <th scope="row">ROLL</th>
              <td>{angle360(attitude?.pointing.rollDeg ?? NaN, 4)}</td>
              {calibration ? null : <td>{signed(errors?.roll ?? NaN, 1)}</td>}
            </tr>
          </tbody>
        </table>
      </section>

      <section className="group">
        <header className="group__head">
          <h2>Sky position</h2>
          <span className="group__aside">{attitude ? 'solved' : frame?.truthQ ? 'truth' : ''}</span>
        </header>
        <SkyLocator
          q={attitude?.q ?? frame?.truthQ ?? null}
          pointing={attitude?.pointing ?? truth}
          intrinsics={frame?.solve.intrinsics ?? null}
          width={frame?.width ?? 0}
          height={frame?.height ?? 0}
          locked={attitude !== null}
          {...(path ? { path } : {})}
        />
      </section>

      <section className="group">
        <header className="group__head">
          <h2>Solution</h2>
        </header>
        <table className="data">
          <tbody>
            {calibration ? (
              <>
                <Row label="FOV fitted" value={fixed(frame?.solve.intrinsics.fovXDeg ?? NaN, 3)} unit={'°'} />
                <Row label="k1 fitted" value={signed(frame?.solve.intrinsics.k1 ?? NaN, 4)} />
                <Row label="Scales tried" value={frame ? String(frame.solve.scalesTried) : fixed(NaN, 0)} />
              </>
            ) : (
              <>
                <Row label="Cross-boresight" value={fixed(frame?.error?.crossArcsec ?? NaN, 2)} unit={'″'} />
                <Row label="About boresight" value={fixed(frame?.error?.rollArcsec ?? NaN, 1)} unit={'″'} />
              </>
            )}
            <Row label="Residual RMS" value={fixed(attitude?.rmsArcsec ?? NaN, 2)} unit={'″'} />
            <Row
              label="Matched"
              value={frame ? `${attitude?.matches.length ?? 0} / ${frame.detect.detections.length}` : fixed(NaN, 0)}
            />
            <Row
              label={'1σ x · y · roll'}
              value={
                attitude?.sigmaArcsec
                  ? attitude.sigmaArcsec.map((v) => fixed(v, 1)).join(' · ')
                  : fixed(NaN, 1)
              }
              unit={'″'}
            />
            <Row label={'QUEST − q-method'} value={scientific(attitude?.questQMethodRad ?? NaN)} unit="rad" />
            <Row label={'QUEST − SVD'} value={scientific(attitude?.questSvdRad ?? NaN)} unit="rad" />
            <Row label={'λ max'} value={fixed(attitude?.lambdaMax ?? NaN, 10)} />
            <Row label="Chance match" value={scientific(attitude?.chanceProbability ?? NaN)} />
          </tbody>
        </table>
      </section>

      <section className="group group--fill">
        <header className="group__head">
          <h2>Identified stars</h2>
          <span className="group__aside">{stars.length}</span>
        </header>
        <div className="scroll">
          <table className="data data--stars">
            <thead>
              <tr>
                <th scope="col">HIP</th>
                <th scope="col">mag</th>
                <th scope="col">res (&Prime;)</th>
              </tr>
            </thead>
            <tbody>
              {stars.map((star) => (
                <tr
                  key={star.hip}
                  className={star.detection === highlight ? 'is-active' : undefined}
                  onMouseEnter={() => {
                    onHighlight(star.detection);
                  }}
                  onMouseLeave={() => {
                    onHighlight(null);
                  }}
                >
                  <td>
                    {star.hip}
                    {STAR_NAMES.has(star.hip) ? <span className="star-name">{STAR_NAMES.get(star.hip)}</span> : null}
                  </td>
                  <td>{fixed(star.mag, 2)}</td>
                  <td>{fixed(star.residualArcsec, 1)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {stars.length === 0 ? <p className="empty">No stars identified.</p> : null}
        </div>
      </section>
    </aside>
  );
}
