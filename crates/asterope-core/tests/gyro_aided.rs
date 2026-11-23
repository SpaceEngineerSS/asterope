//! A slewing camera with a rolling shutter, blinded for twenty seconds:
//! the star tracker on its own against the star tracker with a gyro.
//!
//! `cargo test --release -p asterope-core --test gyro_aided -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::filter::GyroNoise;
use asterope_core::flight::{GyroAiding, StarTracker};
use asterope_core::index::PairIndex;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, Gyro, Imperfections, SensorModel};
use asterope_core::track::{rotation_increment, TrackEvent};
use asterope_core::validation::attitude_error;

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const GYRO_STEP: f64 = 0.02;
const GYRO_STEPS_PER_FRAME: usize = 10;
const FRAMES: usize = 300;
/// Frames during which the camera sees no stars.
const BLIND: core::ops::Range<usize> = 100..200;
const ROW_TIME: f64 = 60e-6;
const NOISE: GyroNoise = GyroNoise {
    angle_random_walk: 0.003 * DEG,
    rate_random_walk: 3e-5 * DEG,
};

fn rate_at(t: f64) -> Vec3 {
    Vec3::new(
        0.8 * DEG * (0.05 * t).cos(),
        0.3 * DEG * (0.08 * t).sin(),
        0.4 * DEG,
    )
}

#[derive(Default)]
struct Tally {
    with_attitude: usize,
    cross: f64,
    roll: f64,
    scored: usize,
    worst_blind: f64,
    searches_after_blind: usize,
}

impl Tally {
    fn score(&mut self, frame: usize, estimate: Option<Quat>, truth: Quat) {
        let Some(estimate) = estimate else {
            return;
        };
        self.with_attitude += 1;
        let (cross, roll, total) = attitude_error(estimate, truth);
        if BLIND.contains(&frame) {
            self.worst_blind = self.worst_blind.max(total);
        } else if frame >= 20 {
            self.cross += cross * cross;
            self.roll += roll * roll;
            self.scored += 1;
        }
    }

    fn row(&self, name: &str) -> String {
        let rms = |sum: f64| (sum / self.scored.max(1) as f64).sqrt() / ARCSEC;
        let blind = if self.worst_blind > 0.0 {
            format!("{:.0}\"", self.worst_blind / ARCSEC)
        } else {
            "no attitude".to_string()
        };
        format!(
            "| {name} | {} of {FRAMES} | {:.1} | {:.1} | {blind} | {} |",
            self.with_attitude,
            rms(self.cross),
            rms(self.roll),
            self.searches_after_blind
        )
    }
}

#[test]
fn a_gyro_carries_the_attitude_through_what_the_camera_cannot_see() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let mut alone = StarTracker::new(camera, catalog.clone(), index.clone()).unwrap();
    let mut aided = StarTracker::new(camera, catalog.clone(), index).unwrap();
    aided.enable_gyro(GyroAiding {
        row_time: ROW_TIME,
        ..GyroAiding::new(NOISE)
    });

    let mut rng = Rng::new(2024);
    let mut truth = rng.unit_quat();
    let mut gyro = Gyro {
        noise: NOISE,
        bias: Vec3::new(0.02 * DEG, -0.015 * DEG, 0.01 * DEG),
    };
    let (mut tally_alone, mut tally_aided) = (Tally::default(), Tally::default());
    let mut time = 0.0;
    for frame in 0..FRAMES {
        for _ in 0..GYRO_STEPS_PER_FRAME {
            let rate = rate_at(time + 0.5 * GYRO_STEP);
            truth = (rotation_increment(rate.scale(GYRO_STEP)) * truth)
                .normalized()
                .unwrap();
            aided.gyro(gyro.read(rate, GYRO_STEP, &mut rng), GYRO_STEP);
            time += GYRO_STEP;
        }
        let sensor = SensorModel {
            body_rate: rate_at(time),
            imperfections: Imperfections {
                row_time: ROW_TIME,
                ..Imperfections::default()
            },
            ..SensorModel::default()
        };
        let stars = if BLIND.contains(&frame) {
            &[][..]
        } else {
            catalog.stars()
        };
        let image = simulate(stars, &truth.to_matrix(), &camera, &sensor, rng.next_u64()).unwrap();

        let fix = alone
            .update(&image.pixels, GYRO_STEP * GYRO_STEPS_PER_FRAME as f64)
            .unwrap();
        tally_alone.score(frame, fix.attitude(), truth);
        if frame >= BLIND.end {
            tally_alone.searches_after_blind += usize::from(fix.event == TrackEvent::Acquired);
        }
        let fix = aided.update_aided(&image.pixels).unwrap();
        tally_aided.score(frame, fix.attitude, truth);
        if frame >= BLIND.end {
            tally_aided.searches_after_blind += usize::from(fix.fix.event == TrackEvent::Acquired);
        }
        if frame == FRAMES - 1 {
            let bias = (gyro.bias - fix.gyro_bias).norm() / DEG * 3600.0;
            println!("gyro bias error at the end: {bias:.1} deg/h, from 97 at the start");
            assert!(bias < 20.0);
        }
    }
    println!("| | Frames with an attitude | Cross RMS (\") | Roll RMS (\") | Largest error while blind | Searches after the gap |");
    println!("|---|---|---|---|---|---|");
    println!("{}", tally_alone.row("Star tracker alone"));
    println!("{}", tally_aided.row("With the gyro"));
    assert_eq!(tally_aided.with_attitude, FRAMES);
    assert!(tally_aided.cross < tally_alone.cross && tally_aided.roll < tally_alone.roll);
    assert!(tally_aided.worst_blind < 0.5 * DEG);
    assert_eq!(tally_aided.searches_after_blind, 0);
}
