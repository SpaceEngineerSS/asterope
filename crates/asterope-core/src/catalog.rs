//! Asterope Catalog v1 (`.astc`): a compact little-endian star table.
//!
//! Header (32 bytes): magic `ASTC`, `u16` version, `u16` record size, `u32`
//! count, `f32` epoch year, `f32` magnitude limit, `u32` CRC-32 of the record
//! payload, `u32` CRC-32 of the header with this field as zero, 4 reserved
//! bytes. Records (24 bytes): `u32` HIP id, three `f32`
//! ICRS unit-vector components, `i16` proper motions in RA*cos(dec) and Dec
//! (0.25 mas/yr units), `i16` V magnitude and `i16` B-V (millimagnitudes).
//! Records are sorted by ascending magnitude, so any prefix is a brighter
//! sub-catalog.

use alloc::vec::Vec;

pub use crate::bytes::crc32;
use crate::bytes::{header_crc, header_intact, Reader};
use crate::error::AsteropeError;
use crate::math::{round, Vec3, ARCSEC};

pub const MAGIC: [u8; 4] = *b"ASTC";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 32;
pub const RECORD_LEN: usize = 24;
/// Offset of the header's own check in the header.
const HEADER_CHECK: usize = 24;
const PM_UNIT_MAS: f64 = 0.25;
const B_V_MISSING: i16 = i16::MIN;

/// Identifiers with this bit set name a Tycho-2 star, not a Hipparcos one.
const TYCHO_FLAG: u32 = 1 << 31;

/// Packs a Tycho-2 designation `TYC1-TYC2-TYC3` into a star identifier:
/// the flag bit, then 14 bits of `TYC1`, 14 of `TYC2` and 2 of `TYC3`.
pub fn tycho_id(tyc1: u32, tyc2: u32, tyc3: u32) -> Option<u32> {
    (tyc1 < 1 << 14 && tyc2 < 1 << 14 && tyc3 < 1 << 2)
        .then_some(TYCHO_FLAG | tyc1 << 16 | tyc2 << 2 | tyc3)
}

/// A star identifier as astronomers write it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Designation {
    Hipparcos(u32),
    Tycho(u32, u32, u32),
}

impl Designation {
    pub fn of(id: u32) -> Designation {
        if id & TYCHO_FLAG == 0 {
            Designation::Hipparcos(id)
        } else {
            Designation::Tycho(id >> 16 & 0x3fff, id >> 2 & 0x3fff, id & 3)
        }
    }
}

impl core::fmt::Display for Designation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Designation::Hipparcos(number) => write!(f, "HIP {number}"),
            Designation::Tycho(a, b, c) => write!(f, "TYC {a}-{b}-{c}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// Hipparcos number, or a packed Tycho-2 designation (see [`tycho_id`]).
    pub hip: u32,
    /// ICRS unit vector at the catalog epoch.
    pub dir: Vec3,
    /// Proper motion in RA * cos(dec), mas/yr.
    pub pm_ra: f64,
    /// Proper motion in declination, mas/yr.
    pub pm_dec: f64,
    pub mag: f64,
    pub b_v: Option<f64>,
}

impl Star {
    /// Linear proper-motion propagation on the tangent plane.
    pub fn propagated(&self, years: f64) -> Star {
        let east = match Vec3::Z.cross(self.dir).normalized() {
            Some(e) => e,
            None => return *self,
        };
        let north = self.dir.cross(east);
        let mas = ARCSEC / 1000.0;
        let shifted = self.dir
            + east.scale(self.pm_ra * mas * years)
            + north.scale(self.pm_dec * mas * years);
        Star {
            dir: shifted.normalized().unwrap_or(self.dir),
            ..*self
        }
    }
}

#[derive(Clone, Debug)]
pub struct Catalog {
    stars: Vec<Star>,
    epoch_year: f64,
    mag_limit: f64,
}

fn quantize_i16(value: f64) -> i16 {
    let rounded = round(value);
    if rounded >= f64::from(i16::MAX) {
        i16::MAX
    } else if rounded <= f64::from(i16::MIN + 1) {
        i16::MIN + 1
    } else {
        rounded as i16
    }
}

impl Catalog {
    /// Builds a catalog from stars, sorting them by ascending magnitude.
    pub fn from_stars(mut stars: Vec<Star>, epoch_year: f64) -> Catalog {
        stars.sort_by(|a, b| a.mag.total_cmp(&b.mag).then(a.hip.cmp(&b.hip)));
        let mag_limit = stars.last().map_or(0.0, |s| s.mag);
        Catalog {
            stars,
            epoch_year,
            mag_limit,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Catalog, AsteropeError> {
        let mut header = Reader::new(bytes, AsteropeError::CatalogTruncated);
        if header.take::<4>()? != MAGIC {
            return Err(AsteropeError::CatalogBadMagic);
        }
        let version = header.u16()?;
        let record_len = usize::from(header.u16()?);
        if version != VERSION || record_len != RECORD_LEN {
            return Err(AsteropeError::CatalogUnsupportedVersion);
        }
        let count = header.u32()? as usize;
        let epoch_year = f64::from(header.f32()?);
        let mag_limit = f64::from(header.f32()?);
        let checksum = header.u32()?;
        header.take::<8>()?;
        if !header_intact(bytes, HEADER_LEN, HEADER_CHECK) {
            return Err(AsteropeError::CatalogChecksumMismatch);
        }

        let payload_len = count
            .checked_mul(RECORD_LEN)
            .ok_or(AsteropeError::CatalogTruncated)?;
        let payload = header
            .bytes
            .get(..payload_len)
            .ok_or(AsteropeError::CatalogTruncated)?;
        if crc32(payload) != checksum {
            return Err(AsteropeError::CatalogChecksumMismatch);
        }

        let mut records = Reader::new(payload, AsteropeError::CatalogTruncated);
        let mut stars = Vec::with_capacity(count);
        for _ in 0..count {
            let hip = records.u32()?;
            let x = f64::from(records.f32()?);
            let y = f64::from(records.f32()?);
            let z = f64::from(records.f32()?);
            let pm_ra = f64::from(records.i16()?) * PM_UNIT_MAS;
            let pm_dec = f64::from(records.i16()?) * PM_UNIT_MAS;
            let mag = f64::from(records.i16()?) / 1000.0;
            let b_v_raw = records.i16()?;
            let dir = Vec3::new(x, y, z)
                .normalized()
                .ok_or(AsteropeError::CatalogTruncated)?;
            stars.push(Star {
                hip,
                dir,
                pm_ra,
                pm_dec,
                mag,
                b_v: (b_v_raw != B_V_MISSING).then(|| f64::from(b_v_raw) / 1000.0),
            });
        }
        Ok(Catalog {
            stars,
            epoch_year,
            mag_limit,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(self.stars.len() * RECORD_LEN);
        for star in &self.stars {
            payload.extend_from_slice(&star.hip.to_le_bytes());
            payload.extend_from_slice(&(star.dir.x as f32).to_le_bytes());
            payload.extend_from_slice(&(star.dir.y as f32).to_le_bytes());
            payload.extend_from_slice(&(star.dir.z as f32).to_le_bytes());
            payload.extend_from_slice(&quantize_i16(star.pm_ra / PM_UNIT_MAS).to_le_bytes());
            payload.extend_from_slice(&quantize_i16(star.pm_dec / PM_UNIT_MAS).to_le_bytes());
            payload.extend_from_slice(&quantize_i16(star.mag * 1000.0).to_le_bytes());
            let b_v = star.b_v.map_or(B_V_MISSING, |v| quantize_i16(v * 1000.0));
            payload.extend_from_slice(&b_v.to_le_bytes());
        }
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(RECORD_LEN as u16).to_le_bytes());
        out.extend_from_slice(&(self.stars.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.epoch_year as f32).to_le_bytes());
        out.extend_from_slice(&(self.mag_limit as f32).to_le_bytes());
        out.extend_from_slice(&crc32(&payload).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&payload);
        if let Some(check) = header_crc(&out, HEADER_LEN, HEADER_CHECK) {
            if let Some(slot) = out.get_mut(HEADER_CHECK..HEADER_CHECK + 4) {
                slot.copy_from_slice(&check.to_le_bytes());
            }
        }
        out
    }

    pub fn stars(&self) -> &[Star] {
        &self.stars
    }

    pub fn len(&self) -> usize {
        self.stars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stars.is_empty()
    }

    pub fn epoch_year(&self) -> f64 {
        self.epoch_year
    }

    pub fn mag_limit(&self) -> f64 {
        self.mag_limit
    }

    /// Number of leading stars with magnitude `<= mag`.
    pub fn count_brighter_than(&self, mag: f64) -> usize {
        self.stars.partition_point(|s| s.mag <= mag)
    }

    /// The stars no fainter than `mag`; indices into the result agree with
    /// indices into `self`.
    pub fn brighter_than(&self, mag: f64) -> Catalog {
        let stars: Vec<Star> = self
            .stars
            .iter()
            .take(self.count_brighter_than(mag))
            .copied()
            .collect();
        Catalog {
            mag_limit: stars.last().map_or(0.0, |s| s.mag),
            stars,
            epoch_year: self.epoch_year,
        }
    }

    /// Shifts every star by the aberration of an observer whose velocity
    /// changed by `beta = v / c` (see `ephem`). Shifts add to first order,
    /// so a later call with the change in velocity keeps the catalog current
    /// without a second copy.
    pub fn aberrate(&mut self, beta: Vec3) {
        for star in &mut self.stars {
            star.dir = crate::ephem::aberrate(star.dir, beta);
        }
    }

    /// Moves every star to where the atmosphere shows it to an observer
    /// whose zenith is `zenith` (ICRS axes; see `solar::zenith`). An
    /// attitude solved against the result refers to the true sky.
    pub fn refract(&mut self, zenith: Vec3) {
        for star in &mut self.stars {
            star.dir = crate::solar::add_refraction(star.dir, zenith);
        }
    }

    /// Catalog with proper motion applied up to `epoch_year`.
    pub fn at_epoch(&self, epoch_year: f64) -> Catalog {
        let years = epoch_year - self.epoch_year;
        Catalog {
            stars: self.stars.iter().map(|s| s.propagated(years)).collect(),
            epoch_year,
            mag_limit: self.mag_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Catalog {
        Catalog::from_stars(
            vec![
                Star {
                    hip: 32349,
                    dir: Vec3::new(-0.1875, 0.9392, -0.2876).normalized().unwrap(),
                    pm_ra: -546.01,
                    pm_dec: -1223.08,
                    mag: -1.44,
                    b_v: Some(0.009),
                },
                Star {
                    hip: 57939,
                    dir: Vec3::new(-0.78, 0.05, 0.62).normalized().unwrap(),
                    pm_ra: 4003.69,
                    pm_dec: -5813.0,
                    mag: 6.42,
                    b_v: None,
                },
            ],
            2000.0,
        )
    }

    #[test]
    fn crc_matches_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip() {
        let catalog = sample();
        let parsed = Catalog::parse(&catalog.encode()).unwrap();
        assert_eq!(parsed.len(), 2);
        let (a, b) = (parsed.stars()[1], catalog.stars()[1]);
        assert_eq!(a.hip, 57939);
        assert!(a.dir.angle_to(b.dir) < 1e-7);
        assert!((a.pm_dec - b.pm_dec).abs() <= 0.125);
        assert!((a.mag - b.mag).abs() < 1e-3);
        assert_eq!(a.b_v, None);
        assert_eq!(parsed.count_brighter_than(0.0), 1);
    }

    #[test]
    fn rejects_corruption_without_panicking() {
        let bytes = sample().encode();
        assert_eq!(
            Catalog::parse(&bytes[..10]).unwrap_err(),
            AsteropeError::CatalogTruncated
        );
        assert_eq!(
            Catalog::parse(&bytes[..bytes.len() - 1]).unwrap_err(),
            AsteropeError::CatalogTruncated
        );
        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert_eq!(
            Catalog::parse(&bad_magic).unwrap_err(),
            AsteropeError::CatalogBadMagic
        );
        let mut flipped = bytes.clone();
        flipped[HEADER_LEN + 5] ^= 0x10;
        assert_eq!(
            Catalog::parse(&flipped).unwrap_err(),
            AsteropeError::CatalogChecksumMismatch
        );
        let mut version = bytes;
        version[4] = 9;
        assert_eq!(
            Catalog::parse(&version).unwrap_err(),
            AsteropeError::CatalogUnsupportedVersion
        );
    }

    #[test]
    fn tycho_designations_survive_packing() {
        use alloc::string::ToString;
        let id = tycho_id(9537, 12121, 3).unwrap();
        assert_eq!(Designation::of(id), Designation::Tycho(9537, 12121, 3));
        assert_eq!(Designation::of(id).to_string(), "TYC 9537-12121-3");
        assert_eq!(Designation::of(118_322).to_string(), "HIP 118322");
        assert_eq!(tycho_id(1 << 14, 1, 1), None);
        assert_eq!(tycho_id(1, 1, 4), None);
    }

    #[test]
    fn proper_motion_moves_star_by_expected_angle() {
        let star = sample().stars()[1];
        let moved = star.propagated(10.0);
        let expected = (4003.69f64.hypot(5813.0)) * 10.0 / 1000.0 * ARCSEC;
        assert!((moved.dir.angle_to(star.dir) - expected).abs() < 1e-9);
    }
}
