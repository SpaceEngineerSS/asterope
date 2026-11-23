//! Narrow fields with the V <= 6.5 catalog against the V <= 8 one.
//!
//! `cargo test --release -p asterope-core --test deep_catalog -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use std::time::Instant;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::DetectConfig;
use asterope_core::index::PairIndex;
use asterope_core::math::DEG;
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::validation::{run_trial, Scenario, TrialClass};

const SHALLOW: &[u8] = include_bytes!("../../../data/hip65.astc");
const DEEP: &[u8] = include_bytes!("../../../data/hip80.astc");
const TRIALS: u64 = 300;

struct Row {
    locks: u64,
    false_locks: u64,
    index_bytes: usize,
    milliseconds: f64,
}

fn run(catalog: &Catalog, fov_deg: f64) -> Row {
    let camera = Camera::from_fov(1024, 1024, fov_deg * DEG).unwrap();
    // Four times the collecting area of the reference sensor: a lens this
    // long has the aperture for it, and V 8 stars need it.
    let sensor = SensorModel {
        zero_point: 8.0e6,
        ..SensorModel::default()
    };
    let scenario = Scenario {
        camera,
        sensor,
        solver: SolverConfig {
            detect: DetectConfig {
                max_detections: 150,
                ..DetectConfig::default()
            },
            ..SolverConfig::default()
        },
        disturbances: false,
    };
    let index = PairIndex::for_camera(catalog, &camera, 25.0, 4.0).unwrap();
    let (mut locks, mut false_locks) = (0, 0);
    let started = Instant::now();
    for trial in 0..TRIALS {
        match run_trial(catalog, &index, &scenario, 8, trial)
            .unwrap()
            .class
        {
            TrialClass::Success | TrialClass::Inaccurate => locks += 1,
            TrialClass::FalsePositive => false_locks += 1,
            TrialClass::NoSolution => {}
        }
    }
    Row {
        locks,
        false_locks,
        index_bytes: index.as_bytes().len(),
        milliseconds: started.elapsed().as_secs_f64() * 1000.0 / TRIALS as f64,
    }
}

#[test]
fn a_deeper_catalog_extends_the_narrow_field_limit() {
    let shallow = Catalog::parse(SHALLOW).unwrap();
    let deep = Catalog::parse(DEEP).unwrap();
    println!("| Field (deg) | Locks, V 6.5 / V 8 (%) | False locks, V 6.5 / V 8 | Index, V 6.5 / V 8 (kB) | Per frame, V 6.5 / V 8 (ms) |");
    println!("|---|---|---|---|---|");
    for fov in [3.0, 4.0, 5.0, 6.0, 8.0] {
        let (a, b) = (run(&shallow, fov), run(&deep, fov));
        println!(
            "| {fov} | {:.1} / {:.1} | {} / {} | {} / {} | {:.1} / {:.1} |",
            100.0 * a.locks as f64 / TRIALS as f64,
            100.0 * b.locks as f64 / TRIALS as f64,
            a.false_locks,
            b.false_locks,
            a.index_bytes / 1000,
            b.index_bytes / 1000,
            a.milliseconds,
            b.milliseconds
        );
        assert_eq!(a.false_locks + b.false_locks, 0);
        assert!(b.locks >= a.locks);
    }
}
