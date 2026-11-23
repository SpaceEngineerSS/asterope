//! A frame that still shows a faint copy of the previous one must not hold
//! the tracker on the previous attitude.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::flight::StarTracker;
use asterope_core::index::PairIndex;
use asterope_core::math::{Vec3, ARCSEC, DEG};
use asterope_core::pipeline::SolverConfig;
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::{propagate, TrackConfig, TrackEvent};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
/// Brightness of the leftover image relative to the frame.
const GHOST: f64 = 0.45;

#[test]
fn leftovers_of_the_previous_frame_do_not_hold_the_lock() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let sensor = SensorModel::default();
    let ghost = SensorModel {
        exposure: GHOST * sensor.exposure,
        noise: false,
        sky_background: 0.0,
        dark_current: 0.0,
        ..sensor
    };
    let render = |attitude, model: &SensorModel, seed| {
        simulate(catalog.stars(), &attitude, &camera, model, seed)
            .unwrap()
            .pixels
    };
    let step = Vec3::new(0.0, 8.0 * camera.pixel_scale(), 0.0);

    let mut rng = Rng::new(21);
    let (mut held_without_rule, mut wrong_with_rule, mut cases) = (0, 0, 0);
    for _ in 0..20 {
        let first = rng.unit_quat();
        let second = propagate(first, step, 1.0);
        let still = render(first.to_matrix(), &sensor, rng.next_u64());
        let leftover = render(first.to_matrix(), &ghost, 0);
        let moved: Vec<u16> = render(second.to_matrix(), &sensor, rng.next_u64())
            .iter()
            .zip(&leftover)
            .map(|(pixel, ghost)| pixel.saturating_add(ghost.saturating_sub(sensor.bias as u16)))
            .collect();

        for min_bright_fraction in [0.0, TrackConfig::default().min_bright_fraction] {
            let mut tracker = StarTracker::new(camera, catalog.clone(), index.clone()).unwrap();
            tracker.set_solver(SolverConfig {
                tolerance_px: 2.0,
                match_radius_px: 2.8,
                ..SolverConfig::default()
            });
            tracker.set_track_config(TrackConfig {
                max_misses: 0,
                min_bright_fraction,
                ..TrackConfig::default()
            });
            let acquired = tracker.update(&still, 0.1).unwrap();
            if acquired.attitude().is_none() {
                continue;
            }
            assert_eq!(
                tracker.update(&still, 0.1).unwrap().event,
                TrackEvent::Tracked
            );
            let fix = tracker.update(&moved, 0.1).unwrap();
            let wrong = fix
                .attitude()
                .is_some_and(|q| q.angle_to(second) > 60.0 * ARCSEC);
            if min_bright_fraction == 0.0 {
                cases += 1;
                held_without_rule += u32::from(wrong);
            } else {
                wrong_with_rule += u32::from(wrong);
            }
        }
    }
    println!("wrong attitude in {held_without_rule} of {cases} cases without the rule, {wrong_with_rule} with it");
    assert_eq!(wrong_with_rule, 0);
    assert!(
        held_without_rule > 0,
        "the scenario no longer exercises the rule"
    );
}
