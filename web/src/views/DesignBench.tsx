import { useEffect, useMemo } from 'react';

import { NumberField, Section, Select } from '../components/controls';
import { SkyCoverage } from '../components/SkyCoverage';
import { fixed, integer, percent } from '../lib/format';
import {
  VERIFY_TRIALS,
  evaluateDesign,
  openDesignOnSyntheticBench,
  setDesign,
  setVerifyTrials,
  stopVerification,
  useDesignState,
  verifyDesign,
} from '../state/design';
import type { AppState } from '../state/store';

function median(values: number[]): number {
  const sorted = values.filter((v) => Number.isFinite(v)).sort((a, b) => a - b);
  return sorted.length === 0 ? NaN : (sorted[Math.floor((sorted.length - 1) / 2)] ?? NaN);
}

function Cell({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="strip__cell">
      <span className="strip__label">{label}</span>
      <span className="strip__value">{value}</span>
      {detail ? <span className="strip__detail">{detail}</span> : null}
    </div>
  );
}

export function DesignBench({ state }: { state: AppState }) {
  const { spec, report, error, verification } = useDesignState();

  useEffect(() => {
    if (state.ready) {
      evaluateDesign();
    }
  }, [state.ready]);

  const measured = useMemo(() => {
    const { outcomes } = verification;
    if (outcomes.length === 0) {
      return null;
    }
    const solved = outcomes.filter((o) => o.class === 'success' || o.class === 'inaccurate');
    return {
      trials: outcomes.length,
      lockRate: solved.length / outcomes.length,
      falseLocks: outcomes.filter((o) => o.class === 'falsePositive').length,
      detections: outcomes.reduce((sum, o) => sum + o.detections, 0) / outcomes.length,
      cross: median(solved.map((o) => o.crossArcsec)),
      roll: median(solved.map((o) => o.rollArcsec)),
    };
  }, [verification]);

  const dash = fixed(NaN, 1);
  const depth = report ? Math.min(report.limitingMagnitude, 6.5) : NaN;

  return (
    <div className="bench bench--design">
      <aside className="panel panel--left">
        <Section title="Lens" aside={report ? `f/${fixed(report.focalRatio, 2)}` : ''}>
          <NumberField label="Focal length" value={spec.focalLengthMm} min={2} max={400} step={0.5} digits={1} unit="mm" slider log
            onChange={(focalLengthMm) => { setDesign({ focalLengthMm }); }} />
          <NumberField label="Aperture" value={spec.apertureMm} min={1} max={200} step={0.5} digits={1} unit="mm" slider log
            title="Diameter of the entrance pupil"
            onChange={(apertureMm) => { setDesign({ apertureMm }); }} />
          <NumberField label="Transmission" value={spec.transmission} min={0.1} max={1} step={0.01} digits={2}
            title="Fraction of the light that passes the lens"
            onChange={(transmission) => { setDesign({ transmission }); }} />
        </Section>

        <Section title="Detector">
          <NumberField label="Pixel pitch" value={spec.pixelUm} min={1} max={30} step={0.05} digits={2} unit={'µm'}
            onChange={(pixelUm) => { setDesign({ pixelUm }); }} />
          <NumberField label="Width" value={spec.width} min={128} max={2048} step={8} digits={0} unit="px"
            onChange={(width) => { setDesign({ width: Math.round(width) }); }} />
          <NumberField label="Height" value={spec.height} min={128} max={2048} step={8} digits={0} unit="px"
            onChange={(height) => { setDesign({ height: Math.round(height) }); }} />
          <NumberField label="Quantum efficiency" value={spec.quantumEfficiency} min={0.05} max={1} step={0.01} digits={2}
            title="Mean over the passband"
            onChange={(quantumEfficiency) => { setDesign({ quantumEfficiency }); }} />
          <NumberField label="Passband" value={spec.bandwidthNm} min={50} max={600} step={10} digits={0} unit="nm"
            title="Width of the spectral band the detector sees through the lens"
            onChange={(bandwidthNm) => { setDesign({ bandwidthNm }); }} />
          <NumberField label="Read noise" value={spec.readNoise} min={0.5} max={100} step={0.5} digits={1} unit={'e⁻'}
            onChange={(readNoise) => { setDesign({ readNoise }); }} />
          <NumberField label="Dark current" value={spec.darkCurrent} min={0} max={5000} step={1} digits={0} unit={'e⁻/px/s'}
            onChange={(darkCurrent) => { setDesign({ darkCurrent }); }} />
        </Section>

        <Section title="Exposure">
          <NumberField label="Exposure" value={spec.exposure} min={0.001} max={2} step={0.005} digits={3} unit="s" slider log
            onChange={(exposure) => { setDesign({ exposure }); }} />
          <NumberField label={'PSF σ'} value={spec.psfSigma} min={0.4} max={4} step={0.05} digits={2} unit="px"
            title="Blur of a star image; trackers defocus slightly so the centroid can be interpolated"
            onChange={(psfSigma) => { setDesign({ psfSigma }); }} />
          <NumberField label="Sky background" value={spec.skyBackground} min={0} max={5000} step={1} digits={0} unit={'e⁻/px/s'}
            onChange={(skyBackground) => { setDesign({ skyBackground }); }} />
          <NumberField label="Peak threshold" value={spec.peakSigma} min={3} max={20} step={0.5} digits={1} unit={'σ'}
            onChange={(peakSigma) => { setDesign({ peakSigma }); }} />
        </Section>

        <Section title="Motion" aside="smears the exposure">
          <NumberField label="Body rate" value={spec.rateDeg} min={0} max={10} step={0.05} digits={2} unit={'°/s'} slider
            title="Rotation across the boresight while the shutter is open"
            onChange={(rateDeg) => { setDesign({ rateDeg }); }} />
        </Section>

        <Section title="Assumptions">
          <ul className="notes">
            <li>A star of V = 0 delivers 1005 photons per second, per cm&sup2; and per &Aring; at 550 nm, taken as constant across the passband. Real stars differ by tens of percent with colour.</li>
            <li>A star counts as detectable when its brightest pixel reaches the peak threshold over the background noise, for half of the possible positions within a pixel.</li>
            <li>The catalog stops at V 6.5. A camera that sees fainter stars is shown with the catalog&rsquo;s counts, which then understate it.</li>
            <li>Accuracy adds up the information of the stars in the field, as the solver's weighting does, with a centroid 1.3 times the photon and background bound, as measured.</li>
            <li>Lens distortion, vignetting, stray light and hot pixels are not part of the prediction; the simulator adds them on the Synthetic bench.</li>
          </ul>
        </Section>
      </aside>

      <section className="design">
        <header className="design__head">
          <h2>Sky coverage</h2>
          <p>
            {report
              ? `${fixed(report.fovXDeg, 2)}° × ${fixed(report.fovYDeg, 2)}° · ${fixed(report.pixelScaleArcsec, 1)}″ per pixel · stars to V ${fixed(depth, 2)}`
              : 'Evaluating'}
          </p>
        </header>
        {error ? <p className="fault">{error}</p> : null}
        <SkyCoverage counts={report?.counts ?? null} />
      </section>

      <aside className="panel panel--right">
        <section className="group">
          <header className="group__head">
            <h2>Prediction</h2>
            <span className="group__aside">closed form</span>
          </header>
          <table className="data">
            <tbody>
              <tr><th scope="row">Limiting magnitude</th><td>{fixed(report?.limitingMagnitude ?? NaN, 2)}<span className="unit">V</span></td></tr>
              <tr><th scope="row">Stars per field</th><td>{fixed(report?.starsPerField ?? NaN, 1)}</td></tr>
              <tr><th scope="row">Fewest · median</th><td>{report ? `${report.fewestStars} · ${report.medianStars}` : dash}</td></tr>
              <tr><th scope="row">Sky with 4 or more</th><td>{report ? percent(report.coverage[0], 1) : dash}</td></tr>
              <tr><th scope="row">Sky with 5 or more</th><td>{report ? percent(report.coverage[1], 1) : dash}</td></tr>
              <tr><th scope="row">Sky with 8 or more</th><td>{report ? percent(report.coverage[2], 1) : dash}</td></tr>
              <tr><th scope="row">{'1σ cross-boresight'}</th><td>{fixed(report?.sigmaCrossArcsec ?? NaN, 2)}<span className="unit">{'″'}</span></td></tr>
              <tr><th scope="row">{'1σ about boresight'}</th><td>{fixed(report?.sigmaRollArcsec ?? NaN, 1)}<span className="unit">{'″'}</span></td></tr>
              <tr><th scope="row">Star trail</th><td>{fixed(report?.trailPx ?? NaN, 1)}<span className="unit">px</span></td></tr>
              <tr><th scope="row">Streak limit</th><td>{fixed(report?.streakRateDeg ?? NaN, 2)}<span className="unit">{'°/s'}</span></td></tr>
              <tr><th scope="row">Zero point</th><td>{report ? integer(Math.round(report.zeroPoint)) : dash}<span className="unit">{'e⁻/s'}</span></td></tr>
              <tr><th scope="row">Background noise</th><td>{fixed(report?.noise ?? NaN, 2)}<span className="unit">{'e⁻'}</span></td></tr>
            </tbody>
          </table>
          {report?.catalogLimited ? (
            <p className="notice">This camera sees fainter than the catalog. Counts, coverage and accuracy are those of V 6.5 and understate it.</p>
          ) : null}
          {report && report.trailPx > 7.94 * spec.psfSigma ? (
            <p className="notice">At this body rate stars trail past the streak limit and the detector rejects them.</p>
          ) : null}
        </section>

        <section className="group">
          <header className="group__head">
            <h2>Verification</h2>
            <span className="group__aside">
              {verification.running ? `${verification.outcomes.length} / ${verification.trials}` : 'simulator'}
            </span>
          </header>
          <div className="actions">
            {verification.running ? (
              <button type="button" className="button" onClick={stopVerification}>
                Stop
              </button>
            ) : (
              <button type="button" className="button" disabled={!report} onClick={verifyDesign}>
                Simulate and solve
              </button>
            )}
            <button type="button" className="button" disabled={!report} onClick={openDesignOnSyntheticBench}>
              Open on Synthetic bench
            </button>
          </div>
          <Select
            label="Random attitudes"
            value={verification.trials}
            options={VERIFY_TRIALS.map((trials) => ({ value: trials, label: `${trials}` }))}
            onChange={setVerifyTrials}
          />
          {verification.error ? <p className="fault">{verification.error}</p> : null}
          <table className="data data--pointing">
            <thead>
              <tr>
                <th />
                <th scope="col">PREDICTED</th>
                <th scope="col">MEASURED</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <th scope="row">Stars detected</th>
                <td>{fixed(report?.starsPerField ?? NaN, 1)}</td>
                <td>{fixed(measured?.detections ?? NaN, 1)}</td>
              </tr>
              <tr>
                <th scope="row">Locks</th>
                <td>{report ? percent(report.coverage[1], 1) : dash}</td>
                <td>{measured ? percent(measured.lockRate, 1) : dash}</td>
              </tr>
              <tr>
                <th scope="row">{'Cross median (″)'}</th>
                <td>{fixed(report?.medianCrossArcsec ?? NaN, 2)}</td>
                <td>{fixed(measured?.cross ?? NaN, 2)}</td>
              </tr>
              <tr>
                <th scope="row">{'Roll median (″)'}</th>
                <td>{fixed(report?.medianRollArcsec ?? NaN, 1)}</td>
                <td>{fixed(measured?.roll ?? NaN, 1)}</td>
              </tr>
              <tr>
                <th scope="row">False locks</th>
                <td>{dash}</td>
                <td>{measured ? `${measured.falseLocks} / ${measured.trials}` : dash}</td>
              </tr>
            </tbody>
          </table>
          <p className="hint">
            The designed camera is rendered at random attitudes with noise and solved from scratch. The predicted lock rate is the
            share of sky with five or more detectable stars.
          </p>
        </section>
      </aside>

      <div className="strip">
        <Cell label="Field of view" value={report ? `${fixed(report.fovXDeg, 2)}°` : dash} detail={report ? `${fixed(report.pixelScaleArcsec, 1)}″/px` : ''} />
        <Cell label="Limiting magnitude" value={fixed(report?.limitingMagnitude ?? NaN, 2)} detail={report?.catalogLimited ? 'catalog ends at 6.5' : ''} />
        <Cell label="Stars per field" value={fixed(report?.starsPerField ?? NaN, 1)} detail={report ? `min ${report.fewestStars}` : ''} />
        <Cell label="Solvable sky" value={report ? percent(report.coverage[1], 1) : dash} detail="5 or more stars" />
        <Cell label="Cross-boresight" value={report ? `${fixed(report.medianCrossArcsec, 2)}″` : dash} detail="median" />
        <Cell label="About boresight" value={report ? `${fixed(report.medianRollArcsec, 1)}″` : dash} detail="median" />
        <Cell label="Streak limit" value={report ? `${fixed(report.streakRateDeg, 2)}°/s` : dash} detail={report ? `trail ${fixed(report.trailPx, 1)} px` : ''} />
      </div>
    </div>
  );
}
