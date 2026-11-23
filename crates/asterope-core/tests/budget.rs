//! Heap budget of the flight path, measured with a counting allocator.
//!
//! `cargo test --release -p asterope-core --test budget -- --nocapture`
//! prints the table quoted in `docs/integration.md`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::{detect, Frame};
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::math::DEG;
use asterope_core::pipeline::{identify_detections, SolverConfig, MAX_PYRAMIDS_PER_FRAME};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::{TrackConfig, TrackEvent, TrackScene, Tracker};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
            COUNT.fetch_add(1, Relaxed);
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
struct Usage {
    /// Largest heap growth above the level at the start, bytes.
    peak: usize,
    /// Heap still held at the end, bytes.
    retained: usize,
    allocations: usize,
    /// Sum of every allocation, which is what an allocator that frees
    /// nothing until the frame is done has to hold.
    total: usize,
}

impl Usage {
    fn max(self, other: Usage) -> Usage {
        Usage {
            peak: self.peak.max(other.peak),
            retained: self.retained.max(other.retained),
            allocations: self.allocations.max(other.allocations),
            total: self.total.max(other.total),
        }
    }
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Usage) {
    let base = CURRENT.load(Relaxed);
    PEAK.store(base, Relaxed);
    let count = COUNT.load(Relaxed);
    let total = TOTAL.load(Relaxed);
    let value = f();
    let usage = Usage {
        peak: PEAK.load(Relaxed).saturating_sub(base),
        retained: CURRENT.load(Relaxed).saturating_sub(base),
        allocations: COUNT.load(Relaxed) - count,
        total: TOTAL.load(Relaxed) - total,
    };
    (value, usage)
}

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const FOV_DEG: f64 = 14.0;
const FRAMES: u64 = 12;

struct Row {
    size: u32,
    frame_bytes: usize,
    index_bytes: usize,
    detect: Usage,
    identify: Usage,
    track: Usage,
}

fn row(sky: &Catalog, size: u32) -> Row {
    let camera = Camera::from_fov(size, size, FOV_DEG * DEG).unwrap();
    let mut config = IndexConfig::for_camera(sky, &camera, 16.0, 4.0).unwrap();
    config.max_angle = 9.85 * DEG;
    let stored = PairIndex::build(sky, &config).unwrap().as_bytes().to_vec();
    let (index, index_usage) = measure(|| PairIndex::from_bytes(&stored).unwrap());
    assert_eq!(index_usage.peak, 0, "a borrowed index must not allocate");
    let catalog = sky.brighter_than(index.mag_limit() + 0.5);

    let solver = SolverConfig::default();
    let track = TrackConfig::default();
    let scene = TrackScene {
        camera: &camera,
        index: &index,
        catalog: &catalog,
        config: &solver,
        track: &track,
    };
    let sensor = SensorModel::default();
    let mut rng = Rng::new(u64::from(size));
    let mut row = Row {
        size,
        frame_bytes: 2 * (size as usize) * (size as usize),
        index_bytes: stored.len(),
        detect: Usage::default(),
        identify: Usage::default(),
        track: Usage::default(),
    };
    let mut solved = 0;
    for _ in 0..FRAMES {
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

        let (detected, usage) = measure(|| detect(&frame, &solver.detect).unwrap());
        row.detect = row.detect.max(usage);

        let (lis, usage) = measure(|| {
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
        row.identify = row.identify.max(usage);
        if lis.solution.is_none() {
            continue;
        }
        solved += 1;

        let mut tracker = Tracker::new();
        tracker.update(&detected.detections, &scene, 0.1);
        let (update, usage) = measure(|| tracker.update(&detected.detections, &scene, 0.1));
        assert_eq!(update.event, TrackEvent::Tracked);
        row.track = row.track.max(usage);
    }
    assert!(solved >= FRAMES - 1, "{solved} of {FRAMES} frames solved");
    row
}

#[test]
fn flight_path_heap_budget() {
    let (sky, catalog_usage) = measure(|| Catalog::parse(CATALOG).unwrap());
    println!(
        "catalog: {} stars, {} bytes on disk, {} bytes of heap",
        sky.len(),
        CATALOG.len(),
        catalog_usage.retained
    );
    println!("| Frame | Frame buffer (kB) | Index, read in place (kB) | Detection peak (kB) | Detection allocations | Identification peak (kB) | Tracking step peak (kB) | Sum of all allocations, search frame (kB) |");
    println!("|---|---|---|---|---|---|---|---|");
    let kb = |bytes: usize| bytes as f64 / 1000.0;
    for size in [512u32, 1024, 2048] {
        let row = row(&sky, size);
        println!(
            "| {0} x {0} | {1:.0} | {2:.0} | {3:.0} | {4} | {5:.0} | {6:.0} | {7:.0} |",
            row.size,
            kb(row.frame_bytes),
            kb(row.index_bytes),
            kb(row.detect.peak),
            row.detect.allocations,
            kb(row.identify.peak),
            kb(row.track.peak),
            kb(row.detect.total + row.identify.total)
        );
        assert!(row.detect.peak < row.frame_bytes);
        assert!(row.identify.peak < 200_000);
        assert!(row.track.peak < 50_000);
    }
}
