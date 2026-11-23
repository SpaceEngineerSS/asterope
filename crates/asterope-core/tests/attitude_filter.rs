//! The gyro and star tracker filter: is its covariance honest, does it
//! find the bias, and what does it buy.
//!
//! `cargo test --release -p asterope-core --test attitude_filter -- --nocapture`
//! prints the numbers quoted in `docs/validation.md`.

use asterope_core::filter::{AttitudeFilter, GyroNoise};
use asterope_core::math::{Mat3, Quat, Vec3, ARCSEC, DEG};
use asterope_core::rng::Rng;
use asterope_core::sim::Gyro;
use asterope_core::track::rotation_increment;

const GYRO_STEP: f64 = 0.02;
const STEPS_PER_FIX: usize = 10;
/// A MEMS gyro of the better kind: 0.18 deg/sqrt(h) of angle random walk.
const NOISE: GyroNoise = GyroNoise {
    angle_random_walk: 0.003 * DEG,
    rate_random_walk: 3e-5 * DEG,
};
/// Star tracker error about x, y and the boresight, one sigma.
const FIX_SIGMA: [f64; 3] = [2.0 * ARCSEC, 2.0 * ARCSEC, 15.0 * ARCSEC];
const NO_GATE: f64 = f64::INFINITY;

fn gaussian(rng: &mut Rng, sigma: [f64; 3]) -> Vec3 {
    Vec3::new(
        sigma[0] * rng.normal(),
        sigma[1] * rng.normal(),
        sigma[2] * rng.normal(),
    )
}

fn fix_covariance() -> Mat3 {
    Mat3::diagonal(Vec3::new(
        FIX_SIGMA[0] * FIX_SIGMA[0],
        FIX_SIGMA[1] * FIX_SIGMA[1],
        FIX_SIGMA[2] * FIX_SIGMA[2],
    ))
}

fn true_rate(t: f64) -> Vec3 {
    Vec3::new(
        0.5 * DEG * (0.1 * t).sin(),
        0.3 * DEG,
        -0.2 * DEG * (0.07 * t).cos(),
    )
}

/// Rotation from `estimate` to `truth` as a vector in sensor axes.
fn rotation_error(truth: Quat, estimate: Quat) -> Vec3 {
    let delta = (truth * estimate.conjugate()).canonical();
    delta.vector().scale(2.0 / delta.w)
}

/// A spacecraft with a gyro and a star tracker, and the filter watching them.
struct Run {
    rng: Rng,
    truth: Quat,
    gyro: Gyro,
    filter: AttitudeFilter,
    time: f64,
}

impl Run {
    fn new(seed: u64) -> Run {
        let mut rng = Rng::new(seed);
        let truth = rng.unit_quat();
        let bias = gaussian(&mut rng, [0.02 * DEG; 3]);
        let first_fix = rotation_increment(gaussian(&mut rng, FIX_SIGMA)) * truth;
        Run {
            filter: AttitudeFilter::new(first_fix, 15.0 * ARCSEC, 0.02 * DEG, NOISE),
            gyro: Gyro { noise: NOISE, bias },
            truth,
            rng,
            time: 0.0,
        }
    }

    fn step(&mut self) {
        let rate = true_rate(self.time + 0.5 * GYRO_STEP);
        self.truth = (rotation_increment(rate.scale(GYRO_STEP)) * self.truth)
            .normalized()
            .unwrap();
        let reading = self.gyro.read(rate, GYRO_STEP, &mut self.rng);
        self.filter.propagate(reading, GYRO_STEP);
        self.time += GYRO_STEP;
    }

    fn fix(&mut self) -> f64 {
        let measured = rotation_increment(gaussian(&mut self.rng, FIX_SIGMA)) * self.truth;
        self.filter
            .update(measured, &fix_covariance(), NO_GATE)
            .unwrap()
            .normalized_squared
    }

    fn errors(&self) -> [f64; 6] {
        let attitude = rotation_error(self.truth, self.filter.attitude());
        let bias = self.gyro.bias - self.filter.bias();
        [attitude.x, attitude.y, attitude.z, bias.x, bias.y, bias.z]
    }

    /// The error weighed by the filter's own covariance; six on average
    /// when the covariance tells the truth.
    fn normalized_error_squared(&self) -> f64 {
        let (aa, ab, bb) = self.filter.covariance();
        let ba = ab.transpose();
        let rows = |top: &Mat3, bottom: &Mat3, row: usize| {
            let pick = |m: &Mat3| [m.r0, m.r1, m.r2][row];
            let (a, b) = (pick(top), pick(bottom));
            [a.x, a.y, a.z, b.x, b.y, b.z]
        };
        let mut p = [[0.0; 6]; 6];
        for row in 0..3 {
            p[row] = rows(&aa, &ab, row);
            p[row + 3] = rows(&ba, &bb, row);
        }
        let e = self.errors();
        let solved = solve(p, e);
        e.iter().zip(solved.iter()).map(|(a, b)| a * b).sum()
    }
}

/// Solves `a x = b` by elimination with partial pivoting.
fn solve(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> [f64; 6] {
    for column in 0..6 {
        let pivot = (column..6)
            .max_by(|i, j| a[*i][column].abs().total_cmp(&a[*j][column].abs()))
            .unwrap();
        a.swap(column, pivot);
        b.swap(column, pivot);
        for row in column + 1..6 {
            let factor = a[row][column] / a[column][column];
            let pivot_row = a[column];
            for (target, source) in a[row].iter_mut().zip(pivot_row.iter()).skip(column) {
                *target -= factor * source;
            }
            b[row] -= factor * b[column];
        }
    }
    let mut x = [0.0; 6];
    for row in (0..6).rev() {
        let tail: f64 = (row + 1..6).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    x
}

#[test]
fn covariance_is_consistent_and_the_bias_is_found() {
    const RUNS: u64 = 200;
    const SECONDS: f64 = 180.0;
    let (mut nees, mut nis, mut samples) = (0.0, 0.0, 0u64);
    let (mut bias_error, mut bias_sigma) = (0.0, 0.0);
    let (mut filtered, mut raw) = ([0.0; 3], [0.0; 3]);
    for seed in 0..RUNS {
        let mut run = Run::new(seed);
        while run.time < SECONDS {
            for _ in 0..STEPS_PER_FIX {
                run.step();
            }
            let innovation = run.fix();
            if run.time > 60.0 {
                nees += run.normalized_error_squared();
                nis += innovation;
                samples += 1;
                let e = run.errors();
                for axis in 0..3 {
                    filtered[axis] += e[axis] * e[axis];
                    raw[axis] += FIX_SIGMA[axis] * FIX_SIGMA[axis];
                }
            }
        }
        let e = run.errors();
        bias_error += e[3] * e[3] + e[4] * e[4] + e[5] * e[5];
        let sigma = run.filter.bias_sigma();
        bias_sigma += sigma.dot(sigma);
    }
    let n = samples as f64;
    let (nees, nis) = (nees / n, nis / n);
    println!("mean normalized state error squared {nees:.2} (6 for a consistent filter)");
    println!("mean normalized residual squared {nis:.2} (3 for a consistent filter)");
    let rms = |sum: f64| (sum / n).sqrt() / ARCSEC;
    println!("| Axis | Star tracker alone, RMS (\") | With the gyro, RMS (\") |");
    println!("|---|---|---|");
    for (axis, name) in ["x", "y", "boresight"].iter().enumerate() {
        println!(
            "| {name} | {:.2} | {:.2} |",
            rms(raw[axis]),
            rms(filtered[axis])
        );
        assert!(filtered[axis] < raw[axis]);
    }
    let (bias_error, bias_sigma) = (
        (bias_error / RUNS as f64).sqrt() / DEG * 3600.0,
        (bias_sigma / RUNS as f64).sqrt() / DEG * 3600.0,
    );
    println!("bias after {SECONDS} s: error {bias_error:.2} deg/h RMS, filter says {bias_sigma:.2}; it started at 125");
    assert!((5.6..6.4).contains(&nees), "{nees}");
    assert!((2.8..3.2).contains(&nis), "{nis}");
    assert!((0.8..1.25).contains(&(bias_error / bias_sigma)));
    assert!(bias_error < 10.0);
}

#[test]
fn error_through_an_outage_grows_as_the_covariance_says() {
    const RUNS: u64 = 300;
    println!("| Seconds without a fix | Error, RMS over runs (\") | Filter's own sigma (\") |");
    println!("|---|---|---|");
    for outage in [10.0, 60.0, 300.0] {
        let (mut actual, mut predicted) = (0.0, 0.0);
        for seed in 0..RUNS {
            let mut run = Run::new(1000 + seed);
            while run.time < 120.0 {
                for _ in 0..STEPS_PER_FIX {
                    run.step();
                }
                run.fix();
            }
            let resume = run.time + outage;
            while run.time < resume {
                run.step();
            }
            let e = run.errors();
            actual += e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
            let sigma = run.filter.attitude_sigma();
            predicted += sigma.dot(sigma);
        }
        let (actual, predicted) = (
            (actual / RUNS as f64).sqrt() / ARCSEC,
            (predicted / RUNS as f64).sqrt() / ARCSEC,
        );
        println!("| {outage} | {actual:.1} | {predicted:.1} |");
        assert!(
            (0.85..1.18).contains(&(actual / predicted)),
            "{outage} s: {actual} against {predicted}"
        );
    }
}

/// A two-state Kalman filter for one axis (angle and bias), written out
/// with scalars. With no rotation the three axes of the attitude filter do
/// not mix, so each must carry exactly this covariance.
#[test]
fn covariance_matches_a_two_state_filter_written_separately() {
    let (v, u) = (
        NOISE.angle_random_walk * NOISE.angle_random_walk,
        NOISE.rate_random_walk * NOISE.rate_random_walk,
    );
    let r = FIX_SIGMA[0] * FIX_SIGMA[0];
    let dt = GYRO_STEP;
    let (mut paa, mut pab, mut pbb) = ((15.0 * ARCSEC).powi(2), 0.0, (0.02 * DEG).powi(2));
    let mut filter = AttitudeFilter::new(Quat::IDENTITY, 15.0 * ARCSEC, 0.02 * DEG, NOISE);
    for step in 0..5000 {
        // x' = [[1, -dt], [0, 1]] x, plus the process noise of the module.
        let (a, b, c) = (paa, pab, pbb);
        paa = a - 2.0 * dt * b + dt * dt * c + v * dt + u * dt * dt * dt / 3.0;
        pab = b - dt * c - 0.5 * u * dt * dt;
        pbb = c + u * dt;
        filter.propagate(filter.bias(), dt);
        if step % STEPS_PER_FIX == STEPS_PER_FIX - 1 {
            let s = paa + r;
            let (ka, kb) = (paa / s, pab / s);
            let (a, b, c) = (paa, pab, pbb);
            paa = (1.0 - ka) * (1.0 - ka) * a + ka * ka * r;
            pab = (1.0 - ka) * (b - kb * a) + ka * kb * r;
            pbb = c - 2.0 * kb * b + kb * kb * a + kb * kb * r;
            let own = filter.attitude();
            filter
                .update(own, &Mat3::IDENTITY.scale(r), NO_GATE)
                .unwrap();
        }
        let (aa, ab, bb) = filter.covariance();
        for (ours, scalar) in [
            (aa.r0.x, paa),
            (ab.r0.x, pab),
            (bb.r0.x, pbb),
            (aa.r2.z, paa),
        ] {
            assert!(
                (ours / scalar - 1.0).abs() < 1e-9,
                "step {step}: {ours} against {scalar}"
            );
        }
        assert!(aa.r0.y.abs() < 1e-30 && ab.r0.y.abs() < 1e-30);
    }
    println!(
        "steady state: {:.3}\" after a fix, bias known to {:.3} deg/h",
        paa.sqrt() / ARCSEC,
        pbb.sqrt() / DEG * 3600.0
    );
}

#[test]
fn a_fix_that_does_not_belong_is_turned_away() {
    let mut run = Run::new(7);
    for _ in 0..600 {
        for _ in 0..STEPS_PER_FIX {
            run.step();
        }
        run.fix();
    }
    let before = run.filter;
    // A wrong identification: an attitude a degree away, offered with the
    // usual confidence. 16.27 is the 99.9 % point of chi-square with 3
    // degrees of freedom.
    let wrong = rotation_increment(Vec3::new(1.0 * DEG, 0.0, 0.0)) * run.truth;
    let outcome = run.filter.update(wrong, &fix_covariance(), 16.27).unwrap();
    assert!(!outcome.accepted && outcome.normalized_squared > 1e4);
    assert_eq!(run.filter, before);
    let honest = rotation_increment(gaussian(&mut run.rng, FIX_SIGMA)) * run.truth;
    assert!(
        run.filter
            .update(honest, &fix_covariance(), 16.27)
            .unwrap()
            .accepted
    );
}
