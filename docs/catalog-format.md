# Asterope Catalog v1 (`.astc`)

A compact, little-endian star table. The shipped file is `data/hip65.astc`:
8 870 Hipparcos stars with V <= 6.5, 212 912 bytes.

## Layout

### Header, 32 bytes

| Offset | Type | Field |
|---|---|---|
| 0 | 4 x `u8` | Magic `ASTC` |
| 4 | `u16` | Version (1) |
| 6 | `u16` | Record size in bytes (24) |
| 8 | `u32` | Record count |
| 12 | `f32` | Epoch of the positions, Julian year (2000.0) |
| 16 | `f32` | Faintest magnitude in the file |
| 20 | `u32` | CRC-32 (IEEE 802.3) of the record payload |
| 24 | `u32` | CRC-32 of the 32 header bytes with this field taken as zero; zero in files written before it existed, which are accepted |
| 28 | 4 x `u8` | Reserved, zero |

### Record, 24 bytes

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | Star identifier (below) |
| 4 | 3 x `f32` | ICRS unit vector `x, y, z` |
| 16 | `i16` | Proper motion in RA times cos(Dec), units of 0.25 mas/yr |
| 18 | `i16` | Proper motion in Dec, units of 0.25 mas/yr |
| 20 | `i16` | V magnitude, millimagnitudes |
| 22 | `i16` | B-V colour, millimagnitudes; `-32768` when unknown |

Records are sorted by ascending magnitude, then by identifier. Any prefix of
the file is therefore a brighter sub-catalog, and choosing a magnitude limit
is a binary search.

## Identifiers

A value below 2^31 is a Hipparcos number. With the top bit set the rest
packs a Tycho-2 designation `TYC1-TYC2-TYC3`: 14 bits of `TYC1`, 14 of
`TYC2`, 2 of `TYC3`, from the high end down. `catalog::Designation` turns
either into the usual text. The Hipparcos files in `data/` hold no Tycho-2
identifiers; the catalog that `xtask build-tycho` writes does.

## Precision

- `f32` unit-vector components resolve about 0.012 arcseconds. The reader
  renormalizes each vector in `f64`.
- The proper-motion range is +/-8.19 arcseconds per year. The fastest star in
  the file, Groombridge 1830 (HIP 57939, about 7.06 arcsec/yr), fits.
- Magnitudes keep 0.001 mag.

## How the file is built

```
cargo run -p xtask -- build-catalog data/raw/hip_vmag65.tsv data/hip65.astc
```

`xtask` reads a VizieR TSV export of the Hipparcos main catalogue (see
`data/PROVENANCE.md`), drops rows without astrometry or photometry, moves
each position from the Hipparcos epoch J1991.25 to J2000.0 with its proper
motion, and writes the table. Propagation is linear on the tangent plane;
parallax and radial velocity are ignored.

## Reading

`asterope_core::catalog::Catalog::parse` validates the magic, version, record
size, length and checksum and returns `Err` for anything else. It never
panics on malformed input.

`Catalog::at_epoch(year)` returns a copy propagated to another epoch. The
browser uses it for real photographs, so star positions match the night the
picture was taken.
