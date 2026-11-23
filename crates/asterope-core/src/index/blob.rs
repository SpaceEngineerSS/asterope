//! Asterope Index (`.asti`): a pair index laid out to be searched in place.
//!
//! Header (80 bytes, little-endian): magic `ASTI`, `u16` version, `u16`
//! header length, `u32` star count, `u32` pair count, `u32` k-vector length,
//! `u32` CRC-32 of everything after the header, `f64` k-vector slope and
//! intercept, `f64` largest and smallest indexed separation and blend radius
//! (radians), `u32` catalog prefix length, `u32` pair limit, `u32` CRC-32 of
//! the header with this field as zero, 4 reserved bytes. Star records follow (40 bytes: `u32` catalog index, `u32` HIP id,
//! three `f64` ICRS unit-vector components, `f64` magnitude), then pair
//! records sorted by separation (`f32` separation in radians and two star
//! indices: `u16` in version 1, 8 bytes a record, `u32` in version 2, 12
//! bytes), then the k-vector (`u32` each). Version 1 is written whenever the
//! stars fit 16 bits.

use alloc::vec::Vec;

use super::{IndexConfig, IndexStar, Pair};
use crate::bytes::{crc32, header_crc, header_intact, Reader};
use crate::error::AsteropeError;
use crate::math::{ceil, floor, Vec3};

pub const MAGIC: [u8; 4] = *b"ASTI";
pub const VERSION: u16 = 1;
/// Version with 32-bit star indices, for more than 65 535 stars.
pub const WIDE_VERSION: u16 = 2;
pub const HEADER_LEN: usize = 80;
/// Offset of the header's own check in the header.
const HEADER_CHECK: usize = 72;
pub const STAR_LEN: usize = 40;
pub const PAIR_LEN: usize = 8;
pub const WIDE_PAIR_LEN: usize = 12;
pub const K_LEN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub stars: usize,
    pub pairs: usize,
    pub k: usize,
    pub slope: f64,
    pub intercept: f64,
    pub config: IndexConfig,
    /// Star indices take 32 bits (version 2).
    pub wide: bool,
}

fn saturate(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl Layout {
    fn pair_len(&self) -> usize {
        if self.wide {
            WIDE_PAIR_LEN
        } else {
            PAIR_LEN
        }
    }

    fn payload_len(&self) -> Option<usize> {
        let stars = self.stars.checked_mul(STAR_LEN)?;
        let pairs = self.pairs.checked_mul(self.pair_len())?;
        let k = self.k.checked_mul(K_LEN)?;
        stars.checked_add(pairs)?.checked_add(k)
    }

    pub fn parse(bytes: &[u8], verify_checksum: bool) -> Result<Layout, AsteropeError> {
        let mut header = Reader::new(bytes, AsteropeError::IndexTruncated);
        if header.take::<4>()? != MAGIC {
            return Err(AsteropeError::IndexBadMagic);
        }
        let version = header.u16()?;
        let header_len = usize::from(header.u16()?);
        if !(version == VERSION || version == WIDE_VERSION) || header_len != HEADER_LEN {
            return Err(AsteropeError::IndexUnsupportedVersion);
        }
        let stars = header.u32()? as usize;
        let pairs = header.u32()? as usize;
        let k = header.u32()? as usize;
        let checksum = header.u32()?;
        let slope = header.f64()?;
        let intercept = header.f64()?;
        let max_angle = header.f64()?;
        let min_angle = header.f64()?;
        let blend_radius = header.f64()?;
        let star_count = header.u32()? as usize;
        let max_pairs = header.u32()? as usize;
        header.take::<8>()?;

        let layout = Layout {
            stars,
            pairs,
            k,
            slope,
            intercept,
            config: IndexConfig {
                star_count,
                max_angle,
                min_angle,
                blend_radius,
                max_pairs,
            },
            wide: version == WIDE_VERSION,
        };
        let payload_len = layout.payload_len().ok_or(AsteropeError::IndexTruncated)?;
        let payload = header
            .bytes
            .get(..payload_len)
            .ok_or(AsteropeError::IndexTruncated)?;
        if !header_intact(bytes, HEADER_LEN, HEADER_CHECK) {
            return Err(AsteropeError::IndexChecksumMismatch);
        }
        let too_many = !layout.wide && stars > usize::from(u16::MAX);
        if too_many || (pairs > 0 && k == 0) || !slope.is_finite() {
            return Err(AsteropeError::IndexCorrupt);
        }
        if verify_checksum && crc32(payload) != checksum {
            return Err(AsteropeError::IndexChecksumMismatch);
        }
        Ok(layout)
    }

    pub fn view<'a>(&self, bytes: &'a [u8]) -> IndexView<'a> {
        let stars_end = HEADER_LEN + self.stars * STAR_LEN;
        let pairs_end = stars_end + self.pairs * self.pair_len();
        let k_end = pairs_end + self.k * K_LEN;
        IndexView {
            stars: bytes
                .get(HEADER_LEN..stars_end)
                .unwrap_or(&[])
                .as_chunks()
                .0,
            pairs: PairRange {
                bytes: bytes.get(stars_end..pairs_end).unwrap_or(&[]),
                record_len: self.pair_len(),
            },
            k: bytes.get(pairs_end..k_end).unwrap_or(&[]).as_chunks().0,
            slope: self.slope,
            intercept: self.intercept,
        }
    }
}

pub(super) fn encode(
    stars: &[IndexStar],
    pairs: &[Pair],
    k: &[u32],
    slope: f64,
    intercept: f64,
    config: &IndexConfig,
) -> Vec<u8> {
    let wide = stars.len() > usize::from(u16::MAX);
    let pair_len = if wide { WIDE_PAIR_LEN } else { PAIR_LEN };
    let payload_len = stars.len() * STAR_LEN + pairs.len() * pair_len + k.len() * K_LEN;
    let mut out = Vec::with_capacity(HEADER_LEN + payload_len);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(if wide { WIDE_VERSION } else { VERSION }).to_le_bytes());
    out.extend_from_slice(&(HEADER_LEN as u16).to_le_bytes());
    out.extend_from_slice(&saturate(stars.len()).to_le_bytes());
    out.extend_from_slice(&saturate(pairs.len()).to_le_bytes());
    out.extend_from_slice(&saturate(k.len()).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&slope.to_le_bytes());
    out.extend_from_slice(&intercept.to_le_bytes());
    out.extend_from_slice(&config.max_angle.to_le_bytes());
    out.extend_from_slice(&config.min_angle.to_le_bytes());
    out.extend_from_slice(&config.blend_radius.to_le_bytes());
    out.extend_from_slice(&saturate(config.star_count).to_le_bytes());
    out.extend_from_slice(&saturate(config.max_pairs).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    for star in stars {
        out.extend_from_slice(&star.catalog_index.to_le_bytes());
        out.extend_from_slice(&star.hip.to_le_bytes());
        out.extend_from_slice(&star.dir.x.to_le_bytes());
        out.extend_from_slice(&star.dir.y.to_le_bytes());
        out.extend_from_slice(&star.dir.z.to_le_bytes());
        out.extend_from_slice(&star.mag.to_le_bytes());
    }
    for pair in pairs {
        out.extend_from_slice(&pair.angle.to_le_bytes());
        if wide {
            out.extend_from_slice(&pair.a.to_le_bytes());
            out.extend_from_slice(&pair.b.to_le_bytes());
        } else {
            out.extend_from_slice(&(pair.a as u16).to_le_bytes());
            out.extend_from_slice(&(pair.b as u16).to_le_bytes());
        }
    }
    for count in k {
        out.extend_from_slice(&count.to_le_bytes());
    }
    let checksum = crc32(out.get(HEADER_LEN..).unwrap_or(&[]));
    if let Some(slot) = out.get_mut(20..24) {
        slot.copy_from_slice(&checksum.to_le_bytes());
    }
    if let Some(check) = header_crc(&out, HEADER_LEN, HEADER_CHECK) {
        if let Some(slot) = out.get_mut(HEADER_CHECK..HEADER_CHECK + 4) {
            slot.copy_from_slice(&check.to_le_bytes());
        }
    }
    out
}

fn decode_star(record: &[u8; STAR_LEN]) -> Option<IndexStar> {
    let ([ids, x, y, z, mag], _) = record.as_chunks::<8>() else {
        return None;
    };
    let [c0, c1, c2, c3, h0, h1, h2, h3] = *ids;
    Some(IndexStar {
        catalog_index: u32::from_le_bytes([c0, c1, c2, c3]),
        hip: u32::from_le_bytes([h0, h1, h2, h3]),
        dir: Vec3::new(
            f64::from_le_bytes(*x),
            f64::from_le_bytes(*y),
            f64::from_le_bytes(*z),
        ),
        mag: f64::from_le_bytes(*mag),
    })
}

fn decode_pair(record: &[u8]) -> Option<Pair> {
    match *record {
        [s0, s1, s2, s3, a0, a1, b0, b1] => Some(Pair {
            angle: f32::from_le_bytes([s0, s1, s2, s3]),
            a: u32::from(u16::from_le_bytes([a0, a1])),
            b: u32::from(u16::from_le_bytes([b0, b1])),
        }),
        [s0, s1, s2, s3, a0, a1, a2, a3, b0, b1, b2, b3] => Some(Pair {
            angle: f32::from_le_bytes([s0, s1, s2, s3]),
            a: u32::from_le_bytes([a0, a1, a2, a3]),
            b: u32::from_le_bytes([b0, b1, b2, b3]),
        }),
        _ => None,
    }
}

/// A run of pair records sorted by separation.
#[derive(Clone, Copy, Debug)]
pub struct PairRange<'a> {
    bytes: &'a [u8],
    record_len: usize,
}

impl<'a> PairRange<'a> {
    const EMPTY: PairRange<'static> = PairRange {
        bytes: &[],
        record_len: PAIR_LEN,
    };

    pub fn len(&self) -> usize {
        self.bytes.len() / self.record_len.max(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn record(&self, position: usize) -> Option<&'a [u8]> {
        let start = position.checked_mul(self.record_len)?;
        self.bytes.get(start..start.checked_add(self.record_len)?)
    }

    pub fn get(&self, position: usize) -> Option<Pair> {
        self.record(position).and_then(decode_pair)
    }

    pub fn iter(&self) -> PairIter<'a> {
        PairIter {
            range: *self,
            position: 0,
        }
    }

    fn angle(&self, position: usize) -> Option<f64> {
        match self.record(position)? {
            [s0, s1, s2, s3, ..] => Some(f64::from(f32::from_le_bytes([*s0, *s1, *s2, *s3]))),
            _ => None,
        }
    }

    fn slice(&self, start: usize, end: usize) -> PairRange<'a> {
        PairRange {
            bytes: self
                .bytes
                .get(start * self.record_len..end * self.record_len)
                .unwrap_or(&[]),
            record_len: self.record_len,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PairIter<'a> {
    range: PairRange<'a>,
    position: usize,
}

impl Iterator for PairIter<'_> {
    type Item = Pair;

    fn next(&mut self) -> Option<Pair> {
        let pair = self.range.get(self.position)?;
        self.position += 1;
        Some(pair)
    }
}

impl<'a> IntoIterator for PairRange<'a> {
    type Item = Pair;
    type IntoIter = PairIter<'a>;

    fn into_iter(self) -> PairIter<'a> {
        self.iter()
    }
}

/// Borrowed sections of an index, decoded record by record on access.
#[derive(Clone, Copy, Debug)]
pub struct IndexView<'a> {
    stars: &'a [[u8; STAR_LEN]],
    pairs: PairRange<'a>,
    k: &'a [[u8; K_LEN]],
    slope: f64,
    intercept: f64,
}

impl<'a> IndexView<'a> {
    pub fn star_count(&self) -> usize {
        self.stars.len()
    }

    pub fn star(&self, index: u32) -> Option<IndexStar> {
        self.stars.get(index as usize).and_then(decode_star)
    }

    pub fn stars(&self) -> impl Iterator<Item = IndexStar> + 'a {
        self.stars.iter().filter_map(decode_star)
    }

    pub fn pairs(&self) -> PairRange<'a> {
        self.pairs
    }

    fn k_at(&self, bin: usize) -> Option<usize> {
        self.k.get(bin).map(|v| u32::from_le_bytes(*v) as usize)
    }

    /// All pairs with `lo <= angle <= hi`, located through the k-vector.
    pub fn range(&self, lo: f64, hi: f64) -> PairRange<'a> {
        let empty = PairRange::EMPTY;
        let n = self.pairs.len();
        let (Some(first), Some(last)) = (self.pairs.angle(0), self.pairs.angle(n.wrapping_sub(1)))
        else {
            return empty;
        };
        if !(lo <= hi) || hi < first || lo > last {
            return empty;
        }
        let bins = self.k.len();
        let (start, end) = if bins < 2 || !(self.slope > 0.0) {
            (0, n)
        } else {
            let top = (bins - 1) as f64;
            let bin_lo = (floor((lo - self.intercept) / self.slope) - 1.0).clamp(0.0, top) as usize;
            let bin_hi = ceil((hi - self.intercept) / self.slope).clamp(0.0, top) as usize;
            (
                self.k_at(bin_lo).unwrap_or(0),
                self.k_at(bin_hi).unwrap_or(n),
            )
        };
        let mut end = end.min(n);
        let mut start = start.min(end);
        while start < end && self.pairs.angle(start).is_some_and(|angle| angle < lo) {
            start += 1;
        }
        while end > start && self.pairs.angle(end - 1).is_some_and(|angle| angle > hi) {
            end -= 1;
        }
        self.pairs.slice(start, end)
    }
}
