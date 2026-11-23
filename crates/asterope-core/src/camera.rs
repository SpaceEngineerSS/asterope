use crate::error::AsteropeError;
use crate::math::{abs, atan, atan2, cos, hypot, sin, tan, Vec3};

/// Brown-Conrady lens distortion on normalized image coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Distortion {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub p1: f64,
    pub p2: f64,
}

impl Distortion {
    pub const NONE: Distortion = Distortion {
        k1: 0.0,
        k2: 0.0,
        k3: 0.0,
        p1: 0.0,
        p2: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Distortion::NONE
    }

    pub fn distort(&self, x: f64, y: f64) -> (f64, f64) {
        let r2 = x * x + y * y;
        let radial = 1.0 + r2 * (self.k1 + r2 * (self.k2 + r2 * self.k3));
        let xd = x * radial + 2.0 * self.p1 * x * y + self.p2 * (r2 + 2.0 * x * x);
        let yd = y * radial + self.p1 * (r2 + 2.0 * y * y) + 2.0 * self.p2 * x * y;
        (xd, yd)
    }

    /// Inverse by fixed-point iteration.
    pub fn undistort(&self, xd: f64, yd: f64) -> (f64, f64) {
        if self.is_identity() {
            return (xd, yd);
        }
        let (mut x, mut y) = (xd, yd);
        for _ in 0..100 {
            let r2 = x * x + y * y;
            let radial = 1.0 + r2 * (self.k1 + r2 * (self.k2 + r2 * self.k3));
            if radial == 0.0 {
                break;
            }
            let dx = 2.0 * self.p1 * x * y + self.p2 * (r2 + 2.0 * x * x);
            let dy = self.p1 * (r2 + 2.0 * y * y) + 2.0 * self.p2 * x * y;
            let nx = (xd - dx) / radial;
            let ny = (yd - dy) / radial;
            let change = abs(nx - x) + abs(ny - y);
            x = nx;
            y = ny;
            if change < 1e-16 {
                break;
            }
        }
        (x, y)
    }
}

/// How the angle `theta` from the boresight becomes a distance `r` from the
/// principal point, in focal lengths and before distortion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Projection {
    /// `r = tan(theta)`: a pinhole, which keeps straight lines straight.
    /// Lenses up to about 70 degrees are built this way.
    #[default]
    Gnomonic,
    /// `r = theta`: the layout wide lenses approach. A polynomial distortion
    /// on a pinhole cannot stand in for it beyond about 45 degrees off
    /// axis, where `r (1 + k1 r^2)` with the `k1` it would need turns back.
    Equidistant,
}

/// Camera in the sensor frame (+Z boresight, +X right, +Y down).
/// Pixel centers sit at integer coordinates; `(0, 0)` is the top-left pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub width: u32,
    pub height: u32,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub distortion: Distortion,
    pub projection: Projection,
}

/// Largest angle from the boresight an equidistant camera projects, radians.
const EQUIDISTANT_LIMIT: f64 = 2.8;

impl Camera {
    /// Square-pixel camera from its horizontal field of view (radians).
    pub fn from_fov(width: u32, height: u32, fov_x: f64) -> Result<Camera, AsteropeError> {
        if width < 16
            || height < 16
            || width > 16_384
            || height > 16_384
            || !fov_x.is_finite()
            || fov_x <= 0.0
            || fov_x >= 3.0
        {
            return Err(AsteropeError::InvalidCamera);
        }
        let focal = 0.5 * f64::from(width) / tan(0.5 * fov_x);
        Ok(Camera {
            width,
            height,
            fx: focal,
            fy: focal,
            cx: 0.5 * (f64::from(width) - 1.0),
            cy: 0.5 * (f64::from(height) - 1.0),
            distortion: Distortion::NONE,
            projection: Projection::Gnomonic,
        })
    }

    /// Square-pixel camera with an equidistant lens from its horizontal
    /// field of view (radians).
    pub fn from_fov_equidistant(
        width: u32,
        height: u32,
        fov_x: f64,
    ) -> Result<Camera, AsteropeError> {
        let pinhole = Camera::from_fov(width, height, fov_x)?;
        let focal = f64::from(width) / fov_x;
        Ok(Camera {
            fx: focal,
            fy: focal,
            projection: Projection::Equidistant,
            ..pinhole
        })
    }

    pub fn with_distortion(mut self, distortion: Distortion) -> Camera {
        self.distortion = distortion;
        self
    }

    pub fn with_focal_scale(mut self, scale: f64) -> Camera {
        self.fx *= scale;
        self.fy *= scale;
        self
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Undistorted image-plane coordinates of a sensor-frame direction, in
    /// focal lengths; `None` when the direction is outside the projection.
    pub fn ideal(&self, body: Vec3) -> Option<(f64, f64)> {
        match self.projection {
            Projection::Gnomonic => (body.z > 1e-6).then(|| (body.x / body.z, body.y / body.z)),
            Projection::Equidistant => {
                let across = hypot(body.x, body.y);
                let theta = atan2(across, body.z);
                if theta > EQUIDISTANT_LIMIT {
                    None
                } else if across < 1e-300 {
                    Some((0.0, 0.0))
                } else {
                    Some((body.x / across * theta, body.y / across * theta))
                }
            }
        }
    }

    /// Projects a sensor-frame direction to pixel coordinates.
    pub fn project(&self, body: Vec3) -> Option<(f64, f64)> {
        let (x, y) = self.ideal(body)?;
        let (xd, yd) = self.distortion.distort(x, y);
        Some((self.fx * xd + self.cx, self.fy * yd + self.cy))
    }

    /// Sensor-frame unit vector of a pixel coordinate.
    pub fn unproject(&self, u: f64, v: f64) -> Vec3 {
        let (x, y) = self
            .distortion
            .undistort((u - self.cx) / self.fx, (v - self.cy) / self.fy);
        match self.projection {
            Projection::Gnomonic => Vec3::new(x, y, 1.0).normalized().unwrap_or(Vec3::Z),
            Projection::Equidistant => {
                let theta = hypot(x, y).min(EQUIDISTANT_LIMIT);
                if theta < 1e-12 {
                    Vec3::Z
                } else {
                    let across = sin(theta) / hypot(x, y);
                    Vec3::new(x * across, y * across, cos(theta))
                }
            }
        }
    }

    /// Angle from the boresight to a point `half` pixels from the principal
    /// point along an axis with focal length `focal`, ignoring distortion.
    fn half_angle(&self, half: f64, focal: f64) -> f64 {
        match self.projection {
            Projection::Gnomonic => atan(half / focal),
            Projection::Equidistant => (half / focal).min(EQUIDISTANT_LIMIT),
        }
    }

    pub fn contains(&self, u: f64, v: f64, margin: f64) -> bool {
        u >= margin - 0.5
            && v >= margin - 0.5
            && u <= f64::from(self.width) - 0.5 - margin
            && v <= f64::from(self.height) - 0.5 - margin
    }

    pub fn fov_x(&self) -> f64 {
        2.0 * self.half_angle(0.5 * f64::from(self.width), self.fx)
    }

    pub fn fov_y(&self) -> f64 {
        2.0 * self.half_angle(0.5 * f64::from(self.height), self.fy)
    }

    /// Largest angular separation between two points on the sensor.
    pub fn fov_diagonal(&self) -> f64 {
        let w = f64::from(self.width) - 0.5;
        let h = f64::from(self.height) - 0.5;
        let a = self.unproject(-0.5, -0.5).angle_to(self.unproject(w, h));
        let b = self.unproject(w, -0.5).angle_to(self.unproject(-0.5, h));
        if a > b {
            a
        } else {
            b
        }
    }

    /// Angle from the boresight to the farthest sensor corner.
    pub fn half_diagonal(&self) -> f64 {
        let w = f64::from(self.width) - 0.5;
        let h = f64::from(self.height) - 0.5;
        [(-0.5, -0.5), (w, -0.5), (-0.5, h), (w, h)]
            .iter()
            .map(|(u, v)| self.unproject(*u, *v).angle_to(Vec3::Z))
            .fold(0.0, |acc, a| if a > acc { a } else { acc })
    }

    /// Radians per pixel at the principal point.
    pub fn pixel_scale(&self) -> f64 {
        2.0 / (self.fx + self.fy)
    }

    /// Solid angle covered by the sensor, ignoring distortion. For an
    /// equidistant lens it is that of the pinhole rectangle with the same
    /// half-angles, a few percent off at 90 degrees.
    pub fn solid_angle(&self) -> f64 {
        let hx = tan((0.5 * self.fov_x()).min(1.5));
        let hy = tan((0.5 * self.fov_y()).min(1.5));
        4.0 * crate::math::atan2(hx * hy, crate::math::sqrt(1.0 + hx * hx + hy * hy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::DEG;

    #[test]
    fn boresight_lands_on_principal_point() {
        let cam = Camera::from_fov(1024, 768, 14.0 * DEG).unwrap();
        let (u, v) = cam.project(Vec3::Z).unwrap();
        assert!((u - 511.5).abs() < 1e-12 && (v - 383.5).abs() < 1e-12);
        assert!((cam.fov_x() - 14.0 * DEG).abs() < 1e-12);
        assert!(cam.project(Vec3::new(0.0, 0.0, -1.0)).is_none());
    }

    #[test]
    fn distortion_round_trip_is_sub_nanopixel() {
        let cam = Camera::from_fov(1024, 1024, 25.0 * DEG)
            .unwrap()
            .with_distortion(Distortion {
                k1: -0.12,
                k2: 0.03,
                k3: -0.004,
                p1: 6e-4,
                p2: -4e-4,
            });
        let mut worst: f64 = 0.0;
        for iy in 0..=16 {
            for ix in 0..=16 {
                let u = f64::from(ix) * 1023.0 / 16.0;
                let v = f64::from(iy) * 1023.0 / 16.0;
                let (u2, v2) = cam.project(cam.unproject(u, v)).unwrap();
                worst = worst.max((u2 - u).hypot(v2 - v));
            }
        }
        assert!(worst < 1e-9, "round trip error {worst:e} px");
    }

    #[test]
    fn rejects_invalid_parameters() {
        assert!(Camera::from_fov(0, 100, 0.2).is_err());
        assert!(Camera::from_fov(100, 100, 0.0).is_err());
        assert!(Camera::from_fov(100, 100, f64::NAN).is_err());
    }

    #[test]
    fn solid_angle_matches_small_field_area() {
        let cam = Camera::from_fov(1000, 1000, 1.0 * DEG).unwrap();
        let expected = (1.0 * DEG) * (1.0 * DEG);
        assert!((cam.solid_angle() / expected - 1.0).abs() < 1e-3);
    }

    #[test]
    fn equidistant_lens_is_linear_in_angle_and_round_trips() {
        let cam = Camera::from_fov_equidistant(1280, 720, 88.0 * DEG).unwrap();
        assert!((cam.fov_x() - 88.0 * DEG).abs() < 1e-12);
        for degrees in [0.0, 10.0, 30.0, 44.0, 80.0, 120.0] {
            let theta = degrees * DEG;
            let (u, v) = cam
                .project(Vec3::new(theta.sin(), 0.0, theta.cos()))
                .unwrap();
            assert!((u - cam.cx - cam.fx * theta).abs() < 1e-9 && (v - cam.cy).abs() < 1e-9);
        }
        let bent = cam.with_distortion(Distortion {
            k1: -0.05,
            k2: 0.01,
            ..Distortion::NONE
        });
        for (u, v) in [(0.0, 0.0), (640.0, 360.0), (1279.0, 30.0), (100.5, 700.25)] {
            let (pu, pv) = bent.project(bent.unproject(u, v)).unwrap();
            assert!((pu - u).abs() < 1e-8 && (pv - v).abs() < 1e-8);
        }
        assert!(cam.project(Vec3::new(0.0, 0.0, -1.0)).is_none());
        assert!(cam.half_diagonal() > 44.0 * DEG && cam.half_diagonal() < 52.0 * DEG);
    }
}
