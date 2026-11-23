# Changelog

## 0.1.0 (2026-11-24)

First public version.

### Core

- Compact Hipparcos catalog (V <= 6.5, 8 870 stars) with a validating reader.
- Synthetic sensor: pinhole and Brown-Conrady projection, pixel-integrated
  Gaussian PSF, shot and read noise, hot pixels, cosmic rays, stray-light
  gradient, motion blur from the body rate, colour term.
- Detection with a local background mesh, run-based connected components,
  false-star filters and an iterated Gaussian-weighted centroid.
- k-vector pair index and Pyramid identification with whole-field
  verification, a multiple-testing correction and a search budget.
- QUEST, Davenport q-method and Jacobi-SVD solutions of Wahba's problem.
- Focal-length search and calibration for photographs; a ladder of fields
  when the field of view is unknown.
- Frame-to-frame tracker with a rate estimate, coasting and fallback.
- Validation suites: centroid accuracy, Wahba equivalence, lost-in-space
  Monte Carlo, stress sweeps.
- `StarTracker`: camera, catalog, index and tracking state in one object;
  frame in, attitude out.
- Pair index kept in its serialized form (`.asti`) and searched in place, so
  flight software reads it from storage without building or copying it.
  `data/flight-14deg.*` ships one for the reference camera.
- Simulator and validation code behind the `sim` and `validation` features;
  the default build is the flight path.
- Second opinion for doubtful tracked solutions, which stops leftovers of
  the previous frame from holding the lock.
- Registration and stacking of frame sequences.
- Camera calibration from several frames (focal length, principal point,
  `k1 k2 p1 p2`) with parameter uncertainties.
- Stellar aberration and a low-precision Earth velocity.
- Equidistant lens model beside the pinhole; wide lenses are locked from
  the image center outward. Solves real frames of 88 degree meteor cameras.
- FITS image extensions by name, including meteor-network four-frame files.
- Closed-form camera design: limiting magnitude, stars per field, sky
  coverage, accuracy, streak limit.
- Attitude fit weighs each star by the accuracy of its centroid and cuts
  the weight of a star that lies far from its predicted place.
- A solution that explains under half the detections gathers the stars
  again around the attitude found before it is reported.
- Second catalog to V 8 (`data/hip80.astc`, 41 394 stars) for fields down
  to about 4 degrees.
- Tycho-2 stars beside Hipparcos ones (`xtask build-tycho`), and index
  records with 32-bit star positions for catalogs of more than 65 535
  stars; fields down to about 2 degrees.
- Lens and detector flaws in the simulator: vignetting, gain and dark
  patterns, column noise, field-dependent PSF, coma, rolling shutter,
  nonlinearity, blooming.
- Rolling shutter undone in the solver when the rate is known.
- Low-precision Sun, Moon and planets (`solar.rs`), checked against ERFA.
- Attitude filter joining a rate gyro to the star tracker (`filter.rs`),
  and `StarTracker::update_aided` to run the two together.
- Stars drawn into trails by rotation can be kept and centered along
  their length (`DetectConfig::trails`).
- Orbit, pointing profiles and Sun, Earth and Moon exclusion angles
  (`orbit.rs`).
- `examples/emulated`: the flight path run on an emulated Cortex-M7.
- Catalog and index headers carry their own checksum.
- `detect::mirrored` and `asterope solve --mirrored` for images reversed
  by a mirror.
- `DetectConfig::max_runs` bounds the memory detection can take on a
  cluttered frame.
- Python package (`crates/asterope-py`).
- `asterope orbit`: what blinds a camera along an orbit.
- Atmospheric refraction for cameras on the ground (`Catalog::refract`,
  `compare-ecsv --refraction`).

### Interface

- Synthetic, Image and Validation benches.
- Search progress and stop, sequence playback, pipeline-stage isolation.
- Star names, all-sky locator, RA/Dec graticule, residual vectors.
- Export to JSON, CSV, FITS WCS and 16-bit FITS; shareable bench links.
- Bundled example frames.
- Video bench: files and live camera, timeline, sky path, frame stacking,
  attitude log export; the Synthetic bench records sequences as video.
- Design bench with an all-sky coverage map and a simulator check of every
  prediction.
- Manual tab and a one-line statement of what each bench answers.
- Mission bench; lens and detector flaws on the Synthetic bench.
- Mirrored images on the Image bench.
- A single-column layout for narrow screens.
- Runs in Chromium, Firefox and WebKit.

### Library and tools

- `asterope-ffi`: C interface with a generated header, static and shared
  library, and a C example.
- `asterope` command: `solve`, `simulate`, `build-index`, `calibrate`,
  `replay`, `compare-ecsv`, `design`, `validate`.
- `examples/embedded`: bare-metal build for Cortex-M7 and RV32, used to
  measure code size.
- Minimum Rust version 1.88.
