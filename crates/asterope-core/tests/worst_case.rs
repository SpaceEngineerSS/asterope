//! The most a frame can cost.
//!
//! The search has fixed budgets: so many triads, so many pyramids, so many
//! detections. This test feeds it frames built to use them up, and frames
//! built to make the detector work, and reports what each cost beside an
//! ordinary frame of the sky.
//!
//! `cargo test --release -p asterope-core --test worst_case -- --nocapture`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::{detect, DetectConfig, Frame};
use asterope_core::error::AsteropeError;
use asterope_core::index::PairIndex;
use asterope_core::lis::identify;
use asterope_core::math::{Vec3, DEG};
use asterope_core::pipeline::{identify_detections, SolverConfig, MAX_PYRAMIDS_PER_FRAME};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
            TOTAL.fetch_add(layout.size(), Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[derive(Clone, Copy, Default)]
struct Cost {
    seconds: f64,
    /// Largest heap growth above the level at the start, bytes.
    peak: usize,
    /// Sum of every allocation, bytes.
    total: usize,
}

impl Cost {
    fn worst(self, other: Cost) -> Cost {
        Cost {
            seconds: self.seconds.max(other.seconds),
            peak: self.peak.max(other.peak),
            total: self.total.max(other.total),
        }
    }
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    let base = CURRENT.load(Relaxed);
    PEAK.store(base, Relaxed);
    let total = TOTAL.load(Relaxed);
    let start = Instant::now();
    let value = f();
    let cost = Cost {
        seconds: start.elapsed().as_secs_f64(),
        peak: PEAK.load(Relaxed).saturating_sub(base),
        total: TOTAL.load(Relaxed) - total,
    };
    (value, cost)
}

const CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
const INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");
const SKY: &[u8] = include_bytes!("../../../data/hip65.astc");
const SIDE: u32 = 1024;
/// A limit on runs that an integrator short of memory might set.
const LIMITED_RUNS: usize = 8192;

fn row(name: &str, frames: usize, units: &str, cost: Cost) {
    println!(
        "| {name} | {frames} | {units} | {:.2} | {:.0} | {:.0} |",
        1e3 * cost.seconds,
        cost.peak as f64 / 1e3,
        cost.total as f64 / 1e3
    );
}

#[test]
fn frames_that_use_up_the_budgets() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let sky = Catalog::parse(SKY).unwrap();
    let index = PairIndex::from_bytes(INDEX).unwrap();
    let camera = Camera::from_fov(SIDE, SIDE, 14.0 * DEG).unwrap();
    let solver = SolverConfig::default();
    let config = solver.lis_config(&camera, &catalog, 0, MAX_PYRAMIDS_PER_FRAME);
    let cap = solver.detect.max_detections;
    let mut rng = Rng::new(2026);

    println!("| Identification | Frames | Triads, pyramids at most | Time, worst (ms) | Heap peak (kB) | Sum of allocations (kB) |");
    println!("|---|---|---|---|---|---|");

    // The sky as it is: the search ends at the first pyramid that holds.
    let sensor = SensorModel::default();
    let (mut ordinary, mut triads, mut pyramids) = (Cost::default(), 0, 0);
    let mut frames = Vec::new();
    for _ in 0..40 {
        let attitude = rng.unit_quat();
        let image = simulate(
            sky.stars(),
            &attitude.to_matrix(),
            &camera,
            &sensor,
            rng.next_u64(),
        )
        .unwrap();
        let frame = Frame::new(image.width, image.height, &image.pixels).unwrap();
        let detected = detect(&frame, &solver.detect).unwrap();
        let (found, cost) = measure(|| {
            identify_detections(
                &detected.detections,
                &camera,
                &index,
                &catalog,
                &solver,
                0,
                MAX_PYRAMIDS_PER_FRAME,
            )
        });
        ordinary = ordinary.worst(cost);
        triads = triads.max(found.triads_tested);
        pyramids = pyramids.max(found.pyramids_tested);
        frames.push(image);
    }
    row(
        "The sky, 14 degree field",
        40,
        &format!("{triads}, {pyramids}"),
        ordinary,
    );

    // As many points as the detector passes on, anywhere, with no sky
    // behind them: nothing can be accepted, so the search runs until a
    // budget stops it.
    let (mut scattered, mut triads, mut pyramids, mut solved) = (Cost::default(), 0, 0, 0);
    for _ in 0..400 {
        let bodies: Vec<Vec3> = (0..cap)
            .map(|_| camera.unproject(rng.range(8.0, 1016.0), rng.range(8.0, 1016.0)))
            .collect();
        let (found, cost) = measure(|| identify(&bodies, &index, &catalog, &config));
        scattered = scattered.worst(cost);
        triads = triads.max(found.triads_tested);
        pyramids = pyramids.max(found.pyramids_tested);
        solved += usize::from(found.solution.is_some());
    }
    row(
        &format!("{cap} points at random, no sky"),
        400,
        &format!("{triads}, {pyramids}"),
        scattered,
    );
    assert_eq!(
        solved, 0,
        "an attitude was reported for points scattered at random"
    );
    assert!(triads as usize <= config.max_triads && pyramids <= MAX_PYRAMIDS_PER_FRAME);

    // The sky turned inside out: every angle between two stars is right and
    // every triangle is the mirror image of a real one, so the lookups find
    // as many candidates as they ever can and the handedness check has to
    // turn each one away.
    let (mut mirrored, mut triads, mut pyramids, mut solved) = (Cost::default(), 0, 0, 0);
    for _ in 0..400 {
        let attitude = rng.unit_quat().to_matrix();
        let mut bodies: Vec<(f64, Vec3)> = catalog
            .stars()
            .iter()
            .filter_map(|star| {
                let body = attitude.mul_vec(star.dir);
                let (x, y) = camera.project(body)?;
                camera
                    .contains(x, y, 8.0)
                    .then(|| (star.mag, camera.unproject(f64::from(SIDE) - 1.0 - x, y)))
            })
            .collect();
        bodies.sort_by(|a, b| a.0.total_cmp(&b.0));
        let bodies: Vec<Vec3> = bodies.into_iter().take(cap).map(|(_, body)| body).collect();
        let (found, cost) = measure(|| identify(&bodies, &index, &catalog, &config));
        mirrored = mirrored.worst(cost);
        triads = triads.max(found.triads_tested);
        pyramids = pyramids.max(found.pyramids_tested);
        solved += usize::from(found.solution.is_some());
    }
    row(
        "The sky mirrored left to right",
        400,
        &format!("{triads}, {pyramids}"),
        mirrored,
    );
    assert_eq!(solved, 0, "a mirrored sky was accepted");
    assert!(triads as usize <= config.max_triads && pyramids <= MAX_PYRAMIDS_PER_FRAME);

    println!();
    println!("| Detection, 1024 x 1024 | Frames | Blobs, stars passed on, at most | Time, worst (ms) | Heap peak (kB) | Sum of allocations (kB) |");
    println!("|---|---|---|---|---|---|");
    let detection = |name: &str, images: &[Vec<u16>], config: &DetectConfig| -> (Cost, usize) {
        let (mut worst, mut blobs, mut stars, mut refused) = (Cost::default(), 0, 0, 0);
        for pixels in images {
            let frame = Frame::new(SIDE, SIDE, pixels).unwrap();
            let (found, cost) = measure(|| detect(&frame, config));
            worst = worst.worst(cost);
            match found {
                Ok(found) => {
                    blobs = blobs.max(found.stats.blobs);
                    stars = stars.max(found.detections.len());
                    assert!(found.detections.len() <= cap);
                }
                Err(error) => {
                    assert_eq!(error, AsteropeError::InvalidFrame);
                    refused += 1;
                }
            }
        }
        let outcome = if refused == images.len() {
            String::from("refused")
        } else if refused > 0 {
            format!("{blobs}, {stars}; {refused} refused")
        } else {
            format!("{blobs}, {stars}")
        };
        row(name, images.len(), &outcome, worst);
        (worst, refused)
    };

    let skies: Vec<Vec<u16>> = frames.iter().map(|image| image.pixels.clone()).collect();
    let mut rendered = |sensor: &SensorModel, count: usize| -> Vec<Vec<u16>> {
        (0..count)
            .map(|_| {
                let attitude = rng.unit_quat();
                simulate(
                    sky.stars(),
                    &attitude.to_matrix(),
                    &camera,
                    sensor,
                    rng.next_u64(),
                )
                .unwrap()
                .pixels
            })
            .collect()
    };
    // The worst the acceptance test of `docs/validation.md` draws: 0.1 % of
    // hot pixels and five particle tracks.
    let disturbed = rendered(
        &SensorModel {
            hot_pixel_fraction: 1e-3,
            cosmic_ray_mean: 5.0,
            ..SensorModel::default()
        },
        12,
    );
    let cluttered = rendered(
        &SensorModel {
            hot_pixel_fraction: 0.02,
            cosmic_ray_mean: 1000.0,
            ..SensorModel::default()
        },
        12,
    );
    // Every pixel drawn at random over the whole range: no background to
    // estimate, and nothing stands out of it.
    let pixels = (SIDE * SIDE) as usize;
    let noise: Vec<Vec<u16>> = (0..6)
        .map(|_| (0..pixels).map(|_| rng.below(4096) as u16).collect())
        .collect();
    // A star on every tenth pixel each way: ten thousand of them.
    let mut grid = vec![64u16; pixels];
    for y in (5..SIDE as usize - 5).step_by(10) {
        for x in (5..SIDE as usize - 5).step_by(10) {
            for (dy, dx, value) in [
                (0, 0, 3000u16),
                (0, 1, 900),
                (1, 0, 900),
                (0, -1, 900),
                (-1, 0, 900),
            ] {
                let at = (y as isize + dy) as usize * SIDE as usize + (x as isize + dx) as usize;
                if let Some(pixel) = grid.get_mut(at) {
                    *pixel = value;
                }
            }
        }
    }
    let grid = [grid];
    let flat = [vec![4095u16; pixels]];

    let open = solver.detect;
    let (ordinary, _) = detection("The sky, 14 degree field", &skies, &open);
    detection(
        "with 0.1 % hot pixels and 5 particle tracks",
        &disturbed,
        &open,
    );
    detection(
        "2 % hot pixels and 1 000 particle tracks",
        &cluttered,
        &open,
    );
    detection("Every pixel at random, 0 to 4095", &noise, &open);
    detection("10 000 stars on a grid", &grid, &open);
    detection("Saturated everywhere", &flat, &open);

    // The same frames with the number of runs limited. What the sky and the
    // disturbances of the acceptance test hold must still pass, and no
    // frame may then cost much more memory than a frame of the sky.
    let limited = DetectConfig {
        max_runs: LIMITED_RUNS,
        ..solver.detect
    };
    println!();
    println!("| Detection with `max_runs` at {LIMITED_RUNS} | Frames | Blobs, stars passed on, at most | Time, worst (ms) | Heap peak (kB) | Sum of allocations (kB) |");
    println!("|---|---|---|---|---|---|");
    let mut worst = Cost::default();
    for (name, images, passes) in [
        ("The sky, 14 degree field", &skies[..], true),
        (
            "with 0.1 % hot pixels and 5 particle tracks",
            &disturbed[..],
            true,
        ),
        (
            "2 % hot pixels and 1 000 particle tracks",
            &cluttered[..],
            false,
        ),
        ("Every pixel at random, 0 to 4095", &noise[..], true),
        ("10 000 stars on a grid", &grid[..], false),
        ("Saturated everywhere", &flat[..], true),
    ] {
        let (cost, refused) = detection(name, images, &limited);
        assert_eq!(refused == 0, passes, "{name}: {refused} refused");
        worst = worst.worst(cost);
    }
    assert!(
        worst.peak < 2 * pixels,
        "limited detection peaked at {} bytes, an ordinary frame at {}",
        worst.peak,
        ordinary.peak
    );
}
