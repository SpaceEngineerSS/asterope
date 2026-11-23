//! Where the Sun, the Moon and the planets are, to the accuracy a star
//! tracker needs to keep clear of them or to recognise them as not stars.
//!
//! Planets follow two-body orbits from Standish's mean elements
//! ("Keplerian Elements for Approximate Positions of the Major Planets",
//! JPL Solar System Dynamics, table 1, valid 1800-2050). The Sun and the
//! Moon use the low-precision series of the Astronomical Almanac
//! (section C and page D22). Directions are geometric, corrected for light
//! time, in the ICRS axes; none of this is an ephemeris for pointing at the
//! bodies. `tests/solar_system.rs` states how far each is from ERFA.

use alloc::vec::Vec;

use crate::catalog::Star;
use crate::math::{abs, atan, atan2, cos, log10, sin, sqrt, tan, Vec3, DEG};

const J2000_JD: f64 = 2_451_545.0;
const AU_KM: f64 = 149_597_870.7;
/// Light time for one astronomical unit, days.
const LIGHT_DAYS_PER_AU: f64 = 0.005_775_518_3;
const EARTH_RADIUS_KM: f64 = 6378.137;
/// Mean obliquity of the ecliptic at J2000.
const OBLIQUITY: f64 = 23.439_291 * DEG;
/// General precession in longitude, per Julian century.
const PRECESSION_PER_CENTURY: f64 = 1.396_971 * DEG;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Planet {
    Mercury,
    Venus,
    Mars,
    Jupiter,
    Saturn,
    Uranus,
    Neptune,
}

pub const PLANETS: [Planet; 7] = [
    Planet::Mercury,
    Planet::Venus,
    Planet::Mars,
    Planet::Jupiter,
    Planet::Saturn,
    Planet::Uranus,
    Planet::Neptune,
];

/// Semi-major axis (au), eccentricity, inclination, mean longitude,
/// longitude of perihelion and of the ascending node (degrees), each with
/// its change per Julian century, on the ecliptic and equinox of J2000.
struct Elements([(f64, f64); 6]);

const EARTH_MOON: Elements = Elements([
    (1.000_002_61, 0.000_005_62),
    (0.016_711_23, -0.000_043_92),
    (-0.000_015_31, -0.012_946_68),
    (100.464_571_66, 35_999.372_449_81),
    (102.937_681_93, 0.323_273_64),
    (0.0, 0.0),
]);

impl Planet {
    fn elements(self) -> Elements {
        Elements(match self {
            Planet::Mercury => [
                (0.387_099_27, 0.000_000_37),
                (0.205_635_93, 0.000_019_06),
                (7.004_979_02, -0.005_947_49),
                (252.250_323_50, 149_472.674_111_75),
                (77.457_796_28, 0.160_476_89),
                (48.330_765_93, -0.125_340_81),
            ],
            Planet::Venus => [
                (0.723_335_66, 0.000_003_90),
                (0.006_776_72, -0.000_041_07),
                (3.394_676_05, -0.000_788_90),
                (181.979_099_50, 58_517.815_387_29),
                (131.602_467_18, 0.002_683_29),
                (76.679_842_55, -0.277_694_18),
            ],
            Planet::Mars => [
                (1.523_710_34, 0.000_018_47),
                (0.093_394_10, 0.000_078_82),
                (1.849_691_42, -0.008_131_31),
                (-4.553_432_05, 19_140.302_684_99),
                (-23.943_629_59, 0.444_410_88),
                (49.559_538_91, -0.292_573_43),
            ],
            Planet::Jupiter => [
                (5.202_887_00, -0.000_116_07),
                (0.048_386_24, -0.000_132_53),
                (1.304_396_95, -0.001_837_14),
                (34.396_440_51, 3_034.746_127_75),
                (14.728_479_83, 0.212_526_68),
                (100.473_909_09, 0.204_691_06),
            ],
            Planet::Saturn => [
                (9.536_675_94, -0.001_250_60),
                (0.053_861_79, -0.000_509_91),
                (2.485_991_87, 0.001_936_09),
                (49.954_244_23, 1_222.493_622_01),
                (92.598_878_31, -0.418_972_16),
                (113.662_424_48, -0.288_677_94),
            ],
            Planet::Uranus => [
                (19.189_164_64, -0.001_961_76),
                (0.047_257_44, -0.000_043_97),
                (0.772_637_83, -0.002_429_39),
                (313.238_104_51, 428.482_027_85),
                (170.954_276_30, 0.408_052_81),
                (74.016_925_03, 0.042_405_89),
            ],
            Planet::Neptune => [
                (30.069_922_76, 0.000_262_91),
                (0.008_590_48, 0.000_051_05),
                (1.770_043_47, 0.000_353_72),
                (-55.120_029_69, 218.459_453_25),
                (44.964_762_27, -0.322_414_64),
                (131.784_225_74, -0.005_086_64),
            ],
        })
    }

    /// Magnitude at one astronomical unit from the Sun and from the observer
    /// at full phase, and the coefficients of the phase angle in degrees,
    /// its square and its cube: the phase laws the Astronomical Almanac
    /// used before 2007 (after Harris 1961). Good to a few tenths of a
    /// magnitude; Saturn's rings, which add up to a magnitude, are left out.
    fn photometry(self) -> (f64, [f64; 3]) {
        match self {
            Planet::Mercury => (-0.42, [0.038, -0.000_273, 0.000_002]),
            Planet::Venus => (-4.40, [0.000_9, 0.000_239, -0.000_000_65]),
            Planet::Mars => (-1.52, [0.016, 0.0, 0.0]),
            Planet::Jupiter => (-9.40, [0.005, 0.0, 0.0]),
            Planet::Saturn => (-8.88, [0.044, 0.0, 0.0]),
            Planet::Uranus => (-7.19, [0.002, 0.0, 0.0]),
            Planet::Neptune => (-6.87, [0.0, 0.0, 0.0]),
        }
    }
}

fn ecliptic_to_icrs(v: Vec3) -> Vec3 {
    let (s, c) = (sin(OBLIQUITY), cos(OBLIQUITY));
    Vec3::new(v.x, v.y * c - v.z * s, v.y * s + v.z * c)
}

impl Elements {
    /// Heliocentric position in the ICRS axes, astronomical units.
    fn position(&self, jd_tt: f64) -> Vec3 {
        let centuries = (jd_tt - J2000_JD) / 36_525.0;
        let [a, e, inclination, mean_longitude, perihelion, node] =
            self.0.map(|(value, rate)| value + rate * centuries);
        let (inclination, perihelion, node) = (inclination * DEG, perihelion * DEG, node * DEG);
        let mean_anomaly = mean_longitude * DEG - perihelion;
        let mut eccentric = mean_anomaly;
        for _ in 0..12 {
            eccentric -=
                (eccentric - e * sin(eccentric) - mean_anomaly) / (1.0 - e * cos(eccentric));
        }
        let (x, y) = (
            a * (cos(eccentric) - e),
            a * sqrt(1.0 - e * e) * sin(eccentric),
        );
        let argument = perihelion - node;
        let (cw, sw) = (cos(argument), sin(argument));
        let (cn, sn) = (cos(node), sin(node));
        let (ci, si) = (cos(inclination), sin(inclination));
        ecliptic_to_icrs(Vec3::new(
            (cw * cn - sw * sn * ci) * x + (-sw * cn - cw * sn * ci) * y,
            (cw * sn + sw * cn * ci) * x + (-sw * sn + cw * cn * ci) * y,
            sw * si * x + cw * si * y,
        ))
    }
}

/// A body as seen from the Earth's center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sighting {
    /// Unit vector toward the body, ICRS axes.
    pub dir: Vec3,
    /// Distance from the observer, astronomical units.
    pub distance_au: f64,
    /// Apparent V magnitude.
    pub mag: f64,
}

/// Where a planet is seen from the center of the Earth at a Julian date in
/// terrestrial time, and how bright.
pub fn planet(planet: Planet, jd_tt: f64) -> Sighting {
    let elements = planet.elements();
    let earth = EARTH_MOON.position(jd_tt);
    let mut sun_to_planet = elements.position(jd_tt);
    let mut seen = sun_to_planet - earth;
    for _ in 0..2 {
        sun_to_planet = elements.position(jd_tt - LIGHT_DAYS_PER_AU * seen.norm());
        seen = sun_to_planet - earth;
    }
    let (distance, from_sun) = (seen.norm(), sun_to_planet.norm());
    let phase = seen.angle_to(sun_to_planet) / DEG;
    let (unit, [linear, square, cube]) = planet.photometry();
    Sighting {
        dir: seen.normalized().unwrap_or(Vec3::X),
        distance_au: distance,
        mag: unit
            + 5.0 * log10(distance * from_sun)
            + phase * (linear + phase * (square + phase * cube)),
    }
}

/// Unit vector from the center of the Earth to the Sun, ICRS axes.
pub fn sun_direction(jd_tt: f64) -> Vec3 {
    EARTH_MOON
        .position(jd_tt)
        .scale(-1.0)
        .normalized()
        .unwrap_or(Vec3::X)
}

/// The Moon as seen from a point `observer_km` from the center of the Earth
/// (ICRS axes): direction, distance in kilometres, and the lit share of the
/// disc. The series is good to about 0.3 degrees in longitude and 0.2 in
/// latitude; from low orbit the parallax handled here is up to 1 degree.
pub fn moon(jd_tt: f64, observer_km: Vec3) -> (Vec3, f64, f64) {
    let t = (jd_tt - J2000_JD) / 36_525.0;
    let term = |amplitude: f64, phase: f64, rate: f64| (amplitude, (phase + rate * t) * DEG);
    let longitude = [
        term(6.29, 135.0, 477_198.87),
        term(-1.27, 259.3, -413_335.36),
        term(0.66, 235.7, 890_534.22),
        term(0.21, 269.9, 954_397.74),
        term(-0.19, 357.5, 35_999.05),
        term(-0.11, 186.5, 966_404.03),
    ]
    .iter()
    .fold(218.32 + 481_267.881 * t, |sum, (amplitude, angle)| {
        sum + amplitude * sin(*angle)
    });
    let latitude = [
        term(5.13, 93.3, 483_202.02),
        term(0.28, 228.2, 960_400.89),
        term(-0.28, 318.3, 6_003.15),
        term(-0.17, 217.6, -407_332.21),
    ]
    .iter()
    .fold(0.0, |sum, (amplitude, angle)| sum + amplitude * sin(*angle));
    let parallax = [
        term(0.0518, 135.0, 477_198.87),
        term(0.0095, 259.3, -413_335.36),
        term(0.0078, 235.7, 890_534.22),
        term(0.0028, 269.9, 954_397.74),
    ]
    .iter()
    .fold(0.9508, |sum, (amplitude, angle)| {
        sum + amplitude * cos(*angle)
    });
    // The series refers to the equinox of date; the ICRS axes are those of J2000.
    let longitude = longitude * DEG - PRECESSION_PER_CENTURY * t;
    let latitude = latitude * DEG;
    let distance = EARTH_RADIUS_KM / sin(parallax * DEG);
    let from_center = ecliptic_to_icrs(Vec3::new(
        cos(latitude) * cos(longitude),
        cos(latitude) * sin(longitude),
        sin(latitude),
    ))
    .scale(distance);
    let seen = from_center - observer_km;
    let to_sun = sun_direction(jd_tt).scale(AU_KM) - from_center;
    let phase = seen.scale(-1.0).angle_to(to_sun);
    (
        seen.normalized().unwrap_or(Vec3::X),
        seen.norm(),
        0.5 * (1.0 + cos(phase)),
    )
}

/// Identifiers given to the planets when they are drawn as stars. They lie
/// above every Hipparcos number and below the Tycho-2 flag.
pub const PLANET_ID_BASE: u32 = 0x7fff_ff00;

/// The planets brighter than `mag_limit`, as stars a simulator can draw.
/// No catalog holds them, so a solver meets them as bright unknown objects.
pub fn planets_as_stars(jd_tt: f64, mag_limit: f64) -> Vec<Star> {
    PLANETS
        .iter()
        .enumerate()
        .filter_map(|(number, body)| {
            let seen = planet(*body, jd_tt);
            (seen.mag <= mag_limit).then_some(Star {
                hip: PLANET_ID_BASE + number as u32,
                dir: seen.dir,
                pm_ra: 0.0,
                pm_dec: 0.0,
                mag: seen.mag,
                b_v: None,
            })
        })
        .collect()
}

/// Atmospheric refraction at an apparent altitude, radians: how far above
/// its true place a star is seen. Bennett's formula (G. G. Bennett, "The
/// calculation of astronomical refraction in marine navigation", Journal
/// of Navigation 35, 1982) for 1010 hPa and 10 degrees Celsius, good to
/// 0.1 arcminutes down to the horizon; scale by `P / 1010 * 283 / (273 + T)`
/// for other conditions.
pub fn refraction(apparent_altitude: f64) -> f64 {
    let degrees = (apparent_altitude / DEG).clamp(-1.0, 90.0);
    let arcminutes = 1.0 / tan((degrees + 7.31 / (degrees + 4.4)) * DEG);
    arcminutes.max(0.0) * DEG / 60.0
}

/// True direction of a star seen at `apparent`, for an observer whose
/// zenith is `zenith` (unit vectors in any common frame): the apparent
/// direction lowered by the refraction at its altitude.
pub fn remove_refraction(apparent: Vec3, zenith: Vec3) -> Vec3 {
    let sine = apparent.dot(zenith).clamp(-1.0, 1.0);
    let altitude = atan2(sine, sqrt(1.0 - sine * sine));
    let Some(horizontal) = (apparent - zenith.scale(sine)).normalized() else {
        return apparent;
    };
    let lowered = altitude - refraction(altitude);
    horizontal.scale(cos(lowered)) + zenith.scale(sin(lowered))
}

/// Where a true direction is seen through the atmosphere: raised by the
/// refraction at the altitude it ends up at. Directions more than a degree
/// below the horizon are returned as they are.
pub fn add_refraction(direction: Vec3, zenith: Vec3) -> Vec3 {
    let sine = direction.dot(zenith).clamp(-1.0, 1.0);
    let altitude = atan2(sine, sqrt(1.0 - sine * sine));
    let Some(horizontal) = (direction - zenith.scale(sine)).normalized() else {
        return direction;
    };
    if altitude < -DEG {
        return direction;
    }
    // Bennett's formula takes the apparent altitude, which is what is sought.
    let mut apparent = altitude;
    for _ in 0..8 {
        apparent = altitude + refraction(apparent);
    }
    horizontal.scale(cos(apparent)) + zenith.scale(sin(apparent))
}

/// Direction of the zenith in the ICRS axes for a place on the Earth
/// (latitude and east longitude, radians) at a Julian date in universal
/// time. Uses the mean sidereal time (Meeus, *Astronomical Algorithms*,
/// eq. 12.4) and leaves out precession since J2000 and nutation, which
/// move the zenith by 0.014 degrees a year. In 2026 that changes the
/// refraction by 5 arcseconds at 15 degrees of altitude and by under one
/// above 40.
pub fn zenith(jd_ut: f64, latitude: f64, east_longitude: f64) -> Vec3 {
    let days = jd_ut - J2000_JD;
    let sidereal = (280.460_618_37 + 360.985_647_366_29 * days) * DEG + east_longitude;
    Vec3::new(
        cos(latitude) * cos(sidereal),
        cos(latitude) * sin(sidereal),
        sin(latitude),
    )
}

/// Largest angle by which the direction of a body at `distance_km` changes
/// between the center of the Earth and an observer `altitude_km` above it.
pub fn parallax(distance_km: f64, altitude_km: f64) -> f64 {
    atan(abs(EARTH_RADIUS_KM + altitude_km) / distance_km)
}
