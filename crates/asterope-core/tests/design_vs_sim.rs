//! The closed-form design predictions against the simulator.
//!
//! `cargo test --release -p asterope-core --test design_vs_sim -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::catalog::Catalog;
use asterope_core::design::{evaluate, Exposure, Optics};
use asterope_core::detect::DetectConfig;
use asterope_core::index::PairIndex;
use asterope_core::math::DEG;
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::validation::{percentile, run_trial, Scenario, TrialClass};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 150;

struct Measured {
    locks: f64,
    false_locks: u32,
    detections: f64,
    cross: f64,
    roll: f64,
}

fn measure(catalog: &Catalog, optics: &Optics, exposure: &Exposure) -> Measured {
    let camera = optics.camera().unwrap();
    let sensor = SensorModel {
        zero_point: optics.zero_point(),
        exposure: exposure.seconds,
        psf_sigma: exposure.psf_sigma_px,
        read_noise: exposure.read_noise,
        dark_current: exposure.dark_current,
        sky_background: exposure.sky_background,
        ..SensorModel::default()
    };
    let scenario = Scenario {
        camera,
        sensor,
        solver: SolverConfig {
            detect: DetectConfig {
                peak_sigma: exposure.peak_sigma,
                max_detections: 150,
                ..DetectConfig::default()
            },
            ..SolverConfig::default()
        },
        disturbances: false,
    };
    let index = PairIndex::for_camera(catalog, &camera, 25.0, 4.0).unwrap();
    let (mut cross, mut roll) = (Vec::new(), Vec::new());
    let (mut detections, mut false_locks) = (0u64, 0u32);
    for trial in 0..TRIALS {
        let outcome = run_trial(catalog, &index, &scenario, 77, trial).unwrap();
        detections += u64::from(outcome.detections);
        match outcome.class {
            TrialClass::Success | TrialClass::Inaccurate => {
                cross.push(outcome.cross_boresight_arcsec);
                roll.push(outcome.roll_arcsec);
            }
            TrialClass::FalsePositive => false_locks += 1,
            TrialClass::NoSolution => {}
        }
    }
    cross.sort_by(f64::total_cmp);
    roll.sort_by(f64::total_cmp);
    Measured {
        locks: cross.len() as f64 / TRIALS as f64,
        false_locks,
        detections: detections as f64 / TRIALS as f64,
        cross: percentile(&cross, 0.5),
        roll: percentile(&roll, 0.5),
    }
}

#[test]
fn predictions_agree_with_the_simulator() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let exposure = Exposure {
        seconds: 0.1,
        psf_sigma_px: 1.2,
        read_noise: 5.0,
        dark_current: 10.0,
        sky_background: 20.0,
        body_rate: 0.0,
        peak_sigma: 5.0,
    };
    println!("| Lens | Field (deg) | Stars per frame, predicted / detected | Locks, predicted / measured (%) | Cross median, predicted / measured (\") | Roll median, predicted / measured (\") | False locks |");
    println!("|---|---|---|---|---|---|---|");
    for (focal_length_mm, aperture_mm) in [
        (17.4, 14.53),
        (25.0, 14.53),
        (35.0, 14.53),
        (50.0, 14.53),
        (25.0, 5.0),
    ] {
        let optics = Optics {
            focal_length_mm,
            aperture_mm,
            pixel_um: 6.0,
            width: 1024,
            height: 1024,
            transmission: 0.8,
            quantum_efficiency: 0.5,
            bandwidth_nm: 300.0,
        };
        let predicted = evaluate(&catalog, &optics, &exposure).unwrap();
        let measured = measure(&catalog, &optics, &exposure);
        println!(
            "| {focal_length_mm} mm f/{:.1} | {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.2} / {:.2} | {:.1} / {:.1} | {} |",
            optics.focal_ratio(),
            predicted.camera.fov_x() / DEG,
            predicted.stars_per_field,
            measured.detections,
            100.0 * predicted.coverage.fractions[1],
            100.0 * measured.locks,
            predicted.median_cross_arcsec(),
            measured.cross,
            predicted.median_roll_arcsec(),
            measured.roll,
            measured.false_locks
        );
        assert_eq!(measured.false_locks, 0);
        let stars = measured.detections / predicted.stars_per_field;
        assert!((0.7..1.3).contains(&stars), "star count ratio {stars}");
        assert!((predicted.coverage.fractions[1] - measured.locks).abs() < 0.12);
        let cross = measured.cross / predicted.median_cross_arcsec();
        assert!((0.5..2.0).contains(&cross), "cross ratio {cross}");
        let roll = measured.roll / predicted.median_roll_arcsec();
        assert!((0.5..2.0).contains(&roll), "roll ratio {roll}");
    }
}
