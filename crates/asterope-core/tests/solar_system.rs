//! The low-precision Sun, Moon and planets against ERFA.
//!
//! `tests/reference/solar_system.csv` is written by the script beside it.
//! `cargo test --release -p asterope-core --test solar_system -- --nocapture`
//! prints the largest difference for each body over 1990-2040.

use asterope_core::math::{Vec3, DEG};
use asterope_core::solar::{
    add_refraction, moon, planet, refraction, remove_refraction, sun_direction, zenith, Planet,
};

const REFERENCE: &str = include_str!("reference/solar_system.csv");
const AU_KM: f64 = 149_597_870.7;

/// Largest difference tolerated, degrees: the stated accuracy of each source.
fn limit(kind: &str) -> f64 {
    match kind {
        "sun" => 0.02,
        "moon" => 0.5,
        "jupiter" | "saturn" => 0.25,
        _ => 0.06,
    }
}

#[test]
fn positions_agree_with_erfa_within_the_stated_accuracy() {
    let mut worst: Vec<(String, f64, f64)> = Vec::new();
    for line in REFERENCE.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        let [kind, jd, x, y, z, distance] = fields[..] else {
            panic!("malformed row {line}");
        };
        if kind == "refraction" {
            continue;
        }
        let number = |text: &str| text.parse::<f64>().unwrap();
        let (jd, expected) = (number(jd), Vec3::new(number(x), number(y), number(z)));
        let (direction, range) = match kind {
            "sun" => (sun_direction(jd), f64::NAN),
            "moon" => {
                let (direction, km, lit) = moon(jd, Vec3::ZERO);
                assert!((0.0..=1.0).contains(&lit));
                (direction, km / AU_KM)
            }
            name => {
                let body = match name {
                    "mercury" => Planet::Mercury,
                    "venus" => Planet::Venus,
                    "mars" => Planet::Mars,
                    "jupiter" => Planet::Jupiter,
                    "saturn" => Planet::Saturn,
                    "uranus" => Planet::Uranus,
                    "neptune" => Planet::Neptune,
                    other => panic!("unknown body {other}"),
                };
                let seen = planet(body, jd);
                (seen.dir, seen.distance_au)
            }
        };
        let angle = direction.angle_to(expected) / DEG;
        let range_error = (range / number(distance) - 1.0).abs();
        match worst.iter_mut().find(|(name, _, _)| name == kind) {
            Some(entry) => {
                entry.1 = entry.1.max(angle);
                entry.2 = entry.2.max(range_error);
            }
            None => worst.push((kind.to_string(), angle, range_error)),
        }
    }
    println!("| Body | Largest difference in direction (deg) | In distance (%) |");
    println!("|---|---|---|");
    for (kind, angle, range) in &worst {
        println!("| {kind} | {angle:.4} | {:.2} |", 100.0 * range);
        assert!(*angle < limit(kind), "{kind}: {angle} deg");
        assert!(range.is_nan() || *range < 0.02, "{kind}: distance {range}");
    }
    assert_eq!(worst.len(), 9);
}

#[test]
fn refraction_agrees_with_erfa_above_fifteen_degrees() {
    for line in REFERENCE
        .lines()
        .filter(|line| line.starts_with("refraction"))
    {
        let fields: Vec<&str> = line.split(',').collect();
        let altitude: f64 = fields[1].parse().unwrap();
        let expected: f64 = fields[2].parse().unwrap();
        // ERFA takes the true zenith distance, Bennett the apparent
        // altitude; the two differ by the refraction itself, which changes
        // the result by under an arcsecond at these altitudes.
        let ours = refraction(altitude * DEG + expected);
        let difference = (ours - expected).abs() / DEG * 3600.0;
        println!(
            "altitude {altitude:>2} deg: {:.1}\" against {:.1}\"",
            ours / DEG * 3600.0,
            expected / DEG * 3600.0
        );
        assert!(difference < 6.0, "{altitude} deg: {difference} arcsec");
    }
    assert!(refraction(0.0) / DEG * 60.0 > 30.0 && refraction(0.0) / DEG * 60.0 < 37.0);
}

#[test]
fn the_zenith_follows_the_sidereal_time_of_astropy() {
    // Local mean sidereal time from astropy 6.1 (`Time.sidereal_time("mean")`)
    // for a Julian date in UT1 and an east longitude, degrees.
    for (jd, longitude, sidereal) in [
        (2_460_061.868, -111.424, 236.145_138_712),
        (2_451_545.0, 0.0, 280.460_622_430),
        (2_466_000.25, 29.0, 247.239_744_161),
    ] {
        let up = zenith(jd, 35.0 * DEG, longitude * DEG);
        let ra = up.y.atan2(up.x).rem_euclid(360.0 * DEG) / DEG;
        let difference = (ra - sidereal + 180.0).rem_euclid(360.0) - 180.0;
        assert!(difference.abs() < 0.001, "{jd}: {ra} against {sidereal}");
        assert!((up.z.asin() / DEG - 35.0).abs() < 1e-9);
    }
}

#[test]
fn refraction_raises_a_star_and_can_be_taken_out_again() {
    let up = Vec3::Z;
    for altitude in [2.0, 10.0, 30.0, 60.0, 89.0] {
        let truth = Vec3::new((altitude * DEG).cos(), 0.0, (altitude * DEG).sin());
        let seen = add_refraction(truth, up);
        let raised = seen.z.asin() - altitude * DEG;
        assert!((raised - refraction(seen.z.asin())).abs() < 1e-8);
        assert!(raised > 0.0 && seen.y.abs() < 1e-15);
        assert!(remove_refraction(seen, up).angle_to(truth) < 1e-8);
    }
    let below = Vec3::new(0.94, 0.0, -0.342).normalized().unwrap();
    assert_eq!(add_refraction(below, up), below);
}
