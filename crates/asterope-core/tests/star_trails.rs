//! A turning camera: stars drawn out into trails, rejected as before or
//! kept and centered along their length.
//!
//! `cargo test --release -p asterope-core --test star_trails -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::DetectConfig;
use asterope_core::index::PairIndex;
use asterope_core::math::{Vec3, DEG};
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::validation::{percentile, run_trial, Scenario, TrialClass};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 300;

struct Measured {
    locks: f64,
    /// Solutions more than 0.1 degrees from the truth.
    far_off: u64,
    /// Of those, the ones more than 2 degrees off: a wrong identification.
    misidentified: u64,
    cross: f64,
    stars: f64,
}

fn measure(
    catalog: &Catalog,
    index: &PairIndex,
    camera: Camera,
    rate: Vec3,
    trails: bool,
) -> Measured {
    let scenario = Scenario {
        camera,
        sensor: SensorModel {
            body_rate: rate,
            ..SensorModel::default()
        },
        solver: SolverConfig {
            detect: DetectConfig {
                trails,
                ..DetectConfig::default()
            },
            ..SolverConfig::default()
        },
        // Hot pixels and cosmic rays, so that keeping trails has to tell
        // them from particle tracks.
        disturbances: true,
    };
    let mut cross = Vec::new();
    let (mut far_off, mut misidentified, mut detections) = (0, 0, 0u64);
    for trial in 0..TRIALS {
        let outcome = run_trial(catalog, index, &scenario, 19, trial).unwrap();
        detections += u64::from(outcome.detections);
        match outcome.class {
            TrialClass::Success | TrialClass::Inaccurate => {
                cross.push(outcome.cross_boresight_arcsec)
            }
            TrialClass::FalsePositive => {
                far_off += 1;
                misidentified += u64::from(outcome.total_arcsec > 2.0 * 3600.0);
            }
            TrialClass::NoSolution => {}
        }
    }
    cross.sort_by(f64::total_cmp);
    Measured {
        locks: 100.0 * cross.len() as f64 / TRIALS as f64,
        far_off,
        misidentified,
        cross: percentile(&cross, 0.5),
        stars: detections as f64 / TRIALS as f64,
    }
}

fn row(label: &str, trail: &str, old: &Measured, new: &Measured) -> String {
    format!(
        "| {label} | {trail} | {:.1} / {:.1} | {} / {} | {:.1} / {:.1} | {:.2} / {:.2} |",
        old.locks, new.locks, old.far_off, new.far_off, old.stars, new.stars, old.cross, new.cross
    )
}

#[test]
fn keeping_trails_raises_the_rate_the_camera_can_turn_at() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let pixels_per_degree = DEG / camera.pixel_scale();
    println!("| Rate across (deg/s) | Trail (px) | Locks, rejected / kept (%) | Over 0.1 deg off | Stars found | Cross median (\") |");
    println!("|---|---|---|---|---|---|");
    let mut kept_at_two = 0.0;
    for rate in [0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0] {
        let across = Vec3::new(0.0, rate * DEG, 0.0);
        let old = measure(&catalog, &index, camera, across, false);
        let new = measure(&catalog, &index, camera, across, true);
        let trail = format!("{:.1}", rate * 0.1 * pixels_per_degree);
        println!("{}", row(&rate.to_string(), &trail, &old, &new));
        // A solution a fraction of a degree off has the right stars and a
        // poor fit; one that is degrees off has the wrong stars.
        assert_eq!(old.misidentified + new.misidentified, 0, "{rate} deg/s");
        assert!(new.locks >= old.locks - 1.0, "{rate} deg/s");
        if rate == 2.0 {
            kept_at_two = new.locks;
        }
    }
    println!("| About the boresight (deg/s) | | Locks, rejected / kept (%) | Over 0.1 deg off | Stars found | Cross median (\") |");
    for rate in [5.0, 10.0, 20.0] {
        let roll = Vec3::new(0.0, 0.0, rate * DEG);
        let old = measure(&catalog, &index, camera, roll, false);
        let new = measure(&catalog, &index, camera, roll, true);
        println!("{}", row(&rate.to_string(), "", &old, &new));
        assert_eq!(
            old.misidentified + new.misidentified,
            0,
            "roll {rate} deg/s"
        );
    }
    assert!(kept_at_two > 90.0);
}
