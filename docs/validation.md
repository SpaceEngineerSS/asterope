# Validation

The test suites live in `asterope_core::validation` and run from one source in
three places: `cargo test --workspace --release`, the `asterope validate`
command, and the Validation bench in the browser. All numbers below were
measured on the committed code with master seed 1.

All mathematics goes through pure-Rust `libm`, so the WebAssembly build
reproduces the native results bit for bit. This is checked, not assumed: a
digest of a full frame and its solution is recorded in `data/parity.txt` and
compared by a Rust test and by a Vitest test that loads the `.wasm` file.

## 1. Sub-pixel centroid accuracy

Single Gaussian stars are placed on a 20 x 20 grid of sub-pixel offsets on a
48 x 48 frame and detected with the default pipeline. The error is the
distance between the measured and true center.

| Condition | PSF sigma (px) | Signal (e-) | Samples | Missed | Mean error (px) | Max error (px) | Cramér-Rao (px) | Verdict |
|---|---|---|---|---|---|---|---|---|
| Noiseless | 0.8 | 20 000 | 400 | 0 | 0.0061 | 0.0110 | | pass |
| Noiseless | 1.0 | 20 000 | 400 | 0 | 0.0004 | 0.0011 | | pass |
| Noiseless | 1.5 | 20 000 | 400 | 0 | 0.0008 | 0.0020 | | pass |
| Noiseless | 2.0 | 20 000 | 400 | 0 | 0.0013 | 0.0036 | | pass |
| Peak SNR 30 | 0.8 | 725 | 10 000 | 0 | 0.0623 | 0.2270 | 0.0474 | reported |
| Peak SNR 30 | 1.0 | 1 083 | 10 000 | 0 | 0.0623 | 0.2125 | 0.0489 | reported |
| Peak SNR 30 | 1.5 | 2 328 | 10 000 | 0 | 0.0630 | 0.2111 | 0.0505 | reported |
| Peak SNR 30 | 2.0 | 4 073 | 10 000 | 0 | 0.0631 | 0.2258 | 0.0511 | reported |
| Peak SNR 60 | 0.8 | 1 449 | 10 000 | 0 | 0.0406 | 0.1335 | 0.0302 | pass |
| Peak SNR 60 | 1.0 | 2 165 | 10 000 | 0 | 0.0405 | 0.1403 | 0.0310 | pass |
| Peak SNR 60 | 1.5 | 4 656 | 10 000 | 0 | 0.0407 | 0.1413 | 0.0319 | pass |
| Peak SNR 60 | 2.0 | 8 147 | 10 000 | 0 | 0.0411 | 0.1463 | 0.0322 | pass |

Pass limit: mean error below 0.05 px.

**Why the noisy limit is enforced at peak SNR 60 and not 30.** "Peak SNR" is
the signal in the brightest pixel of a centered star divided by the
background noise per pixel (5.3 e- here). At peak SNR 30 the Cramér-Rao bound
on the mean radial error is 0.047-0.051 px, so no unbiased estimator can meet
a 0.05 px limit there with margin. The measured error is about 1.3 times the
bound at both levels. The SNR 30 rows are reported without a verdict.

The Cramér-Rao column is an approximation: per-axis variance
`sigma^2 / N + 8 pi sigma^4 b^2 / N^2` for `N` signal electrons and background
noise `b`, times `sqrt(pi / 2)` for the mean of a radial error. The noisy rows
use 25 seeds per grid position.

Error against signal for PSF sigma 1.2 px: 0.25 px at peak SNR 5 (66 %
detected), 0.17 px at 8, 0.12 px at 12, 0.08 px at 20, 0.04 px at 60 and
0.03 px at 120. Every star is detected from peak SNR 8 upward.

## 2. Wahba solver equivalence

10 000 random fields: 3-30 stars inside a cone of 4-12.5 degree half-angle,
0-60 arcsecond direction noise, random weights, and every tenth attitude
within 0.001 degrees of a half turn (where the Gibbs vector is singular).

| Solver | Worst deviation from the SVD solution | Limit | Verdict |
|---|---|---|---|
| QUEST | 8.0e-13 rad | 1e-10 rad | pass |
| Davenport q-method | 8.8e-12 rad | 1e-10 rad | pass |

`tests/wahba_equivalence.rs` repeats the comparison against a second,
independent SVD written with `nalgebra` types inside the test: worst QUEST
4.7e-13 rad, worst q-method 1.6e-12 rad, no case skipped (smallest eigenvalue
gap of K: 2.4e-4).

Two findings from building this suite:

- **Plain QUEST does not meet 1e-10 rad for narrow fields.** Its closed-form
  eigenvector reached 1e-9 rad in the worst-conditioned case. The shipped
  solver adds two inverse-iteration steps (see `docs/algorithms.md`).
- **A library SVD is not automatically a good reference.** `nalgebra`'s
  Golub-Kahan SVD of the same profile matrices deviates from the Jacobi SVD by
  up to 3.9e-9 rad, forty times the tolerance under test, because it loses
  relative accuracy on the two small singular values. The reference is
  therefore a one-sided Jacobi SVD.

## 3. Lost-in-space Monte Carlo

Each trial draws a uniformly random attitude, a hot-pixel fraction between
0.01 % and 0.1 %, zero to five cosmic-ray streaks and, in 30 % of trials, a
stray-light gradient, renders a 1024 x 1024 frame and solves it from scratch.

- **Success**: solved with cross-boresight error <= 60 arcseconds and roll
  error <= 300 arcseconds.
- **False positive**: an attitude reported more than 0.1 degrees from the
  truth.
- **Inaccurate**: solved between those limits.
- No trial is removed from the denominator. Intervals are exact
  Clopper-Pearson 95 %.

### Baseline: 14 degree field, 10 000 trials

| Criterion | Target | Measured | 95 % interval | Verdict |
|---|---|---|---|---|
| Success rate | > 98 % | 100.00 % | 99.96-100.00 % | pass |
| False-positive rate | < 0.1 % | 0 of 10 000 | 0-0.037 % | pass |

| Accuracy of solved trials | Median | 95th percentile | Maximum |
|---|---|---|---|
| Cross-boresight | 0.53″ | 1.28″ | 3.13″ |
| Roll | 3.1″ | 10.2″ | 41.8″ |

The pixel scale is 49″, so the cross-boresight median is about 1/90 pixel.

**On the trial count.** With 1 000 trials and no false positive, the 95 %
upper bound on the false-positive rate is 0.368 %, which does not demonstrate
a 0.1 % target; about 3 700 clean trials are needed. The 1 000-trial run is
what `cargo test` and the browser execute by default and it reports that
caveat. The claim above rests on the 10 000-trial run.

### Field-of-view sweep, 10 000 trials each

| Field | Index stars | Catalog stars per frame | Success | 95 % interval | False positives | No solution | Cross median | Roll median |
|---|---|---|---|---|---|---|---|---|
| 8° | 8 790 | 13.8 | 98.38 % | 98.11-98.62 % | 1 | 161 | 0.56″ | 6.1″ |
| 12° | 7 109 | 31.1 | 100.00 % | 99.96-100.00 % | 0 | 0 | 0.54″ | 3.8″ |
| 16° | 3 990 | 55.2 | 100.00 % | 99.96-100.00 % | 0 | 0 | 0.53″ | 2.8″ |
| 20° | 2 543 | 85.8 | 100.00 % | 99.96-100.00 % | 0 | 0 | 0.54″ | 2.2″ |
| 25° | 1 621 | 133.2 | 100.00 % | 99.96-100.00 % | 0 | 0 | 0.58″ | 1.9″ |

The success target applies to the 12-25 degree range. At 8 degrees the
catalog limit shows: 1.6 % of pointings do not solve, 0.5 % of frames hold
fewer than four catalog stars at all, and one trial in 10 000 returned a
wrong attitude (95 % upper bound 0.056 %).

## 4. Stress sweeps: where it breaks

One sensor quantity is varied at a time with every other disturbance off,
300 trials per step and the same attitudes at every step, on the baseline
camera. "Holds" is the largest tested value with a success rate above 98 %;
"breaks" is the next tested value.

| Quantity | Default | Holds up to | Breaks at | Success there | What fails |
|---|---|---|---|---|---|
| Read noise | 5 e- | 30 e- | 45 e- | 79 % | faint stars drop below the 5 sigma peak threshold |
| Stray light | 0 | 10 000 e-/px/s | 30 000 e-/px/s | 87 % | shot noise of the veiling background |
| Hot pixels | 0 | 0.3 % of pixels | 1 % | 94 % | chance clusters pass the area filter and crowd the pattern stars |
| Cosmic rays | 0 | 20 per frame | 50 per frame | 95 % | short streaks pass as stars |
| PSF sigma | 1.2 px | 3 px | 4 px | 88 % | signal spread below the detection threshold; neighbours merge |
| Exposure | 0.1 s | 0.02 s and longer | 0.01 s | 72 % | too few photons |
| Body rate | 0 | 1 deg/s | 1.5 deg/s | 0 % | an 11 px trail is rejected as a cosmic-ray streak |

Three things in these tables deserve a plain statement:

- **The body-rate limit is a cliff, not a slope.** Between 1 and 1.5 deg/s at
  a 0.1 s exposure the elongation filter starts rejecting every star. A
  shorter exposure moves the cliff proportionally; a real tracker would
  de-streak instead of rejecting.
- **False positives appear under extreme degradation.** Out of 300 trials
  each: one at 2 % hot pixels, one at PSF sigma 4 px and two at 5 px, one at
  3 deg/s. The baseline has none in 10 000, but the 0.1 % bound does not hold
  at those operating points.
- **The sweeps use 300 trials per step**, so a 100 % row only shows the rate
  is above 98.8 % with 95 % confidence.

The full tables are in `docs/validation-report.md`. The browser runs the same
sweeps from the Validation bench.

## 5. Tracking

`tests/lost_in_space.rs` drives the tracker over a rotating camera
(512 x 512, 14 degree field, 4 frames per second):

| Case | Result |
|---|---|
| 0.3, -0.2, 0.5 deg/s, 0.1 s exposure, 40 frames | 1 search, 39 tracked frames, worst error 27.5″ |
| 3 deg/s, 10 ms exposure, 30 frames | 2 searches (the second measures the rate), 28 tracked frames |
| 3 deg/s, 0.1 s exposure | lost in every frame: stars smear over 11 px |

The tracker is a predictor plus the same association and acceptance test as
the lost-in-space path. On its own it has no filter; section 17 puts a gyro
and a filter beside it.

The tracker gives a solution from the prediction a second opinion when it
looks doubtful (`docs/algorithms.md`). `tests/track_ghosts.rs` adds a copy of
the previous frame at 45 % brightness to a frame that has moved by 8 pixels,
as image lag or video compression would: without the check the tracker
reported the wrong attitude in 13 of 20 cases, with it in none.

## 6. Onboard index profiles

`asterope validate --profiles 10000` repeats the baseline Monte Carlo with the
pair index serialized, read back in place, and the solver limited to a cut
catalog, as flight software would run it. Frames are still rendered from the
whole catalog. "Reach" is the largest indexed separation as a fraction of
the frame diagonal.

| Profile | Stars per field | Reach | Index | Catalog | Success (10 000 trials) | 95 % interval | False positives |
|---|---|---|---|---|---|---|---|
| bench (the browser's index) | 25 | 1.0 | 4 205 kB | 213 kB | 100.00 % | [99.96, 100.00] | 0 |
| wide-16 | 16 | 1.0 | 1 768 kB | 142 kB | 99.83 % | [99.73, 99.90] | 0 |
| near-16 | 16 | 0.6 | 740 kB | 142 kB | 99.80 % | [99.69, 99.88] | 0 |
| **tight-16 (shipped as `data/flight-14deg.*`)** | 16 | 0.5 | 558 kB | 142 kB | 99.62 % | [99.48, 99.73] | 0 |
| near-14 | 14 | 0.6 | 585 kB | 126 kB | 99.55 % | [99.40, 99.67] | 0 |
| near-12 | 12 | 0.6 | 447 kB | 108 kB | 98.52 % | [98.26, 98.75] | 0 |
| near-8 | 8 | 0.6 | 223 kB | 72 kB | 87.07 % | [86.40, 87.72] | 0 |

Shortening the reach costs almost nothing down to half the diagonal, because
a pattern only needs four stars that are close together. Dropping fainter
stars is what costs success: below about 12 stars per field the sky has
regions where too few indexed stars share a frame. No profile produced a
false positive in 10 000 trials; every loss is a frame reported as unsolved.

## 7. Memory and code size

Heap use is measured by `tests/budget.rs` with a counting allocator, for the
shipped profile. The frame buffer belongs to the caller and is not counted.

| Frame | Frame buffer | Index, read in place | Detection, peak heap | Identification, peak heap | Tracking step, peak heap | Sum of all allocations in a search frame |
|---|---|---|---|---|---|---|
| 512 x 512 | 524 kB | 0 (558 kB of read-only storage) | 332 kB | 44 kB | 9 kB | 559 kB |
| 1024 x 1024 | 2 097 kB | 0 | 668 kB | 40 kB | 9 kB | 999 kB |
| 2048 x 2048 | 8 389 kB | 0 | 2 630 kB | 36 kB | 9 kB | 3 602 kB |

The decoded catalog stays on the heap: 72 bytes per star, 426 kB for the
shipped 5 923-star catalog. Detection makes about 25 allocations per frame.
The last column is what an arena that frees nothing until the frame is done
has to hold.

`examples/embedded` links the tracker without an operating system, with the
shipped index and catalog in read-only data:

| Target | Code | Read-only data | Flash |
|---|---|---|---|
| `thumbv7em-none-eabihf`, `-C target-cpu=cortex-m7`, `opt-level = "s"` | 64 kB | 702 kB | 768 kB |
| `riscv32imac-unknown-none-elf` (software floating point) | 102 kB | 709 kB | 811 kB |

These are link results. The same code runs on an emulated Cortex-M7
(section 20), which gives an instruction count. No timing exists for a
microcontroller: the 8.0 ms detection and 0.1 ms identification quoted below
are from a desktop processor. Asterope computes in `f64`; a
Cortex-M4F has a single-precision unit and would run the same code with
software doubles.

## 8. Video

**Stacking** (`tests/video_stack.rs`). A camera rotating at up to 1 degree
per second across and 2 about the boresight takes eight frames 0.1 s apart;
the frames are registered on their own stars and averaged
(`docs/algorithms.md`). At the default exposure the stack holds 53 detections
against 44 in a single frame, with stars within 0.045 px of their true
position in the newest frame and every registration within 0.25 px at the
frame corner. Under-exposed, 24 sequences per row:

| Exposure | Frames held on average | Single frame locks | Stack locks | False locks | Cross / roll, single | Cross / roll, stack |
|---|---|---|---|---|---|---|
| 6 ms | 4.1 of 8 | 4 of 24 | 12 of 24 | 0 | 1.5″ / 25″ | 4.9″ / 17″ |
| 8 ms | 5.7 | 12 | 17 | 0 | 2.5″ / 14″ | 3.6″ / 25″ |
| 12 ms | 7.8 | 23 | 24 | 0 | 2.8″ / 19″ | 2.8″ / 17″ |
| 20 ms | 8.0 | 24 | 24 | 0 | 2.7″ / 12″ | 2.4″ / 11″ |
| 100 ms | 8.0 | 24 | 24 | 0 | 0.8″ / 4″ | 0.8″ / 4″ |

Stacking roughly doubles to triples the lock rate where single frames fail
and changes nothing where they succeed. It is limited by registration: three
stars must be detectable in single frames, which is why only about half of
the eight frames are held at 6 ms. It does not make up for an exposure that
is an order of magnitude too short.

**End to end** (`web/e2e/app.spec.ts`). The browser records a simulated
sequence (0.4 deg/s, one frame every 0.25 s) as VP9 video, reads it back by
seeking at 10 samples per second and tracks it: in the recorded run every
sample was solved (29 of 29) and the right ascension advanced by the
commanded 0.1 degrees per frame. Compression costs accuracy: residuals are
15-40 arcseconds RMS against 5 on the uncompressed frames.

**Real footage: tried, not solved.** Two time-lapse videos from the ESO
archive (`uhd_bt_paranal_03` and `p4paranal`, the 1280 x 720 downloads) were
run through the Video bench with the field of view unknown, sampled twice
per second. Neither produced a lock in any of its 50 and 68 samples. Both
open and close on a title card, and the one frame inspected, three seconds
into the second video, is a twilight landscape with a single star-like
detection. The cause was not investigated further; these are processed
time-lapses of a landscape, not the footage the bench is meant for, and real frames from fixed cameras are covered under "Real cameras" below. The
stacking figures above come from the simulator and from its own frames
passed through a video codec. The attempt did change the bench:
a search that finds nothing now waits 1, 2, 4 and then 8 seconds of video
before the next, instead of stopping after three frames of title card.

## 9. Stellar aberration

`ephem::earth_velocity_km_s` agrees with a two-body orbit from Standish's
mean elements within 0.003 km/s over 1975-2025, two independent
approximations of the same motion. In `tests/flight_assets.rs` frames are
rendered from a sky displaced by the aberration of an observer at 34.4 km/s
(Earth plus a low orbit): the median cross-boresight error is 17.4″ without
the correction and 0.8″ with it.

## 10. Camera calibration

`calibrate::tests` fit synthetic star measurements with known cameras:

| Case | Result |
|---|---|
| 60 degree field, 8 frames of 40 stars, 0.1 px noise, all seven parameters | every parameter within 4 sigma of the truth; focal length ±0.06 px, principal point ±0.12 px, k1 ±0.0004; residual 0.14 px against 0.141 expected |
| 14 degree field, 8 frames of 30 stars, 0.05 px noise, focal length, principal point and k1 | focal length ±0.09 px, principal point ±0.6 px: seven times weaker, as the module documentation explains |

Through the whole pipeline (`asterope simulate` then `asterope calibrate`):
eight simulated frames of a 40 degree camera with k1 = -0.06, started from a
41.5 degree guess, gave a focal length of 1406.73 ± 0.04 px (true 1406.71),
a principal point of (511.29, 511.33) ± 0.11 px (true 511.5) and
k1 = -0.0604 ± 0.0002. The uncalibrated guess does not lock on these frames;
the calibrated camera does. No calibration of a real lens was performed.

## 11. Design predictions against the simulator

`tests/design_vs_sim.rs` evaluates five cameras in closed form
(`design.rs`) and then simulates each at 150 random attitudes and solves
every frame. Pixel pitch 6 um, 1024 x 1024, 0.1 s exposure:

| Lens | Field | Stars per frame, predicted / detected | Locks, predicted / measured | Cross-boresight median, predicted / measured | Roll median, predicted / measured |
|---|---|---|---|---|---|
| 17.4 mm f/1.2 | 20.0° | 85.4 / 88.2 | 100.0 % / 100.0 % | 0.44″ / 0.46″ | 1.8″ / 2.1″ |
| 25 mm f/1.7 (the default sensor) | 14.0° | 42.0 / 44.2 | 100.0 % / 100.0 % | 0.44″ / 0.47″ | 2.5″ / 2.9″ |
| 35 mm f/2.4 | 10.0° | 21.6 / 22.9 | 100.0 % / 100.0 % | 0.43″ / 0.48″ | 3.5″ / 4.5″ |
| 50 mm f/3.4 | 7.0° | 10.6 / 11.4 | 94.6 % / 95.3 % | 0.43″ / 0.53″ | 5.0″ / 6.7″ |
| 25 mm f/5.0 | 14.0° | 8.8 / 9.2 | 86.1 % / 91.3 % | 1.56″ / 2.07″ | 9.0″ / 12.1″ |

The predicted lock rate is the share of sky with five or more detectable
catalog stars in the field. Two constants in the prediction are taken from
measurement rather than derived: the centroid error is 1.3 times the photon
and background bound (suite 1), and the detection threshold acts 1.16 sigma
lower on the mean peak because the detector takes the brightest of several
noisy pixels. The first four cameras see every catalog star, so they test
the star counts and the accuracy model; the last is limited by sensitivity
and tests the photometry, within the simulator's own model of a detector.
The accuracy prediction adds up the information of the stars as an ideal
weighting would. The solver's weights (suite 12) come from measured
signal-to-noise ratios and fall 5 to 35 % short of that, so the prediction
is the optimistic side of the measurement on every row.
The predicted streak limit for the default sensor, 1.31 deg/s, falls between
the 1.0 deg/s that holds and the 1.5 deg/s that fails in suite 4.

None of this checks the photometric constant against a real instrument: the
simulator and the prediction share the assumption about how many electrons a
star delivers.

## 12. Weighing stars by centroid accuracy

A faint star's centroid is several times worse than a bright one's, and a
fit that counts both the same lets the faint ones set the error. Each match
therefore enters the attitude fit with the inverse variance of its centroid,
`1 / (0.03^2 + (s / SNR)^2)` in pixels, where `s` is the measured width of
the star image and 0.03 px is the error pixel sampling leaves at high signal
(suite 1). A saturated star gets 0.3 px in place of 0.03. The reported
uncertainty uses the same weights.

`tests/weighted_attitude.rs` solves the same frames both ways: 400
attitudes per row, and 3 000 for the last, which adds hot pixels, cosmic
rays and stray light.

| Field | Read noise | Disturbances | Locks | Cross-boresight median, equal / weighted | 95 % | Worst | Roll median | Worst |
|---|---|---|---|---|---|---|---|---|
| 14° | 5 e- | no | 400 / 400 | 0.73″ / 0.46″ | 1.57″ / 1.16″ | 2.4″ / 2.0″ | 4.5″ / 3.0″ | 21″ / 20″ |
| 14° | 20 e- | no | 400 / 400 | 1.56″ / 0.81″ | 3.60″ / 2.14″ | 6.8″ / 5.3″ | 9.5″ / 5.3″ | 55″ / 51″ |
| 20° | 5 e- | no | 400 / 400 | 0.79″ / 0.49″ | 1.75″ / 1.01″ | 2.7″ / 1.7″ | 3.0″ / 1.9″ | 20″ / 13″ |
| 8° | 5 e- | no | 396 / 396 | 0.69″ / 0.51″ | 1.63″ / 1.35″ | 4.3″ / 5.6″ | 7.4″ / 5.6″ | 48″ / 65″ |
| 14° | 5 e- | yes | 3 000 / 3 000 | 0.80″ / 0.53″ | 2.01″ / 1.22″ | 4.3″ / 2.9″ | 4.8″ / 3.3″ | 39″ / 29″ |

Which frames lock does not change: the weights act after the stars are
identified. The median error falls by a quarter to a half. At 8 degrees,
with a dozen stars in the field, the worst frame of 396 is worse weighted
than unweighted; everywhere else the tail improves with the median.

The first version of the weighting did not have that property. Weights
alone made the median better and the tail worse under disturbances (95 %
of frames within 2.55″ against 2.02″ unweighted over 10 000 frames), because
a bright star whose centroid a hot pixel or a particle track had spoiled
kept the weight its brightness earned. The fit now cuts the weight of a
star that sits more than three times farther from its predicted place than
its weight allows (`docs/algorithms.md`), which is what the last row shows.

Photographs are fitted with equal weights (`SolverConfig::photograph`),
because there the error of a star comes from the fitted lens model and
grows with distance from the center, whatever the star's brightness.

## 13. Narrow fields with a deeper catalog

`data/hip80.astc` holds the 41 394 Hipparcos stars to V 8, against 8 870 to
V 6.5 in `hip65.astc`. `tests/deep_catalog.rs` simulates 300 attitudes per
field with each catalog, on a sensor with four times the collecting area of
the reference one (a lens this long has the aperture, and V 8 stars need
it):

| Field | Locks, V 6.5 / V 8 | False locks | Pair index, V 6.5 / V 8 |
|---|---|---|---|
| 3° | 7.0 % / 85.0 % | 0 / 0 | 0.9 MB / 13.2 MB |
| 4° | 27.0 % / 99.7 % | 0 / 0 | 1.3 MB / 22.2 MB |
| 5° | 49.3 % / 100.0 % | 0 / 0 | 1.9 MB / 21.6 MB |
| 6° | 77.3 % / 100.0 % | 0 / 0 | 2.5 MB / 23.2 MB |
| 8° | 98.7 % / 100.0 % | 0 / 0 | 4.1 MB / 13.0 MB |

The deeper catalog moves the limit from about 8 degrees to about 4. Time
per frame is the same within 7 %. The indexes here are the wide ones the
workbench builds, 25 stars per field; an onboard profile as in suite 6 has
not been made for the deep catalog, so its flight size is not known. The
catalog file itself is 993 kB. The sensor in this test is assumed, not one
that exists.

## 14. Fields of 2 to 4 degrees with Tycho-2

Below 4 degrees Hipparcos runs out as well. `xtask build-tycho` adds the
Tycho-2 stars that Hipparcos does not hold, 212 470 stars to V 9.5 in all
(`data/PROVENANCE.md`; the 5.1 MB file is built locally and not kept in the
repository). That many stars need the index records with 32-bit star
positions (`docs/index-format.md`). `tests/tycho_fields.rs`, 300 attitudes
per field, on a sensor with sixteen times the collecting area of the
reference one, which is what puts a V 9.5 star five sigma above this noise:

| Field | Locks: V 8 / V 9.5 / V 9.5 with 12 M pairs | False locks | Index stars | Index size |
|---|---|---|---|---|
| 1.5° | 9.7 % / 72.0 % / 92.0 % | 0 / 0 / 0 | 41 232 / 108 100 / 211 279 | 4.6 / 36.2 / 133.6 MB |
| 2° | 33.7 % / 86.3 % / 99.7 % | 0 / 0 / 0 | 41 182 / 86 246 / 168 707 | 6.8 / 39.0 / 146.3 MB |
| 3° | 85.0 % / 97.7 % / 100.0 % | 0 / 0 / 0 | 41 090 / 57 867 / 113 246 | 13.2 / 26.6 / 142.5 MB |
| 4° | 99.7 % / 99.3 % / 100.0 % | 0 / 0 / 0 | 41 056 / 40 494 / 63 354 | 22.2 / 22.4 / 54.2 MB |

The index builder stops at 3 million pairs by default and takes fewer stars
to stay under that, which is what holds the middle column back: with 12
million pairs allowed a 2 degree field locks 299 times in 300. The price is
an index of about 140 MB. Time per frame stays within 20 % of the Hipparcos
case throughout. An index of that size suits a ground computer, not a
flight processor; the pair index grows with the square of the star density,
and a pattern hash would be the way to go lower, which this does not have.
As in suite 13, the sensor is assumed.

## 15. Lens and detector flaws

The simulator's ideal sensor can be given the flaws of a real one
(`sim::Imperfections`; each is checked against its definition in the unit
tests of `sim.rs`). `tests/sensor_flaws.rs` solves 300 random attitudes with
each, on the reference 14 degree camera:

| Sensor | Locks | Over 0.1° off | Cross-boresight median | 95 % | Roll median |
|---|---|---|---|---|---|
| Ideal | 100.0 % | 0 | 0.56″ | 1.20″ | 3.0″ |
| Vignetting, full cos⁴ falloff | 100.0 % | 0 | 0.55″ | 1.23″ | 3.0″ |
| Gain pattern, 2 % RMS | 100.0 % | 0 | 0.49″ | 1.22″ | 2.9″ |
| Dark-current pattern, 100 e-/s RMS | 100.0 % | 0 | 0.56″ | 1.40″ | 3.7″ |
| Column noise, 5 e- RMS | 100.0 % | 0 | 0.63″ | 1.51″ | 3.8″ |
| Star images twice as wide at the corner | 100.0 % | 0 | 0.72″ | 1.56″ | 4.0″ |
| Coma, 15 % of the light in a flare at the corner | 100.0 % | 0 | 0.63″ | 1.29″ | 2.8″ |
| Response 10 % short at full well, full blooming | 100.0 % | 0 | 0.55″ | 1.22″ | 3.0″ |
| All of the above together | 100.0 % | 0 | 1.13″ | 2.76″ | 8.0″ |
| Turning at 1°/s, global shutter | 100.0 % | 0 | 1.22″ | 3.35″ | 7.9″ |
| 1°/s, rolling shutter, 20 µs a row | 100.0 % | 0 | 4.98″ | 13.15″ | 30.7″ |
| the same, undone with the known rate | 100.0 % | 0 | 1.29″ | 3.48″ | 6.8″ |
| 1°/s, rolling shutter, 60 µs a row | 98.3 % | 3 | 36.97″ | 70.83″ | 126.8″ |
| the same, undone with the known rate | 100.0 % | 0 | 1.25″ | 3.45″ | 8.6″ |

None of the static flaws costs a lock at these levels; together they double
the error. A rolling shutter in a turning camera is another matter: each
row shows a different attitude, the image is stretched, and at 60 µs a row
three solutions in 300 land more than 0.1 degrees from the truth. Given the
rate (`SolverConfig::rolling`), every star is moved to where it stood when
the middle row was read, and the result is that of a global shutter. The
rate has to come from somewhere: a gyro (suite 17), or the frames before.

The levels are chosen to be plausible, not taken from a particular
detector.

## 16. The Sun, the Moon and the planets

`solar.rs` gives their directions from mean orbital elements and the
Astronomical Almanac's low-precision series. `tests/solar_system.rs`
compares 87 dates between 1990 and 2040 with ERFA (`plan94`, `epv00`,
`moon98`; the reference values and the script that wrote them are in
`tests/reference/`):

| Body | Largest difference in direction | In distance |
|---|---|---|
| Sun | 0.008° | |
| Mercury | 0.006° | 0.01 % |
| Venus | 0.022° | 0.02 % |
| Mars | 0.036° | 0.07 % |
| Jupiter | 0.117° | 0.09 % |
| Saturn | 0.181° | 0.20 % |
| Uranus | 0.030° | 0.05 % |
| Neptune | 0.017° | 0.03 % |
| Moon | 0.304° | 0.22 % |

That is enough to know whether a body is in the field or near it, and not
enough to use one as a reference. Planet magnitudes follow the phase laws
of the older Astronomical Almanac and have not been compared with
anything here. Bennett's refraction formula is within 4 arcseconds of ERFA's
for dry air from 15 degrees of altitude up, where the two differ by 2 to 3
per cent throughout.

A planet is a bright object no star catalog holds. `tests/planets_in_field.rs`
points the reference camera within 5 degrees of a planet brighter than
V 6.5 at 400 random dates, and solves each frame with and without the
planets drawn (483 planet images in all, the brightest at V -4.7):

| Sky | Locks | False locks | Cross-boresight median | 95 % |
|---|---|---|---|---|
| Stars only | 100.0 % | 0 | 0.52″ | 1.16″ |
| Stars and planets | 100.0 % | 0 | 0.50″ | 1.15″ |

The planets are left unmatched and change nothing. The Moon and the Sun are
not drawn: a disc that fills or floods the frame is a matter of stray light
and exclusion angles, which the simulator represents only as a gradient.

## 17. A gyro beside the camera

`filter.rs` is a multiplicative extended Kalman filter for the attitude and
the gyro bias. `tests/attitude_filter.rs` runs it on a simulated gyro (angle
random walk 0.18°/√h, bias wandering at 3e-5 °/s/√s, read at 50 Hz) and star
fixes at 5 Hz with errors of 2″ across and 15″ about the boresight, over 200
runs of 180 s with a varying rate:

| Check | Result |
|---|---|
| State error weighed by the filter's own covariance (6 if honest) | 6.00 |
| Residual of the star fixes weighed likewise (3 if honest) | 2.99 |
| Covariance against a two-state filter written separately, 5 000 steps | equal to 1e-9 |
| Gyro bias, 125°/h RMS at the start | 1.90°/h RMS after 180 s; the filter states 1.92 |
| A fix 1° wrong offered with the usual confidence | turned away, state unchanged |

| Axis | Star tracker alone, RMS | With the gyro |
|---|---|---|
| x | 2.00″ | 1.87″ |
| y | 2.00″ | 1.86″ |
| boresight | 15.00″ | 7.85″ |

| Time without a star fix | Error, RMS over 300 runs | The filter's own sigma |
|---|---|---|
| 10 s | 62.8″ | 63.5″ |
| 60 s | 187.6″ | 196.7″ |
| 300 s | 823.9″ | 851.3″ |

`tests/gyro_aided.rs` puts the whole chain together through
`StarTracker::update_aided`: a camera turning at up to 0.9°/s with a
rolling shutter of 60 µs a row, 300 frames at 5 Hz, and no stars at all in
frames 100 to 199:

| | Frames with an attitude | Cross RMS | Roll RMS | Largest error while blind | Searches after the gap |
|---|---|---|---|---|---|
| Star tracker alone | 200 of 300 | 11.8″ | 174.6″ | no attitude | 1 |
| With the gyro | 300 of 300 | 1.3″ | 8.8″ | 115″ | 0 |

The rolling shutter is undone with the gyro's rate, and the gyro bias,
97°/h at the start, is known to 4.0°/h at the end.
This is one sequence and one gyro. The gyro is a model, with no scale-factor
error, misalignment or temperature dependence, and the filter assumes its
readings and the frames carry the same clock.

## 18. Stars drawn into trails

A star that moves more than about 8 PSF widths during the exposure is
rejected as elongated, and the camera loses lock at 1.3°/s (suite 4). With
`DetectConfig::trails` such images are kept when they are as wide as a star
and centered along their length. `tests/star_trails.rs`, 300 attitudes per
rate with hot pixels and cosmic rays present, exposure 0.1 s:

| Rate across the boresight | Trail | Locks, rejected / kept | Over 0.1° off | Stars found | Cross-boresight median |
|---|---|---|---|---|---|
| 0 | 0 px | 100.0 % / 100.0 % | 0 / 0 | 41.0 / 41.1 | 0.51″ / 0.51″ |
| 0.5°/s | 3.6 px | 100.0 % / 100.0 % | 0 / 0 | 40.1 / 40.2 | 0.66″ / 0.66″ |
| 1°/s | 7.3 px | 100.0 % / 100.0 % | 0 / 0 | 32.2 / 36.7 | 1.48″ / 1.66″ |
| 1.5°/s | 10.9 px | 0.7 % / 99.7 % | 1 / 0 | 6.1 / 30.7 | 85.17″ / 1.82″ |
| 2°/s | 14.6 px | 1.3 % / 98.7 % | 0 / 0 | 5.9 / 25.3 | 139.87″ / 2.70″ |
| 3°/s | 21.8 px | 1.0 % / 85.0 % | 3 / 2 | 5.4 / 19.0 | 247.30″ / 3.58″ |
| 4°/s | 29.1 px | 0.0 % / 62.0 % | 1 / 1 | 5.1 / 15.8 | - / 4.63″ |
| 6°/s | 43.7 px | 0.0 % / 21.0 % | 0 / 0 | 5.2 / 13.0 | - / 6.95″ |

| Rate about the boresight | Locks, rejected / kept | Over 0.1° off | Stars found | Cross-boresight median |
|---|---|---|---|---|
| 5°/s | 100.0 % / 100.0 % | 0 / 0 | 40.1 / 40.2 | 0.65″ / 0.65″ |
| 10°/s | 100.0 % / 100.0 % | 0 / 0 | 29.5 / 36.9 | 1.23″ / 1.32″ |
| 20°/s | 60.0 % / 99.3 % | 21 / 0 | 11.8 / 27.7 | 2.58″ / 2.11″ |

Keeping trails moves the rate at which 98 % of frames lock from about 1°/s
to 2°/s. Beyond that fewer and fewer stars are found: the same light is
spread over more pixels. The solutions more
than 0.1 degrees off were looked at one by one: all lie between 0.10 and
0.77 degrees from the truth, with the error about the boresight and six or
seven stars matched. That is the right stars fitted badly, not the wrong
stars, which would be tens of degrees off; the test fails on any solution
more than 2 degrees off and there is none. Below 1°/s the trail centroid is
slightly worse than the usual one, which is why the option is off by
default.

## 19. A revolution in orbit

`orbit.rs` carries a camera round a 550 km, 53 degree orbit, with the
Sun, the Earth's limb and the Moon blinding it inside 35, 25 and 10 degrees
of the boresight. `tests/orbit_model.rs` checks the orbit against closed
forms and against a numerical integration of two-body gravity with the J2
term: the orbit plane turns 17.97 degrees in five days and model and
integration end 0.09 degrees apart; the node of the textbook
Sun-synchronous orbit (700 km, 98.19°) moves 0.9856°/day within 0.005; the
eclipsed share of an orbit is 0.3886 against 0.3898 from geometry.

`tests/orbit_availability.rs` then flies one revolution (5 740 s, a frame
every 10 s, stars displaced by the aberration of the Earth's and the
spacecraft's velocity), for four ways of carrying the camera:

| Camera | Blinded by Sun / Earth / Moon | Attitude: alone / with gyro | Longest gap alone | Error RMS: alone / with gyro | Largest error with gyro | Without the aberration correction |
|---|---|---|---|---|---|---|
| Looking away from the Earth | 13.2 % / 0 % / 4.0 % | 82.8 % / 97.9 % | 640 s | 4.5″ / 4.4″ | 972″ | 17.3″ |
| Along the orbit normal | 0 % / 100 % / 0 % | 0 % / 0 % | whole orbit | | | |
| 30° above the orbit normal | 7.7 % / 0 % / 0 % | 92.3 % / 100.0 % | 440 s | 4.8″ / 4.8″ | 1 059″ | 19.7″ |
| Tumbling at 0.5°/s | 9.4 % / 52.4 % / 0 % | 38.2 % / 99.8 % | 360 s | 6.6″ / 6.6″ | 1 201″ | 22.5″ |

The star tracker alone solves every frame it is not blinded in. The second
row is geometry: at 550 km the limb is 23 degrees from the orbit normal,
inside the 25 degree exclusion, so a camera mounted there never works. With
the gyro an attitude is available from the first star fix to the end of
the orbit (the missing 2.1 % in the first row is the stretch before the
first fix, which happened to start blinded), and it drifts to about a
third of a degree by the end of the longest gap. The last column is the
error of the same frames solved without `set_velocity`: 17 to 23
arcseconds, which is the aberration.

The exclusion angles are round numbers for a small baffle, and a blinded
frame is simply one without stars. Stray light short of blinding, and the
Moon or the Earth inside the frame, are not modelled.

## 20. On an emulated processor

`examples/emulated` is the flight path as a bare-metal image for the Arm
MPS2 board with a Cortex-M7 (AN500), run under QEMU 8.2 with
`-icount shift=0`. It starts from the reset vector, solves two 1024 x 1024
frames held in memory with the shipped onboard index and catalog, and
compares each quaternion with the one the build machine computed for the
same frame:

| Frame | Event | Same bits as the build machine | Instructions | Heap in use, peak | Stack in use, peak |
|---|---|---|---|---|---|
| 0 | acquired | yes | 112.9 million | 993 kB | 9.8 kB |
| 1 | tracked | yes | 112.8 million | 965 kB | 9.8 kB |

So the result on a 32-bit Arm core with hardware doubles is the same, bit
for bit, as on the desktop, as it is in WebAssembly. A tracked frame costs
as much as an acquired one, which fits the desktop timing, where detection
takes 8.0 ms a frame and identification 0.1 ms. The heap figure is for the example's frame arena,
which frees nothing until the frame is done and so holds the sum of a
frame's allocations; a heap that frees needs 668 kB (suite 7).

An emulator counts instructions. It has no caches, no memory wait states
and no notion of how many cycles a double-precision division takes, so this
is not a time. As a floor: a core that retired one instruction per cycle at
480 MHz would need 0.24 s a frame, and a real one will need more.

## 21. A million fields

The acceptance test bounds the chance that a reported attitude is a
coincidence, and the baseline run found none in 10 000 frames, which only
shows the rate is below 0.037 %. `tests/false_locks.rs` goes to a million by
leaving out the rendering: star positions go straight into the
identification, which is the part that could be fooled.

| Fields | Trials | Attitudes reported | Over 0.1° off | Rate over 0.1° off, 95 % upper bound |
|---|---|---|---|---|
| 8 to 47 points scattered at random, no sky | 1 000 000 | 0 | | 3.7e-6 |
| The sky, one star in ten missed, a tenth of a pixel of centroid error, up to five false objects | 1 000 000 | 999 986 | 0 | 3.7e-6 |

No attitude was reported for a field without a sky, and none more than 0.1
degrees from the truth for a field with one.

The test knows which catalog star each detection is, so matches can be
checked one by one. In 5 152 solutions, half a per cent, one match named a
star other than the one detected: a false object, or a star whose neighbor
was missed, lying within the match radius of a catalog star. One such match
among thirty does not move the attitude.

It did where the fit rested on few stars. The first run of this test found
four solutions between 0.25 and 0.5 degrees off, each with 7 to 10 stars
matched out of 32 to 45 and one of them wrong: stars close together fix the
roll loosely, one bad match turned it, and the stars at the far side of the
field then lay outside the match radius and could not correct it. An
accepted solution that explains under half the detections now gathers the
stars again in a ring four match radii wide around the attitude found
(`lis.rs`), and is replaced if that explains more of them. With that the
four are gone.

The centroid error here is a normal deviate, kinder than a real detector's,
and the false objects are uniformly placed. The bound is for this camera
and these conditions.

## 22. Flipped bits

`tests/bit_flips.rs` flips single bits, as a particle would.

In stored files: every bit of the header and 20 000 bits chosen at random in
the onboard catalog (20 256 flips) and the onboard index (20 640), one at a
time. Every one was refused when the file was opened. The header carries its
own check for this; before it was added, a flipped bit in a header field
such as the slope of the k-vector went unnoticed.

In a frame, 60 frames per row, any of the twelve bits of a pixel:

| Bits flipped per frame | Still solved | Largest change of attitude |
|---|---|---|
| 1 | 60 of 60 | 0.00″ |
| 10 | 59 of 60 | 0.32″ |
| 100 | 60 of 60 | 1.27″ |
| 1 000 | 60 of 60 | 5.50″ |
| 10 000 | 56 of 60 | 19.04″ |

A flipped high bit looks like a hot pixel, and the detector rejects
isolated hot pixels by shape. Memory that holds the program or its working
data is not covered: a flipped bit there is a matter for the processor and
its software, not for this library.

## 23. The error in the terms of a specification

Star tracker specifications (ECSS-E-ST-60-20C) split the error by how it
behaves: noise that changes from frame to frame, error that changes as the
stars cross pixels, error that changes as they cross the field, and bias.
`tests/error_terms.rs` measures the first three on the reference camera, 40
pointings with 40 frames each:

| Term | How it is measured | Across the boresight, per axis | About the boresight |
|---|---|---|---|
| Temporal noise (noise equivalent angle) | one attitude, 40 noisy frames, scatter about their mean | 0.43″ | 4.56″ |
| Pixel spatial error | no noise, attitude moved by up to a pixel, scatter about the mean | 0.08″ | 0.85″ |
| the same with a 2 % gain pattern | | 0.09″ | 0.91″ |
| Field spatial error | no noise, attitude moved by up to 2 degrees, error itself | 0.08″ | 0.80″ |
| the same with the radial distortion wrong by 0.002 | | 0.11″ | 0.85″ |

All are one-sigma values. The other terms of the standard are not errors of
this software: bias and thermo-elastic error belong to the mounting, the
aberration of light is corrected when the velocity is supplied (suite 9),
and the measurement date error is that of the clock the frames are stamped
with.

## 24. The most a frame can cost

A flight computer has to know what a frame can cost at worst, and an
average says nothing about that. `tests/worst_case.rs` builds frames meant
to cost as much as they can, for the shipped onboard index and catalog.

Identification works within fixed budgets: the 14 brightest detections
form at most 364 triads, and at most 2 000 pyramids are verified. Two kinds
of field use them up without ever being accepted:

| Field | Fields | Triads, pyramids at most | Time, worst | Heap, peak |
|---|---|---|---|---|
| The sky | 40 | 24, 1 | 0.23 ms | 40 kB |
| 64 points scattered at random, no sky | 400 | 364, 1 | 0.52 ms | 38 kB |
| The sky mirrored left to right | 400 | 364, 7 | 0.73 ms | 38 kB |

The mirrored sky is the hardest case there is for the lookups: every angle
between two stars is in the index, so each finds as many candidates as it
ever can, and only the handedness check tells a triangle from its mirror
image. None of the 800 fields was given an attitude. A search that fails
costs about three times one that succeeds, and no more memory.

Detection is different. Its time and memory follow what is in the frame:

| Frame, 1024 x 1024 | Frames | Blobs at most | Time, worst | Heap, peak |
|---|---|---|---|---|
| The sky | 40 | 2 676 | 9.7 ms | 668 kB |
| the same with 0.1 % hot pixels and 5 particle tracks | 12 | 3 613 | 9.2 ms | 668 kB |
| 2 % hot pixels and 1 000 particle tracks | 12 | 20 920 | 17.5 ms | 5 141 kB |
| 10 000 stars on a grid | 1 | 10 404 | 37.8 ms | 5 345 kB |
| Every pixel at random, 0 to 4 095 | 6 | 0 | 8.7 ms | 29 kB |
| Saturated everywhere | 1 | 0 | 5.6 ms | 29 kB |

So the 668 kB of section 7 is what a frame of the sky needs, and it is
not a bound: the detector keeps a record of about 100 bytes for every run
of bright pixels in a row, and a frame full of hot pixels has many. The
time goes with the number of blobs that pass the shape tests and get a
centroid, which is why ten thousand clean stars cost more than twenty
thousand hot pixels. A frame with no structure at all is the cheapest.

`DetectConfig::max_runs` is the bound. A frame with more runs is refused
with an error before any more memory is taken. Set to 8 192:

| Frame | Outcome | Time, worst | Heap, peak |
|---|---|---|---|
| The sky | as before | 9.6 ms | 668 kB |
| the same with 0.1 % hot pixels and 5 particle tracks | as before | 8.9 ms | 668 kB |
| 2 % hot pixels and 1 000 particle tracks | refused | 6.3 ms | 1 786 kB |
| 10 000 stars on a grid | refused | 3.9 ms | 1 786 kB |

The default is 400 000, which refuses nothing the stress sweeps of section
4 use and bounds little. Flight software short of memory should set it
from its own detector's worst hot-pixel count.

The times are from a desktop processor and only their ratios carry over.
No frame was built to make one centroid slow: a centroid looks at a window
of at most 39 pixels across, whatever the blob.

## Supporting tests

| Property | Test | Result |
|---|---|---|
| Native and WebAssembly agree bit for bit | `parity_digest_matches_the_recorded_value`, `web/src/engine/parity.test.ts` | digest `19664654c34f4e16` on both |
| No panic on hostile input | `tests/robustness.rs`, `fits::tests::survives_corrupted_and_random_files` | 100 000 corrupted catalogs, 100 000 corrupted FITS files, 4 000 arbitrary frames, 3 000 arbitrary sensors, 20 000 degenerate Wahba problems, garbage detections |
| k-vector range equals a brute-force scan | `index::tests::k_vector_range_matches_brute_force` | 10 000 random ranges identical |
| Distortion round trip | `camera::tests::distortion_round_trip_is_sub_nanopixel` | < 1e-9 px |
| Frame and sign conventions | `attitude::angles::tests`, `orion_belt_projects_where_expected`, `web/src/lib/export.test.ts` | north up, east left at roll 0; WCS agrees with the camera model to 1e-8 degrees |
| Motion blur | `sim::tests::body_rate_smears_stars_without_losing_flux` | flux kept, trail centered, variance grows by L²/12 |
| Catalog reader rejects corruption | `catalog::tests::rejects_corruption_without_panicking` | error values, no panic |
| Same seed, same frame | `sim::tests::same_seed_gives_identical_frames` | bit-identical |
| Scale search and calibration | `scale_search_recovers_an_approximate_field_of_view` | 6 % focal error recovered to 0.005 % |
| Unknown field of view | `unknown_field_of_view_is_found_by_the_ladder` | 17 degree frame found from a 30 degree guess |
| No lock on empty or noise-only frames | `empty_and_noise_only_frames_give_no_solution` | 30 of 30 |
| Serialized index equals the built one | `index::tests::serialized_index_is_searched_in_place`, `tests/flight_assets.rs` | same stars and same 2 000 range results; shipped files reproduce byte for byte |
| Damaged index is refused | `index::tests::damaged_index_bytes_are_rejected` | truncation, magic, version and checksum each reported |
| C interface equals the Rust one | `asterope-ffi/tests/api.rs`, `examples/solve.c` | identical quaternion bits; the C example builds with MSVC `/W4` and with `cc -Wall -Wextra`, and prints the same attitudes as `asterope replay` |
| Command line on real photographs | `asterope solve` on the frames below | same locks, fields and match counts as the browser |
| FITS writer | `fits::tests::encoded_frame_reads_back_unchanged` | round trip exact |
| Whole application | `web/e2e/app.spec.ts` (Playwright) | 11 scenarios in each of three browser engines: lock and slew, loss of lock, sequence tracking, bundled frames with and without a field of view, validation suites, recorded video tracked and exported, design prediction against the simulator, what blinds a camera along an orbit, manual, a mirrored image, a phone-width screen |
| A mirrored frame is refused | `tests/mirrored_image.rs` | 60 of 60 refused; 60 of 60 solved once flipped back |
| Python interface equals the command line | `crates/asterope-py/tests` | 7 tests; the same attitude from the same onboard files |

**On "panic-free".** The core crates deny `unwrap`, `expect`, `panic!` and
slice indexing through Clippy, and the robustness tests run with overflow
checks on. A check that the compiled WebAssembly contains no panic machinery
at all was planned and dropped: `alloc::Vec` links it for capacity overflow
regardless of what this code does.

## Real cameras

Frames from three kinds of real camera, none of them taken for this
project, solved with `asterope solve` and no field of view given. Each set
comes with something to check the answer against that this software had no
part in.

### Meteor cameras, 88 degree lens (Global Meteor Network, Arizona)

Zenodo record 7904716 (CC BY 4.0): 23 cameras that recorded the same event
on 27 April 2023, each a 1280 x 720 video camera fixed to a building. A
frame here is the `AVEPIXEL` plane of a four-frame file, the mean of 256
video frames (10.24 s at 25 frames per second).

| | Result |
|---|---|
| Cameras solved without a field of view | 23 of 23, in 5 to 6 seconds each |
| Detections matched | 64 to 79 of 80 |
| Lens found | equidistant, 81.1 to 82.7 degrees with `k1` from -0.06 to -0.17 (the network's own calibrations give 88.0 to 89.5 degrees with their distortion model) |

For 21 of the cameras the record holds measurement files in which the
network's calibration assigns a right ascension and declination to pixel
positions along the event's trail. `asterope compare-ecsv` takes the same
pixels through this solution, allows for the sky's rotation between the
frame and each measurement, and reports the angle between the two:

| Median difference at the measured pixels | Cameras |
|---|---|
| under 0.2 degrees (3 pixels) | 11 |
| 0.2 to 0.5 degrees | 6 |
| 0.7 and 1.3 degrees | 2 |
| 2.0 and 2.5 degrees | 2 |

The median over the 21 cameras is 0.18 degrees. The measured pixels lie 7
to 50 degrees off the optical axis, mostly near the frame edge, which is
where a camera model with one distortion coefficient is weakest; the four
larger differences are all 37 degrees or more off axis. A solution that
was wrong would differ by tens of degrees. So the stars are
identified correctly on all 21, and the fitted lens is good to a few
pixels on 17 of them and visibly off toward the edge on four.

These cameras look through the atmosphere, which lifts a star by an
arcminute at 45 degrees of altitude and five at 10, and the solution above
takes no account of it. `compare-ecsv --refraction` moves the catalog stars
to where the atmosphere shows them from each camera's site and time
(Bennett's formula) before solving, and takes the refraction back out of the
sky positions it reports. The median over the 21 cameras goes from 0.183 to
0.184 degrees, and no camera changes by more than 0.07. Refraction is not
what separates the two calibrations; the lens model is.

Six consecutive frames of one camera (US001P, 10.24 s apart), given to
`asterope replay` with the lens found above: one search, then five frames
tracked. The camera looks 4 degrees from the celestial pole and the solved
roll turns by 0.21 degrees in 51 seconds, the sidereal rate.

### Meteor camera, 54 degree lens (Global Meteor Network, Germany)

Zenodo record 6786601 (CC BY 4.0): nine frames of camera DE000C over two
hours of 30 June 2022, with the network's astrometric calibration
(`platepar`).

| | This solution | Network calibration |
|---|---|---|
| Frames solved without a field of view | 8 of 9 | |
| Declination of the frame center | 8.49 to 8.54 degrees | 8.52 degrees |
| Rotation of the frame | 21.63 to 21.66 degrees | 21.86 degrees |
| Scale at the center | 24.0 px per degree (pinhole, 49.9 degrees, `k1` -0.32) | 24.03 px per degree |
| Right ascension between the first two frames, 64.9 minutes apart | 16.26 degrees | 16.27 degrees of sidereal rotation |

The ninth frame is overcast: two stars are visible in it.

### Star tracker test frames, 11 degree field (ESA)

The eight frames of 29 July 2019 that ship with the tetra3 and cedar-solve
star trackers (ESA / Gustav Pettersson, CC BY-SA 3.0 IGO), taken at two
altitudes and four azimuths.

| | Result |
|---|---|
| Frames solved without a field of view | 8 of 8, in 3 to 4 seconds each |
| Field of view found, independently on each frame | 11.428 to 11.445 degrees |
| Stars matched | 9 to 29 |
| Residual | 6 to 11 arcseconds RMS |

Frames taken at the same altitude and at azimuths mirrored about the
meridian must have the same declination. The four such pairs give 11.04
and 11.31, 57.65 and 58.15, 28.94 and 28.94, and 64.20 and 64.22 degrees.

The same eight frames solved by tetra3 itself, with its own centroiding,
pattern database and fit (`crates/asterope-py/examples/compare_tetra3.py`):

| Frame | tetra3: RA, Dec, field | Asterope: RA, Dec, field | Boresights apart | Rolls apart |
|---|---|---|---|---|
| Alt 40, Az -135 | 230.6677°, 11.0356°, 11.424° | 230.6684°, 11.0358°, 11.431° | 2.6″ | 0.001° |
| Alt 40, Az -45 | 172.3688°, 57.6494°, 11.425° | 172.3680°, 57.6491°, 11.430° | 1.9″ | 0.002° |
| Alt 40, Az 135 | 296.7565°, 11.3142°, 11.424° | 296.7567°, 11.3140°, 11.435° | 1.2″ | 0.010° |
| Alt 40, Az 45 | 355.2067°, 58.1531°, 11.422° | 355.2034°, 58.1518°, 11.439° | 7.8″ | 0.002° |
| Alt 60, Az -135 | 240.4642°, 28.9402°, 11.421° | 240.4640°, 28.9408°, 11.433° | 2.2″ | 0.004° |
| Alt 60, Az -45 | 212.2140°, 64.2002°, 11.421° | 212.2133°, 64.2007°, 11.441° | 2.1″ | 0.007° |
| Alt 60, Az 135 | 286.4363°, 28.9445°, 11.419° | 286.4354°, 28.9440°, 11.433° | 3.2″ | 0.000° |
| Alt 60, Az 45 | 314.6939°, 64.2204°, 11.425° | 314.6924°, 64.2243°, 11.436° | 14.1″ | 0.043° |

A pixel of these frames is 40 arcseconds, so the two solvers put the
boresight within a third of a pixel of each other on every frame, with a
median of 2.4 arcseconds. The fields of view differ by 0.1 to 0.2 %;
Asterope fits a radial distortion term together with the field, which moves
the field it reports. The two count roll in opposite senses; the column
compares them after allowing for that. Neither result is the truth, but two
programs that share no code agree.

### What made the wide cameras work

The first attempt solved the 54 degree camera and none of the 88 degree
ones. Three things were missing (`docs/algorithms.md`):

- an equidistant lens model, because a pinhole with a polynomial
  distortion cannot represent a lens that wide;
- a tolerance chosen for that model, since the tolerance meant to absorb
  unknown distortion spans most of a degree at 88 degrees and makes every
  star pattern ambiguous;
- a search over the distortion coefficient that counts matched stars,
  after which the fit no longer stalled with only the central stars
  matched (on one camera matches went from 34 to 74 and the difference at
  the edge from 3.0 to 0.17 degrees).

### Not solved, and not tried

- One frame of camera ES000W (Zenodo 6123485) was taken in evening
  twilight with trees in the corners. It does not solve, and neither does
  the video rendered from that camera's frames.
- Two ESO time-lapse films (section 8).
- **These frames are 10 second means of video, not single video frames,
  and no raw real-time video of the night sky was available under an open
  licence.** What a single 40 ms frame of such a camera would give is not
  measured here.
- No frame from a star tracker in orbit.

## Real photographs
Checked in the browser by dropping each file on the Image bench, once with
the field of view shown and once with "Field of view unknown".

| Image | Field entered | Result | Fitted field | Matches | Without a field of view |
|---|---|---|---|---|---|
| Orion and the Winter Triangle, A. Fujii (ESA/Hubble `heic0206j`), 800 x 600 | 52° | lock | 52.0° | 58 of 80 | lock, 51.8° |
| Big Dipper (`apod4.jpg`, astrometry.net demo set), 719 x 507 | 33° | lock | 33.13° | 71 of 80 | lock, 33.13° |
| Vela supernova remnant (`apod3.jpg`), 800 x 600 | 8° | lock | 8.44° | 25 of 80 | lock, 8.44° |
| Horsehead and Orion's Belt (`apod2.jpg`), 800 x 600 | 7.2° | no lock | | | no lock |
| Cone Nebula (`apod1.jpg`), about 2° | 2° | no lock | | | no lock |
| Illustration of a planet over a star field (`apod5.jpg`), 900 x 675 | 30° | no lock (the guess is wrong) | | | lock, 72.2°, 73 of 80 |

The first image is bundled as an example; the others are not redistributed.

Notes:

- Residuals on photographs are 25-700 arcseconds RMS, that is 0.7-3 pixels.
  They are set by lens distortion and image processing, not by the solver.
- A blind solve takes 2-11 seconds natively for the three frames that lock
  and 15-21 seconds to give up on the three that do not; the browser is
  about twice as slow.
- The Horsehead frame is a narrow field dominated by bright nebulosity, and
  the Cone Nebula field is far too narrow for a V 6.5 catalog. They are
  documented limits, not solved cases.
- The Orion frame carries a watermark whose letters are detected as blobs.
  Ranking pattern candidates by how star-like they are keeps the letters out
  of the search.
- An earlier version of the acceptance test produced a **false lock** on the
  Horsehead frame under loose settings. The multiple-testing correction
  described in `docs/algorithms.md` removed it.
- **A correction.** The illustration was listed here as "painted stars,
  correctly rejected". It does not solve at the 30 degrees that was guessed
  for it, and that was taken for the right answer. Since wide fields
  solve, a search without a field of view locks on it at 72 degrees with
  73 of 80 detections within half a pixel of catalog stars around Orion.
  The background is a real star map: the three bright stars the solution
  places behind the painted planet (Betelgeuse, Alnitak, Mintaka) are
  exactly the bright ones left unmatched, while Rigel, Bellatrix, Saiph,
  Sirius and Aldebaran, outside its disc, are matched. The test set
  therefore holds no image that should be rejected; rejection is covered
  by the empty and noise-only frames of the test suite.
- Three photographs is a small sample. It shows the pipeline works on real
  data at 8, 33 and 52 degrees; it is not a success-rate measurement.

## Timing

Median over 40 frames, 1024 x 1024, default sensor. Native figures are in
`docs/validation-report.md`.

| Stage | Browser (WebAssembly, Edge) |
|---|---|
| Frame synthesis | 29 ms |
| Detection and centroiding | 12.5 ms |
| Identification and attitude | 0.3 ms |

The browser spreads Monte Carlo trials over up to four workers; 100 trials
take about 4 seconds on the development machine.

## Not verified

- The GitHub Actions workflow runs every check here on Linux: the tests, the
  emulated processor, the C example, the Python package and the three
  browsers. macOS was not tried, and neither was any ARM host.
- Nothing has run on flight hardware or on any microcontroller. The embedded
  build runs under an emulator (section 20), which shows the arithmetic is
  the same and says nothing about time.
- The gyro filter has only seen the simulator's gyro: white noise, a bias
  that walks. It has no scale error or misalignment, and no recorded gyro
  data was used.
- The detector flaws of section 15 are models with chosen levels. None was
  fitted to a real detector.
- No raw real-time video of the night sky has been solved. The real frames
  under "Real cameras" are 10 second means of video, plus stills.
- No real lens was calibrated; the calibration is tested on simulated
  measurements only.
- The C example was compiled with MSVC on Windows and with the system
  compiler on Linux, and with nothing else.
- The photometric constant behind the Design bench is a textbook value and
  was not compared with an instrument.
- No FITS file from a real instrument was tested; the reader is covered by
  generated and corrupted files and by the bundled synthetic frame.
- EXIF field-of-view prefill is unit-tested but was not exercised with a
  camera original.
- The browser checks run in Chromium (Microsoft Edge), Firefox and
  Playwright's WebKit, eleven scenarios each. Recording a sequence as video
  is skipped in WebKit, which has no WebM recorder; Safari itself and any
  phone were not tried. The narrow layout is checked for width only, at
  390 pixels.
- The star-name table was checked against the catalog for existence and
  brightness of all 87 entries and for position of nine of them.
