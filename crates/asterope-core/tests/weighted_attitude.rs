//! Attitude error with every star counted equally against stars weighed by
//! the expected accuracy of their centroids.
//!
//! `cargo test --release -p asterope-core --test weighted_attitude -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::index::PairIndex;
use asterope_core::math::DEG;
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::validation::{percentile, run_trial, Scenario, TrialClass};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 400;

struct Errors {
    cross: f64,
    cross_95: f64,
    cross_max: f64,
    roll: f64,
    roll_max: f64,
    locks: u64,
}

fn errors(
    catalog: &Catalog,
    camera: Camera,
    sensor: SensorModel,
    weighted: bool,
    disturbances: bool,
    trials: u64,
) -> Errors {
    let scenario = Scenario {
        camera,
        sensor,
        solver: SolverConfig {
            weighted,
            ..SolverConfig::default()
        },
        disturbances,
    };
    let index = PairIndex::for_camera(catalog, &camera, 25.0, 4.0).unwrap();
    let (mut cross, mut roll) = (Vec::new(), Vec::new());
    for trial in 0..trials {
        let outcome = run_trial(catalog, &index, &scenario, 31, trial).unwrap();
        assert_ne!(outcome.class, TrialClass::FalsePositive);
        if matches!(outcome.class, TrialClass::Success | TrialClass::Inaccurate) {
            cross.push(outcome.cross_boresight_arcsec);
            roll.push(outcome.roll_arcsec);
        }
    }
    cross.sort_by(f64::total_cmp);
    roll.sort_by(f64::total_cmp);
    Errors {
        cross: percentile(&cross, 0.5),
        cross_95: percentile(&cross, 0.95),
        cross_max: cross.last().copied().unwrap_or(f64::NAN),
        roll: percentile(&roll, 0.5),
        roll_max: roll.last().copied().unwrap_or(f64::NAN),
        locks: cross.len() as u64,
    }
}

#[test]
fn weighing_stars_by_centroid_accuracy_does_not_cost_accuracy() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    println!("| Field (deg) | Read noise (e-) | Hot pixels, cosmic rays, stray light | Locks | Cross median, equal / weighted (\") | Cross 95 % (\") | Cross worst (\") | Roll median (\") | Roll worst (\") |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (fov, read_noise, disturbances, trials) in [
        (14.0, 5.0, false, TRIALS),
        (14.0, 20.0, false, TRIALS),
        (20.0, 5.0, false, TRIALS),
        (8.0, 5.0, false, TRIALS),
        (14.0, 5.0, true, 3000),
    ] {
        let camera = Camera::from_fov(1024, 1024, fov * DEG).unwrap();
        let sensor = SensorModel {
            read_noise,
            ..SensorModel::default()
        };
        let equal = errors(&catalog, camera, sensor, false, disturbances, trials);
        let weighted = errors(&catalog, camera, sensor, true, disturbances, trials);
        println!(
            "| {fov} | {read_noise} | {} | {} / {} | {:.2} / {:.2} | {:.2} / {:.2} | {:.1} / {:.1} | {:.1} / {:.1} | {:.0} / {:.0} |",
            if disturbances { "yes" } else { "no" },
            equal.locks,
            weighted.locks,
            equal.cross,
            weighted.cross,
            equal.cross_95,
            weighted.cross_95,
            equal.cross_max,
            weighted.cross_max,
            equal.roll,
            weighted.roll,
            equal.roll_max,
            weighted.roll_max
        );
        assert_eq!(equal.locks, weighted.locks);
        assert!(weighted.cross <= 1.05 * equal.cross);
        assert!(weighted.roll <= 1.05 * equal.roll);
        // Weighing must not make the rare bad frame worse than counting
        // every star the same did.
        assert!(
            weighted.cross_95 <= 1.1 * equal.cross_95,
            "{fov} deg: 95 % {} against {}",
            weighted.cross_95,
            equal.cross_95
        );
    }
}
