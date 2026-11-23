//! `cargo run -p xtask -- build-catalog <vizier.tsv> <out.astc> [faintest V]`
//!
//! Converts a VizieR TSV export of the Hipparcos main catalogue (I/239) with
//! the columns `HIP, Vmag, RAICRS, DEICRS, pmRA, pmDE, B-V` into the compact
//! Asterope catalog format. Stars fainter than the limit (6.5 unless given)
//! are left out.
//!
//! `cargo run -p xtask -- build-tycho <hipparcos.tsv> <tycho2.tsv> <out.astc> <faintest V>`
//!
//! Extends a Hipparcos export with Tycho-2 (I/259/tyc2, columns `TYC1, TYC2,
//! TYC3, pflag, RAmdeg, DEmdeg, pmRA, pmDE, BTmag, VTmag, HIP`). Hipparcos
//! rows are taken as they are; a Tycho-2 star is added when Hipparcos did
//! not supply it.

use std::error::Error;
use std::fs;
use std::process::ExitCode;

use std::collections::HashSet;

use asterope_core::catalog::{tycho_id, Catalog, Star};
use asterope_core::math::{Vec3, DEG};

/// Hipparcos positions are given at this epoch (ICRS).
const HIPPARCOS_EPOCH: f64 = 1991.25;
const TARGET_EPOCH: f64 = 2000.0;
const DEFAULT_MAG_LIMIT: f64 = 6.5;

fn parse_row(line: &str, mag_limit: f64) -> Option<Star> {
    let mut fields = line.split('\t').map(str::trim);
    let hip: u32 = fields.next()?.parse().ok()?;
    let mag: f64 = fields.next()?.parse().ok()?;
    let ra: f64 = fields.next()?.parse().ok()?;
    let dec: f64 = fields.next()?.parse().ok()?;
    let pm_ra: f64 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0.0);
    let pm_dec: f64 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0.0);
    let b_v: Option<f64> = fields.next().and_then(|f| f.parse().ok());
    if mag > mag_limit {
        return None;
    }
    let (ra, dec) = (ra * DEG, dec * DEG);
    let star = Star {
        hip,
        dir: Vec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()),
        pm_ra,
        pm_dec,
        mag,
        b_v,
    };
    Some(star.propagated(TARGET_EPOCH - HIPPARCOS_EPOCH))
}

/// One Tycho-2 row. The mean position is given at epoch J2000 already.
///
/// Johnson V and B-V follow from the Tycho magnitudes by the linear
/// relations of the Hipparcos and Tycho Catalogues (ESA SP-1200, vol. 1,
/// sect. 1.3): `V = VT - 0.090 (BT - VT)`, `B-V = 0.850 (BT - VT)`.
fn parse_tycho(line: &str, mag_limit: f64) -> Option<(Star, Option<u32>)> {
    let mut fields = line.split('\t').map(str::trim);
    let tyc1: u32 = fields.next()?.parse().ok()?;
    let tyc2: u32 = fields.next()?.parse().ok()?;
    let tyc3: u32 = fields.next()?.parse().ok()?;
    let _pflag = fields.next()?;
    let ra: f64 = fields.next()?.parse().ok()?;
    let dec: f64 = fields.next()?.parse().ok()?;
    let pm_ra: f64 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0.0);
    let pm_dec: f64 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0.0);
    let bt: Option<f64> = fields.next().and_then(|f| f.parse().ok());
    let vt: f64 = fields.next()?.parse().ok()?;
    let hip: Option<u32> = fields.next().and_then(|f| f.parse().ok());
    let color = bt.map(|bt| bt - vt);
    let mag = vt - 0.090 * color.unwrap_or(0.0);
    if mag > mag_limit {
        return None;
    }
    let (ra, dec) = (ra * DEG, dec * DEG);
    let star = Star {
        hip: tycho_id(tyc1, tyc2, tyc3)?,
        dir: Vec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()),
        pm_ra,
        pm_dec,
        mag,
        b_v: color.map(|c| 0.850 * c),
    };
    Some((star, hip))
}

fn is_data(line: &str) -> bool {
    line.trim_start().starts_with(|c: char| c.is_ascii_digit())
}

fn build_tycho(
    hipparcos: &str,
    tycho: &str,
    output: &str,
    mag_limit: f64,
) -> Result<(), Box<dyn Error>> {
    let mut stars: Vec<Star> = fs::read_to_string(hipparcos)?
        .lines()
        .filter(|line| is_data(line))
        .filter_map(|line| parse_row(line, mag_limit))
        .collect();
    let from_hipparcos = stars.len();
    let mut taken: HashSet<u32> = stars.iter().map(|star| star.hip).collect();
    let mut duplicates = 0usize;
    for line in fs::read_to_string(tycho)?
        .lines()
        .filter(|line| is_data(line))
    {
        let Some((star, hip)) = parse_tycho(line, mag_limit) else {
            continue;
        };
        match hip {
            Some(hip) if taken.contains(&hip) => duplicates += 1,
            Some(hip) => {
                taken.insert(hip);
                stars.push(Star { hip, ..star });
            }
            None => stars.push(star),
        }
    }
    let catalog = Catalog::from_stars(stars, TARGET_EPOCH);
    let bytes = catalog.encode();
    let check = Catalog::parse(&bytes).map_err(|e| e.to_string())?;
    fs::write(output, &bytes)?;
    println!(
        "{} stars (V <= {:.2}): {} from Hipparcos, {} from Tycho-2, {} Tycho-2 rows already in Hipparcos, {} bytes -> {}",
        check.len(),
        check.mag_limit(),
        from_hipparcos,
        check.len() - from_hipparcos,
        duplicates,
        bytes.len(),
        output
    );
    Ok(())
}

fn build_catalog(input: &str, output: &str, mag_limit: f64) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(input)?;
    let mut skipped = 0usize;
    let mut stars = Vec::new();
    for line in text.lines() {
        if !is_data(line) {
            continue;
        }
        match parse_row(line, mag_limit) {
            Some(star) => stars.push(star),
            None => skipped += 1,
        }
    }
    if stars.is_empty() {
        return Err("no catalog rows found in input".into());
    }
    let catalog = Catalog::from_stars(stars, TARGET_EPOCH);
    let bytes = catalog.encode();
    let check = Catalog::parse(&bytes).map_err(|e| e.to_string())?;
    fs::write(output, &bytes)?;
    println!(
        "{} stars (V <= {:.2}), {} rows skipped (missing astrometry or photometry), {} bytes -> {}",
        check.len(),
        check.mag_limit(),
        skipped,
        bytes.len(),
        output
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, input, output] if command == "build-catalog" => {
            build_catalog(input, output, DEFAULT_MAG_LIMIT)
        }
        [command, input, output, limit] if command == "build-catalog" => limit
            .parse()
            .map_err(|_| "the faintest magnitude must be a number".into())
            .and_then(|limit| build_catalog(input, output, limit)),
        [command, hipparcos, tycho, output, limit] if command == "build-tycho" => limit
            .parse()
            .map_err(|_| "the faintest magnitude must be a number".into())
            .and_then(|limit| build_tycho(hipparcos, tycho, output, limit)),
        _ => Err(
            "usage: xtask build-catalog <vizier.tsv> <out.astc> [faintest V]
       xtask build-tycho <hipparcos.tsv> <tycho2.tsv> <out.astc> <faintest V>"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
