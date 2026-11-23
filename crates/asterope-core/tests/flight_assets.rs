//! The onboard index and catalog shipped in `data/` for the reference
//! camera (1024 x 1024, 14 degrees) match what the builder produces today.
//!
//! Regenerate with
//! `asterope build-index data/hip65.astc data/flight-14deg.asti --fov 14
//! --stars-per-field 16 --max-angle 9.85 --catalog-out data/flight-14deg.astc`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::flight::StarTracker;
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::math::{ARCSEC, DEG};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::TrackEvent;
use asterope_core::AsteropeError;

const SKY: &[u8] = include_bytes!("../../../data/hip65.astc");
const CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
const INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");

fn camera() -> Camera {
    Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap()
}

#[test]
fn shipped_files_are_reproducible() {
    let sky = Catalog::parse(SKY).unwrap();
    let mut config = IndexConfig::for_camera(&sky, &camera(), 16.0, 4.0).unwrap();
    config.max_angle = 9.85 * DEG;
    let index = PairIndex::build(&sky, &config).unwrap();
    assert!(index.as_bytes() == INDEX, "data/flight-14deg.asti is stale");
    let onboard = sky.brighter_than(index.mag_limit() + 0.5);
    assert!(
        onboard.encode() == CATALOG,
        "data/flight-14deg.astc is stale"
    );
}

#[test]
fn tracker_built_from_shipped_files_acquires_and_tracks() {
    let sky = Catalog::parse(SKY).unwrap();
    let mut tracker = StarTracker::from_bytes(camera(), CATALOG, INDEX).unwrap();
    let mut rng = Rng::new(31);
    let mut solved = 0;
    for _ in 0..20 {
        tracker.reset();
        let attitude = rng.unit_quat();
        let render = |seed| {
            simulate(
                sky.stars(),
                &attitude.to_matrix(),
                &camera(),
                &SensorModel::default(),
                seed,
            )
            .unwrap()
            .pixels
        };
        let first = tracker.update(&render(rng.next_u64()), 0.1).unwrap();
        let Some(q) = first.attitude() else {
            continue;
        };
        assert_eq!(first.event, TrackEvent::Acquired);
        assert!(q.angle_to(attitude) < 60.0 * ARCSEC);
        let second = tracker.update(&render(rng.next_u64()), 0.1).unwrap();
        assert_eq!(second.event, TrackEvent::Tracked);
        assert!(second.attitude().unwrap().angle_to(attitude) < 60.0 * ARCSEC);
        assert_eq!(
            tracker.solve(&[0u16; 16]).unwrap_err(),
            AsteropeError::InvalidFrame
        );
        solved += 1;
    }
    assert!(solved >= 19, "{solved} of 20 attitudes acquired");
}

#[test]
fn an_index_from_another_catalog_is_refused() {
    let sky = Catalog::parse(SKY).unwrap();
    let shallow = sky.brighter_than(4.0);
    let index = PairIndex::from_bytes(INDEX).unwrap();
    assert_eq!(
        StarTracker::new(camera(), shallow, index).err(),
        Some(AsteropeError::IndexCatalogMismatch)
    );
}

#[test]
fn aberration_correction_removes_the_apparent_shift() {
    use asterope_core::ephem::{beta, earth_velocity_km_s};
    use asterope_core::math::Vec3;
    use asterope_core::validation::{attitude_error, percentile};

    let velocity = earth_velocity_km_s(2_461_000.5) + Vec3::new(0.0, 5.4, 5.4);
    let mut apparent_sky = Catalog::parse(SKY).unwrap();
    apparent_sky.aberrate(beta(velocity));
    let mut rng = Rng::new(41);
    let (mut raw, mut corrected) = (Vec::new(), Vec::new());
    for _ in 0..40 {
        let attitude = rng.unit_quat();
        let frame = simulate(
            apparent_sky.stars(),
            &attitude.to_matrix(),
            &camera(),
            &SensorModel::default(),
            rng.next_u64(),
        )
        .unwrap()
        .pixels;
        for (errors, apply) in [(&mut raw, false), (&mut corrected, true)] {
            let mut tracker = StarTracker::from_bytes(camera(), CATALOG, INDEX).unwrap();
            if apply {
                tracker.set_velocity(velocity.scale(0.5));
                tracker.set_velocity(velocity);
            }
            if let Some(q) = tracker.solve(&frame).unwrap().attitude() {
                errors.push(attitude_error(q, attitude).0 / ARCSEC);
            }
        }
    }
    raw.sort_by(f64::total_cmp);
    corrected.sort_by(f64::total_cmp);
    let (raw_median, corrected_median) = (percentile(&raw, 0.5), percentile(&corrected, 0.5));
    println!(
        "cross-boresight error at {:.1} km/s: {raw_median:.1}\" uncorrected ({} frames), {corrected_median:.1}\" corrected ({} frames)",
        velocity.norm(),
        raw.len(),
        corrected.len()
    );
    assert!(raw_median > 8.0);
    assert!(corrected_median < 1.5);
}
