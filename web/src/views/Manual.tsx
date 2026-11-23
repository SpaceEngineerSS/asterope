import { BENCHES } from '../data/benches';

const PIPELINE = [
  {
    step: 'Frame',
    detail: 'A 16-bit image of the sky from the sensor, the simulator, a photograph or a video.',
  },
  {
    step: 'Detection',
    detail: 'The local background is removed and connected bright pixels are grouped. Hot pixels, cosmic rays and streaks are rejected by their shape.',
  },
  {
    step: 'Centroids',
    detail: 'Each star image is located to a few hundredths of a pixel from its intensity-weighted center.',
  },
  {
    step: 'Identification',
    detail: 'Angles between stars are looked up in an index of catalog pairs. Four stars that fit together form a pyramid; the rest of the field must then agree.',
  },
  {
    step: 'Attitude',
    detail: 'The rotation that best maps catalog directions onto the measured ones is solved in closed form (QUEST) and checked against two other methods.',
  },
  {
    step: 'Tracking',
    detail: 'The next frame is solved from the predicted attitude in a fraction of the time. A doubtful result is checked by a new search.',
  },
] as const;

const GLOSSARY = [
  ['Attitude', 'The orientation of the sensor relative to the stars: three angles, or equivalently one rotation.'],
  ['Quaternion', 'Four numbers that encode a rotation without the singularities of angles. Here: ICRS to sensor, scalar part last, kept non-negative.'],
  ['Boresight', 'The direction the camera looks along; the sensor +Z axis, through the middle of the image.'],
  ['RA, Dec, Roll', 'Right ascension and declination are the sky coordinates of the boresight. Roll is the position angle of image-up, counted from north through east.'],
  ['ICRS', 'The celestial reference frame the catalog is given in; for this purpose the same as J2000 equatorial coordinates.'],
  ['Field of view', 'The angle the image spans, here always its horizontal extent. Wide fields see more bright stars; narrow fields measure angles more finely.'],
  ['Magnitude', 'Brightness on an inverted logarithmic scale: five magnitudes are a factor of 100, and larger numbers are fainter. The catalog here ends at V 6.5, roughly the naked-eye limit.'],
  ['Arcsecond', '1/3600 of a degree. A two-centimeter coin four kilometers away is about one arcsecond wide.'],
  ['Centroid', 'The measured center of a star image, in pixels and fractions of a pixel.'],
  ['Lost in space', 'Finding the attitude from one frame with no prior knowledge. The hard case, and the one a tracker falls back to.'],
  ['Pair index', 'Every pair of catalog stars close enough to share a frame, sorted by their separation, so that a measured angle can be looked up at once.'],
  ['Pyramid', 'Four stars whose six mutual angles all match the catalog: the smallest pattern that is rarely matched by chance.'],
  ['Aberration', 'The apparent shift of every star toward the direction the observer moves, up to 20 arcseconds from the motion of the Earth.'],
  ['PSF', 'Point spread function: the shape a star takes on the detector. Trackers blur it slightly on purpose so that the center can be interpolated.'],
  ['Exclusion angle', 'How close to the boresight the Sun, the Moon or the edge of the Earth may come before stray light keeps the camera from working. A property of the baffle.'],
  ['Gyro bias', 'The rate a gyro reads when nothing turns. It drifts, and a filter that has star fixes can estimate it and take it out.'],
  ['Kalman filter', 'A way of combining a prediction and a measurement by how much each can be trusted. Here it joins a gyro, which is smooth and drifts, to a star tracker, which is noisy and does not.'],
  ['Rolling shutter', 'A detector that reads its rows one after another. While the camera turns, each row shows the sky at a slightly different attitude.'],
  ['Noise equivalent angle', 'How much a reported attitude scatters from frame to frame with nothing else changing: the part of the error that averaging removes.'],
  ['Refraction', 'The bending of starlight by the atmosphere, which makes stars appear higher than they are. It matters only for cameras on the ground.'],
] as const;

/** The plain guide: what the application is for, how it works and how to build on it. */
export function Manual() {
  return (
    <article className="manual">
      <header>
        <h2>Manual</h2>
        <p className="manual__lede">
          Asterope finds which way a camera is pointing by recognizing the stars in its image. It is the software of a star
          tracker, the instrument a satellite uses to know its orientation, together with a workbench for trying that
          software, stressing it, and choosing a camera for it before one is bought. Asterope is a star of the Pleiades,
          one of the 8 870 in its catalog.
        </p>
      </header>

      <section>
        <h3>What it is made of</h3>
        <dl className="manual__parts">
          <dt>Core</dt>
          <dd>
            A Rust library without operating-system dependencies: frame in, attitude out. This is the part that would
            fly. Linked for a Cortex-M7 it takes 64 kB of code and 702 kB of star data.
          </dd>
          <dt>Simulator</dt>
          <dd>
            Renders what a given sensor would record at a given attitude, from the real sky, with photon noise, read
            noise, hot pixels, cosmic rays, stray light and motion blur. Vignetting, gain and dark patterns, column
            noise, coma, a rolling shutter and blooming can be added one at a time. It stands in for hardware you do
            not have.
          </dd>
          <dt>Workbench</dt>
          <dd>This application. Everything runs in your browser; no image or video leaves it.</dd>
        </dl>
      </section>

      <section>
        <h3>From a frame to an attitude</h3>
        <ol className="manual__pipeline">
          {PIPELINE.map((entry, index) => (
            <li key={entry.step}>
              <span className="manual__index">{index + 1}</span>
              <strong>{entry.step}</strong>
              <p>{entry.detail}</p>
            </li>
          ))}
        </ol>
        <p>
          A solution is reported only when the stars beyond the first four make a coincidence improbable, and the
          probability shown with every solution is that bound. No lock is an answer; a wrong lock is the failure this
          design works hardest to avoid.
        </p>
      </section>

      <section>
        <h3>Beside the camera</h3>
        <p>
          One frame gives one attitude. A satellite needs one all the time, including while the Earth or the Sun is in
          the way, and that takes more than a camera.
        </p>
        <dl className="manual__parts">
          <dt>Stars by accuracy</dt>
          <dd>
            A bright star is located better than a faint one, so the attitude is fitted with each star weighted by how
            well its center is known. Over 3 000 disturbed frames that brought the median error from 0.80 to 0.53
            arcseconds. A star that lies far from where the fit puts it has its weight cut.
          </dd>
          <dt>Trails</dt>
          <dd>
            A camera that turns during the exposure draws each star into a line. Left alone the detector takes those
            for particle tracks; told to keep them, it measures the middle of each line, and the lock holds to about
            twice the rate.
          </dd>
          <dt>A gyro</dt>
          <dd>
            A filter in the library joins a rate gyro to the star fixes. The gyro carries the attitude through frames
            without stars and tells the camera where to look for them again; the stars measure the drift of the gyro.
            It is in the library, from Rust, C and Python, and on no bench here.
          </dd>
          <dt>The orbit</dt>
          <dd>
            The Mission bench carries the camera round a circular orbit and shows when the Sun, the Earth or the Moon
            comes too close to the boresight. The orbit and the positions of those bodies are simple models, good for
            asking how much of an orbit is usable.
          </dd>
        </dl>
      </section>

      <section>
        <h3>The benches</h3>
        <table className="manual__table">
          <thead>
            <tr>
              <th scope="col">Bench</th>
              <th scope="col">The question it answers</th>
              <th scope="col">How to use it</th>
            </tr>
          </thead>
          <tbody>
            {BENCHES.map((bench) => (
              <tr key={bench.name}>
                <th scope="row">{bench.name}</th>
                <td>{bench.asks}</td>
                <td>{bench.use}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section>
        <h3>Reading the display</h3>
        <ul className="manual__list">
          <li><b>Green</b> marks a detection matched to a catalog star; <b>amber</b> one that is not; <b>red crosses</b> are blobs the detector rejected.</li>
          <li>The <b>lamp</b> reads LOCK, TRACK (solved from the prediction), ACQUIRE (solved by a search during a sequence), COAST, NO LOCK or FALSE LOCK. The last can only be known on the Synthetic bench, where the truth is available.</li>
          <li><b>Residuals</b> are drawn magnified from each star toward where the catalog says it should be. A pattern in them means the camera model is off.</li>
          <li><b>Cross-boresight</b> error tilts the pointing direction. Error <b>about the boresight</b> turns the image around its center and is several times larger, because stars in a small field constrain it weakly.</li>
        </ul>
      </section>

      <section>
        <h3>Using it in your own project</h3>
        <div className="manual__ways">
          <div>
            <h4>Rust</h4>
            <pre>{`let camera = Camera::from_fov(1024, 1024, 14f64.to_radians())?;
let mut tracker = StarTracker::from_bytes(camera, CATALOG, INDEX)?;
let fix = tracker.update(&frame, 0.1)?;
if let Some(q) = fix.attitude() { /* ICRS -> sensor */ }`}</pre>
            <p>The <code>asterope-core</code> crate is <code>no_std</code> with <code>alloc</code>. Simulator and validation code are optional features and are not part of a flight build.</p>
          </div>
          <div>
            <h4>C</h4>
            <pre>{`AsteropeTracker *tracker;
asterope_tracker_new(&camera, catalog, catalog_len, index, index_len, &tracker);
AsteropeFix fix;
asterope_tracker_update(tracker, pixels, width * height, 0.1, &fix);
if (fix.solved) { /* fix.q[0..3] */ }`}</pre>
            <p><code>asterope.h</code> and a static or shared library come from the <code>asterope-ffi</code> crate.</p>
          </div>
          <div>
            <h4>Command line</h4>
            <pre>{`asterope solve photo.jpg
asterope build-index hip65.astc cam.asti --fov 14 --catalog-out cam.astc
asterope calibrate frames/ --fov 40 --out camera.toml
asterope replay frames/ --camera camera.toml --out attitude.csv`}</pre>
            <p>The same solver without a browser, for scripts and ground processing.</p>
          </div>
          <div>
            <h4>Python</h4>
            <pre>{`tracker = asterope.StarTracker(1024, 1024, fov_deg=14.0)
fix = tracker.solve(frame)          # 2-D uint16 array
if fix.locked:
    print(fix.quaternion, fix.ra_deg, fix.dec_deg)`}</pre>
            <p>NumPy arrays in, quaternions out. <code>docs/python.md</code> has the rest.</p>
          </div>
        </div>
        <p>
          The pair index is built on the ground for one camera and stored next to the code; the tracker searches it where
          it lies and copies nothing. Per frame of 1024 &times; 1024 pixels the solver needs about 0.7 MB of working
          memory beside the frame itself. The integration guide in <code>docs/integration.md</code> lists the budget,
          the error cases and what has and has not been verified.
        </p>
      </section>

      <section>
        <h3>Limits to keep in mind</h3>
        <ul className="manual__list">
          <li>The catalog in the browser ends at V 6.5, so fields narrower than about 8 degrees often hold too few stars. The command line can use a V 8 catalog that reaches about 4 degrees.</li>
          <li>The simulator is a model. Nothing here has run on flight hardware; the real frames it has solved were all taken from the ground.</li>
          <li>Phone video shows few stars per frame; stacking helps only when at least three are visible in single frames.</li>
          <li>In the browser the tracker predicts from the last two attitudes. The gyro filter is in the library and is not on any bench.</li>
          <li>Every figure quoted in this application is measured by a test that is part of the source; <code>docs/validation.md</code> says how, and lists what is not verified.</li>
        </ul>
      </section>

      <section>
        <h3>Terms</h3>
        <dl className="manual__glossary">
          {GLOSSARY.map(([term, meaning]) => (
            <div key={term}>
              <dt>{term}</dt>
              <dd>{meaning}</dd>
            </div>
          ))}
        </dl>
      </section>
    </article>
  );
}
