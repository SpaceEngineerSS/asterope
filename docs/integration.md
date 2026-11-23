# Putting Asterope in flight software

This guide is for someone who has a camera, or is choosing one, and wants an
attitude quaternion out of its frames. It states what the library needs,
what it returns, what it costs, and what has not been verified. Read the
last section before relying on any of it.

## What goes in and what comes out

```
16-bit frame  ->  StarTracker::update(frame, dt)  ->  Fix
                                                       event      Acquired | Tracked | Coasting | Lost
                                                       solution   quaternion, RA/Dec/roll, matched stars,
                                                                  residual, 1-sigma, chance probability
                                                       body_rate  rad/s about the sensor axes
```

- **Frame**: `width * height` values of `u16`, row-major, top row first.
  Any bit depth up to 16; set `SolverConfig::detect.saturation` to the
  largest value the sensor delivers.
- **`dt`**: seconds since the previous frame. Only the tracker uses it.
- **Quaternion**: ICRS to sensor, scalar last, scalar part non-negative.
  `b = A(q) r` takes a catalog direction `r` to the sensor frame, where +Z
  is the boresight, +X is image right and +Y is image down. All conventions
  are in `docs/conventions.md`.

A frame without a solution is a normal outcome (`solution` is `None`), not
an error. Errors are reserved for bad input: a frame of the wrong size, a
damaged catalog or index.

## Three steps

### 1. Describe the camera

A `Camera` is a pinhole with Brown-Conrady distortion: focal length in
pixels, principal point, `k1 k2 k3 p1 p2`. `Camera::from_fov(width, height,
fov_x)` is enough to start. A lens wider than about 70 degrees is better
described as equidistant (`Camera::from_fov_equidistant`); `asterope solve`
without a field of view tries both and reports which one fits. For a real lens, take a handful of frames of
different parts of the sky and run

```
asterope calibrate frames/ --fov 14 --out camera.toml
```

which solves each frame with the approximate field of view, fits focal
length, principal point and `k1` to all matched stars, and prints the
uncertainty of each. Add `--k2` and `--tangential` for wide lenses. On a
narrow field the principal point is weakly determined; the printed sigma
says how weakly.

### 2. Build the index on the ground

```
asterope build-index data/hip65.astc camera.asti \
    --fov 14 --width 1024 --height 1024 \
    --stars-per-field 16 --max-angle 9.85 \
    --catalog-out camera.astc
```

writes the pair index (`docs/index-format.md`) and the catalog cut to match
it. `data/flight-14deg.asti` and `.astc` are exactly this command's output for
the reference camera. Smaller indexes lose success rate in a known way:

| Stars per field | Largest separation | Index | Success at 14 degrees |
|---|---|---|---|
| 25 | frame diagonal | 4.2 MB | 100.00 % |
| 16 | half the diagonal | 558 kB | 99.62 % |
| 12 | 0.6 of the diagonal | 447 kB | 98.52 % |
| 8 | 0.6 of the diagonal | 223 kB | 87.07 % |

(10 000 simulated trials each, no false positive in any; the full table is
in `docs/validation.md`, section 6.) Run `asterope validate --fov <yours>
--profiles 2000` to get the same table for your camera.

### 3. Link it

**Rust.** `asterope-core` is `#![no_std]` with `alloc` and depends only on
`libm`. Default features contain no simulator and no validation code.

```rust
use asterope_core::camera::Camera;
use asterope_core::flight::StarTracker;

static CATALOG: &[u8] = include_bytes!("camera.astc");
static INDEX: &[u8] = include_bytes!("camera.asti");

let camera = Camera::from_fov(1024, 1024, 14f64.to_radians())?;
let mut tracker = StarTracker::from_bytes(camera, CATALOG, INDEX)?;
loop {
    let fix = tracker.update(frame, dt)?;
    if let Some(q) = fix.attitude() {
        // q.x, q.y, q.z, q.w
    }
}
```

`examples/embedded` is this, as a bare-metal image with a frame arena for a
heap.

**C and C++.** `cargo build --release -p asterope-ffi` produces a static and
a shared library; the header is `crates/asterope-ffi/include/asterope.h`.

```c
AsteropeCamera camera = {0};
camera.width = 1024; camera.height = 1024; camera.fov_x_deg = 14.0;

AsteropeTracker *tracker;
if (asterope_tracker_new(&camera, catalog, catalog_len, index, index_len, &tracker) != ASTEROPE_STATUS_OK) { /* ... */ }

AsteropeFix fix;
asterope_tracker_update(tracker, pixels, 1024 * 1024, dt, &fix);
if (fix.solved) { /* fix.q[0..3], fix.ra_deg, fix.dec_deg, fix.roll_deg */ }
```

The catalog bytes may be released after `asterope_tracker_new`; the index
bytes must stay valid, and unchanged, until `asterope_tracker_free`. A
tracker must not be used from two threads at once. The C library needs the
Rust standard library and therefore an operating system; on bare metal use
the Rust crate. `crates/asterope-ffi/examples/solve.c` is a complete program.

## What it costs

Measured for the shipped index and catalog (`docs/validation.md`,
section 7):

| | 1024 x 1024 frame |
|---|---|
| Code, Cortex-M7, size-optimized | 64 kB |
| Index and catalog in read-only storage | 558 kB + 142 kB |
| Catalog decoded on the heap, once | 426 kB |
| Working heap per frame, peak | 668 kB (detection); 40 kB (identification); 9 kB (tracking step) |
| Allocations per frame | about 25 |
| Frame buffer (yours) | 2 097 kB |

These are for a frame of the sky. Detection memory also grows with what is
in the frame, by about 100 bytes for every run of bright pixels: a frame
with 2 % of hot pixels took 5.1 MB. `DetectConfig::max_runs` bounds it by
refusing such a frame; at 8 192 the most any frame took was 1.8 MB, and
the sky with 0.1 % of hot pixels still passed (`docs/validation.md`,
section 24). Identification is bounded by its search budgets: a search
that fails costs about three times one that succeeds, and no more memory.

Detection memory scales with the pixel count; the rest does not. Nothing is
kept on the heap between frames except the catalog, so an arena that is
reset after each frame works (it then has to hold the sum of a frame's
allocations, about 1 MB).

Time on a desktop processor, single thread: 8.0 ms detection and 0.1 ms
identification per frame; a tracking step replaces the identification. On an
emulated Cortex-M7 a frame takes 113 million instructions and gives the same
bits as the desktop (`docs/validation.md`, section 20). **There is no timing
on a real microcontroller.** Detection is a few passes over the image in
`f64` and integer arithmetic, and it dominates. A core without
a double-precision floating-point unit (Cortex-M4F) runs the same code with
software doubles.

## Things flight data needs that a simulator does not

- **Aberration.** Call `tracker.set_velocity(v)` with the spacecraft's
  velocity relative to the solar-system barycenter in km/s, ICRS axes:
  `ephem::earth_velocity_km_s(julian_date_tt)` plus the orbital velocity.
  Without it attitudes are off by up to 25 arcseconds. Update it a few times
  per orbit.
- **Proper motion.** `Catalog::at_epoch(year)` moves the stars from J2000 to
  the observation date; apply it on the ground before `build-index`, or pass
  `--epoch` to the command-line tools.
- **Saturation and bit depth.** `SolverConfig::default()` assumes 12 bits.
- **Hot pixels** are rejected by shape when isolated. A sensor with clusters
  of them needs a dark frame subtracted first.
- **The Sun, Moon and Earth in or near the field** are not handled beyond a
  smooth background gradient. `orbit::Surroundings` gives their angles from
  the boresight for an orbit position and date; gate the tracker with your
  own exclusion angles.
- **Rate.** Stars trailed over more than about 8 PSF sigmas are rejected as
  streaks: 1.3 deg/s for the reference camera at 0.1 s. Setting
  `DetectConfig::trails` keeps them and holds lock to about 2 deg/s;
  beyond that shorten the exposure.
- **A gyro.** `StarTracker::enable_gyro`, then `gyro(reading, dt)` for every
  reading and `update_aided(frame)` for every frame: the attitude is carried
  through frames without stars, the gyro bias is estimated, and a rolling
  shutter is undone with the gyro's rate (`GyroAiding::row_time`). The
  frames and the gyro readings must share a clock.
- **Rolling shutter without a gyro.** Set `SolverConfig::rolling` with the
  row time and a rate from elsewhere; left alone it costs tens of
  arcseconds at 1 deg/s.

## With a gyro

A star tracker has no attitude while the Earth or the Sun is in the way,
and takes a search to find one again. A rate gyro beside it carries the
attitude across.

```rust
use asterope_core::filter::GyroNoise;
use asterope_core::flight::GyroAiding;

tracker.enable_gyro(GyroAiding::new(GyroNoise {
    angle_random_walk: 5.2e-5,   // rad / sqrt(s): 0.18 deg / sqrt(h)
    rate_random_walk: 5.2e-7,    // rad / s / sqrt(s)
}));

// for every gyro reading, rad/s about the sensor axes, covering dt seconds:
tracker.gyro(reading, dt);
// for every frame, taken at the time of the latest reading:
let aided = tracker.update_aided(frame)?;
// aided.attitude     filtered quaternion, there through frames without stars
// aided.sigma        its one-sigma uncertainty about the sensor axes
// aided.gyro_bias    estimated bias, rad/s
// aided.fix          what the camera alone found in this frame
```

In C the same three calls are `asterope_tracker_enable_gyro`,
`asterope_tracker_gyro` and `asterope_tracker_update_aided`, in degrees per
second, with the filtered attitude in an `AsteropeAided`.

- The two noise figures come from the gyro's data sheet or an Allan
  variance. They set how fast the filter lets the attitude uncertainty grow
  without stars; too small and honest star fixes are turned away.
- The gyro's axes are taken to be the sensor's. A mounting rotation between
  the two has to be applied to the readings first.
- Readings and frames must share a clock. At 1 deg/s a millisecond between
  them is 3.6 arcseconds.
- A star fix that disagrees with the gyro's prediction by more than the
  gate (`GyroAiding::gate`, 30 by default) is reported in
  `aided.innovation` and left out. After five in a row the filter starts
  again from the fix, since it may be the gyro that is wrong.
- The gyro's attitude is where the frame is searched, in a window of three
  sigma up to 40 pixels. Beyond that a pattern search is run.
- With `GyroAiding::row_time` set, the gyro's rate undoes a rolling shutter.

Measured on a simulated sequence in which a third of the frames show no
stars: an attitude on 300 of 300 frames against 200, an error across the
boresight of 1.3 arcseconds RMS against 11.8, and a bias of 97 deg/h known
to 4 deg/h at the end (`docs/validation.md`, section 17).

## On the ground

Under the atmosphere stars are seen higher than they are, by 1 arcminute at
45 degrees of altitude and 5 at 10. For a camera of a few degrees that
shifts the whole field and bends it little; for a wide one it squeezes the
field toward the zenith.

```rust
use asterope_core::solar::zenith;

catalog.refract(zenith(jd_ut, latitude, east_longitude));   // radians
```

The catalog is then the sky as the site sees it at that moment, and an
attitude solved against it refers to the true sky. Do it before building
the index, and again when the sky has turned noticeably: 15 degrees an
hour. `asterope compare-ecsv --refraction` does it for a meteor-camera
frame from the site and time its measurement file states.

On the 21 meteor cameras of the test set it did not bring this solver and
the network's astrometry closer together (median 0.184 degrees against
0.183); the difference there is the fitted lens model
(`docs/validation.md`, Real cameras).

## Planets, and what else is not in the catalog

Venus, Jupiter, Mars and Saturn are brighter than most catalog stars and
will be among the brightest detections when they are in the field. Nothing
has to be done about them: the pattern search steps over a detection it
cannot place, and the acceptance test counts it as unmatched. In 400
simulated fields holding 483 planet images, the brightest at V -4.7, every
frame solved, none falsely, and the accuracy did not change
(`docs/validation.md`, section 16).

`solar::planet` and `solar::moon` give their positions to a few tenths of a
degree, which is enough to know that a bright unknown object is a planet
and not enough to use one as a reference.

## Narrow fields

The stars a camera needs grow fainter as its field shrinks, and the catalog
has to follow:

| Field | Catalog | Stars | Index |
|---|---|---|---|
| 8 degrees and wider | `data/hip65.astc`, V 6.5 | 8 870 | 0.2 to 4 MB |
| down to 4 degrees | `data/hip80.astc`, V 8 | 41 394 | 22 MB at 4 degrees |
| down to 2 degrees | Hipparcos and Tycho-2 to V 9.5, built with `cargo run -p xtask -- build-tycho` | 212 470 | 146 MB at 2 degrees |

The detector has to reach those stars, which is a matter of aperture and
exposure; the Design bench and `asterope design` say whether a camera
does. Catalogs of more than 65 535 stars are written with the wide index
records, which every reader takes. The Tycho-2 catalog is not in the
repository; `data/PROVENANCE.md` says where its source is and how it is
built. No onboard profile has been sized for either deep catalog: the
figures above are for the whole sky and the full pair index.

## File integrity

A catalog and an index each carry a CRC-32 over their body and one over
their header, checked by `StarTracker::from_bytes`. In the test that flips
every bit of the shipped pair in turn, 40 896 of them, every flip was
refused (`docs/validation.md`, section 22). An index also records the
catalog it was built from and is refused against any other.

The check is made once, when the files are opened. A bit that flips in
memory afterwards is not noticed. A flipped bit in a frame is a hot pixel
to the detector: 1 000 of them in a frame cost no lock in 60 frames and
moved the attitude by 5.5 arcseconds at most.

## From Python

`crates/asterope-py` wraps the same tracker for NumPy arrays:
`docs/python.md`.

## Failure behaviour

| Situation | What you get |
|---|---|
| Too few stars, a bright object, a slew | `Fix` with `solution: None`; `Coasting` for up to three frames when a rate is known, then `Lost` and a new search |
| A solution from the prediction that keeps under half of the last frame's matches, or matches under half of the brightest detections | a pattern search decides; the event is then `Acquired` |
| Frame of the wrong length | `Err(InvalidFrame)` |
| Damaged or mismatched catalog or index | `Err` from `StarTracker::from_bytes`, with the reason |
| Arithmetic corner cases, hostile bytes | error values; the core denies `unwrap`, `expect`, `panic!` and slice indexing, and is tested with corrupted and random inputs |

The acceptance test bounds the probability that a reported solution is a
coincidence, and that bound is returned with every solution
(`chance_probability`). In 10 000 simulated baseline trials there was no
false solution, which bounds the rate below 0.037 % at 95 % confidence, and
only for conditions like the simulated ones. Cross-check against a second
sensor before acting on a single frame.

## What has not been verified

- No part of this has run on flight hardware or on a microcontroller. The
  flight path runs on an emulated Cortex-M7 and gives the same bits as the
  desktop; that is a statement about arithmetic and not about time.
- The real frames it has solved are from ground cameras: meteor cameras and
  a set of star tracker test frames taken from the ground. None is from
  orbit.
- The simulator is a model of a detector. It has vignetting, gain and dark
  patterns, column noise, field-dependent blur, coma, a rolling shutter,
  nonlinearity and blooming at chosen levels; none was fitted to a real
  detector, and ghosts and baffles are not modelled.
- The gyro filter has only seen the simulator's gyro, which has white noise
  and a wandering bias and no scale-factor error or misalignment.
- The lost-in-space statistics are for the reference camera and the
  simulated disturbances. Run the validation for your own camera.
- There is no radiation analysis and no timing on a processor. The
  worst-case figures are measured on frames built to be costly, not proved
  from the code. The code has not been through any software-assurance
  process.

`docs/error-budget.md` lists what moves a reported attitude away from the
true one, term by term, and says for each whether it was measured or
derived.

Asterope is a laboratory and a starting point. A team that flies it owns its
verification.
