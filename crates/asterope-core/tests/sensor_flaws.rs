//! What each lens and detector flaw of the simulator costs the solver.
//!
//! `cargo test --release -p asterope-core --test sensor_flaws -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::index::PairIndex;
use asterope_core::math::{Vec3, DEG};
use asterope_core::pipeline::{RollingShutter, SolverConfig};
use asterope_core::sim::{Imperfections, SensorModel};
use asterope_core::validation::{percentile, run_trial, Scenario, TrialClass};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 300;

struct Measured {
    locks: f64,
    /// Solutions more than 0.1 degrees from the truth.
    far_off: u64,
    cross: f64,
    cross_95: f64,
    roll: f64,
}

fn measure(
    catalog: &Catalog,
    index: &PairIndex,
    camera: Camera,
    sensor: SensorModel,
    undo_shutter: bool,
) -> Measured {
    let scenario = Scenario {
        camera,
        sensor,
        solver: SolverConfig {
            rolling: undo_shutter.then_some(RollingShutter {
                row_time: sensor.imperfections.row_time,
                rate: sensor.body_rate,
            }),
            ..SolverConfig::default()
        },
        disturbances: false,
    };
    let (mut cross, mut roll) = (Vec::new(), Vec::new());
    let mut far_off = 0;
    for trial in 0..TRIALS {
        let outcome = run_trial(catalog, index, &scenario, 52, trial).unwrap();
        match outcome.class {
            TrialClass::Success | TrialClass::Inaccurate => {
                cross.push(outcome.cross_boresight_arcsec);
                roll.push(outcome.roll_arcsec);
            }
            TrialClass::FalsePositive => far_off += 1,
            TrialClass::NoSolution => {}
        }
    }
    cross.sort_by(f64::total_cmp);
    roll.sort_by(f64::total_cmp);
    Measured {
        locks: 100.0 * cross.len() as f64 / TRIALS as f64,
        far_off,
        cross: percentile(&cross, 0.5),
        cross_95: percentile(&cross, 0.95),
        roll: percentile(&roll, 0.5),
    }
}

#[test]
fn each_flaw_is_measured_against_the_ideal_sensor() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let none = Imperfections::default();
    let with = |imperfections: Imperfections| SensorModel {
        imperfections,
        ..SensorModel::default()
    };
    let slewing = SensorModel {
        body_rate: Vec3::new(1.0 * DEG, 0.0, 0.0),
        ..SensorModel::default()
    };
    let rolling = |row_time: f64| SensorModel {
        imperfections: Imperfections { row_time, ..none },
        ..slewing
    };
    let everything = Imperfections {
        vignetting: 1.0,
        prnu: 0.02,
        dsnu: 100.0,
        column_noise: 5.0,
        psf_growth: 1.0,
        coma: 0.15,
        nonlinearity: 0.1,
        blooming: 1.0,
        ..none
    };
    let cases = [
        ("Ideal sensor", with(none), false),
        (
            "Vignetting, full cos^4",
            with(Imperfections {
                vignetting: 1.0,
                ..none
            }),
            false,
        ),
        (
            "Gain pattern, 2 % RMS",
            with(Imperfections { prnu: 0.02, ..none }),
            false,
        ),
        (
            "Dark pattern, 100 e-/s RMS",
            with(Imperfections {
                dsnu: 100.0,
                ..none
            }),
            false,
        ),
        (
            "Column noise, 5 e- RMS",
            with(Imperfections {
                column_noise: 5.0,
                ..none
            }),
            false,
        ),
        (
            "PSF twice as wide at the corner",
            with(Imperfections {
                psf_growth: 1.0,
                ..none
            }),
            false,
        ),
        (
            "Coma, 15 % flare at the corner",
            with(Imperfections { coma: 0.15, ..none }),
            false,
        ),
        (
            "Nonlinearity 10 %, full blooming",
            with(Imperfections {
                nonlinearity: 0.1,
                blooming: 1.0,
                ..none
            }),
            false,
        ),
        ("All of the above", with(everything), false),
        ("1 deg/s, global shutter", slewing, false),
        (
            "1 deg/s, rolling shutter 20 us a row",
            rolling(20e-6),
            false,
        ),
        ("the same, undone with the rate", rolling(20e-6), true),
        (
            "1 deg/s, rolling shutter 60 us a row",
            rolling(60e-6),
            false,
        ),
        ("the same, undone with the rate", rolling(60e-6), true),
    ];
    println!("| Sensor | Locks (%) | Over 0.1 deg off | Cross median (\") | Cross 95 % (\") | Roll median (\") |");
    println!("|---|---|---|---|---|---|");
    for (name, sensor, undo_shutter) in cases {
        let m = measure(&catalog, &index, camera, sensor, undo_shutter);
        println!(
            "| {name} | {:.1} | {} | {:.2} | {:.2} | {:.1} |",
            m.locks, m.far_off, m.cross, m.cross_95, m.roll
        );
        // A rolling shutter left in the image bends it; nothing else may
        // put a solution far from the truth.
        let bent = sensor.imperfections.row_time > 0.0 && !undo_shutter;
        assert!(m.far_off == 0 || bent, "{name}");
    }
}
