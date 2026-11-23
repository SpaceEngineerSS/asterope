use crate::math::{atan2, cos, hypot, sin, wrap_two_pi, Mat3, Vec3};

/// Boresight right ascension / declination and roll, all in radians.
///
/// Sensor frame: +Z boresight, +X image right, +Y image down. `roll` is the
/// position angle of the image "up" direction, measured from celestial north
/// through east. At `roll = 0` north is up and east is to the left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaDecRoll {
    pub ra: f64,
    pub dec: f64,
    pub roll: f64,
}

fn sky_basis(ra: f64, dec: f64) -> (Vec3, Vec3, Vec3) {
    let (sa, ca) = (sin(ra), cos(ra));
    let (sd, cd) = (sin(dec), cos(dec));
    let boresight = Vec3::new(cd * ca, cd * sa, sd);
    let north = Vec3::new(-sd * ca, -sd * sa, cd);
    let east = Vec3::new(-sa, ca, 0.0);
    (boresight, north, east)
}

/// Attitude matrix (ICRS -> sensor) for a pointing.
pub fn attitude_from_radec_roll(pointing: RaDecRoll) -> Mat3 {
    let (boresight, north, east) = sky_basis(pointing.ra, pointing.dec);
    let up = north.scale(cos(pointing.roll)) + east.scale(sin(pointing.roll));
    let y_axis = -up;
    let x_axis = y_axis.cross(boresight);
    Mat3::from_rows(x_axis, y_axis, boresight)
}

pub fn radec_roll_from_attitude(attitude: &Mat3) -> RaDecRoll {
    let boresight = attitude.r2;
    let horizontal = hypot(boresight.x, boresight.y);
    let dec = atan2(boresight.z, horizontal);
    let ra = if horizontal > 1e-12 {
        wrap_two_pi(atan2(boresight.y, boresight.x))
    } else {
        0.0
    };
    let (_, north, east) = sky_basis(ra, dec);
    let up = -attitude.r1;
    let roll = wrap_two_pi(atan2(up.dot(east), up.dot(north)));
    RaDecRoll { ra, dec, roll }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Quat, DEG};

    #[test]
    fn zero_roll_has_north_up_east_left() {
        let a = attitude_from_radec_roll(RaDecRoll {
            ra: 0.0,
            dec: 0.0,
            roll: 0.0,
        });
        let north = a.mul_vec(Vec3::Z);
        let east = a.mul_vec(Vec3::Y);
        assert!((north.y + 1.0).abs() < 1e-15, "north maps to image up (-Y)");
        assert!((east.x + 1.0).abs() < 1e-15, "east maps to image left (-X)");
        assert!((a.det() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn round_trip() {
        for &(ra, dec, roll) in &[
            (83.822, -5.391, 12.0),
            (0.0, 89.0, 359.0),
            (271.5, -72.25, 180.0),
            (359.999, 0.0, 0.001),
        ] {
            let p = RaDecRoll {
                ra: ra * DEG,
                dec: dec * DEG,
                roll: roll * DEG,
            };
            let a = attitude_from_radec_roll(p);
            let back = radec_roll_from_attitude(&a);
            assert!((back.ra - p.ra).abs() < 1e-10);
            assert!((back.dec - p.dec).abs() < 1e-12);
            assert!((back.roll - p.roll).abs() < 1e-10);
            let q = Quat::from_matrix(&a).unwrap();
            let a2 = q.to_matrix();
            assert!((a2.r0 - a.r0).norm() < 1e-14);
            assert!((a2.r1 - a.r1).norm() < 1e-14);
            assert!((a2.r2 - a.r2).norm() < 1e-14);
        }
    }
}
