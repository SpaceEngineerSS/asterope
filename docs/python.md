# Asterope from Python

`crates/asterope-py` is the same tracker as the Rust and C interfaces,
built with PyO3 for Python 3.9 and later. Frames are NumPy arrays; nothing
is copied on the way in.

## Install

```
pip install asterope
```

Wheels are published for Linux (x86-64, ARM64), macOS (Intel, Apple
silicon) and Windows. From the repository root, with a Rust toolchain:

```
pip install maturin
maturin build --release -m crates/asterope-py/Cargo.toml
pip install target/wheels/asterope-*.whl
```

## One frame

```python
import asterope

tracker = asterope.StarTracker(1024, 1024, fov_deg=14.0)
fix = tracker.solve(frame)
if fix.locked:
    print(fix.ra_deg, fix.dec_deg, fix.roll_deg)
    print(fix.quaternion)                 # (x, y, z, w), ICRS to sensor
    print(fix.sigma_arcsec)               # about sensor x, y and the boresight
    print(fix.chance_probability)         # bound on a coincidence
    for match in fix.matches:
        print(match.star, match.magnitude, match.residual_arcsec)
```

`frame` is a two-dimensional, C-contiguous `uint16` array, top row first,
of the size the tracker was made for. Another shape raises `ValueError`,
another type `TypeError`. A frame without a solution is not an error:
`fix.locked` is false and the quaternion is `None`.

Building a tracker builds its pair index, which takes a moment. The
keyword arguments are `k1` (radial distortion), `equidistant` (a lens that
maps angle and not its tangent), `catalog` (the bytes of another `.astc`
file; the Hipparcos stars to V 6.5 are built in), `stars_per_field`,
`saturation` (the largest value the sensor delivers, 4095 by default) and
`trails` (keep stars drawn out by rotation).

`asterope.StarTracker.from_bytes(width, height, fov_deg, catalog, index)`
makes one from the files `asterope build-index` writes, and gives the same
attitude as the command line for the same frame, which a test checks.

## A sequence

```python
for frame in frames:
    fix = tracker.update(frame, dt)       # seconds since the previous frame
    print(fix.event)                      # acquired, tracked, coasting, lost
```

`tracker.reset()` forgets the tracked attitude. `tracker.set_velocity((vx,
vy, vz))`, in km/s against the solar-system barycenter in ICRS axes,
corrects for stellar aberration.

## With a gyro

```python
tracker.enable_gyro(0.003, 3e-5)          # deg/sqrt(s), deg/s/sqrt(s)
for reading, dt in readings_since_last_frame:
    tracker.gyro(reading, dt)             # (x, y, z) in deg/s, sensor axes
fix, attitude, sigma, bias = tracker.update_aided(frame)
```

`attitude` is the filtered quaternion and is there through frames without
stars, once a first star fix has started the filter; `sigma` is its
one-sigma uncertainty in arcseconds and `bias` the estimated gyro bias in
degrees per second. `row_time=` in `enable_gyro` undoes a rolling shutter
with the gyro's rate.

## A photograph

```python
found = asterope.solve_image(frame, hint_fov_deg=20.0)
print(found.fov_deg, found.k1, found.equidistant)
print(found.fix.ra_deg, found.fix.dec_deg)
```

Fields from 6 to 100 degrees are tried, those nearest the hint first, and
the lens is fitted. It takes seconds to a minute. An image reversed by a
mirror, as through a star diagonal, matches nothing until it is flipped:
`numpy.ascontiguousarray(frame[:, ::-1])`.

## A simulated frame

```python
frame = asterope.simulate(1024, 1024, 14.0, ra_deg=83.8, dec_deg=-5.4,
                          roll_deg=12.0, seed=3, exposure=0.1)
```

The simulator's default 12-bit sensor, for trying things without a camera.
`rate_deg_s=` adds motion blur.

## Conventions

The quaternion turns ICRS vectors into the sensor frame, scalar last, with
the scalar part non-negative. In the sensor frame +Z is the boresight, +X
image right and +Y image down. `docs/conventions.md` has the rest.

## Against another solver

`crates/asterope-py/examples/compare_tetra3.py` solves the eight ESA test
frames with this package and with tetra3 and prints the two side by side.
The boresights agree to between 1.2 and 14.1 arcseconds, a third of a pixel
at most (`docs/validation.md`, Real cameras).

## What is not here

The camera design calculations, the orbit model and the validation suites
are in the Rust crate and the command line only.
