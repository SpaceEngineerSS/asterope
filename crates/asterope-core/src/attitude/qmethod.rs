use super::wahba::{attitude_profile, Observation};
use crate::error::AsteropeError;
use crate::math::{jacobi_eigen, Quat};

/// Davenport q-method: the optimal quaternion is the eigenvector of K with the
/// largest eigenvalue. Returns the quaternion and that eigenvalue.
pub fn q_method(observations: &[Observation]) -> Result<(Quat, f64), AsteropeError> {
    let profile = attitude_profile(observations)?;
    let eigen = jacobi_eigen(profile.davenport_k());
    let index = eigen.max_index();
    let [x, y, z, w] = eigen.vector(index);
    let lambda = eigen.values.get(index).copied().unwrap_or(f64::NAN);
    let q = Quat::new(x, y, z, w)
        .normalized()
        .ok_or(AsteropeError::DegenerateGeometry)?
        .canonical();
    Ok((q, lambda))
}
