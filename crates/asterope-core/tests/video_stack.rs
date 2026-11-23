//! Registration and stacking of a rotating frame sequence.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::{detect, DetectOutput, Frame};
use asterope_core::index::PairIndex;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::pipeline::{
    detection_vectors, identify_detections, SolverConfig, MAX_PYRAMIDS_PER_FRAME,
};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel, SimFrame};
use asterope_core::stack::{Stacked, Stacker};
use asterope_core::track::propagate;
use asterope_core::validation::{attitude_error, percentile, FALSE_POSITIVE_DEG};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const DEPTH: usize = 8;
const FRAME_PERIOD: f64 = 0.1;

struct Bench {
    catalog: Catalog,
    camera: Camera,
    index: PairIndex<'static>,
    solver: SolverConfig,
    sensor: SensorModel,
}

/// Outcome of solving one frame against the true attitude.
#[derive(Clone, Copy)]
enum Outcome {
    NoLock,
    FalseLock,
    /// Cross-boresight and roll error, arcseconds.
    Lock(f64, f64),
}

impl Bench {
    fn new(exposure: f64) -> Bench {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
        let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
        Bench {
            catalog,
            camera,
            index,
            solver: SolverConfig::default(),
            sensor: SensorModel {
                exposure,
                ..SensorModel::default()
            },
        }
    }

    fn render(&self, attitude: Quat, seed: u64) -> SimFrame {
        simulate(
            self.catalog.stars(),
            &attitude.to_matrix(),
            &self.camera,
            &self.sensor,
            seed,
        )
        .unwrap()
    }

    /// Detects on pixels that are `scale` times the sensor's counts.
    fn detect(&self, pixels: &[u16], scale: u16) -> DetectOutput {
        let frame = Frame::new(self.camera.width, self.camera.height, pixels).unwrap();
        let mut config = self.solver.detect;
        config.saturation = config.saturation.saturating_mul(scale);
        detect(&frame, &config).unwrap()
    }

    fn solve(&self, pixels: &[u16], scale: u16, truth: Quat) -> Outcome {
        let detected = self.detect(pixels, scale);
        let lis = identify_detections(
            &detected.detections,
            &self.camera,
            &self.index,
            &self.catalog,
            &self.solver,
            0,
            MAX_PYRAMIDS_PER_FRAME,
        );
        match lis.solution {
            None => Outcome::NoLock,
            Some(solution) => {
                let (cross, roll, total) = attitude_error(solution.q, truth);
                if total > FALSE_POSITIVE_DEG * DEG {
                    Outcome::FalseLock
                } else {
                    Outcome::Lock(cross / ARCSEC, roll / ARCSEC)
                }
            }
        }
    }
}

struct Sequence {
    newest: SimFrame,
    newest_attitude: Quat,
    stacked: Stacked,
    frames_stacked: usize,
    /// Largest displacement error of a frame-to-frame registration at a
    /// sensor corner, pixels.
    worst_registration: f64,
}

fn run_sequence(bench: &Bench, rng: &mut Rng) -> Sequence {
    let rate = Vec3::new(
        rng.range(-1.0, 1.0),
        rng.range(-1.0, 1.0),
        rng.range(-2.0, 2.0),
    )
    .scale(DEG);
    let scale = bench.camera.pixel_scale();
    let corner = bench.camera.unproject(0.0, 0.0);
    let mut attitude = rng.unit_quat();
    let mut stacker = Stacker::new(DEPTH);
    let mut worst_registration: f64 = 0.0;
    let mut newest = bench.render(attitude, rng.next_u64());
    for step in 0..DEPTH {
        let earlier = attitude;
        if step > 0 {
            attitude = propagate(attitude, rate, FRAME_PERIOD);
            newest = bench.render(attitude, rng.next_u64());
        }
        let bodies = detection_vectors(&bench.camera, &bench.detect(&newest.pixels, 1).detections);
        let registration = stacker
            .push(&bench.camera, &newest.pixels, &bodies, 24.0 * scale, scale)
            .unwrap();
        if let Some(found) = registration {
            let truth = (attitude * earlier.conjugate()).to_matrix();
            let error = found
                .rotation
                .mul_vec(corner)
                .angle_to(truth.mul_vec(corner));
            worst_registration = worst_registration.max(error / scale);
        }
    }
    Sequence {
        stacked: stacker.stacked(&bench.camera),
        frames_stacked: stacker.len(),
        newest,
        newest_attitude: attitude,
        worst_registration,
    }
}

#[derive(Default)]
struct Score {
    locks: u32,
    false_locks: u32,
    cross: Vec<f64>,
    roll: Vec<f64>,
}

impl Score {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::NoLock => {}
            Outcome::FalseLock => self.false_locks += 1,
            Outcome::Lock(cross, roll) => {
                self.locks += 1;
                self.cross.push(cross);
                self.roll.push(roll);
            }
        }
    }

    fn medians(&mut self) -> (f64, f64) {
        self.cross.sort_by(f64::total_cmp);
        self.roll.sort_by(f64::total_cmp);
        (percentile(&self.cross, 0.5), percentile(&self.roll, 0.5))
    }
}

#[test]
fn stacking_keeps_stars_in_place_and_reveals_fainter_ones() {
    const SEQUENCES: u64 = 12;
    let bench = Bench::new(SensorModel::default().exposure);
    let mut rng = Rng::new(11);
    let (mut single_total, mut stacked_total) = (0usize, 0usize);
    let (mut error_sum, mut error_count) = (0.0, 0usize);
    for _ in 0..SEQUENCES {
        let sequence = run_sequence(&bench, &mut rng);
        assert_eq!(sequence.frames_stacked, DEPTH);
        assert!(
            sequence.worst_registration < 0.25,
            "registration error {:.2} px at the corner",
            sequence.worst_registration
        );
        single_total += bench.detect(&sequence.newest.pixels, 1).detections.len();
        let stacked = bench.detect(&sequence.stacked.pixels, sequence.stacked.scale);
        stacked_total += stacked.detections.len();
        for star in &sequence.newest.truth {
            let nearest = stacked
                .detections
                .iter()
                .map(|d| (d.x - star.x).hypot(d.y - star.y))
                .fold(f64::INFINITY, f64::min);
            if nearest < 1.0 {
                error_sum += nearest;
                error_count += 1;
            }
        }
    }
    let mean_error = error_sum / error_count as f64;
    println!(
        "detections per frame: single {:.1}, stack of {DEPTH} {:.1}; stacked centroid error {mean_error:.3} px over {error_count} stars",
        single_total as f64 / SEQUENCES as f64,
        stacked_total as f64 / SEQUENCES as f64
    );
    assert!(stacked_total as f64 > 1.2 * single_total as f64);
    assert!(mean_error < 0.2);
}

#[test]
fn stacking_recovers_lock_on_underexposed_frames() {
    const SEQUENCES: u32 = 24;
    let (mut faint_single, mut faint_stacked) = (0, 0);
    println!("| Exposure (s) | Frames held | Single lock | Stacked lock | False locks | Single cross / roll (\") | Stacked cross / roll (\") |");
    println!("|---|---|---|---|---|---|---|");
    for exposure in [0.006, 0.008, 0.012, 0.02, 0.1] {
        let bench = Bench::new(exposure);
        let mut rng = Rng::new(12);
        let (mut single, mut stacked) = (Score::default(), Score::default());
        let mut held = 0;
        for _ in 0..SEQUENCES {
            let sequence = run_sequence(&bench, &mut rng);
            held += sequence.frames_stacked;
            single.add(bench.solve(&sequence.newest.pixels, 1, sequence.newest_attitude));
            stacked.add(bench.solve(
                &sequence.stacked.pixels,
                sequence.stacked.scale,
                sequence.newest_attitude,
            ));
        }
        let (single_cross, single_roll) = single.medians();
        let (stacked_cross, stacked_roll) = stacked.medians();
        println!(
            "| {exposure} | {:.1} | {} / {SEQUENCES} | {} / {SEQUENCES} | {} | {single_cross:.1} / {single_roll:.0} | {stacked_cross:.1} / {stacked_roll:.0} |",
            f64::from(held as u32) / f64::from(SEQUENCES),
            single.locks,
            stacked.locks,
            single.false_locks + stacked.false_locks
        );
        assert_eq!(single.false_locks + stacked.false_locks, 0);
        if exposure < 0.015 {
            faint_single += single.locks;
            faint_stacked += stacked.locks;
        } else {
            assert!(stacked.locks + 1 >= single.locks);
        }
    }
    assert!(
        faint_stacked >= faint_single + 10,
        "stacked {faint_stacked}, single {faint_single}"
    );
}
