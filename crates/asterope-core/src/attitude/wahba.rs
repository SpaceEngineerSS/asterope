use crate::error::AsteropeError;
use crate::math::{Mat3, Vec3};

/// One unit-vector observation pair: `body ~ A * reference`.
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub body: Vec3,
    pub reference: Vec3,
    pub weight: f64,
}

/// Attitude profile matrix `B = sum(a_i b_i r_i^T)` with weights normalized to sum 1.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub b: Mat3,
    pub weight_sum: f64,
}

impl Profile {
    pub fn sigma(&self) -> f64 {
        self.b.trace()
    }

    pub fn s(&self) -> Mat3 {
        self.b.add(&self.b.transpose())
    }

    /// `z = sum(a_i b_i x r_i)`
    pub fn z(&self) -> Vec3 {
        let b = &self.b;
        Vec3::new(b.r1.z - b.r2.y, b.r2.x - b.r0.z, b.r0.y - b.r1.x)
    }

    /// Davenport's `K = [[S - sigma I, z], [z^T, sigma]]`.
    pub fn davenport_k(&self) -> [[f64; 4]; 4] {
        let s = self.s();
        let z = self.z();
        let sigma = self.sigma();
        [
            [s.r0.x - sigma, s.r0.y, s.r0.z, z.x],
            [s.r1.x, s.r1.y - sigma, s.r1.z, z.y],
            [s.r2.x, s.r2.y, s.r2.z - sigma, z.z],
            [z.x, z.y, z.z, sigma],
        ]
    }
}

pub fn attitude_profile(observations: &[Observation]) -> Result<Profile, AsteropeError> {
    if observations.len() < 2 {
        return Err(AsteropeError::TooFewObservations);
    }
    let weight_sum: f64 = observations.iter().map(|o| o.weight).sum();
    if weight_sum <= 0.0 || !weight_sum.is_finite() {
        return Err(AsteropeError::DegenerateGeometry);
    }
    let mut b = Mat3::ZERO;
    for o in observations {
        b = b.add(&Mat3::outer(o.body, o.reference).scale(o.weight / weight_sum));
    }
    Ok(Profile { b, weight_sum })
}

/// Wahba loss `0.5 * sum(a_i |b_i - A r_i|^2)` with normalized weights.
pub fn wahba_loss(attitude: &Mat3, observations: &[Observation]) -> f64 {
    let weight_sum: f64 = observations.iter().map(|o| o.weight).sum();
    if weight_sum <= 0.0 {
        return 0.0;
    }
    let total: f64 = observations
        .iter()
        .map(|o| o.weight * (o.body - attitude.mul_vec(o.reference)).norm_squared())
        .sum();
    0.5 * total / weight_sum
}
