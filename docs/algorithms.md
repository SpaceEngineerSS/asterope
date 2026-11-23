# Algorithms

```
frame (u16) -> background mesh -> segmentation -> blob filters -> centroids -> unit vectors
                                                                                   |
catalog -> pair index + k-vector (once per camera) ----------------------> Pyramid search
                                                                                   |
                         QUEST / q-method / SVD <- matches <- whole-field verification
                                  |
                 quaternion, RA / Dec / roll, residuals, covariance
```

Module paths refer to `crates/asterope-core/src`.

## Frame synthesis (`sim.rs`)

1. Every catalog star inside the field (plus a margin) is projected through
   the pinhole and Brown-Conrady model.
2. Its signal, `zero_point * exposure * 10^(-0.4 m)` electrons, is spread by a
   Gaussian PSF **integrated over each pixel** (differences of `erf`), not
   sampled at pixel centers. The instrumental magnitude is
   `m = V + color_term * (B - V)`; the term is 0 by default (V band) and about
   -0.3 for unfiltered silicon.
   When the sensor rotates during the exposure (`body_rate`), a star drifts
   at `-rate x direction` and its signal is spread along the straight segment
   between its positions at the start and end of the exposure, sampled at
   least twice per PSF sigma. The attitude passed in is the mid-exposure one.
3. Sky background, dark current and an optional stray-light term are added.
   Stray light is a phenomenological veiling gradient: an exponential decay
   away from the sensor edge that faces the Sun or Moon, on top of a uniform
   floor of 10 % of its peak. It is not a ray-traced baffle model.
4. Hot pixels follow a fixed pattern with its own seed, so the same pixels
   are hot in every frame of a run.
5. Shot noise is drawn from the exact Poisson distribution for pixels
   expecting fewer than 100 electrons. Above that, shot and read noise are
   drawn together from a normal distribution of the same variance (the
   Poisson skewness there is below 0.1).
6. Cosmic rays are straight one-pixel-wide streaks of random length, angle
   and charge, added after the noise.
7. Electrons are clipped at the full well, divided by the gain, offset by
   the bias and quantized.

`Imperfections` adds what an ideal sensor lacks, each off by default:

- *Vignetting.* Light (not dark current) is scaled by `1 - k (1 - cos^4 t)`
  for a pixel at field angle `t`; `k = 1` is the natural falloff of an
  uncorrected lens.
- *Fixed patterns.* A gain that differs from pixel to pixel by a normal
  deviate of given RMS, and extra dark current distributed as the absolute
  value of one; both repeat from frame to frame.
- *Column noise.* An offset per column, drawn anew for each frame.
- *Field-dependent star images.* The PSF width grows with the square of the
  distance from the principal point, and a share of the light, growing the
  same way, is moved into a wider image 2.5 widths farther out. The second
  stands in for coma: it makes the image lopsided and pulls its centroid
  outward, which a symmetric widening does not.
- *Rolling shutter.* Row `y` is exposed `(y - y_mid) t_row` after the
  middle row, so with a body rate each star is drawn where it stands when
  its own row is read.
- *Response.* A pixel holding `e` electrons reads `e (1 - a e / full_well)`,
  and a chosen share of the charge above full well moves to the two
  neighbors in the column, which may overflow in turn.

These are textbook forms, not fits to a particular lens or detector.

The generator is xoshiro256** seeded through splitmix64. Normal deviates use
the ziggurat method; small-mean Poisson deviates use inversion, larger ones
Hörmann's PTRS. All mathematics goes through the pure-Rust `libm`, so a seed
produces the same frame natively and in WebAssembly.

## Detection and centroiding (`detect.rs`)

**Background.** The frame is divided into cells (32 px by default). Each
cell yields a sigma-clipped mean and standard deviation; a 3 x 3 median over
the mesh removes cells dominated by a bright star; the mesh is interpolated
bilinearly to every pixel. This follows a stray-light gradient without
subtracting stars.

**Segmentation.** Pixels more than 3 sigma above the local background are
foreground. Runs of foreground pixels are merged across rows with a
union-find structure (8-connectivity). No label image is allocated.

**Blob filters**, in this order:

| Test | Rejected as |
|---|---|
| Peak below 5 sigma | noise (counted, not listed) |
| Area above `max_area` | too large |
| Area below 3 px, or peak pixel above 70 % of the blob flux | hot pixel |
| Area >= 5 px and axis ratio above 2.5, or all pixels collinear with a peak above 12 sigma | cosmic ray |
| Touches the frame border | edge |

The collinearity rule relies on a bright star always lighting pixels on both
sides of any line through it.

**Centroid.** The PSF width is estimated from the footprint: a Gaussian of
peak `P` covers an area `2 pi sigma^2 ln(P / T)` above level `T`. The default
centroid is an intensity-weighted center of gravity under a Gaussian window
of that width, re-centered until it stops moving. For a symmetric PSF its
fixed point is the true center, and it weights pixels close to the
maximum-likelihood way for background-limited stars. A plain center of
gravity over a hard circular window is available for comparison.

**Ranking.** Accepted blobs are ordered by `flux * stellarity`, and the
pattern search takes them from the top. Stellarity is the eighth power of
the "core ratio": the brightest pixel within one pixel of the centroid
divided by the blob's peak. A star peaks at its centroid (ratio 0.94-1.00 on
the test photographs); nebula knots (0.2-0.55), watermark letters (mostly
0.5-0.9) and merged pairs do not. Nothing is discarded by this measure, so a
low-ranked blob can still be matched during verification.

## Pair index and k-vector (`index/`)

For a camera, the index holds every pair of pattern stars closer than the
sensor diagonal, sorted by angular separation. The magnitude cut is chosen so
that about 25 catalog stars fall on the sensor on average (V <= 6.05 for a
14 degree field).

Range queries use the k-vector of Mortari and Neta: with `n` sorted values
`s`, a line `z(i) = m i + q` is drawn from just below `s[0]` to just above
`s[n-1]`, and `k[i]` counts the values not exceeding `z(i)`. The pairs in
`[lo, hi]` then lie between `k[floor((lo - q) / m) - 1]` and
`k[ceil((hi - q) / m)]`, found without a search. The line has one bin per
four pairs, so a query trims a few pairs at each end. A test compares 10 000
random ranges against brute force.

The index lives in its serialized form (`docs/index-format.md`) and records
are decoded as they are read. Building it takes about 50 ms natively for the
default camera; flight software skips that by reading an index written on
the ground with `asterope build-index`, in place and without copying it.

## Lost-in-space identification (`lis.rs`)

The search follows the Pyramid algorithm (Mortari, Samaan, Bruccoleri and
Junkins 2004).

1. **Triads** of the brightest detections are visited in Mortari's order
   (`dj`, `dk`, `i` loops), which keeps one spurious detection from occupying
   a long run of consecutive triads.
2. For a triad, the legs are looked up in the k-vector within the pair
   tolerance. The leg with the most candidate pairs is not searched: the
   other two meet at a "hub" star, their candidates are joined through
   per-star partner lists, and the third leg is tested directly on the
   catalog directions.
3. A **handedness check** compares the sign of the triple product of the
   observed directions with that of the catalog triangle. A triangle and its
   mirror image share all three legs; this is what separates them.
4. A **fourth star** must match the triangle through its three additional
   legs. Unlike the original algorithm, every candidate triangle is tried
   rather than only a unique one, because the next step is the final judge.
5. **Verification.** The four stars give a first attitude. Every catalog star
   in the field is projected, and each detection is associated with the
   nearest prediction within the match radius. The attitude is re-solved
   from all matches and the association repeated, for as long as it brings
   more stars in. If the accepted solution still explains under half the
   detections, the stars are gathered once more in a ring four match radii
   wide around it and the result kept when it explains more: a fit that
   rests on a few stars close together can be off about the boresight by
   more than the match radius reaches at the far side of the field
   (`docs/validation.md`, section 21).
6. **Acceptance.** With `p` the chance that an unrelated detection lands
   within the match radius of some catalog star (local star density times the
   area of the association disc), the probability of seeing at least the
   observed number of extra matches among the remaining detections is a
   binomial tail. Every verified pyramid is another opportunity for a
   coincidence, so the tail is multiplied by the number of pyramids verified
   so far for the frame (a Bonferroni correction), counted across all
   candidate focal lengths. A solution is reported only if the corrected
   value is below `1e-3` and at least five stars match.
7. **Budget.** The search gives up after 2000 verified pyramids per frame
   (6000 per rung when the field of view is unknown) and reports "no
   solution". By then the correction demands an overwhelming match anyway,
   and an unbounded search on a frame with no usable stars would run for
   minutes.

The correction in step 6 was added after a real photograph produced a false
lock: with loose tolerances, 21 000 pyramids were verified and one of them
collected eight chance matches at a per-attempt probability of 2e-4.

Planets, satellites and stars missing from the catalog are unmatched
detections; the search tolerates them by design.

An image reversed by a mirror is not solved: step 3 turns every triangle
of it away. Of 400 mirrored fields none was accepted
(`docs/validation.md`, section 24), and of 60 rendered frames flipped left
to right none solved until flipped back (`tests/mirrored_image.rs`).
`detect::mirrored` does the flip.

## Attitude determination (`attitude/`)

All three solvers minimize Wahba's loss for the matched pairs, with the
attitude profile matrix `B = sum(a_i b_i r_i^T)`.

**QUEST** (Shuster and Oh 1981). The largest eigenvalue of Davenport's K
matrix is found by Newton-Raphson on the characteristic quartic, starting
from 1. The quaternion comes from the Gibbs-vector construction. Two
additions:

- *Sequential rotations.* The Gibbs vector is singular for 180 degree
  rotations. The problem is solved in four reference frames (the original and
  three rotated by a half turn about an axis) and the one with the largest
  scalar part is kept.
- *Inverse-iteration polish.* The closed-form eigenvector is built from
  adjugate terms that cancel heavily for narrow fields, where the two largest
  eigenvalues of K almost coincide. Without the polish the result differs
  from the SVD solution by up to 1e-9 rad in the test set; two inverse
  iterations on `K - lambda I` bring that below 1e-12 rad.

**Davenport q-method.** The eigenvector of K with the largest eigenvalue,
from a cyclic Jacobi iteration.

**SVD method** (Markley 1988). `A = U diag(1, 1, det U det V) V^T`. The SVD is
a one-sided Jacobi iteration, which keeps high relative accuracy for the two
small singular values of a narrow-field profile matrix.

**Covariance.** The one-sigma attitude uncertainty comes from the QUEST
covariance `[sum(w_i (I - b_i b_i^T)) / sigma^2]^-1`, with `sigma`, the
direction error of a star of weight one, estimated from the weighted fit
residuals.

**Weights.** A match enters the fit with the inverse variance of its
centroid, `w = 1 / (0.03^2 + (s / SNR)^2)` in pixels: `s` is the measured
width of the star image, `SNR` its aperture signal-to-noise ratio, and 0.03
px the error that pixel sampling leaves at any signal. A saturated star
takes 0.3 px in place of 0.03. The weights are applied once the stars are
identified, so they change the attitude and not which frames lock.

A bright star whose centroid is spoiled by a hot pixel or a particle track
would keep the weight its brightness earned and drag the fit. So after each
fit the residual of every star is compared with what its weight allows, on
a scale taken from the median star (the median of a squared two-dimensional
normal error is `2 ln 2` times its variance), and a star more than three
times over has its weight cut by that factor: Huber's weights, three
rounds.
Photographs are fitted with equal weights, because the error there is that
of the fitted lens model and does not depend on brightness.

## Approximately known field of view (`calib.rs`)

For photographs, the focal length is searched outward from the entered value
in steps of `tolerance / image size`, which leaves every leg inside the
matching window at some step. After the first lock, focal length and the
radial coefficient `k1` are fitted by linear least squares to the matched
stars, alternating with new attitude solutions. The principal point stays at
the image center: with a free attitude it is almost perfectly degenerate with
a small tilt.

The pair tolerance follows the field of view unless set by hand:
`max(2, 0.021 * tan^2(fov / 2) * long side)` pixels, capped at 12, because
lens distortion grows roughly with the square of the field angle.

**Unknown field of view.** A ladder of fields (6, 9, 13, 20, 30, 45, 68 and
100 degrees) is tried, nearest to the hint first. Each rung builds its own
pair index and searches +/-22 % in focal length, so neighbouring rungs
overlap. The multiple-testing count is shared across rungs. The search is a
resumable state machine (`ScaleSearchState`); the browser advances it a few
scales at a time so it can show progress and stop on request.

### Wide lenses

A lens up to about 70 degrees is a pinhole with a little distortion, and
`r (1 + k1 r^2)` on `r = tan(theta)` describes it. Wider lenses are built
closer to `r = theta`. A polynomial on the pinhole cannot follow that: with
the `k1` it would need, the mapping turns back about 45 degrees off axis,
before the corner of an 88 degree frame. `Camera` therefore carries a
projection, gnomonic or equidistant, under the same distortion terms, and
the ladder tries both at 68 and 100 degrees.

Three further steps were needed before real wide cameras solved
(`docs/validation.md`, "Real cameras"):

- **Tolerance by lens model.** The pinhole rule widens the matching window
  with the square of the field to absorb unknown distortion, up to 12
  pixels. At 88 degrees that is most of a degree, and every triangle then
  matches dozens of catalog triangles. An equidistant camera already
  follows the lens, so it gets 0.3 % of the long side instead.
- **Center first.** When a pattern search over the whole frame fails on a
  field of 40 degrees or more, it is repeated over the stars within half
  the diagonal of the center, where a radial distortion is an eighth of
  its corner value.
- **Growing outward.** After a lock the camera is refitted and the stars
  are associated again from the attitude already found rather than by a
  new pattern search, so stars come within reach as the model improves.
  Fitting `k1` to the matched stars alone stalls when they all lie near
  the center, so `k1` is also stepped directly, 0.02 at a time, keeping
  the value that matches the most stars.

## Tracking (`track.rs`)

Once an attitude is known, the next frame is solved without a pattern
search:

1. The previous attitude is propagated by the estimated body rate over the
   frame period.
2. Each detection is associated with the catalog star predicted within a
   12 pixel window, the attitude is refitted, and the same acceptance test as
   in the lost-in-space path decides whether to trust it.
3. The solution gets a second opinion from a pattern search when it keeps
   fewer than half of the previous frame's matches, or matches fewer than
   half of the brightest detections. Leftovers of the previous frame (image
   lag, video compression) sit exactly where a stationary prediction
   expects stars and hold the lock on the old attitude; the true stars are
   then the brighter, unmatched detections. If the search finds a solution
   it wins; if it finds none, the doubtful solution stands. With a leftover
   image at 45 % of the frame's brightness the tracker reported the wrong
   attitude in 13 of 20 cases without this check and in none with it.
4. The body rate is updated from the two latest attitudes.
5. If a frame fails and a rate is known, the prediction is carried forward
   for up to three frames ("coasting"). Otherwise a lost-in-space search
   runs; two consecutive searches give the rate when the motion per frame
   exceeds the window.

This is a predictor, not a filter: there is no gyro input and no smoothing.

## Video: registration and stacking (`stack.rs`)

A video frame is short and noisy, and often shows only a handful of stars.
Averaging the last `n` frames lowers the noise by about `sqrt(n)`, but only
if the stars are first brought back to the same pixels.

1. **Registration.** The detections of two consecutive frames are paired
   when they lie within 24 pixels of each other after applying the rotation
   measured between the previous two frames, and only when each is the
   other's nearest. QUEST then solves `current = R previous` on the paired
   sensor-frame vectors; no catalog is involved. The pairing is repeated
   twice with the fitted rotation, each time with a quarter of the radius,
   which undoes pairs crossed between neighboring stars. At least three
   pairs and an RMS disagreement under one pixel are required.
2. **Alignment.** Chaining frame-to-frame rotations would let their errors
   add up along the sequence, so every older frame is registered against
   the newest one directly, starting from its chained rotation.
3. **Resampling.** Each held frame keeps its rotation to the newest frame.
   The source position of every output pixel is computed exactly on a
   16 pixel mesh (unproject, rotate, project, lens distortion included) and
   interpolated linearly in between; the frame is sampled bilinearly.
4. **Averaging.** The mean is taken over the frames that cover a pixel and
   stored at up to eight times the input scale: averaging brings the noise
   below one count, and rounding the mean back to whole counts would undo
   the gain. (An earlier version did round, and gained almost nothing.)

Detection and identification then run on the stacked frame, whose star
positions are those of the newest frame. A failed registration empties the
stack, so a wrong alignment is never averaged in. The limits are stated in
`docs/validation.md`: three stars must be visible in single frames, and
bilinear resampling widens the star images slightly.

## Camera calibration (`calibrate.rs`)

Several solved frames give hundreds of stars with a measured pixel position
and a known catalog direction. The fit minimizes their pixel residuals over
the focal length, the principal point and the distortion coefficients,
together with a small rotation of every frame, by Levenberg-Marquardt.

Each frame's rotation couples only to the intrinsics, so the normal
equations have an arrowhead shape. The 3 x 3 rotation blocks are eliminated
one by one (a Schur complement) and a 7 x 7 system in the intrinsics
remains, scaled to unit diagonal before it is solved. Its inverse, times the
residual variance, is the covariance of the intrinsics with the attitudes
marginalized; the square roots of its diagonal are the reported
uncertainties.

A shift of the principal point is, to first order, a tilt of every frame.
Star fields separate the two only through the curvature of the gnomonic
projection and through the distortion being centered on the principal
point, which makes the principal point several times less certain than the
focal length on a narrow field, and biased by tangential distortion that is
present but not fitted.

## Stellar aberration (`ephem.rs`)

An observer moving at velocity `v` sees every star displaced toward the
direction of motion; to first order the unit vector `s` becomes
`normalize(s + v / c)`. The catalog is shifted in place, so that a solved
attitude refers to the ICRS axes and not to the apparent sky. Shifts add to
first order, so a change in velocity is applied as the difference.

The Earth's velocity comes from differentiating the low-precision solar
coordinates of the Astronomical Almanac, with the longitude carried back to
J2000 by the general precession. A two-body orbit from Standish's mean
elements, used only in the test, agrees within 0.003 km/s.

## Camera design (`design.rs`)

The Design bench predicts in closed form, from a lens, a detector and an
exposure:

- **Zero point.** A V = 0 star delivers 1005 photons per second, per square
  centimeter and per angstrom at 550 nm (Bessell 1979); this is multiplied
  by the pupil area, the passband, the transmission and the quantum
  efficiency.
- **Limiting magnitude.** The star whose brightest pixel reaches the
  detection threshold over the background noise, for the mean of a star
  centered on a pixel and one centered on a pixel corner. The threshold is
  lowered by 1.163 sigma, the expected maximum of five unit normals, because
  the detector takes the brightest of several noisy pixels. A rotating
  camera spreads the light along a trail of length `L` and lowers the peak
  by `sqrt(2 pi) sigma / L * erf(L / (2 sqrt(2) sigma))`.
- **Coverage.** At each cell of a 5 degree grid, the catalog stars brighter
  than the limit inside a circle of the field's solid angle.
- **Accuracy.** A centroid variance per star of 1.3^2 times
  `sigma^2 / N + 8 pi sigma^4 b^2 / N^2` (photon and background terms for a
  Gaussian PSF of width `sigma`, `N` electrons, background noise `b`), and
  an attitude variance of one over the summed inverse variances of the stars
  in the field, since the solver weighs each star by the accuracy of its
  centroid. About the boresight it is larger by the inverse mean squared
  off-axis angle.
- **Streak limit.** A trail of length `L` raises the axis ratio of a star
  image to `sqrt(1 + L^2 / (12 sigma^2))`; the detector's limit of 2.5 is
  reached at `L = 7.94 sigma`.

`docs/validation.md`, section 11, compares each prediction with the
simulator.

## Rolling shutter (`pipeline.rs`)

A rolling shutter reads row `y` a time `(y - y_mid) t_row` after the middle
row. In the sensor frame a star moves at `-w x b` when the camera turns at
rate `w`, so a star read late by `d` is seen at `b - (w x b) d`. With the
rate known, each observed direction is put back by `(w x b) d` before
anything else is done with it, which refers the whole frame to the instant
of the middle row. The rate has to be supplied: `StarTracker::update_aided`
takes it from the gyro.

## Star trails (`detect.rs`)

During an exposure of length `T` a star moves `w T` across the sensor and
leaves a trail. The detector rejects elongated blobs, because a cosmic-ray
track is one. With `DetectConfig::trails` it keeps those whose width across
is that of a star image (a variance of at least 0.3 px^2 across; a particle
track measures 0.08 to 0.2). The position reported is the center of gravity
of the light inside an ellipse laid along the trail and opened by three
widths beyond its measured length. A hard-edged window weighs every part of
the trail the same, so the result is the middle of the trail: where the
star stood at the middle of the exposure, which is the instant the frame's
attitude refers to.

## Gyro filter (`filter.rs`)

A multiplicative extended Kalman filter (Lefferts, Markley and Shuster
1982). The state is the attitude quaternion `q` and the gyro bias `b`; the
covariance is that of `(dtheta, db)`, where `dtheta` is the rotation in
sensor axes that takes the estimate to the truth,
`q_true = dq(dtheta) q_est`. The gyro reads `w + b + noise`, with white
noise of density `v` (the squared angle random walk) and a bias that walks
with density `u`.

- *Between fixes.* The quaternion is turned by the bias-corrected reading.
  The error obeys `d(dtheta)/dt = -w x dtheta - db - noise`; for a constant
  rate the transition matrix `[[F, G], [0, I]]` has the closed form
  `F = I - [w x] sin(a)/|w| + [w x]^2 (1 - cos a)/|w|^2`,
  `G = [w x] (1 - cos a)/|w|^2 - I dt - [w x]^2 (a - sin a)/|w|^3` with
  `a = |w| dt`, and the process noise over the step is
  `[[v dt + u dt^3/3, -u dt^2/2], [-u dt^2/2, u dt]]` times the identity.
- *At a fix.* The residual is the rotation from the predicted attitude to
  the measured one, `2 vec(dq) / scalar(dq)`. Its covariance is that of the
  prediction plus that of the fix, which is the star tracker's own
  (above). The update is in Joseph form.
- *Gate.* The residual weighed by its covariance follows a chi-square
  distribution with three degrees of freedom when the fix belongs. A fix
  beyond a chosen level is reported and left out. The default of 30 turns
  away about one honest fix in a million; a wrong identification is off
  by thousands. After five in a row the filter starts again from the fix,
  since it may be the gyro that is wrong.

## The Sun, the Moon and the planets (`solar.rs`)

Planets are two-body orbits from Standish's mean elements, referred to the
Earth-Moon barycenter from the same table and corrected for light time.
The Moon and the Sun use the Astronomical Almanac's low-precision series,
with the Moon's longitude carried from the equinox of date to J2000 by the
general precession, and the Moon's direction referred to the observer's
position, since from low orbit its parallax reaches a degree. Stellar
parallax is at most 0.75 arcseconds (Alpha Centauri) and under 0.05 for all
but a few hundred catalog stars, and the Sun bends starlight by 0.03
arcseconds 15 degrees from it and less farther out; neither is applied.

Refraction is Bennett's formula for the apparent altitude `h` in degrees,
`R = cot(h + 7.31 / (h + 4.4))` arcminutes. It matters only for a camera on
the ground, where it raises a star by 1 arcminute at 45 degrees and 5 at
10, and so squeezes a wide field toward the zenith.

## Orbit and exclusion angles (`orbit.rs`)

The orbit is a Kepler ellipse whose node, perigee and mean anomaly drift at
the secular rates the Earth's oblateness gives them, to first order in
`J2` (Vallado, section 9.6). With `n` the mean motion, `p = a (1 - e^2)`
and `k = n J2 (R / p)^2`:

```
node rate     = -3/2 k cos i
perigee rate  =  3/4 k (5 cos^2 i - 1)
anomaly rate  =  n + 3/4 k sqrt(1 - e^2) (3 cos^2 i - 1)
```

The node rate is what turns the orbit plane against the Sun over weeks, and
it is why this much is kept. Nothing else is: no drag, no periodic terms,
no higher harmonics. Along the track the model runs ahead of or behind a
real spacecraft by tens of kilometres a day. It answers how much of an
orbit a camera can see stars in, and does not say where a spacecraft is.

`Pointing` gives the attitude along the orbit. `Local` fixes the boresight
in the frame whose x axis is along the velocity, y along the orbit normal
and z away from the Earth, and takes the body rate from attitudes one
second apart; `Inertial` holds an attitude against the stars; `Spinning`
turns about a fixed sensor axis at a steady rate.

`Surroundings` gives three angles from the boresight: to the center of the
Sun, to the center of the Moon as seen from the spacecraft, and to the
nearest point of the Earth's limb, which lies `atan(R / sqrt(d^2 - R^2))`
from the direction of the Earth's center for a spacecraft `d` from it. The
Earth is a sphere of its equatorial radius with no atmosphere. The
spacecraft is in eclipse when it is behind the Earth as seen from the Sun
and within one Earth radius of the Earth-Sun line: a cylindrical shadow,
which leaves out the penumbra, a matter of seconds in a low orbit.

A body inside its `Exclusion` angle blinds the camera and one outside does
nothing. The Earth is tested first, then the Sun, which blinds nothing in
eclipse, then the Moon. Real stray light comes in gradually and depends on
the baffle; the angles stand for where a baffle's makers say it stops
working.

## Deep catalogs and the wide index (`catalog.rs`, `index/`)

A star is known by a 32-bit number. Hipparcos numbers stay as they are. A
Tycho-2 star without one carries the top bit and its designation packed
under it: 14 bits of `TYC1`, 14 of `TYC2`, 2 of `TYC3`. The planets, when a
simulation draws them as stars, take numbers from `0x7fffff00`, above every
Hipparcos number and below the Tycho-2 bit, so no solver can find them in a
catalog.

A pair record of the index names its two stars by their position in the
index's star list. Up to 65 535 stars that fits 16 bits and a record is 8
bytes (version 1); beyond it positions are 32 bits and a record 12 bytes
(version 2). The search reads either through the same iterator, and a
version 1 file is read as before (`docs/index-format.md`).

The size of a pair index grows with the square of the star density times
the area searched. For a 2 degree field and stars to V 9.5 it is about
140 MB, which is where a pair index stops being the right structure.

## Atmospheric refraction (`solar.rs`, `Catalog::refract`)

The formula is given with the Sun and the planets above. `add_refraction`
raises a true direction toward the zenith by the refraction at the altitude
it ends up at, found by eight fixed-point steps since Bennett's formula
takes the apparent altitude; `remove_refraction` lowers an apparent
direction by the refraction at its own altitude. `Catalog::refract` moves
every star to where an observer with a given zenith sees it, so that an
attitude solved against the result refers to the true sky. The zenith comes
from the mean sidereal time (Meeus, eq. 12.4) without precession since
J2000 or nutation, which is 0.4 degrees of zenith in 2026 and changes the
refraction by 5 arcseconds at 15 degrees of altitude.

## References
- D. Mortari, M. A. Samaan, C. Bruccoleri, J. L. Junkins, "The Pyramid Star
  Identification Technique", *Navigation* 51(3), 2004.
- D. Mortari, B. Neta, "k-vector Range Searching Techniques", AAS 00-128, 2000.
- M. D. Shuster, S. D. Oh, "Three-Axis Attitude Determination from Vector
  Observations", *Journal of Guidance and Control* 4(1), 1981.
- F. L. Markley, "Attitude Determination Using Vector Observations and the
  Singular Value Decomposition", *Journal of the Astronautical Sciences*
  36(3), 1988.
- F. L. Markley, D. Mortari, "Quaternion Attitude Estimation Using Vector
  Observations", *Journal of the Astronautical Sciences* 48(2-3), 2000.
- W. Hörmann, "The transformed rejection method for generating Poisson
  random variables", *Insurance: Mathematics and Economics* 12(1), 1993.
- G. Marsaglia, W. W. Tsang, "The Ziggurat Method for Generating Random
  Variables", *Journal of Statistical Software* 5(8), 2000.
- M. S. Bessell, "UBVRI photometry II: the Cousins VRI system, its temperature
  and absolute flux calibration", *PASP* 91, 1979.
- E. M. Standish, "Keplerian Elements for Approximate Positions of the Major
  Planets", JPL Solar System Dynamics.
- D. C. Brown, "Close-range camera calibration", *Photogrammetric
  Engineering* 37(8), 1971.
- E. J. Lefferts, F. L. Markley, M. D. Shuster, "Kalman Filtering for
  Spacecraft Attitude Estimation", *Journal of Guidance, Control, and
  Dynamics* 5(5), 1982.
- F. L. Markley, J. L. Crassidis, *Fundamentals of Spacecraft Attitude
  Determination and Control*, Springer, 2014.
- G. G. Bennett, "The Calculation of Astronomical Refraction in Marine
  Navigation", *Journal of Navigation* 35(2), 1982.
- *The Astronomical Almanac*, low-precision formulae for the Sun and the
  Moon.
- E. Høg et al., "The Tycho-2 Catalogue of the 2.5 million brightest
  stars", *Astronomy and Astrophysics* 355, L27, 2000.
- ESA, *The Hipparcos and Tycho Catalogues*, ESA SP-1200, 1997.
- D. A. Vallado, *Fundamentals of Astrodynamics and Applications*, 4th
  edition, Microcosm Press, 2013.
- J. Meeus, *Astronomical Algorithms*, 2nd edition, Willmann-Bell, 1998.
- P. J. Huber, "Robust Estimation of a Location Parameter", *Annals of
  Mathematical Statistics* 35(1), 1964.
- ECSS-E-ST-60-20C Rev. 2, *Star sensor terminology and performance
  specification*, European Cooperation for Space Standardization, 2019.
