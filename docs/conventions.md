# Frames, signs and units

Every convention below is pinned by a test (`attitude::angles::tests`,
`tests/lost_in_space.rs::orion_belt_projects_where_expected`,
`web/src/lib/sky.test.ts`).

## Reference frame

Star directions are ICRS unit vectors at epoch J2000.0. The catalog treats
ICRS and the J2000 mean equator and equinox ("J2000 ECI") as the same frame;
they differ by about 20 milliarcseconds, far below the accuracy reported
here.

## Sensor frame

| Axis | Direction |
|---|---|
| +Z | Boresight, out of the lens |
| +X | Image right |
| +Y | Image down |

The sensor frame is also the body frame: no mounting rotation is modelled.

## Pixels

Pixel centers sit at integer coordinates. `(0, 0)` is the center of the
top-left pixel, so the image spans `[-0.5, width - 0.5]`. The principal point
of a camera built with `Camera::from_fov` is `((width - 1) / 2, (height - 1) / 2)`.

Projection of a sensor-frame direction `b`:

```
x = b.x / b.z          y = b.y / b.z          r^2 = x^2 + y^2
radial = 1 + k1 r^2 + k2 r^4 + k3 r^6
xd = x radial + 2 p1 x y + p2 (r^2 + 2 x^2)
yd = y radial + p1 (r^2 + 2 y^2) + 2 p2 x y
u = fx xd + cx         v = fy yd + cy
```

This is the Brown-Conrady model in the same form OpenCV uses.

## Attitude

The attitude matrix `A` maps reference vectors into the sensor frame:
`b = A r`. Its rows are the sensor axes expressed in ICRS.

The quaternion is **scalar-last** (Shuster, Markley): `q = [q1, q2, q3, q4]`
with `q4` the scalar part and

```
A(q) = (q4^2 - |v|^2) I + 2 v v^T - 2 q4 [v x]        v = [q1, q2, q3]
```

Composition follows `A(p * q) = A(p) A(q)`. The sign ambiguity is resolved by
reporting `q4 >= 0`.

## RA / Dec / Roll

RA and Dec are the direction of the boresight. Roll is the position angle of
the image "up" direction (`-Y`), measured from celestial north through east.

At roll 0, north is up and east is to the **left**, which is how the sky
looks from inside the celestial sphere. Increasing roll turns the sensor so
that image-up swings from north toward east.

With `n` and `e` the unit vectors toward north and east at the boresight
`z`:

```
up = n cos(roll) + e sin(roll)
y_axis = -up
x_axis = y_axis x z
A = [x_axis; y_axis; z]
```

At a celestial pole `n` and `e` are undefined; RA is reported as 0 there.

## Rates and gyro readings

A body rate is the angular velocity of the sensor frame against the stars,
expressed in the sensor frame, in radians per second. A star at direction
`b` moves in the sensor frame at `-w x b`: under a positive rate about +Z a
star to the right of the center moves up in the image, and under a positive
rate about +Y the stars near the center move to the left.

A gyro reading is taken in the same frame and sense, and as the mean rate
over the interval it covers. The estimated bias is what has to be
subtracted from a reading to get the rate.

## Rolling shutter

Rows are read from the top. Row `y` is read `(y - (height - 1) / 2) t_row`
after the middle of the frame, and a corrected frame refers to that middle
instant.

## Time

Positions of the Sun, the Moon and the planets, and orbit epochs, take a
Julian date in terrestrial time. The zenith of a site takes one in
universal time. In 2026 the two differ by 69 seconds, which moves the Sun
by 3 arcseconds and the zenith by 0.3 degrees: the second one matters.

## Orbit frame

`Pointing::Local` gives the boresight in the frame whose x axis is
`normal x up` (along the velocity for a circular orbit), y the orbit normal
`r x v`, and z away from the center of the Earth. At zero roll image right
is `normal x boresight`, or `along x boresight` when the camera looks along
the normal.

## Star identifiers

| Range | Meaning |
|---|---|
| below `0x7fffff00` | Hipparcos number |
| `0x7fffff00` to `0x7fffff06` | Mercury, Venus, Mars, Jupiter, Saturn, Uranus, Neptune, when a simulation draws them |
| top bit set | Tycho-2: 14 bits of `TYC1`, 14 of `TYC2`, 2 of `TYC3` below it |

## Units

| Quantity | Unit |
|---|---|
| Core API angles | radians |
| WebAssembly boundary and UI angles | degrees |
| Residuals and attitude errors | arcseconds |
| Sensor signal | electrons; frames are stored in digital numbers (DN) |
| Proper motion | milliarcseconds per year, RA component includes cos(Dec) |
| Body rates and gyro readings | rad/s in the core; degrees per second in C, Python and the interface |
| Gyro noise | rad/sqrt(s) and rad/s/sqrt(s) in the core; the same in degrees in C and Python |
| Orbit positions and velocities | kilometres and km/s from the center of the Earth, ICRS axes |
| Magnitudes | Johnson V |

## Attitude error

The error between an estimate and the truth is the rotation
`q_est * q_truth^-1`, expressed as a rotation vector in the sensor frame. Its
x-y part is the **cross-boresight** error (where the camera points) and its z
part is the **roll** error (rotation about the line of sight). Roll is always
the weaker axis for a narrow field: it is determined only by how far stars
sit from the boresight.
