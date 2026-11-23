//! Bright planets in the field: objects no star catalog holds.
//!
//! `cargo test --release -p asterope-core --test planets_in_field -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::attitude::{attitude_from_radec_roll, RaDecRoll};
use asterope_core::camera::Camera;
use asterope_core::catalog::{Catalog, Star};
use asterope_core::detect::Frame;
use asterope_core::index::PairIndex;
use asterope_core::math::{Quat, DEG, TAU};
use asterope_core::pipeline::{solve_frame, SolverConfig};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::solar::{planets_as_stars, PLANET_ID_BASE};
use asterope_core::validation::{attitude_error, percentile};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 400;

#[derive(Default)]
struct Tally {
    locks: u64,
    false_locks: u64,
    cross: Vec<f64>,
}

fn solve(
    sky: &[Star],
    attitude_q: Quat,
    camera: &Camera,
    catalog: &Catalog,
    index: &PairIndex,
    seed: u64,
    tally: &mut Tally,
) {
    let frame = simulate(
        sky,
        &attitude_q.to_matrix(),
        camera,
        &SensorModel::default(),
        seed,
    )
    .unwrap();
    let view = Frame::new(frame.width, frame.height, &frame.pixels).unwrap();
    let result = solve_frame(&view, camera, index, catalog, &SolverConfig::default()).unwrap();
    if let Some(solution) = result.lis.solution {
        let (cross, _, total) = attitude_error(solution.q, attitude_q);
        if total > 0.1 * DEG {
            tally.false_locks += 1;
        } else {
            tally.locks += 1;
            tally.cross.push(cross / DEG * 3600.0);
        }
    }
}

#[test]
fn planets_do_not_cause_false_locks() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let mut rng = Rng::new(404);
    let (mut without, mut with) = (Tally::default(), Tally::default());
    let (mut planets_in_frames, mut brightest) = (0usize, f64::INFINITY);
    for _ in 0..TRIALS {
        // A date between 1990 and 2040 and a planet brighter than V 6.5
        // somewhere within 5 degrees of the boresight.
        let jd = 2_451_545.0 + rng.range(-3_650.0, 14_600.0);
        let planets = planets_as_stars(jd, 6.5);
        let target = planets[rng.below(planets.len() as u32) as usize];
        let (ra, dec) = (target.dir.y.atan2(target.dir.x), target.dir.z.asin());
        let pointing = RaDecRoll {
            ra: ra + rng.range(-5.0, 5.0) * DEG / dec.cos().max(0.2),
            dec: (dec + rng.range(-5.0, 5.0) * DEG).clamp(-89.0 * DEG, 89.0 * DEG),
            roll: rng.range(0.0, TAU),
        };
        let attitude = attitude_from_radec_roll(pointing);
        let q = Quat::from_matrix(&attitude).unwrap();
        let seed = rng.next_u64();
        let mut sky = catalog.stars().to_vec();
        solve(&sky, q, &camera, &catalog, &index, seed, &mut without);
        sky.extend(planets.iter().copied());
        let drawn = simulate(&sky, &attitude, &camera, &SensorModel::default(), seed).unwrap();
        for star in drawn.truth.iter().filter(|star| star.hip >= PLANET_ID_BASE) {
            planets_in_frames += 1;
            brightest = brightest.min(star.mag);
        }
        solve(&sky, q, &camera, &catalog, &index, seed, &mut with);
    }
    println!(
        "{planets_in_frames} planet images in {TRIALS} frames, the brightest at V {brightest:.1}"
    );
    println!("| Sky | Locks (%) | False locks | Cross median (\") | Cross 95 % (\") |");
    println!("|---|---|---|---|---|");
    for (name, tally) in [
        ("Stars only", &mut without),
        ("Stars and planets", &mut with),
    ] {
        tally.cross.sort_by(f64::total_cmp);
        println!(
            "| {name} | {:.1} | {} | {:.2} | {:.2} |",
            100.0 * tally.locks as f64 / TRIALS as f64,
            tally.false_locks,
            percentile(&tally.cross, 0.5),
            percentile(&tally.cross, 0.95)
        );
    }
    assert!(planets_in_frames as u64 >= TRIALS);
    assert_eq!(with.false_locks + without.false_locks, 0);
}
