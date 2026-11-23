//! A million fields with no sky in them, and a million with: how often does
//! the identification report an attitude that is not there?
//!
//! The frames are not rendered. The star positions go straight into the
//! identification, which is the part that could be fooled, so that a
//! million trials take minutes instead of half a day. Takes several
//! minutes on eight cores, so it does not run by default:
//!
//! `cargo test --release -p asterope-core --test false_locks -- --ignored --nocapture`

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::index::PairIndex;
use asterope_core::lis::identify;
use asterope_core::math::{Vec3, DEG};
use asterope_core::pipeline::{SolverConfig, MAX_PYRAMIDS_PER_FRAME};
use asterope_core::rng::{derive_seed, Rng};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const TRIALS: u64 = 1_000_000;
const SIDE: f64 = 1024.0;

/// Upper end of the exact (Clopper-Pearson) 95 % interval for `k` events in
/// `n` trials, for the small `k` met here: the rate at which seeing `k` or
/// fewer has probability 2.5 %, from the Poisson limit of the binomial.
fn upper_bound(k: u64, n: u64) -> f64 {
    let at_most_k = |rate: f64| {
        let (mut term, mut sum) = ((-rate).exp(), 0.0);
        for i in 0..=k {
            sum += term;
            term *= rate / (i + 1) as f64;
        }
        sum
    };
    let (mut low, mut high) = (k as f64, k as f64 + 40.0);
    for _ in 0..80 {
        let middle = 0.5 * (low + high);
        if at_most_k(middle) > 0.025 {
            low = middle;
        } else {
            high = middle;
        }
    }
    high / n as f64
}

fn scattered(rng: &mut Rng, camera: &Camera, count: usize) -> Vec<Vec3> {
    (0..count)
        .map(|_| camera.unproject(rng.range(8.0, SIDE - 8.0), rng.range(8.0, SIDE - 8.0)))
        .collect()
}

struct Counts {
    solved: AtomicU64,
    /// More than 0.1 degrees from the truth (any attitude, without a sky).
    wrong: AtomicU64,
    /// Solutions in which some match names a star other than the one the
    /// detection is.
    misidentified: AtomicU64,
}

fn run(real_sky: bool, counts: &Counts) {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let config = SolverConfig::default().lis_config(&camera, &catalog, 0, MAX_PYRAMIDS_PER_FRAME);
    let workers = thread::available_parallelism().map_or(4, |n| n.get()) as u64;
    thread::scope(|scope| {
        for worker in 0..workers {
            let (catalog, index, camera, config) = (&catalog, &index, &camera, &config);
            scope.spawn(move || {
                for trial in (worker..TRIALS).step_by(workers as usize) {
                    let mut rng = Rng::new(derive_seed(if real_sky { 71 } else { 72 }, trial));
                    let truth = rng.unit_quat();
                    // Each detection with the catalog star it really is,
                    // or none for a false object.
                    let (bodies, identity): (Vec<Vec3>, Vec<Option<u32>>) = if real_sky {
                        let attitude = truth.to_matrix();
                        let mut seen: Vec<(Vec3, Option<u32>)> = Vec::new();
                        for (star_index, star) in catalog.stars().iter().enumerate() {
                            let body = attitude.mul_vec(star.dir);
                            let Some((x, y)) = camera.project(body) else {
                                continue;
                            };
                            // One star in ten is missed; the rest are found
                            // within a tenth of a pixel.
                            if !camera.contains(x, y, 8.0) || rng.uniform() < 0.1 {
                                continue;
                            }
                            seen.push((
                                camera.unproject(x + 0.1 * rng.normal(), y + 0.1 * rng.normal()),
                                Some(star_index as u32),
                            ));
                            if seen.len() == 40 {
                                break;
                            }
                        }
                        // Up to five things that are not stars, anywhere in
                        // the brightness order.
                        for _ in 0..rng.below(6) {
                            let place = rng.below(seen.len() as u32 + 1) as usize;
                            seen.insert(place, (scattered(&mut rng, camera, 1)[0], None));
                        }
                        seen.into_iter().unzip()
                    } else {
                        let count = 8 + rng.below(40) as usize;
                        (scattered(&mut rng, camera, count), vec![None; count])
                    };
                    if let Some(solution) = identify(&bodies, index, catalog, config).solution {
                        counts.solved.fetch_add(1, Ordering::Relaxed);
                        let off = solution.q.angle_to(truth);
                        let wrong_stars = solution
                            .matches
                            .iter()
                            .filter(|m| identity.get(m.detection as usize) != Some(&Some(m.catalog_index)))
                            .count();
                        if !real_sky || off > 0.1 * DEG {
                            counts.wrong.fetch_add(1, Ordering::Relaxed);
                        }
                        if wrong_stars > 0 {
                            counts.misidentified.fetch_add(1, Ordering::Relaxed);
                        }
                        if real_sky && (off > 0.1 * DEG || wrong_stars * 2 > solution.matches.len()) {
                            println!(
                                "trial {trial}: {:.3} deg off, {} stars matched of {}, {wrong_stars} of them the wrong star",
                                off / DEG,
                                solution.matches.len(),
                                bodies.len()
                            );
                        }
                    }
                }
            });
        }
    });
}

#[test]
#[ignore = "a million trials each; run with --ignored"]
fn a_million_fields_with_and_without_a_sky() {
    println!("| Fields | Trials | Attitudes reported | Over 0.1 deg off | With a wrong star among the matches | Rate over 0.1 deg off, 95 % upper bound |");
    println!("|---|---|---|---|---|---|");
    for (name, real_sky) in [
        ("8 to 47 points scattered at random, no sky", false),
        (
            "the sky, one star in ten missed, up to five false objects",
            true,
        ),
    ] {
        let counts = Counts {
            solved: AtomicU64::new(0),
            wrong: AtomicU64::new(0),
            misidentified: AtomicU64::new(0),
        };
        run(real_sky, &counts);
        let (solved, wrong, misidentified) = (
            counts.solved.load(Ordering::Relaxed),
            counts.wrong.load(Ordering::Relaxed),
            counts.misidentified.load(Ordering::Relaxed),
        );
        println!(
            "| {name} | {TRIALS} | {solved} | {wrong} | {misidentified} | {:.1e} |",
            upper_bound(wrong, TRIALS)
        );
        // The acceptance test promises a chance below 1e-3 per frame; the
        // measured rate has to honor that with room to spare.
        assert!(upper_bound(wrong, TRIALS) < 1e-4, "{name}");
        if real_sky {
            assert!(solved > TRIALS * 99 / 100);
        }
    }
}

#[test]
fn the_upper_bound_is_the_exact_one() {
    // Zero events in n trials: 1 - 0.025^(1/n), which is 3.69 / n for large n.
    assert!((upper_bound(0, 10_000) / 3.689e-4 - 1.0).abs() < 0.01);
    // Three events: the tabulated Poisson limit is 8.77.
    assert!((upper_bound(3, 1_000_000) / 8.767e-6 - 1.0).abs() < 0.01);
}
