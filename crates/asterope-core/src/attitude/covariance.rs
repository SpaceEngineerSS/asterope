use super::wahba::Observation;
use crate::math::{sqrt, Mat3, Vec3};

/// One-sigma attitude uncertainty about the sensor axes, in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttitudeSigma {
    pub about_x: f64,
    pub about_y: f64,
    pub about_boresight: f64,
}

/// QUEST covariance (Shuster & Oh 1981, eq. 99) for the body-frame rotation
/// error, `P = [sum((I - b_i b_i^T) / sigma_i^2)]^-1`. `sigma` (radians) is
/// the direction error of an observation of weight one; an observation of
/// weight `w` has variance `sigma^2 / w`.
pub fn attitude_sigma(observations: &[Observation], sigma: f64) -> Option<AttitudeSigma> {
    let mut information = Mat3::ZERO;
    for o in observations {
        let projector = Mat3::IDENTITY.add(&Mat3::outer(o.body, o.body).scale(-1.0));
        information = information.add(&projector.scale(o.weight / (sigma * sigma)));
    }
    let p = information.inverse()?;
    let diag = Vec3::new(p.r0.x, p.r1.y, p.r2.z);
    if diag.x < 0.0 || diag.y < 0.0 || diag.z < 0.0 {
        return None;
    }
    Some(AttitudeSigma {
        about_x: sqrt(diag.x),
        about_y: sqrt(diag.y),
        about_boresight: sqrt(diag.z),
    })
}
