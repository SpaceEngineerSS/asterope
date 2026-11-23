//! One revolution in low Earth orbit: how much of it a camera can see stars
//! in, and what a gyro does for the rest.
//!
//! `cargo test --release -p asterope-core --test orbit_availability -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::{Catalog, Star};
use asterope_core::ephem::{aberrate, beta, earth_velocity_km_s};
use asterope_core::filter::GyroNoise;
use asterope_core::flight::{GyroAiding, StarTracker};
use asterope_core::index::PairIndex;
use asterope_core::math::{Vec3, ARCSEC, DEG};
use asterope_core::orbit::{Blinder, Exclusion, Orbit, Pointing, Surroundings};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, Gyro, SensorModel};
use asterope_core::validation::attitude_error;

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const FRAME_PERIOD: f64 = 10.0;
const GYRO_STEP: f64 = 0.1;
const NOISE: GyroNoise = GyroNoise {
    angle_random_walk: 0.003 * DEG,
    rate_random_walk: 3e-5 * DEG,
};
const EXCLUSION: Exclusion = Exclusion {
    sun: 35.0 * DEG,
    earth_limb: 25.0 * DEG,
    moon: 10.0 * DEG,
};

#[derive(Default)]
struct Report {
    frames: usize,
    blinded: [usize; 3],
    alone: usize,
    aided: usize,
    /// Frames after the first star fix, and those of them with an attitude.
    since_first_fix: usize,
    aided_since_first_fix: usize,
    alone_error: f64,
    aided_error: f64,
    aided_worst: f64,
    uncorrected_error: f64,
    scored: usize,
    longest_gap: f64,
}

fn fly(
    catalog: &Catalog,
    camera: Camera,
    index: &PairIndex,
    pointing: Pointing,
    seed: u64,
) -> Report {
    // 550 km, 53 degrees: a common orbit for small satellites.
    let orbit = Orbit::circular(550.0, 53.0 * DEG, 40.0 * DEG, 2_460_400.5);
    let mut alone = StarTracker::new(camera, catalog.clone(), index.clone()).unwrap();
    let mut aided = StarTracker::new(camera, catalog.clone(), index.clone()).unwrap();
    let mut uncorrected = StarTracker::new(camera, catalog.clone(), index.clone()).unwrap();
    aided.enable_gyro(GyroAiding::new(NOISE));
    let mut rng = Rng::new(seed);
    let mut gyro = Gyro {
        noise: NOISE,
        bias: Vec3::new(0.01 * DEG, -0.02 * DEG, 0.015 * DEG),
    };
    let mut report = Report::default();
    let (mut gap, mut time) = (0.0, 0.0);
    let gyro_steps = (FRAME_PERIOD / GYRO_STEP) as usize;
    while time < orbit.period() {
        for step in 0..gyro_steps {
            let t = time + (step as f64 + 0.5) * GYRO_STEP;
            let (_, rate) = pointing.attitude(&orbit, t);
            aided.gyro(gyro.read(rate, GYRO_STEP, &mut rng), GYRO_STEP);
        }
        time += FRAME_PERIOD;
        let (truth, rate) = pointing.attitude(&orbit, time);
        let (position, velocity) = orbit.state(time);
        let jd = orbit.julian_date(time);
        let boresight = truth.to_matrix().r2;
        let blinder = Surroundings::of(boresight, position, jd).blinder(&EXCLUSION);

        // The stars as this observer sees them: displaced by the velocity
        // of the Earth about the Sun and of the spacecraft about the Earth.
        let speed = beta(earth_velocity_km_s(jd) + velocity);
        let sky: Vec<Star> = if blinder.is_some() {
            Vec::new()
        } else {
            catalog
                .stars()
                .iter()
                .map(|star| Star {
                    dir: aberrate(star.dir, speed),
                    ..*star
                })
                .collect()
        };
        let sensor = SensorModel {
            body_rate: rate,
            // A bright body near the boresight floods the frame.
            sky_background: if blinder.is_some() { 2.0e5 } else { 20.0 },
            ..SensorModel::default()
        };
        let image = simulate(&sky, &truth.to_matrix(), &camera, &sensor, rng.next_u64()).unwrap();

        let velocity_km_s = earth_velocity_km_s(jd) + velocity;
        alone.set_velocity(velocity_km_s);
        aided.set_velocity(velocity_km_s);
        let fix_alone = alone.update(&image.pixels, FRAME_PERIOD).unwrap();
        let fix_aided = aided.update_aided(&image.pixels).unwrap();
        let fix_uncorrected = uncorrected.update(&image.pixels, FRAME_PERIOD).unwrap();

        report.frames += 1;
        match blinder {
            Some(Blinder::Sun) => report.blinded[0] += 1,
            Some(Blinder::Earth) => report.blinded[1] += 1,
            Some(Blinder::Moon) => report.blinded[2] += 1,
            None => {}
        }
        if fix_alone.attitude().is_some() {
            report.alone += 1;
            gap = 0.0;
        } else {
            gap += FRAME_PERIOD;
            report.longest_gap = f64::max(report.longest_gap, gap);
        }
        if report.aided > 0 || fix_aided.attitude.is_some() {
            report.since_first_fix += 1;
            report.aided_since_first_fix += usize::from(fix_aided.attitude.is_some());
        }
        if let Some(q) = fix_aided.attitude {
            report.aided += 1;
            report.aided_worst = report.aided_worst.max(attitude_error(q, truth).2);
        }
        if let (Some(a), Some(b), Some(c)) = (
            fix_alone.attitude(),
            fix_aided.attitude,
            fix_uncorrected.attitude(),
        ) {
            if blinder.is_none() {
                report.alone_error += attitude_error(a, truth).2.powi(2);
                report.aided_error += attitude_error(b, truth).2.powi(2);
                report.uncorrected_error += attitude_error(c, truth).2.powi(2);
                report.scored += 1;
            }
        }
    }
    report
}

#[test]
fn availability_over_an_orbit() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let mountings = [
        (
            "Looking away from the Earth",
            Pointing::Local {
                boresight: Vec3::Z,
                roll: 0.0,
            },
        ),
        (
            "Along the orbit normal",
            Pointing::Local {
                boresight: Vec3::Y,
                roll: 0.0,
            },
        ),
        (
            "30 degrees above the orbit normal",
            Pointing::Local {
                boresight: Vec3::new(0.0, 0.866, 0.5),
                roll: 0.0,
            },
        ),
        (
            "Tumbling at 0.5 deg/s",
            Pointing::Spinning {
                start: Rng::new(5).unit_quat(),
                rate: Vec3::new(0.4 * DEG, 0.3 * DEG, 0.0),
            },
        ),
    ];
    println!("| Camera | Blinded by Sun / Earth / Moon (%) | Attitude: alone / with gyro (%) | Longest gap alone (s) | Error RMS: alone / with gyro (\") | Largest error with gyro (\") | Without the aberration correction (\") |");
    println!("|---|---|---|---|---|---|---|");
    for (name, pointing) in mountings {
        let r = fly(&catalog, camera, &index, pointing, 11);
        let share = |count: usize| 100.0 * count as f64 / r.frames as f64;
        let rms = |sum: f64| (sum / r.scored.max(1) as f64).sqrt() / ARCSEC;
        println!(
            "| {name} | {:.1} / {:.1} / {:.1} | {:.1} / {:.1} | {:.0} | {:.1} / {:.1} | {:.0} | {:.1} |",
            share(r.blinded[0]),
            share(r.blinded[1]),
            share(r.blinded[2]),
            share(r.alone),
            share(r.aided),
            r.longest_gap,
            rms(r.alone_error),
            rms(r.aided_error),
            r.aided_worst / ARCSEC,
            rms(r.uncorrected_error)
        );
        let seen = r.frames - r.blinded.iter().sum::<usize>();
        assert!(
            r.alone as f64 >= 0.97 * seen as f64,
            "{name}: {} of {seen}",
            r.alone
        );
        assert_eq!(r.aided_since_first_fix, r.since_first_fix, "{name}");
        assert!(
            r.scored == 0 || r.uncorrected_error > r.alone_error,
            "{name}"
        );
    }
}
