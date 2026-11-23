//! An orbit, a way of pointing, and what blinds a star tracker along the way.
//!
//! The orbit is a Kepler ellipse whose node, perigee and mean motion drift
//! at the secular rates of the Earth's oblateness (first order in J2; see
//! Vallado, *Fundamentals of Astrodynamics and Applications*). That follows
//! the slow turning of the orbit plane, which decides how the Sun stands to
//! the orbit over weeks. Along the orbit it is a mean ellipse and will run
//! ahead of or behind a real spacecraft by tens of kilometres a day; there
//! is no drag and no periodic term. It serves for asking how much of an
//! orbit a camera can see stars in, not for saying where a spacecraft is.

use crate::math::{atan2, cos, sin, sqrt, Mat3, Quat, Vec3, TAU};
use crate::solar::{moon, sun_direction};

const MU_KM3_S2: f64 = 398_600.441_8;
pub const EARTH_RADIUS_KM: f64 = 6378.137;
const J2: f64 = 1.082_63e-3;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Orbital elements at `epoch_jd`, angles in radians, equator and equinox
/// of J2000.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub semi_major_km: f64,
    pub eccentricity: f64,
    pub inclination: f64,
    /// Right ascension of the ascending node.
    pub node: f64,
    pub perigee: f64,
    pub mean_anomaly: f64,
    /// Julian date, terrestrial time, at which the elements hold.
    pub epoch_jd: f64,
}

impl Orbit {
    /// A circular orbit `altitude_km` above the equatorial radius.
    pub fn circular(altitude_km: f64, inclination: f64, node: f64, epoch_jd: f64) -> Orbit {
        Orbit {
            semi_major_km: EARTH_RADIUS_KM + altitude_km,
            eccentricity: 0.0,
            inclination,
            node,
            perigee: 0.0,
            mean_anomaly: 0.0,
            epoch_jd,
        }
    }

    fn mean_motion(&self) -> f64 {
        sqrt(MU_KM3_S2 / (self.semi_major_km * self.semi_major_km * self.semi_major_km))
    }

    /// Seconds for one revolution, without the oblateness correction.
    pub fn period(&self) -> f64 {
        TAU / self.mean_motion()
    }

    /// Secular rates of the node, the perigee and the mean anomaly, rad/s.
    pub fn secular_rates(&self) -> (f64, f64, f64) {
        let n = self.mean_motion();
        let e2 = self.eccentricity * self.eccentricity;
        let p = self.semi_major_km * (1.0 - e2);
        let factor = n * J2 * (EARTH_RADIUS_KM / p) * (EARTH_RADIUS_KM / p);
        let c = cos(self.inclination);
        (
            -1.5 * factor * c,
            0.75 * factor * (5.0 * c * c - 1.0),
            n + 0.75 * factor * sqrt(1.0 - e2) * (3.0 * c * c - 1.0),
        )
    }

    /// Position (km) and velocity (km/s) from the Earth's center, `seconds`
    /// after the epoch, in the ICRS axes.
    pub fn state(&self, seconds: f64) -> (Vec3, Vec3) {
        let (node_rate, perigee_rate, anomaly_rate) = self.secular_rates();
        let node = self.node + node_rate * seconds;
        let perigee = self.perigee + perigee_rate * seconds;
        let mean = self.mean_anomaly + anomaly_rate * seconds;
        let e = self.eccentricity;
        let mut eccentric = mean;
        for _ in 0..12 {
            eccentric -= (eccentric - e * sin(eccentric) - mean) / (1.0 - e * cos(eccentric));
        }
        let (a, root) = (self.semi_major_km, sqrt(1.0 - e * e));
        let radius = a * (1.0 - e * cos(eccentric));
        let speed = sqrt(MU_KM3_S2 * a) / radius;
        let in_plane = (a * (cos(eccentric) - e), a * root * sin(eccentric));
        let in_plane_rate = (-speed * sin(eccentric), speed * root * cos(eccentric));
        let (cw, sw) = (cos(perigee), sin(perigee));
        let (cn, sn) = (cos(node), sin(node));
        let (ci, si) = (cos(self.inclination), sin(self.inclination));
        let p = Vec3::new(cw * cn - sw * sn * ci, cw * sn + sw * cn * ci, sw * si);
        let q = Vec3::new(-sw * cn - cw * sn * ci, -sw * sn + cw * cn * ci, cw * si);
        (
            p.scale(in_plane.0) + q.scale(in_plane.1),
            p.scale(in_plane_rate.0) + q.scale(in_plane_rate.1),
        )
    }

    pub fn julian_date(&self, seconds: f64) -> f64 {
        self.epoch_jd + seconds / SECONDS_PER_DAY
    }
}

/// How the camera is carried.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointing {
    /// Fixed to the orbit: the boresight is given in the local frame whose
    /// x axis is along the velocity (for a circular orbit), y along the
    /// orbit normal and z away from the Earth. `roll` turns the image about
    /// the boresight.
    Local { boresight: Vec3, roll: f64 },
    /// Held against the stars.
    Inertial(Quat),
    /// Turning about a fixed axis of the sensor at a steady rate (rad/s),
    /// from `start` at the epoch: a spinning or tumbling spacecraft.
    Spinning { start: Quat, rate: Vec3 },
}

impl Pointing {
    /// Attitude, ICRS to sensor, and the body rate about the sensor axes.
    pub fn attitude(&self, orbit: &Orbit, seconds: f64) -> (Quat, Vec3) {
        match *self {
            Pointing::Inertial(q) => (q, Vec3::ZERO),
            Pointing::Spinning { start, rate } => {
                (crate::track::propagate(start, rate, seconds), rate)
            }
            Pointing::Local { boresight, roll } => {
                let at = |t: f64| local_attitude(orbit, t, boresight, roll);
                let q = at(seconds);
                // The rate follows from the attitudes a second apart.
                let rate = crate::track::rate_between(at(seconds - 0.5), at(seconds + 0.5), 1.0);
                (q, rate)
            }
        }
    }
}

fn local_attitude(orbit: &Orbit, seconds: f64, boresight: Vec3, roll: f64) -> Quat {
    let (position, velocity) = orbit.state(seconds);
    let up = position.normalized().unwrap_or(Vec3::Z);
    let normal = position.cross(velocity).normalized().unwrap_or(Vec3::Z);
    let along = normal.cross(up);
    let local = boresight.normalized().unwrap_or(Vec3::Z);
    let z = (along.scale(local.x) + normal.scale(local.y) + up.scale(local.z))
        .normalized()
        .unwrap_or(up);
    // Image "up" starts from the orbit normal, or from the velocity when
    // the camera looks along the normal, and is then turned by the roll.
    let reference = if z.cross(normal).norm() > 1e-6 {
        normal
    } else {
        along
    };
    let x0 = reference.cross(z).normalized().unwrap_or(along);
    let y0 = z.cross(x0);
    let (c, s) = (cos(roll), sin(roll));
    let x = x0.scale(c) + y0.scale(s);
    let y = z.cross(x);
    Quat::from_matrix(&Mat3::from_rows(x, y, z)).unwrap_or(Quat::IDENTITY)
}

/// Angles inside which a bright body keeps the camera from working,
/// radians from the boresight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exclusion {
    /// From the center of the Sun.
    pub sun: f64,
    /// From the Earth's limb, sunlit or not.
    pub earth_limb: f64,
    /// From the center of the Moon.
    pub moon: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blinder {
    Sun,
    Earth,
    Moon,
}

/// Where the bright bodies stand for an observer, as angles from a
/// boresight given in the ICRS axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings {
    pub sun_angle: f64,
    /// Angle to the nearest point of the Earth's limb; negative when the
    /// boresight meets the Earth.
    pub earth_limb_angle: f64,
    pub moon_angle: f64,
    /// In the Earth's shadow, taken as a cylinder.
    pub in_eclipse: bool,
}

impl Surroundings {
    pub fn of(boresight: Vec3, position_km: Vec3, jd_tt: f64) -> Surroundings {
        let sun = sun_direction(jd_tt);
        let (moon_direction, _, _) = moon(jd_tt, position_km);
        let distance = position_km.norm();
        let to_earth = position_km.scale(-1.0 / distance.max(1.0));
        let limb = if distance > EARTH_RADIUS_KM {
            atan2(
                EARTH_RADIUS_KM,
                sqrt(distance * distance - EARTH_RADIUS_KM * EARTH_RADIUS_KM),
            )
        } else {
            0.5 * crate::math::PI
        };
        let along_sun = position_km.dot(sun);
        let off_axis = (position_km - sun.scale(along_sun)).norm();
        Surroundings {
            sun_angle: boresight.angle_to(sun),
            earth_limb_angle: boresight.angle_to(to_earth) - limb,
            moon_angle: boresight.angle_to(moon_direction),
            in_eclipse: along_sun < 0.0 && off_axis < EARTH_RADIUS_KM,
        }
    }

    /// The first body inside its exclusion angle, the Earth before the Sun
    /// before the Moon. In the Earth's shadow the Sun blinds nothing.
    pub fn blinder(&self, exclusion: &Exclusion) -> Option<Blinder> {
        if self.earth_limb_angle < exclusion.earth_limb {
            Some(Blinder::Earth)
        } else if self.sun_angle < exclusion.sun && !self.in_eclipse {
            Some(Blinder::Sun)
        } else if self.moon_angle < exclusion.moon {
            Some(Blinder::Moon)
        } else {
            None
        }
    }
}
