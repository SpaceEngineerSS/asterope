#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use asterope_core::attitude::{attitude_from_radec_roll, RaDecRoll};
use asterope_core::calib::{solve_with_scale_search, widest_camera, ScaleSearch};
use asterope_core::camera::{Camera, Distortion};
use asterope_core::catalog::Catalog;
use asterope_core::detect::Frame;
use asterope_core::index::PairIndex;
use asterope_core::math::{Quat, ARCSEC, DEG};
use asterope_core::pipeline::{solve_frame, SolverConfig};
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::{propagate, TrackConfig, TrackEvent, TrackScene, Tracker};
use asterope_core::validation::{
    clopper_pearson, percentile, run_trial, Scenario, Tally, TrialClass, FALSE_POSITIVE_DEG,
};

const STARS_PER_FIELD: f64 = 25.0;
const BLEND_PIXELS: f64 = 4.0;

fn catalog() -> Catalog {
    Catalog::parse(include_bytes!("../../../data/hip65.astc")).unwrap()
}

fn baseline() -> Scenario {
    Scenario {
        camera: Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap(),
        sensor: SensorModel::default(),
        solver: SolverConfig::default(),
        disturbances: true,
    }
}

#[test]
fn embedded_catalog_is_sane() {
    let catalog = catalog();
    assert!(
        catalog.len() > 8000 && catalog.len() < 9500,
        "{} stars",
        catalog.len()
    );
    assert!((catalog.epoch_year() - 2000.0).abs() < 1e-6);
    let sirius = catalog.stars().iter().find(|s| s.hip == 32349).unwrap();
    assert!((sirius.mag + 1.44).abs() < 0.01);
    let (ra, dec) = (101.287155 * DEG, -16.716116 * DEG);
    let expected =
        asterope_core::math::Vec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
    assert!(
        sirius.dir.angle_to(expected) < 1.0 * ARCSEC,
        "Sirius J2000 position"
    );
    assert!(catalog.stars().windows(2).all(|w| w[0].mag <= w[1].mag));
    let fastest = catalog
        .stars()
        .iter()
        .max_by(|a, b| a.pm_ra.hypot(a.pm_dec).total_cmp(&b.pm_ra.hypot(b.pm_dec)))
        .unwrap();
    assert_eq!(
        fastest.hip, 57939,
        "Groombridge 1830 is the fastest star brighter than V 6.5"
    );
    assert!((fastest.pm_ra.hypot(fastest.pm_dec) - 7058.0).abs() < 5.0);
}

#[test]
fn orion_belt_projects_where_expected() {
    let catalog = catalog();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let pointing = RaDecRoll {
        ra: 83.822 * DEG,
        dec: -1.2 * DEG,
        roll: 0.0,
    };
    let attitude = attitude_from_radec_roll(pointing);
    let pixel = |hip: u32| {
        let star = catalog.stars().iter().find(|s| s.hip == hip).unwrap();
        camera.project(attitude.mul_vec(star.dir)).unwrap()
    };
    let (mintaka, alnilam, alnitak) = (pixel(25930), pixel(26311), pixel(26727));
    // North is up and east is left: Alnitak (east, south) sits left of and below Mintaka.
    assert!(alnitak.0 < alnilam.0 && alnilam.0 < mintaka.0);
    assert!(alnitak.1 > alnilam.1 && alnilam.1 > mintaka.1);
    let scale = camera.pixel_scale();
    let separation = (mintaka.0 - alnitak.0).hypot(mintaka.1 - alnitak.1) * scale / DEG;
    assert!(
        (separation - 2.74).abs() < 0.05,
        "belt length {separation} deg"
    );
}

#[test]
fn solves_a_clean_frame_of_orion() {
    let catalog = catalog();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, STARS_PER_FIELD, BLEND_PIXELS).unwrap();
    let pointing = RaDecRoll {
        ra: 83.822 * DEG,
        dec: -5.391 * DEG,
        roll: 12.0 * DEG,
    };
    let attitude = attitude_from_radec_roll(pointing);
    let frame = simulate(
        catalog.stars(),
        &attitude,
        &camera,
        &SensorModel::default(),
        42,
    )
    .unwrap();
    let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
    let result = solve_frame(&view, &camera, &index, &catalog, &SolverConfig::default()).unwrap();
    let solution = result.lis.solution.expect("Orion must solve");
    let error = solution.q.angle_to(Quat::from_matrix(&attitude).unwrap()) / ARCSEC;
    println!(
        "Orion: {} detections, {} matches, error {error:.2}\", rms {:.2}\", index {} stars / {} pairs",
        result.detect.detections.len(),
        solution.matches.len(),
        solution.rms_arcsec,
        index.star_count(),
        index.pair_count()
    );
    assert!(error < 30.0);
    assert!(
        solution.matches.iter().any(|m| m.hip == 26311),
        "Alnilam identified"
    );
    assert!(solution.quest_q_method_delta < 1e-10);
}

#[test]
fn solves_through_lens_distortion() {
    let catalog = catalog();
    let camera = Camera::from_fov(1024, 1024, 20.0 * DEG)
        .unwrap()
        .with_distortion(Distortion {
            k1: -0.08,
            k2: 0.01,
            k3: 0.0,
            p1: 3e-4,
            p2: -2e-4,
        });
    let index = PairIndex::for_camera(&catalog, &camera, STARS_PER_FIELD, BLEND_PIXELS).unwrap();
    let attitude = attitude_from_radec_roll(RaDecRoll {
        ra: 200.0 * DEG,
        dec: 40.0 * DEG,
        roll: 250.0 * DEG,
    });
    let frame = simulate(
        catalog.stars(),
        &attitude,
        &camera,
        &SensorModel::default(),
        7,
    )
    .unwrap();
    let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
    let result = solve_frame(&view, &camera, &index, &catalog, &SolverConfig::default()).unwrap();
    let solution = result.lis.solution.expect("distorted frame must solve");
    assert!(solution.q.angle_to(Quat::from_matrix(&attitude).unwrap()) < 60.0 * ARCSEC);
}

#[test]
fn scale_search_recovers_an_approximate_field_of_view() {
    let catalog = catalog();
    let truth_camera = Camera::from_fov(1024, 1024, 14.0 * DEG)
        .unwrap()
        .with_distortion(Distortion {
            k1: -0.05,
            ..Distortion::NONE
        });
    let attitude = attitude_from_radec_roll(RaDecRoll {
        ra: 310.0 * DEG,
        dec: 45.0 * DEG,
        roll: 33.0 * DEG,
    });
    let frame = simulate(
        catalog.stars(),
        &attitude,
        &truth_camera,
        &SensorModel::default(),
        3,
    )
    .unwrap();
    let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
    let config = SolverConfig {
        tolerance_px: 2.0,
        match_radius_px: 3.0,
        ..SolverConfig::default()
    };
    let detections = asterope_core::detect::detect(&view, &config.detect)
        .unwrap()
        .detections;

    let guess = Camera::from_fov(1024, 1024, 14.9 * DEG).unwrap();
    let search = ScaleSearch {
        span: 0.1,
        refine: true,
    };
    let index = PairIndex::for_camera(
        &catalog,
        &widest_camera(&guess, &search),
        STARS_PER_FIELD,
        BLEND_PIXELS,
    )
    .unwrap();
    assert!(
        asterope_core::pipeline::identify_detections(
            &detections,
            &guess,
            &index,
            &catalog,
            &config,
            0,
            2000
        )
        .solution
        .is_none(),
        "a 6 % scale error must not lock without the search"
    );
    let solved = solve_with_scale_search(&detections, &guess, &index, &catalog, &config, &search);
    let solution = solved.lis.solution.expect("scale search must lock");
    let focal_error = solved.camera.fx / truth_camera.fx - 1.0;
    println!(
        "scale search: {} scales, focal error {:.4} %, k1 {:.4} (truth -0.05), {} matches, rms {:.1}\"",
        solved.scales_tried,
        100.0 * focal_error,
        solved.camera.distortion.k1,
        solution.matches.len(),
        solution.rms_arcsec
    );
    assert!(solved.refined);
    assert!(focal_error.abs() < 2e-3, "focal error {focal_error}");
    assert!(solution.q.angle_to(Quat::from_matrix(&attitude).unwrap()) < 120.0 * ARCSEC);
}

/// Runs a tracker over a rotating camera and returns the events per frame.
fn track_sequence(
    rate_deg: [f64; 3],
    frames: usize,
    period: f64,
    exposure: f64,
) -> (Vec<TrackEvent>, f64) {
    let catalog = catalog();
    let camera = Camera::from_fov(512, 512, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, STARS_PER_FIELD, BLEND_PIXELS).unwrap();
    let rate = asterope_core::math::Vec3::new(rate_deg[0], rate_deg[1], rate_deg[2]).scale(DEG);
    // The aperture is scaled so every exposure collects the same signal.
    let base = SensorModel::default();
    let sensor = SensorModel {
        body_rate: rate,
        exposure,
        zero_point: base.zero_point * base.exposure / exposure,
        ..base
    };
    let config = SolverConfig::default();
    let track = TrackConfig::default();
    let scene = TrackScene {
        camera: &camera,
        index: &index,
        catalog: &catalog,
        config: &config,
        track: &track,
    };

    let mut truth = Quat::from_matrix(&attitude_from_radec_roll(RaDecRoll {
        ra: 100.0 * DEG,
        dec: 20.0 * DEG,
        roll: 0.0,
    }))
    .unwrap();
    let mut tracker = Tracker::new();
    let mut events = Vec::new();
    let mut worst: f64 = 0.0;
    for frame_index in 0..frames {
        let frame = simulate(
            catalog.stars(),
            &truth.to_matrix(),
            &camera,
            &sensor,
            frame_index as u64,
        )
        .unwrap();
        let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
        let detections = asterope_core::detect::detect(&view, &config.detect)
            .unwrap()
            .detections;
        let update = tracker.update(&detections, &scene, period);
        if let Some(solution) = &update.solution {
            worst = worst.max(solution.q.angle_to(truth) / ARCSEC);
        }
        events.push(update.event);
        truth = propagate(truth, rate, period);
    }
    (events, worst)
}

#[test]
fn tracker_follows_a_slow_slew_without_new_searches() {
    let (events, worst) = track_sequence([0.3, -0.2, 0.5], 40, 0.25, 0.1);
    let count = |e: TrackEvent| events.iter().filter(|x| **x == e).count();
    println!(
        "slow slew: {} acquired, {} tracked, {} coasting, {} lost, worst error {worst:.1}\"",
        count(TrackEvent::Acquired),
        count(TrackEvent::Tracked),
        count(TrackEvent::Coasting),
        count(TrackEvent::Lost)
    );
    assert_eq!(events[0], TrackEvent::Acquired);
    assert_eq!(count(TrackEvent::Lost), 0);
    assert!(count(TrackEvent::Tracked) >= 37, "{events:?}");
    assert!(worst < 120.0);
}

#[test]
fn tracker_learns_a_fast_rate_from_two_searches() {
    // 3 deg/s moves the field by 27 px per frame, outside the 12 px window;
    // a 10 ms exposure keeps the smear near one pixel.
    let (events, worst) = track_sequence([0.0, 3.0, 0.0], 30, 0.25, 0.01);
    let count = |e: TrackEvent| events.iter().filter(|x| **x == e).count();
    println!(
        "fast slew: {} acquired, {} tracked, {} coasting, {} lost, worst error {worst:.1}\"",
        count(TrackEvent::Acquired),
        count(TrackEvent::Tracked),
        count(TrackEvent::Coasting),
        count(TrackEvent::Lost)
    );
    assert_eq!(&events[..2], &[TrackEvent::Acquired, TrackEvent::Acquired]);
    assert!(count(TrackEvent::Tracked) >= 20, "{events:?}");
}

#[test]
fn a_long_exposure_at_the_same_rate_smears_stars_beyond_use() {
    let (events, _) = track_sequence([0.0, 3.0, 0.0], 6, 0.25, 0.1);
    assert!(events.iter().all(|e| *e == TrackEvent::Lost), "{events:?}");
}

#[test]
fn empty_and_noise_only_frames_give_no_solution() {
    let catalog = catalog();
    let camera = Camera::from_fov(512, 512, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, STARS_PER_FIELD, BLEND_PIXELS).unwrap();
    let sensor = SensorModel {
        hot_pixel_fraction: 2e-3,
        cosmic_ray_mean: 25.0,
        ..SensorModel::default()
    };
    for seed in 0..30 {
        let frame = simulate(&[], &Quat::IDENTITY.to_matrix(), &camera, &sensor, seed).unwrap();
        let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
        let result =
            solve_frame(&view, &camera, &index, &catalog, &SolverConfig::default()).unwrap();
        assert!(
            result.lis.solution.is_none(),
            "seed {seed} produced a false lock"
        );
    }
}

#[test]
fn monte_carlo_baseline_1000_trials() {
    const TRIALS: u64 = 1000;
    const MASTER_SEED: u64 = 1;
    let catalog = catalog();
    let scenario = baseline();
    let index =
        PairIndex::for_camera(&catalog, &scenario.camera, STARS_PER_FIELD, BLEND_PIXELS).unwrap();

    let mut tally = Tally::default();
    let mut cross = Vec::new();
    let mut roll = Vec::new();
    for trial in 0..TRIALS {
        let outcome = run_trial(&catalog, &index, &scenario, MASTER_SEED, trial).unwrap();
        tally.add(&outcome);
        match outcome.class {
            TrialClass::Success | TrialClass::Inaccurate => {
                cross.push(outcome.cross_boresight_arcsec);
                roll.push(outcome.roll_arcsec);
            }
            TrialClass::FalsePositive => println!(
                "false positive: trial {trial}, error {:.3} deg, {} matches",
                outcome.total_arcsec / 3600.0,
                outcome.matches
            ),
            TrialClass::NoSolution => println!(
                "no solution: trial {trial}, {} catalog stars in frame, {} detections",
                outcome.truth_stars, outcome.detections
            ),
        }
    }
    cross.sort_by(f64::total_cmp);
    roll.sort_by(f64::total_cmp);
    let pick = |v: &[f64], f: f64| percentile(v, f);
    let (success_lo, success_hi) = clopper_pearson(tally.success, tally.trials);
    let (_, false_hi) = clopper_pearson(tally.false_positive, tally.trials);
    println!(
        "baseline {TRIALS} trials: success {:.2} % [{:.2}, {:.2}], false positive {} (95 % upper \
         bound {:.3} %), inaccurate {}, no solution {}, starved {}",
        100.0 * f64::from(tally.success) / f64::from(tally.trials),
        100.0 * success_lo,
        100.0 * success_hi,
        tally.false_positive,
        100.0 * false_hi,
        tally.inaccurate,
        tally.no_solution,
        tally.starved
    );
    println!(
        "cross-boresight median {:.2}\" p95 {:.2}\" max {:.2}\" | roll median {:.1}\" p95 {:.1}\" max {:.1}\"",
        pick(&cross, 0.5),
        pick(&cross, 0.95),
        pick(&cross, 1.0),
        pick(&roll, 0.5),
        pick(&roll, 0.95),
        pick(&roll, 1.0)
    );
    assert!(f64::from(tally.success) > 0.98 * f64::from(tally.trials));
    assert_eq!(
        tally.false_positive, 0,
        "false positives (> {FALSE_POSITIVE_DEG} deg)"
    );
}
