import { useEffect, useMemo } from 'react';

import { NumberField, Section } from '../components/controls';
import { MissionAngles, MissionTrack } from '../components/MissionChart';
import { fixed, percent } from '../lib/format';
import { missionSpec } from '../lib/mission';
import {
  evaluateMission,
  openSelectedOnSyntheticBench,
  selectSample,
  setMission,
  useMissionState,
} from '../state/mission';
import type { AppState } from '../state/store';

const BLINDER_NAME = { none: 'nothing', sun: 'the Sun', earth: 'the Earth', moon: 'the Moon' } as const;

function Cell({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="strip__cell">
      <span className="strip__label">{label}</span>
      <span className="strip__value">{value}</span>
      {detail ? <span className="strip__detail">{detail}</span> : null}
    </div>
  );
}

export function MissionBench({ state }: { state: AppState }) {
  const { setup, report, error, selected } = useMissionState();
  const spec = useMemo(() => missionSpec(setup), [setup]);

  useEffect(() => {
    if (state.ready) {
      evaluateMission();
    }
  }, [state.ready]);

  const dash = fixed(NaN, 1);
  const clear = report ? 1 - report.blindedBySun - report.blindedByEarth - report.blindedByMoon : NaN;
  const sample = selected === null ? undefined : report?.samples[selected];

  return (
    <div className="bench bench--design">
      <aside className="panel panel--left">
        <Section title="Orbit" aside={report ? `${fixed(report.periodSeconds / 60, 1)} min` : 'circular'}>
          <NumberField label="Altitude" value={setup.altitudeKm} min={200} max={36000} step={10} digits={0} unit="km" slider log
            onChange={(altitudeKm) => { setMission({ altitudeKm }); }} />
          <NumberField label="Inclination" value={setup.inclinationDeg} min={0} max={180} step={0.5} digits={1} unit={'°'} slider
            onChange={(inclinationDeg) => { setMission({ inclinationDeg }); }} />
          <NumberField label="Node" value={setup.nodeDeg} min={0} max={360} step={1} digits={0} unit={'°'} wrap slider
            title="Right ascension of the ascending node; with the date it sets how the Sun stands to the orbit"
            onChange={(nodeDeg) => { setMission({ nodeDeg }); }} />
        </Section>

        <Section title="Date" aside="where the Sun and Moon are">
          <NumberField label="Year" value={setup.year} min={1990} max={2040} step={1} digits={0}
            onChange={(year) => { setMission({ year: Math.round(year) }); }} />
          <NumberField label="Month" value={setup.month} min={1} max={12} step={1} digits={0}
            onChange={(month) => { setMission({ month: Math.round(month) }); }} />
          <NumberField label="Day" value={setup.day} min={1} max={31} step={1} digits={0}
            onChange={(day) => { setMission({ day: Math.round(day) }); }} />
        </Section>

        <Section title="Camera" aside={setup.tumbleDeg > 0 ? 'tumbling' : 'fixed to the orbit'}>
          <NumberField label="Tilt" value={setup.tiltDeg} min={0} max={180} step={1} digits={0} unit={'°'} slider
            title="Angle of the boresight from the direction away from the Earth; 180 looks straight down"
            onChange={(tiltDeg) => { setMission({ tiltDeg }); }} />
          <NumberField label="Toward" value={setup.azimuthDeg} min={0} max={360} step={5} digits={0} unit={'°'} wrap slider
            title="Direction of the tilt: 0 along the track, 90 toward the orbit normal"
            onChange={(azimuthDeg) => { setMission({ azimuthDeg }); }} />
          <NumberField label="Tumble" value={setup.tumbleDeg} min={0} max={3} step={0.05} digits={2} unit={'°/s'} slider
            title="Above zero the camera turns at this rate instead of following the orbit"
            onChange={(tumbleDeg) => { setMission({ tumbleDeg }); }} />
        </Section>

        <Section title="Keep clear of" aside="from the boresight">
          <NumberField label="Sun" value={setup.sunExclusionDeg} min={0} max={90} step={1} digits={0} unit={'°'} slider
            onChange={(sunExclusionDeg) => { setMission({ sunExclusionDeg }); }} />
          <NumberField label={'Earth’s limb'} value={setup.earthLimbExclusionDeg} min={0} max={90} step={1} digits={0} unit={'°'} slider
            title="Measured from the edge of the Earth's disc, sunlit or not"
            onChange={(earthLimbExclusionDeg) => { setMission({ earthLimbExclusionDeg }); }} />
          <NumberField label="Moon" value={setup.moonExclusionDeg} min={0} max={90} step={1} digits={0} unit={'°'} slider
            onChange={(moonExclusionDeg) => { setMission({ moonExclusionDeg }); }} />
        </Section>

        <Section title="Span">
          <NumberField label="Revolutions" value={setup.orbits} min={1} max={30} step={1} digits={0}
            onChange={(orbits) => { setMission({ orbits: Math.max(1, Math.round(orbits)) }); }} />
        </Section>

        <Section title="Assumptions">
          <ul className="notes">
            <li>The orbit is a circle whose plane turns at the rate the Earth&rsquo;s oblateness gives it. It shows how much of an orbit is usable, not where a spacecraft is.</li>
            <li>The Sun, the Moon and the planets come from low-precision series, good to a few tenths of a degree.</li>
            <li>A body inside its angle blinds the camera completely and one outside does nothing. Real baffles let stray light in gradually.</li>
            <li>In the Earth&rsquo;s shadow, taken as a cylinder, the Sun blinds nothing.</li>
          </ul>
        </Section>
      </aside>

      <section className="design">
        <header className="design__head">
          <h2>Angles from the boresight</h2>
          <p>
            {report
              ? `${report.samples.length} moments over ${fixed((report.periodSeconds * setup.orbits) / 60, 0)} min · turning at ${fixed(report.bodyRateDeg, 3)}°/s`
              : 'Evaluating'}
          </p>
        </header>
        {error ? <p className="fault">{error}</p> : null}
        <MissionAngles report={report} spec={spec} selected={selected} onSelect={selectSample} />
        <MissionTrack report={report} selected={selected} />
      </section>

      <aside className="panel panel--right">
        <section className="group">
          <header className="group__head">
            <h2>Over the span</h2>
            <span className="group__aside">geometry</span>
          </header>
          <table className="data">
            <tbody>
              <tr><th scope="row">Clear</th><td>{report ? percent(clear, 1) : dash}</td></tr>
              <tr><th scope="row">Blinded by the Sun</th><td>{report ? percent(report.blindedBySun, 1) : dash}</td></tr>
              <tr><th scope="row">Blinded by the Earth</th><td>{report ? percent(report.blindedByEarth, 1) : dash}</td></tr>
              <tr><th scope="row">Blinded by the Moon</th><td>{report ? percent(report.blindedByMoon, 1) : dash}</td></tr>
              <tr><th scope="row">Longest blind stretch</th><td>{fixed((report?.longestBlindSeconds ?? NaN) / 60, 1)}<span className="unit">min</span></td></tr>
              <tr><th scope="row">In the Earth&rsquo;s shadow</th><td>{report ? percent(report.inEclipse, 1) : dash}</td></tr>
              <tr><th scope="row">Body rate</th><td>{fixed(report?.bodyRateDeg ?? NaN, 3)}<span className="unit">{'°/s'}</span></td></tr>
            </tbody>
          </table>
          {report && clear <= 0 ? (
            <p className="notice">Carried this way the camera never has a clear view. Tilt it away from what blinds it, or allow a smaller angle.</p>
          ) : null}
          {report && report.longestBlindSeconds > 0 && clear > 0 ? (
            <p className="hint">
              A star tracker alone has no attitude for {fixed(report.longestBlindSeconds / 60, 1)} minutes at a stretch. A gyro
              carries it across; see the gyro section of the validation document for how far it drifts.
            </p>
          ) : null}
        </section>

        <section className="group">
          <header className="group__head">
            <h2>At one moment</h2>
            <span className="group__aside">{sample ? `${fixed(sample.seconds / 60, 1)} min` : 'click the chart'}</span>
          </header>
          <table className="data">
            <tbody>
              <tr><th scope="row">Sun</th><td>{fixed(sample?.sunDeg ?? NaN, 1)}<span className="unit">{'°'}</span></td></tr>
              <tr><th scope="row">Earth&rsquo;s limb</th><td>{fixed(sample?.earthLimbDeg ?? NaN, 1)}<span className="unit">{'°'}</span></td></tr>
              <tr><th scope="row">Moon</th><td>{fixed(sample?.moonDeg ?? NaN, 1)}<span className="unit">{'°'}</span></td></tr>
              <tr><th scope="row">Blinded by</th><td>{sample ? BLINDER_NAME[sample.blinder] : dash}</td></tr>
              <tr><th scope="row">RA</th><td>{fixed(sample?.pointing.raDeg ?? NaN, 2)}<span className="unit">{'°'}</span></td></tr>
              <tr><th scope="row">Dec</th><td>{fixed(sample?.pointing.decDeg ?? NaN, 2)}<span className="unit">{'°'}</span></td></tr>
            </tbody>
          </table>
          <div className="actions">
            <button type="button" className="button" disabled={!sample} onClick={openSelectedOnSyntheticBench}>
              Open this sky on Synthetic bench
            </button>
          </div>
        </section>
      </aside>

      <div className="strip">
        <Cell label="Period" value={report ? `${fixed(report.periodSeconds / 60, 1)} min` : dash} detail={`${fixed(setup.altitudeKm, 0)} km`} />
        <Cell label="Clear" value={report ? percent(clear, 1) : dash} detail="can see stars" />
        <Cell label="Sun" value={report ? percent(report.blindedBySun, 1) : dash} detail={`inside ${fixed(setup.sunExclusionDeg, 0)}°`} />
        <Cell label="Earth" value={report ? percent(report.blindedByEarth, 1) : dash} detail={`limb inside ${fixed(setup.earthLimbExclusionDeg, 0)}°`} />
        <Cell label="Moon" value={report ? percent(report.blindedByMoon, 1) : dash} detail={`inside ${fixed(setup.moonExclusionDeg, 0)}°`} />
        <Cell label="Longest gap" value={report ? `${fixed(report.longestBlindSeconds / 60, 1)} min` : dash} detail="without stars" />
        <Cell label="Shadow" value={report ? percent(report.inEclipse, 1) : dash} detail="of the span" />
      </div>
    </div>
  );
}
