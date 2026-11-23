//! Stellar aberration and the velocity that causes it.
//!
//! An observer moving at velocity `v` sees a star displaced toward the
//! direction of motion: to first order in `v / c` the unit vector `s` becomes
//! `normalize(s + v / c)`. Earth's orbital motion gives up to 20.5
//! arcseconds and a low Earth orbit adds up to 5; the neglected second-order
//! term is below 0.001 arcseconds. The correction is applied to the catalog,
//! so a solved attitude refers to the ICRS axes rather than to the apparent
//! sky.

use crate::math::{cos, sin, Vec3, DEG};

pub const SPEED_OF_LIGHT_KM_S: f64 = 299_792.458;
const AU_KM: f64 = 149_597_870.7;
const SECONDS_PER_DAY: f64 = 86_400.0;
const J2000_JD: f64 = 2_451_545.0;

/// Velocity in km/s as a fraction of the speed of light.
pub fn beta(velocity_km_s: Vec3) -> Vec3 {
    velocity_km_s.scale(1.0 / SPEED_OF_LIGHT_KM_S)
}

/// Apparent direction of `star` for an observer with velocity `beta = v / c`.
pub fn aberrate(star: Vec3, beta: Vec3) -> Vec3 {
    (star + beta).normalized().unwrap_or(star)
}

/// Heliocentric position of the Earth in the ICRS axes, astronomical units.
///
/// Low-precision solar coordinates of the Astronomical Almanac (mean
/// longitude, mean anomaly, equation of center to two terms), with the
/// longitude carried from the equinox of date back to J2000 by the general
/// precession, 1.397 degrees per century. Good to about 0.01 degrees between
/// 1950 and 2050.
fn earth_position_au(jd_tt: f64) -> Vec3 {
    let days = jd_tt - J2000_JD;
    let mean_longitude = (280.460 + 0.985_647_4 * days) * DEG;
    let anomaly = (357.528 + 0.985_600_3 * days) * DEG;
    let sun_longitude = mean_longitude + (1.915 * sin(anomaly) + 0.020 * sin(2.0 * anomaly)) * DEG
        - 1.396_971 * DEG * days / 36_525.0;
    let distance = 1.000_14 - 0.016_71 * cos(anomaly) - 0.000_14 * cos(2.0 * anomaly);
    let obliquity = 23.439_291 * DEG;
    Vec3::new(
        cos(sun_longitude),
        sin(sun_longitude) * cos(obliquity),
        sin(sun_longitude) * sin(obliquity),
    )
    .scale(-distance)
}

/// Velocity of the Earth about the Sun in the ICRS axes, km/s, at a Julian
/// date in terrestrial time. It agrees with a two-body orbit from Standish's
/// mean elements within 0.003 km/s over 1975-2025 (0.002 arcseconds of
/// aberration); the Sun's own motion about the barycenter, at most
/// 0.016 km/s, is left out. Add the spacecraft's velocity relative to the
/// Earth before applying the correction.
pub fn earth_velocity_km_s(jd_tt: f64) -> Vec3 {
    const HALF_STEP_DAYS: f64 = 0.5;
    (earth_position_au(jd_tt + HALF_STEP_DAYS) - earth_position_au(jd_tt - HALF_STEP_DAYS))
        .scale(AU_KM / (2.0 * HALF_STEP_DAYS * SECONDS_PER_DAY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::ARCSEC;

    /// Velocity from a two-body orbit with Standish's mean elements of the
    /// Earth-Moon barycenter ("Keplerian Elements for Approximate Positions
    /// of the Major Planets", JPL), an independent route to the same quantity.
    fn keplerian_velocity_km_s(jd_tt: f64) -> Vec3 {
        let centuries = (jd_tt - J2000_JD) / 36_525.0;
        let a = 1.000_002_61 + 0.000_005_62 * centuries;
        let e = 0.016_711_23 - 0.000_043_92 * centuries;
        let mean_longitude = (100.464_571_66 + 35_999.372_449_81 * centuries) * DEG;
        let perihelion = (102.937_681_93 + 0.323_273_64 * centuries) * DEG;
        let mean_anomaly = mean_longitude - perihelion;
        let mut eccentric = mean_anomaly;
        for _ in 0..8 {
            eccentric = mean_anomaly + e * eccentric.sin();
        }
        let gauss = 0.017_202_098_95f64;
        let radius = a * (1.0 - e * eccentric.cos());
        let speed = gauss * a.sqrt() / radius;
        let in_plane = (
            -speed * eccentric.sin(),
            speed * (1.0 - e * e).sqrt() * eccentric.cos(),
        );
        let ecliptic = (
            in_plane.0 * perihelion.cos() - in_plane.1 * perihelion.sin(),
            in_plane.0 * perihelion.sin() + in_plane.1 * perihelion.cos(),
        );
        let obliquity = 23.439_291 * DEG;
        Vec3::new(
            ecliptic.0,
            ecliptic.1 * obliquity.cos(),
            ecliptic.1 * obliquity.sin(),
        )
        .scale(AU_KM / SECONDS_PER_DAY)
    }

    #[test]
    fn earth_velocity_agrees_with_a_keplerian_orbit() {
        let mut worst: f64 = 0.0;
        for month in 0..600 {
            let jd = J2000_JD - 9_000.0 + 30.4 * f64::from(month);
            let difference = (earth_velocity_km_s(jd) - keplerian_velocity_km_s(jd)).norm();
            worst = worst.max(difference);
        }
        println!("largest difference over 1975-2025: {worst:.3} km/s");
        assert!(worst < 0.01, "{worst}");
        let at_j2000 = earth_velocity_km_s(J2000_JD);
        assert!((at_j2000.norm() - 30.28).abs() < 0.05, "{at_j2000:?}");
        assert!(at_j2000.x < -29.0);
    }
    #[test]
    fn earth_velocity_stays_perpendicular_to_the_ecliptic_pole_and_within_orbital_speeds() {
        let obliquity = 23.439_291 * DEG;
        let pole = Vec3::new(0.0, -sin(obliquity), cos(obliquity));
        for day in 0..730 {
            let velocity = earth_velocity_km_s(J2000_JD + 9_000.0 + f64::from(day));
            let speed = velocity.norm();
            assert!((29.2..=30.4).contains(&speed), "{speed}");
            assert!(velocity.dot(pole).abs() < 0.01);
        }
    }

    #[test]
    fn aberration_displaces_a_star_by_v_over_c_toward_the_motion() {
        let velocity = Vec3::new(0.0, 29.8, 0.0);
        let star = Vec3::X;
        let apparent = aberrate(star, beta(velocity));
        let shift = apparent.angle_to(star);
        assert!((shift / ARCSEC - 20.5).abs() < 0.05, "{}", shift / ARCSEC);
        assert!(apparent.y > 0.0);
        let along = aberrate(Vec3::Y, beta(velocity));
        assert!(along.angle_to(Vec3::Y) < 1e-12);
    }
}
