//! Predicting what a camera will do as a star tracker, before it exists.
//!
//! From a lens, a detector and an exposure this module estimates the
//! faintest detectable star, how many stars a frame holds and over what
//! part of the sky that is enough, the attitude accuracy, and the rotation
//! rate at which stars trail into streaks. Every estimate is a closed
//! formula over the embedded catalog; `docs/validation.md` compares them
//! with the simulator.
//!
//! Photometry assumes a star of V = 0 delivers 1005 photons per second, per
//! square centimeter and per angstrom at 550 nm (Bessell 1979), constant
//! across the passband. That is exact for no real star and within some
//! tens of percent for most; the colour term of the simulator is not
//! modelled here.

use alloc::vec::Vec;

use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::error::AsteropeError;
use crate::math::{atan, cos, erf, log10, pow, sin, sqrt, Vec3, ARCSEC, DEG, PI, TAU};

/// Photons per second, per square centimeter and per angstrom from a V = 0
/// star at 550 nm.
pub const V0_PHOTON_FLUX: f64 = 1005.0;
/// Trail length, in PSF sigmas, at which the detector's default elongation
/// limit of 2.5 rejects a star: a box of length `L` adds `L^2 / 12` to the
/// variance along the trail, and `sqrt(1 + L^2 / (12 sigma^2)) = 2.5` gives
/// `L = 7.94 sigma`.
pub const STREAK_LIMIT_SIGMAS: f64 = 7.94;
/// The detector compares the brightest pixel of a blob with the threshold,
/// and the brightest of several noisy pixels sits above its own mean: the
/// expected maximum of five unit normals is 1.163. The threshold on the mean
/// peak is lower by that much.
const PEAK_SELECTION_BIAS: f64 = 1.163;
/// Centroid error that remains for a bright star, pixels.
const CENTROID_FLOOR_PX: f64 = 0.01;
/// Ratio of the windowed centroid's error to the bound used below, measured
/// at a peak signal-to-noise ratio of 30 (`docs/validation.md`, suite 1).
const CENTROID_EXCESS: f64 = 1.3;
/// Star counts at which the sky is tallied: the four of a pyramid, the five
/// matches the solver accepts by default, and a comfortable margin.
pub const COVERAGE_LEVELS: [u16; 3] = [4, 5, 8];
pub const COVERAGE_COLUMNS: usize = 72;
pub const COVERAGE_ROWS: usize = 36;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Optics {
    pub focal_length_mm: f64,
    /// Diameter of the entrance pupil.
    pub aperture_mm: f64,
    pub pixel_um: f64,
    pub width: u32,
    pub height: u32,
    /// Fraction of the light that passes the lens.
    pub transmission: f64,
    /// Mean quantum efficiency over the passband.
    pub quantum_efficiency: f64,
    /// Width of the passband, nanometers.
    pub bandwidth_nm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    pub seconds: f64,
    /// Gaussian PSF standard deviation, pixels.
    pub psf_sigma_px: f64,
    /// Read noise, electrons RMS.
    pub read_noise: f64,
    /// Dark current, electrons per pixel per second.
    pub dark_current: f64,
    /// Sky background, electrons per pixel per second.
    pub sky_background: f64,
    /// Rotation rate across the boresight during the exposure, radians per second.
    pub body_rate: f64,
    /// Detection threshold on the peak pixel, in background sigmas.
    pub peak_sigma: f64,
}

impl Optics {
    /// Electrons per second from a V = 0 star.
    pub fn zero_point(&self) -> f64 {
        let radius_cm = 0.05 * self.aperture_mm;
        V0_PHOTON_FLUX
            * PI
            * radius_cm
            * radius_cm
            * (10.0 * self.bandwidth_nm)
            * self.transmission
            * self.quantum_efficiency
    }

    /// Pinhole camera with the principal point at the frame center.
    pub fn camera(&self) -> Result<Camera, AsteropeError> {
        if !(self.focal_length_mm > 0.0) || !(self.pixel_um > 0.0) {
            return Err(AsteropeError::InvalidCamera);
        }
        let half_width_mm = 0.5e-3 * self.pixel_um * f64::from(self.width);
        Camera::from_fov(
            self.width,
            self.height,
            2.0 * atan(half_width_mm / self.focal_length_mm),
        )
    }

    pub fn focal_ratio(&self) -> f64 {
        self.focal_length_mm / self.aperture_mm
    }
}

impl Exposure {
    /// Background noise per pixel, electrons RMS.
    pub fn noise(&self) -> f64 {
        sqrt(
            self.read_noise * self.read_noise
                + (self.dark_current + self.sky_background) * self.seconds,
        )
    }

    /// Fraction of a star's light in its brightest pixel: the mean of a
    /// star centered on a pixel and one centered on a pixel corner, reduced
    /// by the trail when the camera rotates.
    pub fn peak_fraction(&self, trail_px: f64) -> f64 {
        let sigma = self.psf_sigma_px.max(1e-3);
        let centered = erf(0.5 / (sqrt(2.0) * sigma));
        let corner = 0.5 * erf(1.0 / (sqrt(2.0) * sigma));
        let still = 0.5 * (centered * centered + corner * corner);
        if trail_px > 1e-6 {
            still * sqrt(TAU) * sigma / trail_px * erf(trail_px / (2.0 * sqrt(2.0) * sigma))
        } else {
            still
        }
    }

    /// Centroid error of a star that delivered `electrons`, pixels per axis:
    /// photon noise, background noise under a Gaussian PSF, and a floor.
    pub fn centroid_sigma(&self, electrons: f64) -> f64 {
        let sigma = self.psf_sigma_px;
        let noise = self.noise();
        let photon = sigma * sigma / electrons;
        let background = 8.0 * PI * pow(sigma, 4.0) * noise * noise / (electrons * electrons);
        sqrt(
            CENTROID_EXCESS * CENTROID_EXCESS * (photon + background)
                + CENTROID_FLOOR_PX * CENTROID_FLOOR_PX,
        )
    }
}

/// Stars seen from each cell of an all-sky grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    /// Detectable catalog stars in the field, row-major from declination
    /// +90 down, right ascension 0 to 360 within a row.
    pub counts: Vec<u16>,
    /// Fraction of the sky with at least `COVERAGE_LEVELS[i]` stars.
    pub fractions: [f64; 3],
    pub minimum: u16,
    pub median: u16,
}

/// Counts the catalog stars no fainter than `mag_limit` inside a field of
/// solid angle `solid_angle`, taken as a circle, at every grid cell.
pub fn sky_coverage(catalog: &Catalog, mag_limit: f64, solid_angle: f64) -> Coverage {
    let cos_radius = (1.0 - solid_angle / TAU).clamp(-1.0, 1.0);
    let stars = catalog
        .stars()
        .get(..catalog.count_brighter_than(mag_limit))
        .unwrap_or(&[]);
    let mut counts = Vec::with_capacity(COVERAGE_COLUMNS * COVERAGE_ROWS);
    let mut weights = Vec::with_capacity(COVERAGE_COLUMNS * COVERAGE_ROWS);
    for row in 0..COVERAGE_ROWS {
        let dec = (90.0 - (row as f64 + 0.5) * 180.0 / COVERAGE_ROWS as f64) * DEG;
        for column in 0..COVERAGE_COLUMNS {
            let ra = (column as f64 + 0.5) * TAU / COVERAGE_COLUMNS as f64;
            let boresight = Vec3::new(cos(dec) * cos(ra), cos(dec) * sin(ra), sin(dec));
            let count = stars
                .iter()
                .filter(|s| s.dir.dot(boresight) >= cos_radius)
                .count();
            counts.push(u16::try_from(count).unwrap_or(u16::MAX));
            weights.push(cos(dec));
        }
    }
    let total: f64 = weights.iter().sum();
    let fraction_with = |level: u16| {
        counts
            .iter()
            .zip(&weights)
            .filter(|(count, _)| **count >= level)
            .map(|(_, weight)| *weight)
            .sum::<f64>()
            / total.max(f64::MIN_POSITIVE)
    };
    let [a, b, c] = COVERAGE_LEVELS;
    let mut order: Vec<(u16, f64)> = counts
        .iter()
        .copied()
        .zip(weights.iter().copied())
        .collect();
    order.sort_by_key(|entry| entry.0);
    let mut cumulative = 0.0;
    let mut median = 0;
    for (count, weight) in &order {
        cumulative += weight;
        if cumulative >= 0.5 * total {
            median = *count;
            break;
        }
    }
    Coverage {
        fractions: [fraction_with(a), fraction_with(b), fraction_with(c)],
        minimum: order.first().map_or(0, |entry| entry.0),
        median,
        counts,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignReport {
    pub camera: Camera,
    /// Electrons per second from a V = 0 star.
    pub zero_point: f64,
    /// Background noise per pixel, electrons RMS.
    pub noise: f64,
    /// Star trail during the exposure, pixels.
    pub trail_px: f64,
    /// Magnitude at which half of the stars are detected.
    pub limiting_magnitude: f64,
    /// The catalog ends before the limiting magnitude; star counts and
    /// coverage are then those of the catalog, not of the camera.
    pub catalog_limited: bool,
    /// Mean number of detectable catalog stars in a frame.
    pub stars_per_field: f64,
    /// One-sigma attitude error about each axis across the boresight, radians.
    pub sigma_cross: f64,
    /// One-sigma attitude error about the boresight, radians.
    pub sigma_roll: f64,
    /// Rotation rate at which stars are rejected as streaks, radians per second.
    pub streak_rate: f64,
    pub coverage: Coverage,
}

/// Evaluates a camera design against `catalog`.
pub fn evaluate(
    catalog: &Catalog,
    optics: &Optics,
    exposure: &Exposure,
) -> Result<DesignReport, AsteropeError> {
    let camera = optics.camera()?;
    let zero_point = optics.zero_point();
    if !(zero_point > 0.0)
        || !(exposure.seconds > 0.0)
        || !(exposure.psf_sigma_px > 0.0)
        || !(exposure.peak_sigma > 0.0)
        || !(exposure.body_rate >= 0.0)
    {
        return Err(AsteropeError::InvalidParameter);
    }
    let scale = camera.pixel_scale();
    let noise = exposure.noise();
    let trail_px = exposure.body_rate * exposure.seconds / scale;
    let threshold_electrons = (exposure.peak_sigma - PEAK_SELECTION_BIAS).max(1.0) * noise
        / exposure.peak_fraction(trail_px).max(1e-12);
    let limiting_magnitude = -2.5 * log10(threshold_electrons / (zero_point * exposure.seconds));
    let catalog_limited = limiting_magnitude > catalog.mag_limit();
    let depth = limiting_magnitude.min(catalog.mag_limit());

    let solid_angle = camera.solid_angle();
    let sky_fraction = solid_angle / (4.0 * PI);
    let detectable = catalog
        .stars()
        .get(..catalog.count_brighter_than(depth))
        .unwrap_or(&[]);
    // The solver weighs each star by the inverse variance of its centroid,
    // so the information of the stars in the field adds up.
    let information: f64 = detectable
        .iter()
        .map(|star| {
            let electrons = zero_point * exposure.seconds * pow(10.0, -0.4 * star.mag);
            let sigma = exposure.centroid_sigma(electrons) * scale;
            1.0 / (sigma * sigma)
        })
        .sum::<f64>()
        * sky_fraction;
    let stars_per_field = detectable.len() as f64 * sky_fraction;
    let variance = 1.0 / information.max(f64::MIN_POSITIVE);
    // Mean squared off-axis angle of a star in a rectangular field.
    let (half_x, half_y) = (0.5 * camera.fov_x(), 0.5 * camera.fov_y());
    let lever = (half_x * half_x + half_y * half_y) / 3.0;

    Ok(DesignReport {
        camera,
        zero_point,
        noise,
        trail_px,
        limiting_magnitude,
        catalog_limited,
        stars_per_field,
        sigma_cross: sqrt(variance),
        sigma_roll: sqrt(variance / lever.max(f64::MIN_POSITIVE)),
        streak_rate: STREAK_LIMIT_SIGMAS * exposure.psf_sigma_px * scale / exposure.seconds,
        coverage: sky_coverage(catalog, depth, solid_angle),
    })
}

impl DesignReport {
    /// Median cross-boresight error expected from `sigma_cross`: the median
    /// of a two-dimensional Gaussian radius, `1.177 sigma`.
    pub fn median_cross_arcsec(&self) -> f64 {
        1.177_41 * self.sigma_cross / ARCSEC
    }

    /// Median error about the boresight: `0.674 sigma`.
    pub fn median_roll_arcsec(&self) -> f64 {
        0.674_49 * self.sigma_roll / ARCSEC
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Star;
    use crate::rng::Rng;

    fn optics() -> Optics {
        Optics {
            focal_length_mm: 25.0,
            aperture_mm: 12.5,
            pixel_um: 6.0,
            width: 1024,
            height: 1024,
            transmission: 0.8,
            quantum_efficiency: 0.5,
            bandwidth_nm: 300.0,
        }
    }

    fn exposure() -> Exposure {
        Exposure {
            seconds: 0.1,
            psf_sigma_px: 1.2,
            read_noise: 5.0,
            dark_current: 10.0,
            sky_background: 20.0,
            body_rate: 0.0,
            peak_sigma: 5.0,
        }
    }

    fn uniform_sky(count: usize, seed: u64) -> Catalog {
        let mut rng = Rng::new(seed);
        let stars = (0..count)
            .map(|i| {
                let z = rng.range(-1.0, 1.0);
                let phi = rng.range(0.0, TAU);
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

    #[test]
    fn photometry_follows_aperture_area_and_the_camera_follows_the_lens() {
        let base = optics();
        let wide = Optics {
            aperture_mm: 25.0,
            ..base
        };
        assert!((wide.zero_point() / base.zero_point() - 4.0).abs() < 1e-12);
        let expected = 1005.0 * PI * 0.625 * 0.625 * 3000.0 * 0.4;
        assert!((base.zero_point() / expected - 1.0).abs() < 1e-12);
        assert!((base.focal_ratio() - 2.0).abs() < 1e-12);
        let camera = base.camera().unwrap();
        let fov = 2.0 * (0.5 * 1024.0 * 0.006 / 25.0f64).atan();
        assert!((camera.fov_x() - fov).abs() < 1e-12);
    }

    #[test]
    fn limiting_magnitude_responds_to_exposure_noise_and_motion() {
        let sky = uniform_sky(4000, 1);
        let base = evaluate(&sky, &optics(), &exposure()).unwrap();
        let longer = evaluate(
            &sky,
            &optics(),
            &Exposure {
                seconds: 0.4,
                read_noise: 20.0,
                ..exposure()
            },
        )
        .unwrap();
        // Read-noise limited at both settings: four times the light against
        // four times the noise leaves the limit where it was.
        assert!((longer.limiting_magnitude - base.limiting_magnitude).abs() < 0.12);
        let quiet = evaluate(
            &sky,
            &optics(),
            &Exposure {
                read_noise: 0.5,
                dark_current: 0.0,
                sky_background: 0.0,
                ..exposure()
            },
        )
        .unwrap();
        assert!(
            (quiet.limiting_magnitude
                - base.limiting_magnitude
                - 2.5 * (base.noise / quiet.noise).log10())
            .abs()
                < 1e-9
        );
        let slewing = evaluate(
            &sky,
            &optics(),
            &Exposure {
                body_rate: 1.0 * DEG,
                ..exposure()
            },
        )
        .unwrap();
        assert!(slewing.trail_px > 5.0);
        assert!(slewing.limiting_magnitude < base.limiting_magnitude - 0.5);
        assert!((base.streak_rate * 0.1 / base.camera.pixel_scale() - 7.94 * 1.2).abs() < 1e-9);
    }

    #[test]
    fn coverage_counts_match_the_mean_density_of_a_uniform_sky() {
        let sky = uniform_sky(6000, 2);
        let report = evaluate(&sky, &optics(), &exposure()).unwrap();
        let depth = report.limiting_magnitude.min(sky.mag_limit());
        let expected =
            sky.count_brighter_than(depth) as f64 * report.camera.solid_angle() / (4.0 * PI);
        assert!((report.stars_per_field - expected).abs() < 1e-9);
        let mut weighted = 0.0;
        let mut weights = 0.0;
        for (index, count) in report.coverage.counts.iter().enumerate() {
            let row = index / COVERAGE_COLUMNS;
            let weight = ((90.0 - (row as f64 + 0.5) * 5.0) * DEG).cos();
            weighted += weight * f64::from(*count);
            weights += weight;
        }
        assert!((weighted / weights / expected - 1.0).abs() < 0.03);
        assert_eq!(
            report.coverage.counts.len(),
            COVERAGE_COLUMNS * COVERAGE_ROWS
        );
        let [four, six, ten] = report.coverage.fractions;
        assert!(four >= six && six >= ten && (0.0..=1.0).contains(&four));
        assert!(report.coverage.minimum <= report.coverage.median);
    }

    #[test]
    fn accuracy_scales_with_the_number_and_brightness_of_stars() {
        let sparse = evaluate(&uniform_sky(2000, 3), &optics(), &exposure()).unwrap();
        let dense = evaluate(&uniform_sky(8000, 3), &optics(), &exposure()).unwrap();
        let ratio = sparse.sigma_cross / dense.sigma_cross;
        assert!((ratio - 2.0).abs() < 0.25, "{ratio}");
        assert!(dense.sigma_roll > 5.0 * dense.sigma_cross);
        assert!(dense.median_cross_arcsec() > 0.0 && dense.median_roll_arcsec() > 0.0);
        let e = exposure();
        assert!(e.centroid_sigma(1e6) < e.centroid_sigma(1e3));
        assert!(e.centroid_sigma(1e9) >= CENTROID_FLOOR_PX);
        assert!(evaluate(
            &uniform_sky(10, 1),
            &Optics {
                focal_length_mm: 0.0,
                ..optics()
            },
            &e
        )
        .is_err());
    }
}
