//! Lost-in-space Monte Carlo trials, shared by the CLI and the browser.

use alloc::vec::Vec;

use crate::attitude::{q_method, quest, svd_method, Observation, RaDecRoll};
use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::detect::{detect, CentroidMethod, DetectConfig, Frame};
use crate::error::AsteropeError;
use crate::index::PairIndex;
use crate::math::{
    abs, cos, erf, exp, hypot, lgamma, ln, sin, sqrt, Quat, Vec3, ARCSEC, DEG, PI, TAU,
};
use crate::pipeline::{solve_frame, SolverConfig};
use crate::rng::{derive_seed, Rng};
use crate::sim::{render_sources, simulate, PointSource, SensorModel, StrayLight};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scenario {
    pub camera: Camera,
    pub sensor: SensorModel,
    pub solver: SolverConfig,
    /// Randomize hot pixels, cosmic rays and stray light per trial.
    pub disturbances: bool,
}

pub const SUCCESS_BORESIGHT_ARCSEC: f64 = 60.0;
pub const SUCCESS_ROLL_ARCSEC: f64 = 300.0;
pub const FALSE_POSITIVE_DEG: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrialClass {
    /// Solved within the accuracy limits.
    Success,
    /// Solved near the truth but outside the accuracy limits.
    Inaccurate,
    /// Reported an attitude more than `FALSE_POSITIVE_DEG` from the truth.
    FalsePositive,
    NoSolution,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrialOutcome {
    pub seed: u64,
    pub class: TrialClass,
    pub truth: RaDecRoll,
    /// Error perpendicular to the boresight, arcseconds (NaN without a solution).
    pub cross_boresight_arcsec: f64,
    /// Error about the boresight, arcseconds (NaN without a solution).
    pub roll_arcsec: f64,
    /// Total rotation error, arcseconds (NaN without a solution).
    pub total_arcsec: f64,
    pub truth_stars: u32,
    pub detections: u32,
    pub rejected: u32,
    pub matches: u32,
    pub triads: u32,
}

/// Sensor disturbances drawn for one trial.
pub fn disturbed_sensor(base: &SensorModel, rng: &mut Rng) -> SensorModel {
    let hot_pixel_fraction = exp(rng.range(ln(1e-4), ln(1e-3)));
    let cosmic_ray_mean = rng.range(0.0, 5.0);
    let stray_light = if rng.uniform() < 0.3 {
        StrayLight {
            intensity: rng.range(200.0, 2000.0),
            azimuth: rng.range(0.0, TAU),
            falloff: rng.range(0.2, 0.6),
        }
    } else {
        base.stray_light
    };
    SensorModel {
        hot_pixel_fraction,
        cosmic_ray_mean,
        stray_light,
        hot_pixel_seed: rng.next_u64(),
        ..*base
    }
}

pub fn run_trial(
    catalog: &Catalog,
    index: &PairIndex,
    scenario: &Scenario,
    master_seed: u64,
    trial: u64,
) -> Result<TrialOutcome, AsteropeError> {
    run_trial_onboard(catalog, catalog, index, scenario, master_seed, trial)
}

/// A trial whose frame is rendered from `sky` while the solver only knows
/// `catalog`, a brighter prefix of it, as an onboard catalog would be.
pub fn run_trial_onboard(
    sky: &Catalog,
    catalog: &Catalog,
    index: &PairIndex,
    scenario: &Scenario,
    master_seed: u64,
    trial: u64,
) -> Result<TrialOutcome, AsteropeError> {
    let seed = derive_seed(master_seed, trial);
    let mut rng = Rng::new(seed);
    let truth_q = rng.unit_quat();
    let truth_attitude = truth_q.to_matrix();
    let sensor = if scenario.disturbances {
        disturbed_sensor(&scenario.sensor, &mut rng)
    } else {
        scenario.sensor
    };
    let frame = simulate(
        sky.stars(),
        &truth_attitude,
        &scenario.camera,
        &sensor,
        rng.next_u64(),
    )?;
    let view = Frame::new(frame.width, frame.height, &frame.pixels)?;
    let result = solve_frame(&view, &scenario.camera, index, catalog, &scenario.solver)?;

    let mut outcome = TrialOutcome {
        seed,
        class: TrialClass::NoSolution,
        truth: crate::attitude::radec_roll_from_attitude(&truth_attitude),
        cross_boresight_arcsec: f64::NAN,
        roll_arcsec: f64::NAN,
        total_arcsec: f64::NAN,
        truth_stars: frame.truth.len() as u32,
        detections: result.detect.detections.len() as u32,
        rejected: result.detect.rejected.len() as u32,
        matches: 0,
        triads: result.lis.triads_tested,
    };
    if let Some(solution) = &result.lis.solution {
        let (cross, roll, total) = attitude_error(solution.q, truth_q);
        outcome.cross_boresight_arcsec = cross / ARCSEC;
        outcome.roll_arcsec = roll / ARCSEC;
        outcome.total_arcsec = total / ARCSEC;
        outcome.matches = solution.matches.len() as u32;
        outcome.class = if total > FALSE_POSITIVE_DEG * DEG {
            TrialClass::FalsePositive
        } else if cross <= SUCCESS_BORESIGHT_ARCSEC * ARCSEC && roll <= SUCCESS_ROLL_ARCSEC * ARCSEC
        {
            TrialClass::Success
        } else {
            TrialClass::Inaccurate
        };
    }
    Ok(outcome)
}

/// Splits the rotation from `truth` to `estimate` into its components
/// perpendicular to and about the boresight, plus the total angle (radians).
pub fn attitude_error(estimate: Quat, truth: Quat) -> (f64, f64, f64) {
    let delta = (estimate * truth.conjugate()).canonical();
    let total = estimate.angle_to(truth);
    let vector = delta.vector();
    let norm = vector.norm();
    if norm <= 0.0 {
        return (0.0, 0.0, total);
    }
    let axis = vector.scale(total / norm);
    (hypot(axis.x, axis.y), abs(axis.z), total)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub trials: u32,
    pub success: u32,
    pub inaccurate: u32,
    pub false_positive: u32,
    pub no_solution: u32,
    /// Trials whose frame held fewer than four catalog stars.
    pub starved: u32,
}

impl Tally {
    pub fn add(&mut self, outcome: &TrialOutcome) {
        self.trials += 1;
        match outcome.class {
            TrialClass::Success => self.success += 1,
            TrialClass::Inaccurate => self.inaccurate += 1,
            TrialClass::FalsePositive => self.false_positive += 1,
            TrialClass::NoSolution => self.no_solution += 1,
        }
        if outcome.truth_stars < 4 {
            self.starved += 1;
        }
    }

    pub fn merge(&mut self, other: &Tally) {
        self.trials += other.trials;
        self.success += other.success;
        self.inaccurate += other.inaccurate;
        self.false_positive += other.false_positive;
        self.no_solution += other.no_solution;
        self.starved += other.starved;
    }
}

/// A sensor quantity swept in isolation to find where the solver breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StressParameter {
    ReadNoise,
    StrayLight,
    HotPixels,
    CosmicRays,
    PsfSigma,
    Exposure,
    BodyRate,
}

impl StressParameter {
    pub const ALL: [StressParameter; 7] = [
        StressParameter::ReadNoise,
        StressParameter::StrayLight,
        StressParameter::HotPixels,
        StressParameter::CosmicRays,
        StressParameter::PsfSigma,
        StressParameter::Exposure,
        StressParameter::BodyRate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StressParameter::ReadNoise => "Read noise",
            StressParameter::StrayLight => "Stray light",
            StressParameter::HotPixels => "Hot pixels",
            StressParameter::CosmicRays => "Cosmic rays",
            StressParameter::PsfSigma => "PSF sigma",
            StressParameter::Exposure => "Exposure",
            StressParameter::BodyRate => "Body rate",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            StressParameter::ReadNoise => "e-",
            StressParameter::StrayLight => "e-/px/s",
            StressParameter::HotPixels => "%",
            StressParameter::CosmicRays => "per frame",
            StressParameter::PsfSigma => "px",
            StressParameter::Exposure => "s",
            StressParameter::BodyRate => "deg/s",
        }
    }

    /// Values visited by a sweep, in the unit given by `unit`.
    pub fn values(self) -> &'static [f64] {
        match self {
            StressParameter::ReadNoise => {
                &[2.0, 5.0, 10.0, 20.0, 30.0, 45.0, 60.0, 80.0, 110.0, 150.0]
            }
            StressParameter::StrayLight => &[0.0, 1e3, 3e3, 1e4, 3e4, 6e4, 1e5, 2e5, 4e5, 8e5],
            StressParameter::HotPixels => &[0.0, 0.01, 0.05, 0.1, 0.3, 1.0, 2.0, 4.0, 8.0, 15.0],
            StressParameter::CosmicRays => &[
                0.0, 5.0, 20.0, 50.0, 100.0, 200.0, 400.0, 600.0, 800.0, 1000.0,
            ],
            StressParameter::PsfSigma => &[0.4, 0.6, 0.8, 1.2, 2.0, 3.0, 4.0, 5.0, 6.5, 8.0],
            StressParameter::Exposure => &[0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0],
            StressParameter::BodyRate => &[0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0],
        }
    }

    /// The sensor with this one quantity set to `value` (in `unit`).
    pub fn apply(self, sensor: &SensorModel, value: f64) -> SensorModel {
        let mut out = *sensor;
        match self {
            StressParameter::ReadNoise => out.read_noise = value,
            StressParameter::StrayLight => out.stray_light.intensity = value,
            StressParameter::HotPixels => out.hot_pixel_fraction = value / 100.0,
            StressParameter::CosmicRays => out.cosmic_ray_mean = value,
            StressParameter::PsfSigma => out.psf_sigma = value,
            StressParameter::Exposure => out.exposure = value,
            StressParameter::BodyRate => out.body_rate = Vec3::new(0.0, value * DEG, 0.0),
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WahbaSummary {
    pub cases: u32,
    /// Largest rotation between QUEST and the SVD solution, radians.
    pub worst_quest: f64,
    /// Largest rotation between the q-method and the SVD solution, radians.
    pub worst_q_method: f64,
    pub failures: u32,
}

/// Random Wahba problems shaped like star-tracker fields: 3-30 stars inside a
/// 4-12.5 degree half-angle cone, 0-60 arcsecond direction noise, random
/// weights, and every tenth attitude within 0.001 degrees of a half turn.
pub fn wahba_case(rng: &mut Rng, near_half_turn: bool) -> Vec<Observation> {
    let truth = if near_half_turn {
        let axis = Vec3::new(rng.normal(), rng.normal(), rng.normal())
            .normalized()
            .unwrap_or(Vec3::X);
        let half = 0.5 * (180.0 - rng.range(0.0, 1e-3)) * DEG;
        Quat::from_parts(axis.scale(sin(half)), cos(half))
    } else {
        rng.unit_quat()
    };
    let to_reference = truth.to_matrix().transpose();
    let count = 3 + rng.below(28) as usize;
    let half_fov = rng.range(4.0, 12.5) * DEG;
    let noise = rng.range(0.0, 60.0) * ARCSEC;
    (0..count)
        .map(|_| {
            let off_axis = half_fov * sqrt(rng.uniform());
            let azimuth = rng.uniform() * TAU;
            let body = Vec3::new(
                sin(off_axis) * cos(azimuth),
                sin(off_axis) * sin(azimuth),
                cos(off_axis),
            );
            let helper = if abs(body.x) < 0.9 { Vec3::X } else { Vec3::Y };
            let e1 = body.cross(helper).normalized().unwrap_or(Vec3::Y);
            let e2 = body.cross(e1);
            let measured = body + e1.scale(noise * rng.normal()) + e2.scale(noise * rng.normal());
            Observation {
                body: measured.normalized().unwrap_or(body),
                reference: to_reference.mul_vec(body),
                weight: rng.range(0.5, 2.0),
            }
        })
        .collect()
}

/// Compares QUEST and the q-method against the SVD solution.
pub fn wahba_suite(cases: u32, seed: u64) -> WahbaSummary {
    let mut rng = Rng::new(seed);
    let mut summary = WahbaSummary {
        cases,
        ..WahbaSummary::default()
    };
    for case in 0..cases {
        let observations = wahba_case(&mut rng, case % 10 == 0);
        let (Ok(reference), Ok(solution), Ok((q_davenport, _))) = (
            svd_method(&observations),
            quest(&observations),
            q_method(&observations),
        ) else {
            summary.failures += 1;
            continue;
        };
        summary.worst_quest = summary.worst_quest.max(solution.q.angle_to(reference));
        summary.worst_q_method = summary.worst_q_method.max(q_davenport.angle_to(reference));
    }
    summary
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CentroidSummary {
    pub samples: u32,
    pub missed: u32,
    /// Mean radial centroid error, pixels.
    pub mean_error: f64,
    pub max_error: f64,
    /// Star signal in electrons.
    pub electrons: f64,
    /// Approximate Cramer-Rao bound on the mean radial error, pixels (0 without noise).
    pub bound: f64,
}

/// Fraction of a pixel-centered Gaussian star's flux that lands in its peak pixel.
pub fn peak_fraction(psf_sigma: f64) -> f64 {
    let half = erf(0.5 / (psf_sigma * core::f64::consts::SQRT_2));
    half * half
}

/// Background noise per pixel in electrons RMS.
pub fn background_noise(sensor: &SensorModel) -> f64 {
    sqrt(
        (sensor.sky_background + sensor.dark_current) * sensor.exposure
            + sensor.read_noise * sensor.read_noise,
    )
}

/// Approximate Cramer-Rao bound on the mean radial centroid error of a
/// Gaussian star: per-axis variance `sigma^2 / N` from photon noise plus
/// `8 pi sigma^4 b^2 / N^2` from background noise `b`, times `sqrt(pi / 2)`
/// to convert a per-axis sigma into a mean radial error.
pub fn centroid_bound(psf_sigma: f64, electrons: f64, noise: f64) -> f64 {
    let s2 = psf_sigma * psf_sigma;
    let variance = s2 / electrons + 8.0 * PI * s2 * s2 * noise * noise / (electrons * electrons);
    sqrt(variance * PI / 2.0)
}

/// Places single stars on a `phases x phases` grid of sub-pixel offsets and
/// measures the centroid error. `peak_snr = None` renders without noise.
pub fn centroid_suite(
    psf_sigma: f64,
    peak_snr: Option<f64>,
    phases: u32,
    seeds: u32,
    method: CentroidMethod,
    master_seed: u64,
) -> Result<CentroidSummary, AsteropeError> {
    const SIZE: u32 = 48;
    const ORIGIN: f64 = 23.0;
    if phases == 0 || seeds == 0 {
        return Err(AsteropeError::InvalidParameter);
    }
    let sensor = SensorModel {
        psf_sigma,
        noise: peak_snr.is_some(),
        ..SensorModel::default()
    };
    let noise = background_noise(&sensor);
    let electrons = match peak_snr {
        Some(snr) => snr * noise / peak_fraction(psf_sigma),
        None => 20_000.0,
    };
    let config = DetectConfig {
        method,
        saturation: sensor.max_dn(),
        ..DetectConfig::default()
    };
    let mut summary = CentroidSummary {
        electrons,
        bound: if peak_snr.is_some() {
            centroid_bound(psf_sigma, electrons, noise)
        } else {
            0.0
        },
        ..CentroidSummary::default()
    };
    let mut total = 0.0;
    for py in 0..phases {
        for px in 0..phases {
            let x = ORIGIN + f64::from(px) / f64::from(phases);
            let y = ORIGIN + f64::from(py) / f64::from(phases);
            for seed in 0..seeds {
                let stream = u64::from(py * phases + px) << 32 | u64::from(seed);
                let pixels = render_sources(
                    SIZE,
                    SIZE,
                    &[PointSource { x, y, electrons }],
                    &sensor,
                    derive_seed(master_seed, stream),
                )?;
                let output = detect(&Frame::new(SIZE, SIZE, &pixels)?, &config)?;
                let nearest = output
                    .detections
                    .iter()
                    .map(|d| hypot(d.x - x, d.y - y))
                    .fold(f64::INFINITY, f64::min);
                if nearest > 1.5 {
                    summary.missed += 1;
                    continue;
                }
                total += nearest;
                summary.max_error = summary.max_error.max(nearest);
                summary.samples += 1;
            }
        }
    }
    summary.mean_error = total / f64::from(summary.samples.max(1));
    Ok(summary)
}

/// `P(X <= k)` for `X ~ Binomial(n, p)`, summed in log space.
fn binomial_cdf(k: u32, n: u32, p: f64) -> f64 {
    if p <= 0.0 || k >= n {
        return 1.0;
    }
    if p >= 1.0 {
        return 0.0;
    }
    let (ln_p, ln_q) = (ln(p), ln(1.0 - p));
    let ln_n_factorial = lgamma(f64::from(n) + 1.0);
    let mut total = 0.0;
    for i in 0..=k {
        let (fi, rest) = (f64::from(i), f64::from(n - i));
        total +=
            exp(ln_n_factorial - lgamma(fi + 1.0) - lgamma(rest + 1.0) + fi * ln_p + rest * ln_q);
    }
    total.min(1.0)
}

fn bisect(mut lo: f64, mut hi: f64, below_target: impl Fn(f64) -> bool) -> f64 {
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if below_target(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Exact (Clopper-Pearson) two-sided 95% confidence interval for a binomial
/// proportion.
pub fn clopper_pearson(successes: u32, trials: u32) -> (f64, f64) {
    const TAIL: f64 = 0.025;
    if trials == 0 || successes > trials {
        return (0.0, 1.0);
    }
    let lower = if successes == 0 {
        0.0
    } else {
        bisect(0.0, 1.0, |p| {
            1.0 - binomial_cdf(successes - 1, trials, p) < TAIL
        })
    };
    let upper = if successes == trials {
        1.0
    } else {
        bisect(0.0, 1.0, |p| binomial_cdf(successes, trials, p) > TAIL)
    };
    (lower, upper)
}

/// Linear-interpolated percentile of ascending-sorted samples (NaN when empty).
pub fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let Some(last) = sorted.len().checked_sub(1) else {
        return f64::NAN;
    };
    let position = fraction.clamp(0.0, 1.0) * last as f64;
    let lower = position as usize;
    let a = sorted.get(lower).copied().unwrap_or(f64::NAN);
    let b = sorted.get(lower + 1).copied().unwrap_or(a);
    a + (b - a) * (position - lower as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clopper_pearson_matches_closed_forms() {
        let (lo, hi) = clopper_pearson(0, 1000);
        assert_eq!(lo, 0.0);
        assert!(
            (hi - (1.0 - 0.025f64.powf(1.0 / 1000.0))).abs() < 1e-9,
            "upper {hi}"
        );
        let (lo, hi) = clopper_pearson(10_000, 10_000);
        assert_eq!(hi, 1.0);
        assert!(
            (lo - 0.025f64.powf(1.0 / 10_000.0)).abs() < 1e-9,
            "lower {lo}"
        );
        // Reference values from the beta-quantile definition.
        let (lo, hi) = clopper_pearson(990, 1000);
        assert!(
            (lo - 0.98168).abs() < 2e-4 && (hi - 0.99520).abs() < 2e-4,
            "[{lo}, {hi}]"
        );
        let (lo, hi) = clopper_pearson(5, 10);
        assert!(
            (lo - 0.18709).abs() < 1e-4 && (hi - 0.81291).abs() < 1e-4,
            "[{lo}, {hi}]"
        );
    }

    #[test]
    fn percentile_interpolates() {
        let v = [1.0, 2.0, 4.0];
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 0.5), 2.0);
        assert_eq!(percentile(&v, 0.75), 3.0);
        assert_eq!(percentile(&v, 1.0), 4.0);
        assert!(percentile(&[], 0.5).is_nan());
    }

    #[test]
    fn attitude_error_separates_roll_from_pointing() {
        let truth = Quat::new(0.1, 0.2, -0.3, 0.9).normalized().unwrap();
        let roll = 100.0 * ARCSEC;
        let about_z = Quat::new(0.0, 0.0, (0.5 * roll).sin(), (0.5 * roll).cos());
        let (cross, about, total) = attitude_error(about_z * truth, truth);
        assert!(cross < 1e-12);
        assert!((about - roll).abs() < 1e-12 && (total - roll).abs() < 1e-12);
    }
}
