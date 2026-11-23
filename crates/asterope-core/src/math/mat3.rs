use super::Vec3;

/// Row-major 3x3 matrix stored as three row vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    pub r0: Vec3,
    pub r1: Vec3,
    pub r2: Vec3,
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3::from_rows(Vec3::X, Vec3::Y, Vec3::Z);
    pub const ZERO: Mat3 = Mat3::from_rows(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);

    pub const fn from_rows(r0: Vec3, r1: Vec3, r2: Vec3) -> Self {
        Self { r0, r1, r2 }
    }

    pub fn from_cols(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        Mat3::from_rows(c0, c1, c2).transpose()
    }

    pub fn diagonal(d: Vec3) -> Self {
        Mat3::from_rows(
            Vec3::new(d.x, 0.0, 0.0),
            Vec3::new(0.0, d.y, 0.0),
            Vec3::new(0.0, 0.0, d.z),
        )
    }

    pub fn outer(a: Vec3, b: Vec3) -> Self {
        Mat3::from_rows(b.scale(a.x), b.scale(a.y), b.scale(a.z))
    }

    pub fn transpose(&self) -> Mat3 {
        Mat3::from_rows(
            Vec3::new(self.r0.x, self.r1.x, self.r2.x),
            Vec3::new(self.r0.y, self.r1.y, self.r2.y),
            Vec3::new(self.r0.z, self.r1.z, self.r2.z),
        )
    }

    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.r0.dot(v), self.r1.dot(v), self.r2.dot(v))
    }

    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let t = o.transpose();
        Mat3::from_rows(t.mul_vec(self.r0), t.mul_vec(self.r1), t.mul_vec(self.r2))
    }

    pub fn add(&self, o: &Mat3) -> Mat3 {
        Mat3::from_rows(self.r0 + o.r0, self.r1 + o.r1, self.r2 + o.r2)
    }

    pub fn scale(&self, s: f64) -> Mat3 {
        Mat3::from_rows(self.r0.scale(s), self.r1.scale(s), self.r2.scale(s))
    }

    pub fn trace(&self) -> f64 {
        self.r0.x + self.r1.y + self.r2.z
    }

    pub fn det(&self) -> f64 {
        self.r0.triple(self.r1, self.r2)
    }

    pub fn inverse(&self) -> Option<Mat3> {
        let det = self.det();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv = Mat3::from_cols(
            self.r1.cross(self.r2),
            self.r2.cross(self.r0),
            self.r0.cross(self.r1),
        );
        Some(inv.scale(1.0 / det))
    }
}
