//! The orbit model against what is known in closed form and against a
//! numerical integration of the same forces.

use asterope_core::math::{Vec3, DEG, TAU};
use asterope_core::orbit::{Blinder, Exclusion, Orbit, Pointing, Surroundings, EARTH_RADIUS_KM};
use asterope_core::solar::sun_direction;

const MU: f64 = 398_600.441_8;
const J2: f64 = 1.082_63e-3;
const EPOCH: f64 = 2_460_400.5;

#[test]
fn a_circular_orbit_keeps_its_radius_speed_and_period() {
    let orbit = Orbit::circular(400.0, 51.6 * DEG, 30.0 * DEG, EPOCH);
    let a = EARTH_RADIUS_KM + 400.0;
    assert!(
        (orbit.period() / 60.0 - 92.4).abs() < 0.2,
        "{}",
        orbit.period() / 60.0
    );
    for step in 0..200 {
        let (r, v) = orbit.state(f64::from(step) * 37.0);
        assert!((r.norm() - a).abs() < 1e-6);
        assert!((v.norm() - (MU / a).sqrt()).abs() < 1e-9);
        assert!(r.dot(v).abs() < 1e-6);
        let tilt = r.cross(v).normalized().unwrap().angle_to(Vec3::Z);
        assert!((tilt - 51.6 * DEG).abs() < 1e-9);
    }
}

#[test]
fn the_node_of_a_sun_synchronous_orbit_follows_the_sun() {
    // 700 km and 98.19 degrees: the textbook Sun-synchronous pair.
    let orbit = Orbit::circular(700.0, 98.19 * DEG, 0.0, EPOCH);
    let per_day = orbit.secular_rates().0 * 86_400.0 / DEG;
    assert!(
        (per_day - 360.0 / 365.2422).abs() < 0.005,
        "{per_day} deg/day"
    );
}

/// Two-body gravity with the J2 term, integrated with fourth-order
/// Runge-Kutta: the forces the secular rates are derived from.
fn acceleration(r: Vec3) -> Vec3 {
    let d = r.norm();
    let k = 1.5 * J2 * MU * EARTH_RADIUS_KM * EARTH_RADIUS_KM / d.powi(5);
    let z2 = 5.0 * r.z * r.z / (d * d);
    r.scale(-MU / d.powi(3))
        + Vec3::new(
            k * r.x * (z2 - 1.0),
            k * r.y * (z2 - 1.0),
            k * r.z * (z2 - 3.0),
        )
}

#[test]
fn the_orbit_plane_turns_as_an_integration_of_the_forces_says() {
    let orbit = Orbit::circular(550.0, 53.0 * DEG, 100.0 * DEG, EPOCH);
    let (mut r, mut v) = orbit.state(0.0);
    let dt = 5.0;
    let days = 5.0;
    let steps = (days * 86_400.0 / dt) as usize;
    for _ in 0..steps {
        let (k1r, k1v) = (v, acceleration(r));
        let (k2r, k2v) = (
            v + k1v.scale(0.5 * dt),
            acceleration(r + k1r.scale(0.5 * dt)),
        );
        let (k3r, k3v) = (
            v + k2v.scale(0.5 * dt),
            acceleration(r + k2r.scale(0.5 * dt)),
        );
        let (k4r, k4v) = (v + k3v.scale(dt), acceleration(r + k3r.scale(dt)));
        r = r + (k1r + k2r.scale(2.0) + k3r.scale(2.0) + k4r).scale(dt / 6.0);
        v = v + (k1v + k2v.scale(2.0) + k3v.scale(2.0) + k4v).scale(dt / 6.0);
    }
    let integrated = r.cross(v).normalized().unwrap();
    let (rm, vm) = orbit.state(days * 86_400.0);
    let modelled = rm.cross(vm).normalized().unwrap();
    let start = {
        let (r0, v0) = orbit.state(0.0);
        r0.cross(v0).normalized().unwrap()
    };
    let turned = start.angle_to(integrated) / DEG;
    let apart = modelled.angle_to(integrated) / DEG;
    println!("orbit normal after {days} days: turned {turned:.2} deg, model and integration {apart:.3} deg apart");
    assert!(turned > 15.0);
    assert!(apart < 0.1, "{apart}");
}

#[test]
fn a_camera_fixed_to_the_orbit_turns_once_a_revolution() {
    let orbit = Orbit::circular(500.0, 97.4 * DEG, 0.0, EPOCH);
    let zenith = Pointing::Local {
        boresight: Vec3::Z,
        roll: 0.0,
    };
    for step in 0..20 {
        let t = f64::from(step) * 300.0;
        let (q, rate) = zenith.attitude(&orbit, t);
        let boresight = q.to_matrix().r2;
        let (r, _) = orbit.state(t);
        assert!(boresight.angle_to(r) < 1e-9);
        assert!(
            (rate.norm() / (TAU / orbit.period()) - 1.0).abs() < 0.01,
            "{}",
            rate.norm()
        );
    }
}

#[test]
fn the_earth_the_sun_and_the_shadow_are_where_geometry_puts_them() {
    let orbit = Orbit::circular(400.0, 0.0, 0.0, EPOCH);
    let limb = (EARTH_RADIUS_KM / (EARTH_RADIUS_KM + 400.0)).asin();
    let exclusion = Exclusion {
        sun: 35.0 * DEG,
        earth_limb: 25.0 * DEG,
        moon: 10.0 * DEG,
    };
    let (mut eclipsed, samples) = (0, 5_000);
    for step in 0..samples {
        let t = orbit.period() * f64::from(step) / f64::from(samples);
        let (r, _) = orbit.state(t);
        let jd = orbit.julian_date(t);
        let up = r.normalized().unwrap();
        let above = Surroundings::of(up, r, jd);
        assert!((above.earth_limb_angle - (180.0 * DEG - limb)).abs() < 1e-9);
        assert_ne!(above.blinder(&exclusion), Some(Blinder::Earth));
        let below = Surroundings::of(up.scale(-1.0), r, jd);
        assert!((below.earth_limb_angle + limb).abs() < 1e-9);
        assert_eq!(below.blinder(&exclusion), Some(Blinder::Earth));
        let at_sun = Surroundings::of(sun_direction(jd), r, jd);
        assert!(at_sun.sun_angle < 1e-9);
        eclipsed += u32::from(above.in_eclipse);
    }
    // With the shadow a cylinder of the Earth's radius R, and the Sun an
    // angle b out of the plane of a circular orbit of radius a, the
    // shadowed share of the orbit is acos(sqrt(1 - (R/a)^2) / cos b) / pi.
    let (r0, v0) = orbit.state(0.0);
    let normal = r0.cross(v0).normalized().unwrap();
    let beta = 90.0 * DEG - normal.angle_to(sun_direction(EPOCH));
    let ratio = EARTH_RADIUS_KM / (EARTH_RADIUS_KM + 400.0);
    let expected = ((1.0 - ratio * ratio).sqrt() / beta.cos()).acos() / (180.0 * DEG);
    let measured = f64::from(eclipsed) / f64::from(samples);
    println!("eclipsed share of the orbit: {measured:.4}, geometry gives {expected:.4}");
    assert!((measured - expected).abs() < 0.002);
}
