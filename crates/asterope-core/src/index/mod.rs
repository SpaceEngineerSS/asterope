//! Angular-separation pair index with k-vector range search
//! (Mortari & Neta, "k-vector range searching techniques").

pub mod blob;

use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;

use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::error::AsteropeError;
use crate::math::{ceil, cos, sin, Vec3};

use blob::Layout;
pub use blob::{IndexView, PairRange};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexStar {
    pub catalog_index: u32,
    pub hip: u32,
    pub dir: Vec3,
    pub mag: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub angle: f32,
    /// Positions of the two stars among the index stars, `a < b`.
    pub a: u32,
    pub b: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexConfig {
    /// Number of brightest catalog stars considered for patterns.
    pub star_count: usize,
    /// Largest indexed separation, radians.
    pub max_angle: f64,
    /// Smallest indexed separation, radians.
    pub min_angle: f64,
    /// Stars with a comparably bright neighbor closer than this are excluded
    /// from patterns because their images merge on the sensor.
    pub blend_radius: f64,
    pub max_pairs: usize,
}

impl IndexConfig {
    /// Settings that put about `stars_per_field` catalog stars on the sensor
    /// on average and index every separation up to the frame diagonal.
    pub fn for_camera(
        catalog: &Catalog,
        camera: &Camera,
        stars_per_field: f64,
        blend_pixels: f64,
    ) -> Result<IndexConfig, AsteropeError> {
        let sky_fraction = camera.solid_angle() / (4.0 * crate::math::PI);
        if !(sky_fraction > 0.0) || !(stars_per_field > 0.0) {
            return Err(AsteropeError::InvalidParameter);
        }
        Ok(IndexConfig {
            star_count: (ceil(stars_per_field / sky_fraction) as usize)
                .clamp(64, MAX_INDEX_STARS)
                .min(catalog.len()),
            max_angle: camera.fov_diagonal(),
            min_angle: 2.0 * blend_pixels * camera.pixel_scale(),
            blend_radius: blend_pixels * camera.pixel_scale(),
            max_pairs: 3_000_000,
        })
    }
}

/// Pair index over its serialized form (`blob`): built in memory, or borrowed
/// from bytes that were written ahead of time and are searched in place.
#[derive(Clone, Debug)]
pub struct PairIndex<'a> {
    bytes: Cow<'a, [u8]>,
    layout: Layout,
}

/// One k-vector bin per this many pairs.
const K_VECTOR_STRIDE: usize = 4;
const MAX_INDEX_STARS: usize = 4_000_000;
const BLEND_MAGNITUDE_SPAN: f64 = 3.0;

fn chord(angle: f64) -> f64 {
    2.0 * sin(0.5 * angle)
}

/// Marks stars that have a neighbor within `radius` no more than
/// `BLEND_MAGNITUDE_SPAN` magnitudes fainter.
fn blended(catalog: &Catalog, count: usize, radius: f64) -> Vec<bool> {
    let stars = catalog.stars();
    let mut order: Vec<u32> = (0..stars.len() as u32).collect();
    order.sort_by(|a, b| {
        let za = stars.get(*a as usize).map_or(0.0, |s| s.dir.z);
        let zb = stars.get(*b as usize).map_or(0.0, |s| s.dir.z);
        za.total_cmp(&zb)
    });
    let mut flags = vec![false; count];
    let cos_radius = cos(radius);
    let band = chord(radius);
    for (position, first) in order.iter().enumerate() {
        let Some(a) = stars.get(*first as usize) else {
            continue;
        };
        for second in order.iter().skip(position + 1) {
            let Some(b) = stars.get(*second as usize) else {
                continue;
            };
            if b.dir.z - a.dir.z > band {
                break;
            }
            if a.dir.dot(b.dir) < cos_radius {
                continue;
            }
            if b.mag <= a.mag + BLEND_MAGNITUDE_SPAN {
                if let Some(flag) = flags.get_mut(*first as usize) {
                    *flag = true;
                }
            }
            if a.mag <= b.mag + BLEND_MAGNITUDE_SPAN {
                if let Some(flag) = flags.get_mut(*second as usize) {
                    *flag = true;
                }
            }
        }
    }
    flags
}

impl PairIndex<'static> {
    pub fn build(
        catalog: &Catalog,
        config: &IndexConfig,
    ) -> Result<PairIndex<'static>, AsteropeError> {
        if !(config.max_angle > config.min_angle)
            || !(config.min_angle >= 0.0)
            || !(config.max_angle < 3.0)
            || !(config.blend_radius >= 0.0)
        {
            return Err(AsteropeError::InvalidParameter);
        }
        let count = config.star_count.min(catalog.len());
        let blend_flags = blended(catalog, count, config.blend_radius);
        let stars: Vec<IndexStar> = catalog
            .stars()
            .iter()
            .take(count)
            .enumerate()
            .filter(|(i, _)| !blend_flags.get(*i).copied().unwrap_or(false))
            .map(|(i, s)| IndexStar {
                catalog_index: i as u32,
                hip: s.hip,
                dir: s.dir,
                mag: s.mag,
            })
            .collect();
        if stars.len() > MAX_INDEX_STARS {
            return Err(AsteropeError::IndexTooLarge);
        }

        let mut order: Vec<u32> = (0..stars.len() as u32).collect();
        order.sort_by(|a, b| {
            let za = stars.get(*a as usize).map_or(0.0, |s| s.dir.z);
            let zb = stars.get(*b as usize).map_or(0.0, |s| s.dir.z);
            za.total_cmp(&zb)
        });
        let cos_max = cos(config.max_angle);
        let band = chord(config.max_angle);
        let mut pairs = Vec::new();
        for (position, first) in order.iter().enumerate() {
            let Some(a) = stars.get(*first as usize) else {
                continue;
            };
            for second in order.iter().skip(position + 1) {
                let Some(b) = stars.get(*second as usize) else {
                    continue;
                };
                if b.dir.z - a.dir.z > band {
                    break;
                }
                if a.dir.dot(b.dir) < cos_max {
                    continue;
                }
                let angle = a.dir.angle_to(b.dir);
                if angle < config.min_angle || angle > config.max_angle {
                    continue;
                }
                if pairs.len() >= config.max_pairs {
                    return Err(AsteropeError::IndexTooLarge);
                }
                pairs.push(Pair {
                    angle: angle as f32,
                    a: (*first).min(*second),
                    b: (*first).max(*second),
                });
            }
        }
        pairs.sort_unstable_by(|p, q| {
            p.angle
                .total_cmp(&q.angle)
                .then(p.a.cmp(&q.a))
                .then(p.b.cmp(&q.b))
        });

        let (k, slope, intercept) = build_k_vector(&pairs);
        let bytes = blob::encode(&stars, &pairs, &k, slope, intercept, config);
        let layout = Layout::parse(&bytes, false)?;
        Ok(PairIndex {
            bytes: Cow::Owned(bytes),
            layout,
        })
    }

    /// Index sized for a camera: the magnitude cut is chosen so that about
    /// `stars_per_field` catalog stars fall on the sensor on average.
    pub fn for_camera(
        catalog: &Catalog,
        camera: &Camera,
        stars_per_field: f64,
        blend_pixels: f64,
    ) -> Result<PairIndex<'static>, AsteropeError> {
        let mut config = IndexConfig::for_camera(catalog, camera, stars_per_field, blend_pixels)?;
        loop {
            match PairIndex::build(catalog, &config) {
                Err(AsteropeError::IndexTooLarge) if config.star_count > 64 => {
                    config.star_count = (config.star_count * 4 / 5).max(64);
                }
                other => return other,
            }
        }
    }
}

impl<'a> PairIndex<'a> {
    /// Reads an index written by [`PairIndex::as_bytes`] without copying it.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<PairIndex<'a>, AsteropeError> {
        Ok(PairIndex {
            layout: Layout::parse(bytes, true)?,
            bytes: Cow::Borrowed(bytes),
        })
    }

    /// Like [`PairIndex::from_bytes`] but skips the checksum pass over the
    /// whole index, for storage whose integrity is checked elsewhere.
    pub fn from_bytes_unverified(bytes: &'a [u8]) -> Result<PairIndex<'a>, AsteropeError> {
        Ok(PairIndex {
            layout: Layout::parse(bytes, false)?,
            bytes: Cow::Borrowed(bytes),
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_owned(self) -> PairIndex<'static> {
        PairIndex {
            bytes: Cow::Owned(self.bytes.into_owned()),
            layout: self.layout,
        }
    }

    pub fn view(&self) -> IndexView<'_> {
        self.layout.view(&self.bytes)
    }

    pub fn star_count(&self) -> usize {
        self.layout.stars
    }

    pub fn pair_count(&self) -> usize {
        self.layout.pairs
    }

    pub fn stars(&self) -> impl Iterator<Item = IndexStar> + '_ {
        self.view().stars()
    }

    pub fn star(&self, index: u32) -> Option<IndexStar> {
        self.view().star(index)
    }

    pub fn pairs(&self) -> PairRange<'_> {
        self.view().pairs()
    }

    pub fn config(&self) -> &IndexConfig {
        &self.layout.config
    }

    /// Faintest magnitude among the indexed stars.
    pub fn mag_limit(&self) -> f64 {
        let last = u32::try_from(self.layout.stars.saturating_sub(1)).unwrap_or(u32::MAX);
        self.star(last).map_or(0.0, |s| s.mag)
    }

    /// All pairs with `lo <= angle <= hi`, located through the k-vector.
    pub fn range(&self, lo: f64, hi: f64) -> PairRange<'_> {
        self.view().range(lo, hi)
    }
}

/// `k[i]` counts the sorted values not exceeding the line `z(i) = slope * i + intercept`,
/// which runs from just below the smallest value to just above the largest
/// in one step per `K_VECTOR_STRIDE` values.
fn build_k_vector(pairs: &[Pair]) -> (Vec<u32>, f64, f64) {
    let n = pairs.len();
    let (Some(first), Some(last)) = (pairs.first(), pairs.last()) else {
        return (Vec::new(), 0.0, 0.0);
    };
    if n < 2 {
        return (vec![1], 0.0, f64::from(first.angle));
    }
    let bins = n.div_ceil(K_VECTOR_STRIDE).max(2);
    let y_min = f64::from(first.angle);
    let y_max = f64::from(last.angle);
    let margin = 1e-9 * y_max.max(1e-6);
    let slope = (y_max - y_min + 2.0 * margin) / (bins - 1) as f64;
    let intercept = y_min - margin;
    let mut k = Vec::with_capacity(bins);
    let mut count = 0usize;
    for i in 0..bins {
        let z = slope * i as f64 + intercept;
        while pairs.get(count).is_some_and(|p| f64::from(p.angle) <= z) {
            count += 1;
        }
        k.push(count as u32);
    }
    if let Some(tail) = k.last_mut() {
        *tail = n as u32;
    }
    (k, slope, intercept)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Star;
    use crate::math::DEG;
    use crate::rng::Rng;

    fn random_catalog(count: usize, seed: u64) -> Catalog {
        let mut rng = Rng::new(seed);
        let stars = (0..count)
            .map(|i| {
                let z = rng.range(-1.0, 1.0);
                let phi = rng.range(0.0, crate::math::TAU);
                let r = (1.0 - z * z).sqrt();
                Star {
                    hip: i as u32 + 1,
                    dir: Vec3::new(r * phi.cos(), r * phi.sin(), z),
                    pm_ra: 0.0,
                    pm_dec: 0.0,
                    mag: rng.range(0.0, 6.5),
                    b_v: None,
                }
            })
            .collect();
        Catalog::from_stars(stars, 2000.0)
    }

    fn config() -> IndexConfig {
        IndexConfig {
            star_count: 1500,
            max_angle: 15.0 * DEG,
            min_angle: 0.05 * DEG,
            blend_radius: 0.02 * DEG,
            max_pairs: 1_000_000,
        }
    }

    #[test]
    fn index_contains_exactly_the_pairs_in_range() {
        let catalog = random_catalog(1500, 3);
        let index = PairIndex::build(&catalog, &config()).unwrap();
        let stars: Vec<IndexStar> = index.stars().collect();
        let mut expected = 0usize;
        for i in 0..stars.len() {
            for j in (i + 1)..stars.len() {
                let angle = stars[i].dir.angle_to(stars[j].dir);
                if (0.05 * DEG..=15.0 * DEG).contains(&angle) {
                    expected += 1;
                }
            }
        }
        assert_eq!(index.pair_count(), expected);
        let pairs: Vec<Pair> = index.pairs().iter().collect();
        assert!(pairs.windows(2).all(|w| w[0].angle <= w[1].angle));
    }

    #[test]
    fn more_stars_than_sixteen_bits_hold_use_the_wide_records() {
        let catalog = random_catalog(70_000, 5);
        let wide = IndexConfig {
            star_count: 70_000,
            max_angle: 0.6 * DEG,
            ..config()
        };
        let index = PairIndex::build(&catalog, &wide).unwrap();
        assert!(index.star_count() > usize::from(u16::MAX));
        assert_eq!(index.as_bytes()[4..6], blob::WIDE_VERSION.to_le_bytes());
        let reread = PairIndex::from_bytes(index.as_bytes()).unwrap();
        assert_eq!(reread.pair_count(), index.pair_count());
        let mut beyond = false;
        for pair in reread.pairs() {
            let (a, b) = (reread.star(pair.a).unwrap(), reread.star(pair.b).unwrap());
            assert!((a.dir.angle_to(b.dir) - f64::from(pair.angle)).abs() < 1e-6);
            beyond |= pair.b > u32::from(u16::MAX);
        }
        assert!(beyond);

        let narrow = PairIndex::build(&random_catalog(1500, 3), &config()).unwrap();
        assert_eq!(narrow.as_bytes()[4..6], blob::VERSION.to_le_bytes());
    }

    #[test]
    fn k_vector_range_matches_brute_force() {
        let catalog = random_catalog(1500, 9);
        let index = PairIndex::build(&catalog, &config()).unwrap();
        let mut rng = Rng::new(17);
        for _ in 0..10_000 {
            let center = rng.range(-1.0, 17.0) * DEG;
            let half = rng.range(0.0, 0.05) * DEG * if rng.uniform() < 0.1 { 40.0 } else { 1.0 };
            let (lo, hi) = (center - half, center + half);
            let brute: Vec<Pair> = index
                .pairs()
                .iter()
                .filter(|p| f64::from(p.angle) >= lo && f64::from(p.angle) <= hi)
                .collect();
            let found: Vec<Pair> = index.range(lo, hi).iter().collect();
            assert_eq!(found, brute, "range [{lo}, {hi}]");
        }
        let probe = index.pairs().get(100).unwrap();
        let exact = f64::from(probe.angle);
        assert!(index.range(exact, exact).iter().any(|p| p == probe));
        assert!(index.range(1.0, 0.5).is_empty());
    }

    #[test]
    fn serialized_index_is_searched_in_place() {
        let catalog = random_catalog(1500, 9);
        let built = PairIndex::build(&catalog, &config()).unwrap();
        let bytes = built.as_bytes().to_vec();
        let loaded = PairIndex::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.star_count(), built.star_count());
        assert_eq!(loaded.pair_count(), built.pair_count());
        assert_eq!(loaded.config(), built.config());
        assert!(loaded.stars().eq(built.stars()));
        let mut rng = Rng::new(23);
        for _ in 0..2_000 {
            let center = rng.range(-1.0, 17.0) * DEG;
            let half = rng.range(0.0, 0.5) * DEG;
            assert!(loaded
                .range(center - half, center + half)
                .iter()
                .eq(built.range(center - half, center + half).iter()));
        }
    }

    #[test]
    fn damaged_index_bytes_are_rejected() {
        let catalog = random_catalog(300, 5);
        let bytes = PairIndex::build(&catalog, &config())
            .unwrap()
            .as_bytes()
            .to_vec();
        let error = |bytes: &[u8]| PairIndex::from_bytes(bytes).unwrap_err();
        assert_eq!(error(&bytes[..40]), AsteropeError::IndexTruncated);
        assert_eq!(
            error(&bytes[..bytes.len() - 1]),
            AsteropeError::IndexTruncated
        );
        let mut damaged = bytes.clone();
        damaged[0] = b'X';
        assert_eq!(error(&damaged), AsteropeError::IndexBadMagic);
        let mut damaged = bytes.clone();
        damaged[4] = 9;
        assert_eq!(error(&damaged), AsteropeError::IndexUnsupportedVersion);
        let mut damaged = bytes.clone();
        damaged[blob::HEADER_LEN + 11] ^= 0x04;
        assert_eq!(error(&damaged), AsteropeError::IndexChecksumMismatch);
        assert!(PairIndex::from_bytes_unverified(&damaged).is_ok());
    }

    #[test]
    fn close_pairs_are_excluded_as_blends() {
        let mut stars: Vec<Star> = random_catalog(200, 4).stars().to_vec();
        let mut twin = stars[10];
        twin.hip = 9999;
        twin.dir = (twin.dir + Vec3::new(1e-5, 0.0, 0.0)).normalized().unwrap();
        stars.push(twin);
        let catalog = Catalog::from_stars(stars, 2000.0);
        let index = PairIndex::build(
            &catalog,
            &IndexConfig {
                star_count: 201,
                ..config()
            },
        )
        .unwrap();
        assert_eq!(index.star_count(), 199);
        assert!(index.stars().all(|s| s.hip != 9999));
    }

    #[test]
    fn oversized_index_is_reported() {
        let catalog = random_catalog(1500, 3);
        let result = PairIndex::build(
            &catalog,
            &IndexConfig {
                max_pairs: 10,
                ..config()
            },
        );
        assert_eq!(result.unwrap_err(), AsteropeError::IndexTooLarge);
    }
}
