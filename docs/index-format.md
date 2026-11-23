# Asterope Index (`.asti`)

The pair index holds every pair of pattern stars closer than a camera's
field of view, sorted by angular separation, with a k-vector for range
searches. It is built once on the ground for one camera and one catalog, and
flight software searches the stored bytes in place: `PairIndex::from_bytes`
checks the header and the checksum and keeps a reference to the slice. No
part of the index is copied to the heap.

All values are little-endian. Records are decoded field by field, so the
bytes need no particular alignment and the same file works on any target.

## Header (80 bytes)

| Offset | Type | Field |
|---|---|---|
| 0 | 4 bytes | magic `ASTI` |
| 4 | `u16` | version: 1, or 2 when star positions take 32 bits |
| 6 | `u16` | header length, 80 |
| 8 | `u32` | star count `S` (at most 65 535 in version 1) |
| 12 | `u32` | pair count `P` |
| 16 | `u32` | k-vector length `K` |
| 20 | `u32` | CRC-32 (IEEE 802.3) of every byte after the header |
| 24 | `f64` | k-vector slope `m`, radians per bin |
| 32 | `f64` | k-vector intercept `q`, radians |
| 40 | `f64` | largest indexed separation, radians |
| 48 | `f64` | smallest indexed separation, radians |
| 56 | `f64` | blend radius, radians |
| 64 | `u32` | number of brightest catalog stars considered |
| 68 | `u32` | pair limit the index was built with |
| 72 | `u32` | CRC-32 of the 80 header bytes with this field taken as zero; zero in files written before it existed, which are accepted |
| 76 | 4 bytes | reserved, zero |

## Star records (`S` x 40 bytes)

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | index of the star in the catalog the index was built from |
| 4 | `u32` | star identifier, as in the catalog |
| 8 | 3 x `f64` | ICRS unit vector at the catalog epoch |
| 32 | `f64` | V magnitude |

Stars appear in catalog order, brightest first. A star with a neighbor
within the blend radius that is no more than three magnitudes fainter is
left out, because the two images merge on the sensor.

## Pair records (`P` x 8 bytes in version 1, 12 in version 2)

| Offset | Type | Field |
|---|---|---|
| 0 | `f32` | angular separation, radians |
| 4 | `u16` or `u32` | position of the first star in the star records |
| 6 or 8 | `u16` or `u32` | position of the second star, larger than the first |

Pairs are sorted by separation, then by the two star positions.

The two versions differ in the width of the star positions and in nothing
else. The writer uses version 1 whenever the index holds at most 65 535
stars, so every index for the Hipparcos catalogs is version 1, and a reader
accepts both.

## k-vector (`K` x 4 bytes)

`k[i]` is the number of pairs whose separation does not exceed
`z(i) = m * i + q`. The line starts just below the smallest separation and
ends just above the largest, with one bin for every four pairs. A range
search for `[lo, hi]` reads

```
start = k[floor((lo - q) / m) - 1]
end   = k[ceil((hi - q) / m)]
```

with both bin numbers clamped to `0 ..= K - 1`, then trims the few pairs at
either end that fall outside the range. The method is Mortari and Neta's
k-vector; the coarser line trades four extra comparisons per search for a
quarter of the table.

## Binding to a catalog

Star records carry catalog positions, so an index must be used with the
catalog it was built from, or with a brighter prefix of that catalog that
still contains every indexed star. `asterope build-index --catalog-out` writes
such a prefix. Catalogs are sorted by magnitude, which makes any prefix a
valid catalog with the same star positions.

## Building

```
asterope build-index data/hip65.astc camera.asti \
    --fov 14 --width 1024 --height 1024 --stars-per-field 16 --max-angle 9.85 \
    --catalog-out camera.astc
```

`--stars-per-field` sets the faintest indexed magnitude through the mean
star density; `--mag` sets it directly. `--max-angle` limits the indexed
separation; below the frame diagonal the patterns the solver can use are
those whose stars lie closer together than that. `docs/validation.md` lists
measured success rates for several sizes.
