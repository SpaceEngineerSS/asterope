# Catalog provenance

`hip65.astc` is derived from the Hipparcos main catalogue:

> ESA, 1997, *The Hipparcos and Tycho Catalogues*, ESA SP-1200.
> VizieR catalogue I/239 (`I/239/hip_main`).

The Hipparcos Catalogue is a product of the European Space Agency and is
free to use with this acknowledgement.

## Source query

The input is a VizieR TSV export restricted to `Vmag <= 6.5` with the columns
`HIP, Vmag, RAICRS, DEICRS, pmRA, pmDE, B-V`:

```
curl -L -o data/raw/hip_vmag65.tsv \
  "https://vizier.cfa.harvard.edu/viz-bin/asu-tsv?-source=I/239/hip_main&-out=HIP,Vmag,RAICRS,DEICRS,pmRA,pmDE,B-V&Vmag=%3C=6.5&-out.max=unlimited"
```

The export used for the committed file came from the CfA mirror of VizieR
(`vizier.cfa.harvard.edu`); the CDS server in Strasbourg serves the same
table at `vizier.cds.unistra.fr`. `data/raw/` is not committed.

| File | SHA-256 |
|---|---|
| `data/raw/hip_vmag65.tsv` (570 078 bytes, 8 874 rows) | `C978A0234FD514545C7744398C1C7C25431909B1336CCCCC4D1B5C425C012740` |
| `data/hip65.astc` (212 912 bytes, 8 870 stars) | `010049BE5BE4C9E39FE2E4D31A3229DCA48C25EFEE09CFF93355F33E30D5277D` |

Four rows lack a position and are dropped.

`hip80.astc` is the same table to `Vmag <= 8.0` (replace `6.5` by `8.0` in
the query and pass `8.0` as the last argument below):

| File | SHA-256 |
|---|---|
| `data/raw/hip_vmag80.tsv` (2 652 158 bytes, 41 411 rows) | `1F35040936E3AFFA5EB2EC2C6BADCB28A9A9B35939EB92C9D1602AB1283906A1` |
| `data/hip80.astc` (993 488 bytes, 41 394 stars) | `6F1E9DCF8A69F1A60175C15381E036E429039987428AE330447DFCE8D22250A6` |

Seventeen rows lack a position or a magnitude and are dropped.

## Tycho-2, for fields under 4 degrees

> Høg E. et al., 2000, *The Tycho-2 Catalogue of the 2.5 million brightest
> stars*, A&A 355, L27. VizieR catalogue I/259 (`I/259/tyc2`).

Not kept in the repository. To build the V 9.5 catalog used in
`docs/validation.md`, section 14:

```
curl -L -o data/raw/tyc2_vt96.tsv \
  "https://vizier.cfa.harvard.edu/viz-bin/asu-tsv?-source=I/259/tyc2&-out=TYC1,TYC2,TYC3,pflag,RAmdeg,DEmdeg,pmRA,pmDE,BTmag,VTmag,HIP&VTmag=%3C=9.6&-out.max=unlimited"
cargo run --release -p xtask -- build-tycho data/raw/hip_vmag80.tsv data/raw/tyc2_vt96.tsv data/raw/tyc95.astc 9.5
```

| File | SHA-256 |
|---|---|
| `data/raw/tyc2_vt96.tsv` (16 552 824 bytes, 221 500 rows) | `81D79AA8AD0E77B7E73883632CFAB7B5E23E7E055B2C1E78EF53EFB1C2115EB8` |

The result holds 212 470 stars: the 41 394 Hipparcos stars to V 8 as above,
and 171 076 from Tycho-2. Tycho-2 gives mean positions at epoch J2000, so
they are used as they are. Johnson V and B-V come from the Tycho magnitudes
by the linear relations of the Hipparcos and Tycho Catalogues (ESA SP-1200,
volume 1, section 1.3), `V = VT - 0.090 (BT - VT)` and `B-V = 0.850 (BT -
VT)`; they hold to a few hundredths of a magnitude for unreddened stars
with `-0.2 < BT - VT < 1.8`. A Tycho-2 row naming a Hipparcos star that is
already present is left out (42 113 rows), which also drops the second
component of a double that Hipparcos lists once. Rows without a mean
position (`pflag` X, mostly very bright stars, which Hipparcos supplies)
are dropped.

## Processing

```
cargo run -p xtask -- build-catalog data/raw/hip_vmag65.tsv data/hip65.astc
cargo run -p xtask -- build-catalog data/raw/hip_vmag80.tsv data/hip80.astc 8.0
```

Positions are propagated from the Hipparcos epoch J1991.25 to J2000.0 with
the catalogued proper motions (linear on the tangent plane). See
`docs/catalog-format.md` for the binary layout.

## Example frames

`web/public/samples/` holds the frames offered by "Load example":

| File | Source | Credit and licence |
|---|---|---|
| `orion-winter-triangle.jpg` | ESA/Hubble image `heic0206j`, "Monoceros, Orion, Canis Major and Canis Minor", <https://esahubble.org/images/heic0206j/> | Akira Fujii. ESA/Hubble releases its images under CC BY 4.0 unless stated otherwise; the photograph is redistributed unmodified with this credit. |
| `meteor-camera-usl00b.png` | Global Meteor Network camera USL00B, 27 April 2023 08:50 UTC: the `AVEPIXEL` plane (mean of 256 video frames) of `FF_USL00B_20230427_085028_627_0566016.fits` from Zenodo record 7904716, <https://doi.org/10.5281/zenodo.7904716>, written as an 8-bit PNG without further processing | Global Meteor Network, CC BY 4.0. |
| `synthetic-cygnus.fits` | Rendered by the Asterope simulator (RA 305, Dec 40, roll 25, 16 degree field, seed 2026) | Part of this project; same licence as the code. |

## Onboard files for the reference camera

`crates/asterope-cli/data/hip65.astc` is a copy of `hip65.astc`, there so that
the published command-line crate contains the catalog it builds in; a test
keeps the two identical. Copy the file across after rebuilding the catalog.

`flight-14deg.asti` and `flight-14deg.astc` are derived from `hip65.astc` by

```
asterope build-index data/hip65.astc data/flight-14deg.asti --fov 14 \
    --stars-per-field 16 --max-angle 9.85 --catalog-out data/flight-14deg.astc
```

for a 1024 x 1024 sensor: 3 334 pattern stars to V 5.65 with 47 210 pairs up
to 9.85 degrees, and the 5 923 catalog stars to V 6.15. A test
(`tests/flight_assets.rs`) rebuilds both and compares them byte for byte.

## Known limitations of the data

- The V <= 6.5 cut leaves about 0.2 stars per square degree on average and
  far fewer near the galactic poles.
- Hipparcos lists some close multiple systems as a single entry and others
  as separate entries. Stars with a comparably bright neighbour within a few
  pixels are excluded from pattern matching at index-build time, because
  their images merge on the sensor.
- No parallax or radial-velocity terms are applied.
