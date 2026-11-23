import { useEffect, useMemo, useRef } from 'react';

import { NumberField, Select } from '../components/controls';
import { STRESS_PARAMETERS } from '../engine/types';
import type { CentroidRow, StressStep, TrialOutcome } from '../engine/types';
import { fixed, integer, percent, scientific } from '../lib/format';
import { clopperPearson, percentile } from '../lib/stats';
import {
  CENTROID_SIGMAS,
  CENTROID_SNRS,
  VALIDATION_CAMERA,
  WAHBA_CASES,
  cancelLisSuite,
  cancelStressSuite,
  replayTrial,
  runCentroidSuite,
  runLisSuite,
  runStressSuite,
  runWahbaSuite,
  setLisOptions,
  setStressOptions,
} from '../state/store';
import type { AppState } from '../state/store';

const CENTROID_LIMIT_PX = 0.05;
const ENFORCED_SNR = 60;
const WAHBA_LIMIT_RAD = 1e-10;
const SUCCESS_TARGET = 0.98;
const FALSE_POSITIVE_TARGET = 0.001;
const TRIAL_OPTIONS = [100, 1000, 4000, 10000].map((value) => ({ value, label: integer(value) }));
const STRESS_TRIAL_OPTIONS = [50, 100, 300, 1000].map((value) => ({ value, label: integer(value) }));

type Verdict = 'pass' | 'fail' | 'info' | 'pending';

function VerdictCell({ verdict }: { verdict: Verdict }) {
  const text = { pass: 'PASS', fail: 'FAIL', info: 'REPORTED', pending: '—' }[verdict];
  return <td className={`verdict verdict--${verdict}`}>{text}</td>;
}

function centroidVerdict(row: CentroidRow): Verdict {
  if (row.peakSnr !== null && row.peakSnr < ENFORCED_SNR) {
    return 'info';
  }
  return row.missed === 0 && row.meanError < CENTROID_LIMIT_PX ? 'pass' : 'fail';
}

function CentroidSuite({ state }: { state: AppState }) {
  const { rows, running, total, error } = state.validation.centroid;
  return (
    <section className="suite">
      <header className="suite__head">
        <div>
          <h2>1 &middot; Sub-pixel centroid accuracy</h2>
          <p>
            Single Gaussian stars on a 20 &times; 20 grid of sub-pixel offsets. Pass limit: mean radial error below{' '}
            {CENTROID_LIMIT_PX} px without noise and at peak SNR {ENFORCED_SNR}. At peak SNR 30 the Cram&eacute;r-Rao
            bound is itself about 0.05 px, so that row is reported without a verdict.
          </p>
        </div>
        <button type="button" className="button" disabled={!state.ready || running} onClick={runCentroidSuite}>
          {running ? `Running ${rows.length} / ${total}` : 'Run'}
        </button>
      </header>
      {error ? <p className="fault">{error}</p> : null}
      <table className="data data--grid">
        <thead>
          <tr>
            <th scope="col">Condition</th>
            <th scope="col">PSF &sigma; (px)</th>
            <th scope="col">Signal (e&#8315;)</th>
            <th scope="col">Samples</th>
            <th scope="col">Missed</th>
            <th scope="col">Mean error (px)</th>
            <th scope="col">Max error (px)</th>
            <th scope="col">Cram&eacute;r-Rao (px)</th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          {CENTROID_SNRS.flatMap((snr) =>
            CENTROID_SIGMAS.map((sigma) => {
              const row = rows.find((r) => r.peakSnr === snr && r.psfSigma === sigma);
              return (
                <tr key={`${snr ?? 'clean'}-${sigma}`}>
                  <th scope="row">{snr === null ? 'Noiseless' : `Peak SNR ${snr}`}</th>
                  <td>{fixed(sigma, 1)}</td>
                  <td>{row ? integer(row.electrons) : fixed(NaN, 0)}</td>
                  <td>{row ? integer(row.samples) : fixed(NaN, 0)}</td>
                  <td>{row ? integer(row.missed) : fixed(NaN, 0)}</td>
                  <td>{fixed(row?.meanError ?? NaN, 4)}</td>
                  <td>{fixed(row?.maxError ?? NaN, 4)}</td>
                  <td>{row && row.bound > 0 ? fixed(row.bound, 4) : fixed(NaN, 4)}</td>
                  <VerdictCell verdict={row ? centroidVerdict(row) : 'pending'} />
                </tr>
              );
            }),
          )}
        </tbody>
      </table>
    </section>
  );
}

function WahbaSuite({ state }: { state: AppState }) {
  const { result, running, error } = state.validation.wahba;
  const verdict = (value: number | undefined): Verdict =>
    value === undefined ? 'pending' : value < WAHBA_LIMIT_RAD && result?.failures === 0 ? 'pass' : 'fail';
  return (
    <section className="suite">
      <header className="suite__head">
        <div>
          <h2>2 &middot; Wahba solver equivalence</h2>
          <p>
            {integer(WAHBA_CASES)} random star fields (3&ndash;30 stars, 8&ndash;25&deg; field, 0&ndash;60&Prime;{' '}
            noise, every tenth attitude near a half turn). QUEST and the q-method must agree with the SVD solution of
            Markley to within {scientific(WAHBA_LIMIT_RAD, 0)} rad.
          </p>
        </div>
        <button type="button" className="button" disabled={!state.ready || running} onClick={runWahbaSuite}>
          {running ? 'Running' : 'Run'}
        </button>
      </header>
      {error ? <p className="fault">{error}</p> : null}
      <table className="data data--grid">
        <thead>
          <tr>
            <th scope="col">Solver</th>
            <th scope="col">Cases</th>
            <th scope="col">Worst deviation from SVD (rad)</th>
            <th scope="col">Limit (rad)</th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row">QUEST (Shuster &amp; Oh 1981)</th>
            <td>{result ? integer(result.cases) : fixed(NaN, 0)}</td>
            <td>{scientific(result?.worstQuestRad ?? NaN, 2)}</td>
            <td>{scientific(WAHBA_LIMIT_RAD, 0)}</td>
            <VerdictCell verdict={verdict(result?.worstQuestRad)} />
          </tr>
          <tr>
            <th scope="row">Davenport q-method</th>
            <td>{result ? integer(result.cases) : fixed(NaN, 0)}</td>
            <td>{scientific(result?.worstQMethodRad ?? NaN, 2)}</td>
            <td>{scientific(WAHBA_LIMIT_RAD, 0)}</td>
            <VerdictCell verdict={verdict(result?.worstQMethodRad)} />
          </tr>
        </tbody>
      </table>
    </section>
  );
}

interface LisSummary {
  trials: number;
  success: number;
  inaccurate: number;
  falsePositive: number;
  noSolution: number;
  cross: number[];
  roll: number[];
  failures: TrialOutcome[];
}

function summarize(outcomes: readonly TrialOutcome[]): LisSummary {
  const summary: LisSummary = {
    trials: outcomes.length,
    success: 0,
    inaccurate: 0,
    falsePositive: 0,
    noSolution: 0,
    cross: [],
    roll: [],
    failures: [],
  };
  for (const outcome of outcomes) {
    summary[outcome.class] += 1;
    if (outcome.class === 'success' || outcome.class === 'inaccurate') {
      summary.cross.push(outcome.crossArcsec);
      summary.roll.push(outcome.rollArcsec);
    }
    if (outcome.class !== 'success') {
      summary.failures.push(outcome);
    }
  }
  summary.cross.sort((a, b) => a - b);
  summary.roll.sort((a, b) => a - b);
  // Trials arrive interleaved from several workers.
  summary.failures.sort((a, b) => a.trial - b.trial);
  return summary;
}

function Histogram({ samples, unit }: { samples: readonly number[]; unit: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const top = percentile(samples, 0.99);
  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return;
    }
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);
    if (samples.length === 0 || !(top > 0)) {
      return;
    }
    const bins = new Array<number>(48).fill(0);
    for (const sample of samples) {
      const index = Math.min(bins.length - 1, Math.floor((sample / top) * bins.length));
      bins[index] = (bins[index] ?? 0) + 1;
    }
    const peak = Math.max(...bins);
    const barWidth = width / bins.length;
    context.fillStyle = '#7bd88f';
    bins.forEach((count, index) => {
      const barHeight = Math.round((count / peak) * (height - 1));
      context.fillRect(Math.round(index * barWidth) + 1, height - barHeight, Math.max(1, Math.floor(barWidth) - 1), barHeight);
    });
  }, [samples, top]);
  return (
    <figure className="histogram">
      <canvas ref={ref} />
      <figcaption>
        <span>0</span>
        <span>
          {fixed(top, 1)} {unit} (99th percentile)
        </span>
      </figcaption>
    </figure>
  );
}

function LisSuite({ state }: { state: AppState }) {
  const lis = state.validation.lis;
  const summary = useMemo(() => summarize(lis.outcomes), [lis.outcomes]);
  const complete = !lis.running && summary.trials > 0;
  const successInterval = clopperPearson(summary.success, summary.trials);
  const falseInterval = clopperPearson(summary.falsePositive, summary.trials);
  const successRate = summary.success / Math.max(summary.trials, 1);
  const falseRate = summary.falsePositive / Math.max(summary.trials, 1);
  const pointVerdict = (ok: boolean): Verdict => (complete ? (ok ? 'pass' : 'fail') : 'pending');

  return (
    <section className="suite">
      <header className="suite__head">
        <div>
          <h2>3 &middot; Lost-in-space Monte Carlo</h2>
          <p>
            Uniformly random attitudes on a {VALIDATION_CAMERA.width} &times; {VALIDATION_CAMERA.height} sensor with a{' '}
            {VALIDATION_CAMERA.fovDeg}&deg; field. Every trial draws its own hot-pixel fraction (0.01&ndash;0.1 %), 0&ndash;5
            cosmic-ray streaks and, in 30 % of trials, a stray-light gradient. Success: cross-boresight error &le; 60&Prime;
            and roll error &le; 300&Prime;. False positive: a reported attitude more than 0.1&deg; from the truth. No
            trial is removed from the denominator.
          </p>
        </div>
        <div className="suite__actions">
          <Select label="Trials" value={lis.trials} options={TRIAL_OPTIONS} onChange={(trials) => { setLisOptions({ trials }); }} />
          <NumberField label="Seed" value={lis.masterSeed} min={0} max={999999} step={1} digits={0}
            onChange={(masterSeed) => { setLisOptions({ masterSeed }); }} />
          {lis.running ? (
            <button type="button" className="button" onClick={cancelLisSuite}>
              Stop
            </button>
          ) : (
            <button type="button" className="button" disabled={!state.ready} onClick={runLisSuite}>
              Run
            </button>
          )}
        </div>
      </header>
      {lis.error ? <p className="fault">{lis.error}</p> : null}
      <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={lis.trials} aria-valuenow={summary.trials}>
        <span style={{ width: `${(100 * summary.trials) / Math.max(lis.trials, 1)}%` }} />
      </div>
      <p className="progress__text">
        {integer(summary.trials)} / {integer(lis.trials)} trials &middot; {fixed(lis.elapsedMs / 1000, 1)} s
        {summary.trials > 0 ? ` · ${fixed(lis.elapsedMs / summary.trials, 1)} ms per trial` : ''}
      </p>

      <table className="data data--grid">
        <thead>
          <tr>
            <th scope="col">Criterion</th>
            <th scope="col">Target</th>
            <th scope="col">Measured</th>
            <th scope="col">95 % interval (exact)</th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row">Success rate</th>
            <td>&gt; {percent(SUCCESS_TARGET, 0)}</td>
            <td>{summary.trials ? percent(successRate) : fixed(NaN, 0)}</td>
            <td>{summary.trials ? `${percent(successInterval[0])} – ${percent(successInterval[1])}` : fixed(NaN, 0)}</td>
            <VerdictCell verdict={pointVerdict(successRate > SUCCESS_TARGET)} />
          </tr>
          <tr>
            <th scope="row">False-positive rate</th>
            <td>&lt; {percent(FALSE_POSITIVE_TARGET, 1)}</td>
            <td>{summary.trials ? `${percent(falseRate, 3)} (${summary.falsePositive})` : fixed(NaN, 0)}</td>
            <td>{summary.trials ? `0 – ${percent(falseInterval[1], 3)}` : fixed(NaN, 0)}</td>
            <VerdictCell verdict={pointVerdict(falseRate < FALSE_POSITIVE_TARGET)} />
          </tr>
        </tbody>
      </table>
      {complete && falseInterval[1] >= FALSE_POSITIVE_TARGET ? (
        <p className="caveat">
          With {integer(summary.trials)} trials the 95 % upper bound on the false-positive rate is{' '}
          {percent(falseInterval[1], 3)}, above the {percent(FALSE_POSITIVE_TARGET, 1)} target. The measured rate passes,
          but demonstrating the target needs about 3 700 trials without a false positive.
        </p>
      ) : null}

      <div className="suite__split">
        <table className="data data--grid">
          <thead>
            <tr>
              <th scope="col">Accuracy of solved trials</th>
              <th scope="col">Median (&Prime;)</th>
              <th scope="col">95th pct (&Prime;)</th>
              <th scope="col">Max (&Prime;)</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">Cross-boresight</th>
              <td>{fixed(percentile(summary.cross, 0.5), 2)}</td>
              <td>{fixed(percentile(summary.cross, 0.95), 2)}</td>
              <td>{fixed(percentile(summary.cross, 1), 2)}</td>
            </tr>
            <tr>
              <th scope="row">About boresight (roll)</th>
              <td>{fixed(percentile(summary.roll, 0.5), 1)}</td>
              <td>{fixed(percentile(summary.roll, 0.95), 1)}</td>
              <td>{fixed(percentile(summary.roll, 1), 1)}</td>
            </tr>
            <tr>
              <th scope="row">Outcomes</th>
              <td colSpan={3}>
                {integer(summary.success)} success &middot; {integer(summary.inaccurate)} inaccurate &middot;{' '}
                {integer(summary.noSolution)} no solution &middot; {integer(summary.falsePositive)} false positive
              </td>
            </tr>
          </tbody>
        </table>
        <div>
          <h3>Cross-boresight error distribution</h3>
          <Histogram samples={summary.cross} unit={'″'} />
        </div>
      </div>

      <h3>Trials that did not succeed</h3>
      {summary.failures.length === 0 ? (
        <p className="empty">{summary.trials > 0 ? 'None.' : 'Run the suite to list them.'}</p>
      ) : (
        <div className="scroll scroll--short">
          <table className="data data--grid">
            <thead>
              <tr>
                <th scope="col">Trial</th>
                <th scope="col">Outcome</th>
                <th scope="col">Catalog stars in frame</th>
                <th scope="col">Detections</th>
                <th scope="col">Matches</th>
                <th scope="col">Error (&Prime;)</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {summary.failures.slice(0, 200).map((outcome) => (
                <tr key={outcome.trial}>
                  <td>{outcome.trial}</td>
                  <td>{outcome.class}</td>
                  <td>{outcome.truthStars}</td>
                  <td>{outcome.detections}</td>
                  <td>{outcome.matches}</td>
                  <td>{fixed(outcome.totalArcsec, 1)}</td>
                  <td>
                    <button type="button" className="link" onClick={() => { replayTrial(outcome.trial); }}>
                      Open on bench
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function formatValue(value: number): string {
  if (value === 0) {
    return '0';
  }
  return Math.abs(value) >= 1000 ? value.toExponential(0).replace('e+', 'e') : String(value);
}

function StressChart({ steps, total }: { steps: readonly StressStep[]; total: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) {
      return;
    }
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);

    const left = 44;
    const right = width - 12;
    const top = 10;
    const bottom = height - 22;
    const x = (index: number) => left + ((right - left) * (index + 0.5)) / Math.max(total, 1);
    const y = (fraction: number) => bottom - (bottom - top) * fraction;

    context.font = '10px "IBM Plex Mono", ui-monospace, monospace';
    context.textBaseline = 'middle';
    context.lineWidth = 1;
    for (const level of [0, 0.5, 0.98, 1]) {
      const py = Math.round(y(level)) + 0.5;
      context.strokeStyle = level === 0.98 ? 'rgba(224, 160, 48, 0.55)' : '#262a29';
      context.beginPath();
      context.moveTo(left, py);
      context.lineTo(right, py);
      context.stroke();
      context.fillStyle = '#565e5b';
      // The 98 % target sits two pixels under 100 %; label it inside the plot.
      if (level === 0.98) {
        context.textAlign = 'left';
        context.fillText('98 % target', right - 66, py + 9);
      } else {
        context.textAlign = 'right';
        context.fillText(`${Math.round(level * 100)} %`, left - 6, py);
      }
    }

    context.strokeStyle = '#7bd88f';
    context.beginPath();
    steps.forEach((step, index) => {
      const py = y(step.success / Math.max(step.trials, 1));
      if (index === 0) {
        context.moveTo(x(index), py);
      } else {
        context.lineTo(x(index), py);
      }
    });
    context.stroke();

    context.textAlign = 'center';
    steps.forEach((step, index) => {
      const px = Math.round(x(index)) + 0.5;
      const [lo, hi] = clopperPearson(step.success, step.trials);
      context.strokeStyle = '#7bd88f';
      context.beginPath();
      context.moveTo(px, y(lo));
      context.lineTo(px, y(hi));
      context.stroke();
      context.fillStyle = step.falsePositive > 0 ? '#c8553d' : '#7bd88f';
      context.fillRect(px - 3, Math.round(y(step.success / Math.max(step.trials, 1))) - 2.5, 6, 6);
      context.fillStyle = '#7f8884';
      context.fillText(formatValue(step.value), px, bottom + 12);
    });
  }, [steps, total]);
  return <canvas ref={ref} className="stress__chart" />;
}

function StressSuite({ state }: { state: AppState }) {
  const stress = state.validation.stress;
  const parameter = STRESS_PARAMETERS[stress.parameter] ?? STRESS_PARAMETERS[0];
  const options = STRESS_PARAMETERS.map((p, index) => ({ value: index, label: p.label }));
  const lastGood = [...stress.steps].reverse().find((s) => s.success / Math.max(s.trials, 1) > SUCCESS_TARGET);
  const firstBad = stress.steps.find((s) => s.success / Math.max(s.trials, 1) <= SUCCESS_TARGET);

  return (
    <section className="suite">
      <header className="suite__head">
        <div>
          <h2>4 &middot; Stress sweep</h2>
          <p>
            One sensor quantity is swept while every other disturbance is off, to find where the solver stops
            locking. The same attitudes are used at every step. Bars are exact 95 % intervals; the amber line is the
            98 % target; a red marker means a false positive occurred at that step. Body rate is a rotation about the
            sensor y axis during the {fixed(0.1, 1)} s exposure.
          </p>
        </div>
        <div className="suite__actions">
          <Select label="Parameter" value={stress.parameter} options={options}
            onChange={(value) => { setStressOptions({ parameter: value }); }} />
          <Select label="Trials per step" value={stress.trialsPerStep} options={STRESS_TRIAL_OPTIONS}
            onChange={(trialsPerStep) => { setStressOptions({ trialsPerStep }); }} />
          {stress.running ? (
            <button type="button" className="button" onClick={cancelStressSuite}>
              Stop
            </button>
          ) : (
            <button type="button" className="button" disabled={!state.ready || state.validation.lis.running} onClick={runStressSuite}>
              Run
            </button>
          )}
        </div>
      </header>
      {stress.error ? <p className="fault">{stress.error}</p> : null}
      <StressChart steps={stress.steps} total={stress.total} />
      <p className="progress__text">
        {parameter.label} ({parameter.unit}) &middot; {stress.steps.length} / {stress.total} steps
        {lastGood && firstBad && firstBad.value > lastGood.value
          ? ` · above ${percent(SUCCESS_TARGET, 0)} up to ${formatValue(lastGood.value)}, below from ${formatValue(firstBad.value)}`
          : ''}
      </p>
      <table className="data data--grid">
        <thead>
          <tr>
            <th scope="col">{parameter.label} ({parameter.unit})</th>
            <th scope="col">Trials</th>
            <th scope="col">Success</th>
            <th scope="col">95 % interval</th>
            <th scope="col">No solution</th>
            <th scope="col">Inaccurate</th>
            <th scope="col">False positives</th>
            <th scope="col">Cross median (&Prime;)</th>
          </tr>
        </thead>
        <tbody>
          {stress.steps.map((step) => {
            const [lo, hi] = clopperPearson(step.success, step.trials);
            return (
              <tr key={step.value}>
                <th scope="row">{formatValue(step.value)}</th>
                <td>{step.trials}</td>
                <td>{percent(step.success / Math.max(step.trials, 1), 1)}</td>
                <td>{`${percent(lo, 1)} – ${percent(hi, 1)}`}</td>
                <td>{step.noSolution}</td>
                <td>{step.inaccurate}</td>
                <td className={step.falsePositive > 0 ? 'verdict--fail' : undefined}>{step.falsePositive}</td>
                <td>{fixed(step.crossMedianArcsec, 2)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {stress.steps.length === 0 ? <p className="empty">Run the sweep to fill the table.</p> : null}
    </section>
  );
}

export function ValidationBench({ state }: { state: AppState }) {
  return (
    <div className="validation">
      <CentroidSuite state={state} />
      <WahbaSuite state={state} />
      <LisSuite state={state} />
      <StressSuite state={state} />
    </div>
  );
}
