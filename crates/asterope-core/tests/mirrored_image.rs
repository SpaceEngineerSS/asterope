//! A frame seen through a mirror must not be solved as it is, and must be
//! solved once it is flipped back.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::mirrored;
use asterope_core::flight::StarTracker;
use asterope_core::index::PairIndex;
use asterope_core::math::DEG;
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");

#[test]
fn a_mirrored_frame_is_refused_and_its_flip_is_solved() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let tracker = StarTracker::new(camera, catalog.clone(), index).unwrap();
    let sensor = SensorModel::default();
    let mut rng = Rng::new(77);
    let (mut refused, mut solved) = (0, 0);
    for _ in 0..60 {
        let truth = rng.unit_quat();
        let image = simulate(
            catalog.stars(),
            &truth.to_matrix(),
            &camera,
            &sensor,
            rng.next_u64(),
        )
        .unwrap();
        let seen = mirrored(1024, &image.pixels);
        assert_eq!(seen.len(), image.pixels.len());
        refused += usize::from(tracker.solve(&seen).unwrap().solution.is_none());
        let back = mirrored(1024, &seen);
        assert_eq!(back, image.pixels);
        if let Some(q) = tracker.solve(&back).unwrap().attitude() {
            assert!(q.angle_to(truth) < 0.02 * DEG);
            solved += 1;
        }
    }
    println!("mirrored frames refused: {refused} of 60; flipped back and solved: {solved} of 60");
    assert_eq!(refused, 60);
    assert!(solved >= 59);
}
