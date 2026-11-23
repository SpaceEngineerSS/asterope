# Error budget

What moves a reported attitude away from the true one, how much, and where
the number comes from. Each line is either **measured** by a test in this
repository (the section of `docs/validation.md` is given) or **derived**
from a stated formula. Values are for the reference camera unless noted:
1024 x 1024 pixels, 14 degree field (49 arcseconds a pixel), star images of
1.2 px, 0.1 s exposure, 5 electrons of read noise, about forty stars in the
field.

An error across the boresight moves where the camera is said to point. An
error about the boresight turns the image and is larger by the ratio of a
radian to the half field, about eight here.

## What the software contributes

| Source | Across the boresight | About the boresight | From |
|---|---|---|---|
| Noise in one frame (temporal noise) | 0.43″ per axis | 4.6″ | measured, 23 |
| Stars crossing pixels (pixel spatial error) | 0.08″ | 0.9″ | measured, 23 |
| Stars crossing the field (field spatial error) | 0.08″ | 0.8″ | measured, 23 |
| Catalog positions as stored (32-bit vectors) | 0.012″ per star | | derived: 2^-24 of a radian |
| The three attitude solvers against one another | under 1e-5″ | | measured, 2 |

The first three add in quadrature to 0.44″ per axis across and 4.7″ about
the boresight, one sigma, which puts the median error at 0.52″ and 3.2″.
The Monte Carlo over random attitudes, which mixes all three, has medians
of 0.46″ and 3.0″ (section 12).

## What depends on how it is used

| Source | Size | When it applies | From |
|---|---|---|---|
| Stellar aberration | 17.4″ left in, 0.8″ after `set_velocity` | always, in orbit or on the ground | measured, 9 |
| Proper motion since J2000 | 1″ for a typical bright star in 2026, up to 7″ a year for the fastest | when `Catalog::at_epoch` is not applied | derived from the catalog's proper motions |
| Rolling shutter, 60 µs a row, turning at 1°/s | 37″ left in, 1.3″ undone with the rate | rolling-shutter detectors in motion | measured, 15 |
| Stars trailed by rotation, 1°/s | 1.5″ median, against 0.5″ at rest | exposures long against the rate | measured, 18 |
| Time stamp of the frame | 3.6″ per millisecond at 1°/s | whenever the attitude is used at another instant | derived: rate times time |
| Lens distortion known to 0.002 in `k1` | raises the field term from 0.08″ to 0.11″ across | after calibration | measured, 23 |
| Lens fitted from the frame itself, 88 degree field | a few pixels at the edge on most cameras, 1 to 2.5 degrees on some | photographs and uncalibrated cameras | measured, Real cameras |
| Atmospheric refraction | 1′ at 45° altitude, 5′ at 10° | cameras on the ground | derived, Bennett 1982 |
| Vignetting, gain and dark patterns, coma, nonlinearity together | doubles the error (0.56″ to 1.13″ median) | at the levels of section 15 | measured, 15 |

## What is left out, and why

| Effect | Size | From |
|---|---|---|
| Stellar parallax | at most 0.75″, under 0.05″ for all but a few hundred catalog stars | derived from the distances of the nearest stars |
| Bending of starlight by the Sun | 0.03″ at 15° from the Sun, less beyond | derived: 0.004″ times the cotangent of half the angle |
| Second-order aberration | under 0.001″ | derived: (v/c)^2 |
| Hipparcos's own position and motion errors | about 0.04″ in 2026 | derived: 1 mas at 1991.25 and 1 mas a year since |
| Precession and nutation in the zenith used for refraction | 0.4° of zenith in 2026, which changes the refraction by 5″ at 15° of altitude and under 1″ above 40° | derived from the slope of Bennett's formula |

## With a gyro

The filter of `filter.rs`, on a gyro with 0.18°/√h of angle random walk
and star fixes at 5 Hz (section 17), brings the error of the reported
attitude to 1.9″ across and 7.9″ about the boresight, from 2.0″ and 15.0″
for the fixes alone in that test, and lets it grow to 63″ after 10 s
without stars, 190″ after a minute and 820″ after five.

## What is not in any line

Mounting misalignment and its change with temperature, the detector's own
timing jitter, stray light short of blinding, and everything about a real
lens that the Brown-Conrady and equidistant models do not describe. None of
these can be measured without hardware, and none is claimed.
