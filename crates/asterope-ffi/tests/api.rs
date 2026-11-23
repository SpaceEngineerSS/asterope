//! The C interface driven the way a C caller would drive it.

use std::ffi::CStr;
use std::ptr;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::flight::StarTracker;
use asterope_core::math::{Quat, DEG};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};
use asterope_ffi::{
    asterope_earth_velocity, asterope_status_message, asterope_tracker_free,
    asterope_tracker_matches, asterope_tracker_new, asterope_tracker_reset,
    asterope_tracker_set_velocity, asterope_tracker_solve, asterope_tracker_update, AsteropeCamera,
    AsteropeEvent, AsteropeFix, AsteropeMatch, AsteropeStatus, AsteropeTracker,
};
use asterope_ffi::{
    asterope_tracker_enable_gyro, asterope_tracker_gyro, asterope_tracker_update_aided,
    AsteropeAided,
};

const SKY: &[u8] = include_bytes!("../../../data/hip65.astc");
const CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
const INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");

const CAMERA: AsteropeCamera = AsteropeCamera {
    width: 1024,
    height: 1024,
    fov_x_deg: 14.0,
    k1: 0.0,
    k2: 0.0,
    k3: 0.0,
    p1: 0.0,
    p2: 0.0,
    equidistant: 0,
};

fn empty_fix() -> AsteropeFix {
    AsteropeFix {
        event: AsteropeEvent::Lost,
        solved: 0,
        q: [0.0; 4],
        ra_deg: 0.0,
        dec_deg: 0.0,
        roll_deg: 0.0,
        rms_arcsec: 0.0,
        sigma_arcsec: [0.0; 3],
        chance_probability: 0.0,
        body_rate_deg_s: [0.0; 3],
        matches: 0,
        detections: 0,
    }
}

fn new_tracker(catalog: &[u8], index: &[u8]) -> (AsteropeStatus, *mut AsteropeTracker) {
    let mut tracker = ptr::null_mut();
    let status = unsafe {
        asterope_tracker_new(
            &CAMERA,
            catalog.as_ptr(),
            catalog.len(),
            index.as_ptr(),
            index.len(),
            &mut tracker,
        )
    };
    (status, tracker)
}

#[test]
fn frames_solve_to_the_same_bits_as_the_rust_interface() {
    let sky = Catalog::parse(SKY).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let mut reference = StarTracker::from_bytes(camera, CATALOG, INDEX).unwrap();
    let (status, tracker) = new_tracker(CATALOG, INDEX);
    assert_eq!(status, AsteropeStatus::Ok);
    assert!(!tracker.is_null());

    let mut rng = Rng::new(3);
    let attitude: Quat = rng.unit_quat();
    let mut events = Vec::new();
    for step in 0..3u64 {
        let frame = simulate(
            sky.stars(),
            &attitude.to_matrix(),
            &camera,
            &SensorModel::default(),
            100 + step,
        )
        .unwrap()
        .pixels;
        let mut fix = empty_fix();
        let status =
            unsafe { asterope_tracker_update(tracker, frame.as_ptr(), frame.len(), 0.1, &mut fix) };
        assert_eq!(status, AsteropeStatus::Ok);
        let expected = reference.update(&frame, 0.1).unwrap();
        let q = expected.attitude().unwrap();
        assert_eq!(fix.solved, 1);
        assert_eq!(
            fix.q.map(f64::to_bits),
            [q.x, q.y, q.z, q.w].map(f64::to_bits)
        );
        assert!(fix.q[3] >= 0.0);
        assert!((0.0..360.0).contains(&fix.ra_deg) && fix.sigma_arcsec[2] > 0.0);
        events.push(fix.event);

        let count = unsafe { asterope_tracker_matches(tracker, ptr::null_mut(), 0) };
        assert_eq!(count, fix.matches as usize);
        let mut matches = vec![
            AsteropeMatch {
                detection: 0,
                hip: 0,
                magnitude: 0.0,
                residual_arcsec: 0.0
            };
            count
        ];
        unsafe { asterope_tracker_matches(tracker, matches.as_mut_ptr(), count) };
        assert!(matches
            .iter()
            .all(|m| m.hip > 0 && m.residual_arcsec < 120.0));
    }
    assert_eq!(
        events,
        [
            AsteropeEvent::Acquired,
            AsteropeEvent::Tracked,
            AsteropeEvent::Tracked
        ]
    );

    unsafe { asterope_tracker_reset(tracker) };
    let dark = vec![64u16; 1024 * 1024];
    let mut fix = empty_fix();
    let status = unsafe { asterope_tracker_solve(tracker, dark.as_ptr(), dark.len(), &mut fix) };
    assert_eq!(status, AsteropeStatus::Ok);
    assert_eq!((fix.solved, fix.event), (0, AsteropeEvent::Lost));
    assert_eq!(
        unsafe { asterope_tracker_matches(tracker, ptr::null_mut(), 0) },
        0
    );
    unsafe { asterope_tracker_free(tracker) };
}

#[test]
fn a_gyro_carries_the_attitude_through_a_frame_without_stars() {
    let (status, tracker) = new_tracker(CATALOG, INDEX);
    assert_eq!(status, AsteropeStatus::Ok);
    let sky = Catalog::parse(SKY).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let truth = Rng::new(21).unit_quat();
    let frame = simulate(
        sky.stars(),
        &truth.to_matrix(),
        &camera,
        &SensorModel::default(),
        4,
    )
    .unwrap();
    let blank = vec![64u16; 1024 * 1024];
    let mut fix = empty_fix();
    let mut aided = AsteropeAided {
        valid: 9,
        q: [0.0; 4],
        sigma_arcsec: [0.0; 3],
        gyro_bias_deg_s: [0.0; 3],
        normalized_residual_squared: 0.0,
        fix_accepted: 9,
    };
    // SAFETY: live tracker, buffers of the stated sizes, writable outputs.
    unsafe {
        asterope_tracker_enable_gyro(tracker, 0.003, 3e-5, 0.0);
        let status = asterope_tracker_update_aided(
            tracker,
            frame.pixels.as_ptr(),
            frame.pixels.len(),
            &mut fix,
            &mut aided,
        );
        assert_eq!(status, AsteropeStatus::Ok);
        assert_eq!((fix.solved, aided.valid), (1, 1));
        assert!(aided.normalized_residual_squared < 0.0);
        let first = Quat::new(aided.q[0], aided.q[1], aided.q[2], aided.q[3]);
        assert!(first.angle_to(truth) < 20.0 * DEG / 3600.0);

        // 0.25 deg/s about y for 0.2 s, then a frame with nothing in it.
        for _ in 0..10 {
            asterope_tracker_gyro(tracker, 0.0, 0.25, 0.0, 0.02);
        }
        let status = asterope_tracker_update_aided(
            tracker,
            blank.as_ptr(),
            blank.len(),
            &mut fix,
            &mut aided,
        );
        assert_eq!(status, AsteropeStatus::Ok);
        assert_eq!((fix.solved, aided.valid, aided.fix_accepted), (0, 1, 0));
        let carried = Quat::new(aided.q[0], aided.q[1], aided.q[2], aided.q[3]);
        assert!((carried.angle_to(first) / DEG - 0.05).abs() < 0.002);
        assert!(aided.sigma_arcsec.iter().all(|s| *s > 0.0));

        assert_eq!(
            asterope_tracker_update_aided(
                tracker,
                blank.as_ptr(),
                blank.len(),
                &mut fix,
                ptr::null_mut()
            ),
            AsteropeStatus::NullPointer
        );
        asterope_tracker_free(tracker);
    }
}

#[test]
fn bad_input_is_reported_as_a_status() {
    let (status, tracker) = new_tracker(&CATALOG[..100], INDEX);
    assert_eq!(status, AsteropeStatus::InvalidCatalog);
    assert!(tracker.is_null());

    let mut damaged = INDEX.to_vec();
    damaged[1000] ^= 1;
    assert_eq!(
        new_tracker(CATALOG, &damaged).0,
        AsteropeStatus::InvalidIndex
    );

    let shallow = Catalog::parse(SKY).unwrap().brighter_than(4.0).encode();
    assert_eq!(
        new_tracker(&shallow, INDEX).0,
        AsteropeStatus::IndexCatalogMismatch
    );

    let mut out = ptr::null_mut();
    let status = unsafe {
        asterope_tracker_new(
            ptr::null(),
            CATALOG.as_ptr(),
            CATALOG.len(),
            INDEX.as_ptr(),
            INDEX.len(),
            &mut out,
        )
    };
    assert_eq!(status, AsteropeStatus::NullPointer);

    let (_, tracker) = new_tracker(CATALOG, INDEX);
    let short = [0u16; 16];
    let mut fix = empty_fix();
    let status =
        unsafe { asterope_tracker_update(tracker, short.as_ptr(), short.len(), 0.1, &mut fix) };
    assert_eq!(status, AsteropeStatus::InvalidFrame);
    let status = unsafe { asterope_tracker_update(tracker, ptr::null(), 0, 0.1, &mut fix) };
    assert_eq!(status, AsteropeStatus::NullPointer);
    unsafe {
        asterope_tracker_free(tracker);
        asterope_tracker_free(ptr::null_mut());
    }

    let mut velocity = [0.0f64; 3];
    unsafe {
        asterope_earth_velocity(2_451_545.0, velocity.as_mut_ptr());
        asterope_tracker_set_velocity(ptr::null_mut(), 1.0, 2.0, 3.0);
    }
    let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
    assert!((speed - 30.28).abs() < 0.05);

    let message = unsafe { CStr::from_ptr(asterope_status_message(AsteropeStatus::InvalidFrame)) };
    assert_eq!(
        message.to_str().unwrap(),
        "pixel count does not match the camera"
    );
}
