//! Fields of 2 to 4 degrees with the V <= 8 Hipparcos catalog against the
//! Hipparcos and Tycho-2 catalog to V 9.5.
//!
//! The Tycho-2 catalog is not kept in the repository. Build it as
//! `data/PROVENANCE.md` describes, then
//! `cargo test --release -p asterope-core --test tycho_fields -- --nocapture`
//! prints the table quoted in `docs/validation.md`. Without the file the
//! test reports that and passes.

use std::time::Instant;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::DetectConfig;
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::math::DEG;
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::validation::{run_trial, Scenario, TrialClass};
use asterope_core::AsteropeError;

const HIPPARCOS: &[u8] = include_bytes!("../../../data/hip80.astc");
const TYCHO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/raw/tyc95.astc");
const TRIALS: u64 = 300;

struct Row {
    locks: u64,
    false_locks: u64,
    index_stars: usize,
    index_bytes: usize,
    milliseconds: f64,
}

/// The builder's own limit on the number of pairs.
const DEFAULT_PAIRS: usize = 3_000_000;
const MORE_PAIRS: usize = 12_000_000;

fn run(catalog: &Catalog, fov_deg: f64, max_pairs: usize) -> Row {
    let camera = Camera::from_fov(1024, 1024, fov_deg * DEG).unwrap();
    // Sixteen times the collecting area of the reference sensor, which is
    // what a V 9.5 star needs to stand five sigma above this noise.
    let sensor = SensorModel {
        zero_point: 3.2e7,
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
    let mut config = IndexConfig::for_camera(catalog, &camera, 25.0, 4.0).unwrap();
    config.max_pairs = max_pairs;
    let index = loop {
        match PairIndex::build(catalog, &config) {
            Err(AsteropeError::IndexTooLarge) => config.star_count = config.star_count * 4 / 5,
            other => break other.unwrap(),
        }
    };
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
        index_stars: index.star_count(),
        index_bytes: index.as_bytes().len(),
        milliseconds: started.elapsed().as_secs_f64() * 1000.0 / TRIALS as f64,
    }
}

#[test]
fn tycho_extends_the_narrow_field_limit() {
    let Ok(bytes) = std::fs::read(TYCHO) else {
        println!("data/raw/tyc95.astc is not present; nothing measured");
        return;
    };
    let hipparcos = Catalog::parse(HIPPARCOS).unwrap();
    let tycho = Catalog::parse(&bytes).unwrap();
    println!("| Field (deg) | Locks: V 8 / V 9.5 / V 9.5 with 12 M pairs (%) | False locks | Index stars | Index (MB) | Per frame (ms) |");
    println!("|---|---|---|---|---|---|");
    for fov in [1.5, 2.0, 3.0, 4.0] {
        let rows = [
            run(&hipparcos, fov, DEFAULT_PAIRS),
            run(&tycho, fov, DEFAULT_PAIRS),
            run(&tycho, fov, MORE_PAIRS),
        ];
        let column =
            |f: &dyn Fn(&Row) -> String| rows.iter().map(f).collect::<Vec<_>>().join(" / ");
        println!(
            "| {fov} | {} | {} | {} | {} | {} |",
            column(&|r| format!("{:.1}", 100.0 * r.locks as f64 / TRIALS as f64)),
            column(&|r| r.false_locks.to_string()),
            column(&|r| r.index_stars.to_string()),
            column(&|r| format!("{:.1}", r.index_bytes as f64 / 1e6)),
            column(&|r| format!("{:.1}", r.milliseconds)),
        );
        assert_eq!(rows.iter().map(|r| r.false_locks).sum::<u64>(), 0);
    }
}
