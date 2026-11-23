# asterope-core

The core of [Asterope](https://github.com/SpaceEngineerSS/asterope), a star tracker: star detection and centroiding,
lost-in-space identification, attitude determination, tracking and a gyro
filter. `no_std` with `alloc`, no `unsafe`.

```rust
let camera = Camera::from_fov(1024, 1024, 14f64.to_radians())?;
let mut tracker = StarTracker::from_bytes(camera, CATALOG, INDEX)?;
let fix = tracker.update(&frame, 0.1)?;
if let Some(q) = fix.attitude() { /* ICRS to sensor, scalar last */ }
```

The catalog and index files, the guide to putting it in flight software and
the record of what has been measured are in the repository:
[docs/integration.md](https://github.com/SpaceEngineerSS/asterope/blob/main/docs/integration.md),
[docs/validation.md](https://github.com/SpaceEngineerSS/asterope/blob/main/docs/validation.md).

The simulator and the validation suites are behind the `sim` and
`validation` features.
