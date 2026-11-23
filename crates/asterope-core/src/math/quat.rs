use core::ops::Mul;

use super::{atan2, sqrt, Mat3, Vec3};

/// Shuster product: `A(p * q) = A(p) A(q)`.
impl Mul for Quat {
    type Output = Quat;

    fn mul(self, o: Quat) -> Quat {
        let pv = self.vector();
        let qv = o.vector();
        let v = qv.scale(self.w) + pv.scale(o.w) - pv.cross(qv);
        Quat::from_parts(v, self.w * o.w - pv.dot(qv))
    }
}

/// Attitude quaternion, scalar-last (Shuster / Markley convention).
///
/// `q = [x, y, z, w]` with `w` the scalar part. The attitude matrix `A(q)` maps
/// reference-frame (ICRS) vectors into the body frame, `b = A(q) r`, and
/// composition follows `A(p * q) = A(p) A(q)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quat {
    pub const IDENTITY: Quat = Quat::new(0.0, 0.0, 0.0, 1.0);

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub fn vector(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    pub fn from_parts(v: Vec3, w: f64) -> Self {
        Quat::new(v.x, v.y, v.z, w)
    }

    pub fn norm(self) -> f64 {
        sqrt(self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w)
    }

    pub fn normalized(self) -> Option<Quat> {
        let n = self.norm();
        if n > 0.0 && n.is_finite() {
            let s = 1.0 / n;
            Some(Quat::new(self.x * s, self.y * s, self.z * s, self.w * s))
        } else {
            None
        }
    }

    /// Resolves the q / -q ambiguity by forcing a non-negative scalar part.
    pub fn canonical(self) -> Quat {
        if self.w < 0.0 {
            Quat::new(-self.x, -self.y, -self.z, -self.w)
        } else {
            self
        }
    }

    pub fn conjugate(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Rotation angle (radians) separating two attitudes.
    pub fn angle_to(self, o: Quat) -> f64 {
        let d = self * o.conjugate();
        2.0 * atan2(d.vector().norm(), super::abs(d.w))
    }

    /// `A = (w^2 - |v|^2) I + 2 v v^T - 2 w [v x]`
    pub fn to_matrix(self) -> Mat3 {
        let Quat { x, y, z, w } = self;
        Mat3::from_rows(
            Vec3::new(
                w * w + x * x - y * y - z * z,
                2.0 * (x * y + w * z),
                2.0 * (x * z - w * y),
            ),
            Vec3::new(
                2.0 * (x * y - w * z),
                w * w - x * x + y * y - z * z,
                2.0 * (y * z + w * x),
            ),
            Vec3::new(
                2.0 * (x * z + w * y),
                2.0 * (y * z - w * x),
                w * w - x * x - y * y + z * z,
            ),
        )
    }

    /// Shepperd's method; `a` must be a proper rotation matrix.
    pub fn from_matrix(a: &Mat3) -> Option<Quat> {
        let (a11, a12, a13) = (a.r0.x, a.r0.y, a.r0.z);
        let (a21, a22, a23) = (a.r1.x, a.r1.y, a.r1.z);
        let (a31, a32, a33) = (a.r2.x, a.r2.y, a.r2.z);
        let tr = a11 + a22 + a33;
        let q = if tr >= a11 && tr >= a22 && tr >= a33 {
            let w4 = 2.0 * sqrt(1.0 + tr);
            Quat::new(
                (a23 - a32) / w4,
                (a31 - a13) / w4,
                (a12 - a21) / w4,
                w4 / 4.0,
            )
        } else if a11 >= a22 && a11 >= a33 {
            let x4 = 2.0 * sqrt(1.0 + a11 - a22 - a33);
            Quat::new(
                x4 / 4.0,
                (a12 + a21) / x4,
                (a13 + a31) / x4,
                (a23 - a32) / x4,
            )
        } else if a22 >= a33 {
            let y4 = 2.0 * sqrt(1.0 - a11 + a22 - a33);
            Quat::new(
                (a12 + a21) / y4,
                y4 / 4.0,
                (a23 + a32) / y4,
                (a31 - a13) / y4,
            )
        } else {
            let z4 = 2.0 * sqrt(1.0 - a11 - a22 + a33);
            Quat::new(
                (a13 + a31) / z4,
                (a23 + a32) / z4,
                z4 / 4.0,
                (a12 - a21) / z4,
            )
        };
        q.normalized().map(Quat::canonical)
    }
}
