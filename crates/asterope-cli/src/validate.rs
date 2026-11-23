//! Lost-in-space Monte Carlo runner: `asterope validate --trials 10000 --seed 1 --sweep`.

use std::error::Error;
use std::time::Instant;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::{detect, Frame};
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::math::DEG;
use asterope_core::pipeline::{identify_detections, SolverConfig};
use asterope_core::rng::{derive_seed, Rng};
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::validation::{
    clopper_pearson, disturbed_sensor, percentile, run_trial, run_trial_onboard, Scenario,
    StressParameter, Tally, TrialClass, TrialOutcome,
};
use rayon::prelude::*;

use crate::commands::{BLEND_PIXELS, CATALOG, STARS_PER_FIELD};
use crate::report;
const SWEEP_FOV_DEG: [f64; 5] = [8.0, 12.0, 16.0, 20.0, 25.0];

struct Options {
    trials: u64,
    seed: u64,
    fov_deg: f64,
    size: u32,
    sweep: bool,
    /// Trials per step of the stress sweeps; 0 skips them.
    stress: u64,
    /// Trials per onboard index profile; 0 skips them.
    profiles: u64,
    out: String,
}

fn parse_options(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options {
        trials: 1000,
        seed: 1,
        fov_deg: 14.0,
        size: 1024,
        sweep: false,
        stress: 0,
        profiles: 0,
        out: "validation-report".to_string(),
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("missing value for {flag}"));
        match flag.as_str() {
            "--trials" => options.trials = value()?.parse()?,
            "--seed" => options.seed = value()?.parse()?,
            "--fov" => options.fov_deg = value()?.parse()?,
            "--size" => options.size = value()?.parse()?,
            "--out" => options.out = value()?.clone(),
            "--sweep" => options.sweep = true,
            "--stress" => options.stress = value()?.parse()?,
            "--profiles" => options.profiles = value()?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    Ok(options)
}

pub struct Accuracy {
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

impl Accuracy {
    fn from(mut samples: Vec<f64>) -> Accuracy {
        samples.sort_by(f64::total_cmp);
        Accuracy {
            median: percentile(&samples, 0.5),
            p95: percentile(&samples, 0.95),
            max: percentile(&samples, 1.0),
        }
    }
}

pub struct StageTimes {
    pub render_ms: f64,
    pub detect_ms: f64,
    pub identify_ms: f64,
}

pub struct RunSummary {
    pub fov_deg: f64,
    pub size: u32,
    pub index_stars: usize,
    pub index_pairs: usize,
    pub index_mag_limit: f64,
    pub index_build_ms: f64,
    pub tally: Tally,
    pub success_interval: (f64, f64),
    pub false_positive_interval: (f64, f64),
    pub cross: Accuracy,
    pub roll: Accuracy,
    pub mean_truth_stars: f64,
    pub mean_detections: f64,
    pub failures: Vec<(u64, TrialOutcome)>,
    pub times: StageTimes,
    pub wall_seconds: f64,
}

fn scenario(fov_deg: f64, size: u32) -> Result<Scenario, Box<dyn Error>> {
    Ok(Scenario {
        camera: Camera::from_fov(size, size, fov_deg * DEG).map_err(|e| e.to_string())?,
        sensor: SensorModel::default(),
        solver: SolverConfig::default(),
        disturbances: true,
    })
}

/// Median per-stage time over a sample of trials, measured single-threaded.
fn time_stages(
    sky: &Catalog,
    catalog: &Catalog,
    index: &PairIndex,
    scenario: &Scenario,
    master_seed: u64,
) -> Result<StageTimes, Box<dyn Error>> {
    const SAMPLES: u64 = 40;
    let (mut render, mut detection, mut identify) = (Vec::new(), Vec::new(), Vec::new());
    for trial in 0..SAMPLES {
        let mut rng = Rng::new(derive_seed(master_seed, trial));
        let attitude = rng.unit_quat().to_matrix();
        let sensor = disturbed_sensor(&scenario.sensor, &mut rng);
        let start = Instant::now();
        let frame = simulate(
            sky.stars(),
            &attitude,
            &scenario.camera,
            &sensor,
            rng.next_u64(),
        )
        .map_err(|e| e.to_string())?;
        render.push(start.elapsed().as_secs_f64() * 1e3);

        let view =
            Frame::new(frame.width, frame.height, &frame.pixels).map_err(|e| e.to_string())?;
        let start = Instant::now();
        let detected = detect(&view, &scenario.solver.detect).map_err(|e| e.to_string())?;
        detection.push(start.elapsed().as_secs_f64() * 1e3);

        let start = Instant::now();
        let _ = identify_detections(
            &detected.detections,
            &scenario.camera,
            index,
            catalog,
            &scenario.solver,
            0,
            asterope_core::pipeline::MAX_PYRAMIDS_PER_FRAME,
        );
        identify.push(start.elapsed().as_secs_f64() * 1e3);
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        percentile(&v, 0.5)
    };
    Ok(StageTimes {
        render_ms: median(render),
        detect_ms: median(detection),
        identify_ms: median(identify),
    })
}

fn run_field(
    catalog: &Catalog,
    fov_deg: f64,
    size: u32,
    trials: u64,
    seed: u64,
) -> Result<RunSummary, Box<dyn Error>> {
    let scenario = scenario(fov_deg, size)?;
    let start = Instant::now();
    let index = PairIndex::for_camera(catalog, &scenario.camera, STARS_PER_FIELD, BLEND_PIXELS)
        .map_err(|e| e.to_string())?;
    let index_build_ms = start.elapsed().as_secs_f64() * 1e3;

    let start = Instant::now();
    let outcomes: Vec<TrialOutcome> = (0..trials)
        .into_par_iter()
        .map(|trial| run_trial(catalog, &index, &scenario, seed, trial))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let wall_seconds = start.elapsed().as_secs_f64();

    let mut tally = Tally::default();
    let (mut cross, mut roll, mut failures) = (Vec::new(), Vec::new(), Vec::new());
    for (trial, outcome) in outcomes.iter().enumerate() {
        tally.add(outcome);
        match outcome.class {
            TrialClass::Success | TrialClass::Inaccurate => {
                cross.push(outcome.cross_boresight_arcsec);
                roll.push(outcome.roll_arcsec);
            }
            TrialClass::FalsePositive | TrialClass::NoSolution => {}
        }
        if outcome.class != TrialClass::Success {
            failures.push((trial as u64, *outcome));
        }
    }
    let count = outcomes.len().max(1) as f64;
    Ok(RunSummary {
        fov_deg,
        size,
        index_stars: index.star_count(),
        index_pairs: index.pair_count(),
        index_mag_limit: index.mag_limit(),
        index_build_ms,
        tally,
        success_interval: clopper_pearson(tally.success, tally.trials),
        false_positive_interval: clopper_pearson(tally.false_positive, tally.trials),
        cross: Accuracy::from(cross),
        roll: Accuracy::from(roll),
        mean_truth_stars: outcomes
            .iter()
            .map(|o| f64::from(o.truth_stars))
            .sum::<f64>()
            / count,
        mean_detections: outcomes
            .iter()
            .map(|o| f64::from(o.detections))
            .sum::<f64>()
            / count,
        failures,
        times: time_stages(catalog, catalog, &index, &scenario, seed)?,
        wall_seconds,
    })
}

pub struct StressRow {
    pub value: f64,
    pub tally: Tally,
    pub cross_median: f64,
}

pub struct StressTable {
    pub parameter: StressParameter,
    pub rows: Vec<StressRow>,
}

/// Sweeps each stress parameter alone, with every other disturbance off and
/// the same attitudes at every step.
fn stress(
    catalog: &Catalog,
    fov_deg: f64,
    size: u32,
    trials: u64,
    seed: u64,
) -> Result<Vec<StressTable>, Box<dyn Error>> {
    let base = Scenario {
        disturbances: false,
        ..scenario(fov_deg, size)?
    };
    let index = PairIndex::for_camera(catalog, &base.camera, STARS_PER_FIELD, BLEND_PIXELS)
        .map_err(|e| e.to_string())?;
    let mut tables = Vec::new();
    for parameter in StressParameter::ALL {
        let mut rows = Vec::new();
        for value in parameter.values() {
            let scenario = Scenario {
                sensor: parameter.apply(&base.sensor, *value),
                ..base
            };
            let outcomes: Vec<TrialOutcome> = (0..trials)
                .into_par_iter()
                .map(|trial| run_trial(catalog, &index, &scenario, seed, trial))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?;
            let mut tally = Tally::default();
            let mut cross = Vec::new();
            for outcome in &outcomes {
                tally.add(outcome);
                if matches!(outcome.class, TrialClass::Success | TrialClass::Inaccurate) {
                    cross.push(outcome.cross_boresight_arcsec);
                }
            }
            cross.sort_by(f64::total_cmp);
            rows.push(StressRow {
                value: *value,
                tally,
                cross_median: percentile(&cross, 0.5),
            });
        }
        println!("stress sweep: {} done", parameter.label());
        tables.push(StressTable { parameter, rows });
    }
    Ok(tables)
}

/// An index and catalog sized for onboard storage.
#[derive(Clone, Copy)]
pub struct Profile {
    pub name: &'static str,
    pub stars_per_field: f64,
    /// Largest indexed separation as a fraction of the frame diagonal.
    pub angle_fraction: f64,
    /// Onboard catalog depth beyond the faintest indexed star, magnitudes;
    /// `None` keeps the whole catalog.
    pub catalog_margin: Option<f64>,
}

const PROFILES: [Profile; 8] = [
    Profile {
        name: "bench",
        stars_per_field: STARS_PER_FIELD,
        angle_fraction: 1.0,
        catalog_margin: None,
    },
    Profile {
        name: "wide-16",
        stars_per_field: 16.0,
        angle_fraction: 1.0,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "wide-12",
        stars_per_field: 12.0,
        angle_fraction: 1.0,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "near-16",
        stars_per_field: 16.0,
        angle_fraction: 0.6,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "tight-16",
        stars_per_field: 16.0,
        angle_fraction: 0.5,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "near-14",
        stars_per_field: 14.0,
        angle_fraction: 0.6,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "near-12",
        stars_per_field: 12.0,
        angle_fraction: 0.6,
        catalog_margin: Some(0.5),
    },
    Profile {
        name: "near-8",
        stars_per_field: 8.0,
        angle_fraction: 0.6,
        catalog_margin: Some(0.5),
    },
];

pub struct ProfileRow {
    pub profile: Profile,
    pub index_stars: usize,
    pub index_pairs: usize,
    pub index_mag_limit: f64,
    pub index_bytes: usize,
    pub catalog_stars: usize,
    pub catalog_file_bytes: usize,
    pub tally: Tally,
    pub cross_median: f64,
    pub identify_ms: f64,
}

/// Runs the baseline Monte Carlo against each onboard profile. The index is
/// serialized and read back in place, as flight software would use it, and
/// the solver sees only the cut catalog while frames show the whole sky.
fn profiles(
    sky: &Catalog,
    fov_deg: f64,
    size: u32,
    trials: u64,
    seed: u64,
) -> Result<Vec<ProfileRow>, Box<dyn Error>> {
    let scenario = scenario(fov_deg, size)?;
    let mut rows = Vec::new();
    for profile in PROFILES {
        let mut config =
            IndexConfig::for_camera(sky, &scenario.camera, profile.stars_per_field, BLEND_PIXELS)
                .map_err(|e| e.to_string())?;
        config.max_angle *= profile.angle_fraction;
        let built = PairIndex::build(sky, &config).map_err(|e| e.to_string())?;
        let index = PairIndex::from_bytes(built.as_bytes()).map_err(|e| e.to_string())?;
        let catalog = match profile.catalog_margin {
            Some(margin) => sky.brighter_than(index.mag_limit() + margin),
            None => sky.clone(),
        };
        let outcomes: Vec<TrialOutcome> = (0..trials)
            .into_par_iter()
            .map(|trial| run_trial_onboard(sky, &catalog, &index, &scenario, seed, trial))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut tally = Tally::default();
        let mut cross = Vec::new();
        for outcome in &outcomes {
            tally.add(outcome);
            if matches!(outcome.class, TrialClass::Success | TrialClass::Inaccurate) {
                cross.push(outcome.cross_boresight_arcsec);
            }
        }
        cross.sort_by(f64::total_cmp);
        let row = ProfileRow {
            profile,
            index_stars: index.star_count(),
            index_pairs: index.pair_count(),
            index_mag_limit: index.mag_limit(),
            index_bytes: index.as_bytes().len(),
            catalog_stars: catalog.len(),
            catalog_file_bytes: catalog.encode().len(),
            tally,
            cross_median: percentile(&cross, 0.5),
            identify_ms: time_stages(sky, &catalog, &index, &scenario, seed)?.identify_ms,
        };
        println!("{}", report::profile_line(&row));
        rows.push(row);
    }
    Ok(rows)
}

/// Runs the suites and writes the report; alse when the baseline misses its criteria.
pub fn run(args: &[String]) -> Result<bool, Box<dyn Error>> {
    let options = parse_options(args)?;
    let catalog = Catalog::parse(CATALOG).map_err(|e| e.to_string())?;

    let baseline = run_field(
        &catalog,
        options.fov_deg,
        options.size,
        options.trials,
        options.seed,
    )?;
    println!("{}", report::console_line(&baseline));
    let mut sweep = Vec::new();
    if options.sweep {
        for fov in SWEEP_FOV_DEG {
            let summary = run_field(&catalog, fov, options.size, options.trials, options.seed)?;
            println!("{}", report::console_line(&summary));
            sweep.push(summary);
        }
    }

    let mut markdown = report::markdown(&baseline, &sweep, options.seed);
    if options.stress > 0 {
        let tables = stress(
            &catalog,
            options.fov_deg,
            options.size,
            options.stress,
            options.seed,
        )?;
        markdown.push_str(&report::stress_markdown(&tables, options.stress));
    }
    if options.profiles > 0 {
        let rows = profiles(
            &catalog,
            options.fov_deg,
            options.size,
            options.profiles,
            options.seed,
        )?;
        markdown.push_str(&report::profiles_markdown(
            &rows,
            options.fov_deg,
            options.profiles,
        ));
    }
    let json = report::json(&baseline, &sweep, options.seed);
    std::fs::write(format!("{}.md", options.out), markdown)?;
    std::fs::write(format!("{}.json", options.out), json)?;
    println!("wrote {0}.md and {0}.json", options.out);
    Ok(report::passes(&baseline))
}
