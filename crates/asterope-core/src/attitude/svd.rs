use super::wahba::{attitude_profile, Observation};
use crate::error::AsteropeError;
use crate::math::{abs, sqrt, Mat3, Quat, Vec3};

const PLANES: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];

/// Wahba solution through the singular value decomposition (Markley 1988):
/// `A = U diag(1, 1, det(U) det(V)) V^T` for `B = U S V^T`.
///
/// The SVD is a one-sided (Hestenes) Jacobi iteration on the columns of `B`,
/// which keeps high relative accuracy for the two small singular values of a
/// narrow-field profile matrix. Since `u3 = det(U) u1 x u2` and
/// `v3 = det(V) v1 x v2`, the result is
/// `u1 v1^T + u2 v2^T + (u1 x u2)(v1 x v2)^T` and never needs the third pair.
pub fn svd_method(observations: &[Observation]) -> Result<Quat, AsteropeError> {
    let b = attitude_profile(observations)?.b.transpose();
    let mut g = [b.r0, b.r1, b.r2];
    let mut v = [Vec3::X, Vec3::Y, Vec3::Z];

    for _sweep in 0..60 {
        let mut rotated = false;
        for (p, q) in PLANES {
            let (Some(gp), Some(gq), Some(vp), Some(vq)) = (
                g.get(p).copied(),
                g.get(q).copied(),
                v.get(p).copied(),
                v.get(q).copied(),
            ) else {
                continue;
            };
            let (alpha, beta, gamma) = (gp.norm_squared(), gq.norm_squared(), gp.dot(gq));
            if gamma == 0.0 || abs(gamma) <= f64::EPSILON * sqrt(alpha * beta) {
                continue;
            }
            rotated = true;
            let zeta = (beta - alpha) / (2.0 * gamma);
            let sign = if zeta >= 0.0 { 1.0 } else { -1.0 };
            let t = sign / (abs(zeta) + sqrt(1.0 + zeta * zeta));
            let c = 1.0 / sqrt(1.0 + t * t);
            let s = c * t;
            let updates = [
                (p, gp.scale(c) - gq.scale(s), vp.scale(c) - vq.scale(s)),
                (q, gp.scale(s) + gq.scale(c), vp.scale(s) + vq.scale(c)),
            ];
            for (slot, new_g, new_v) in updates {
                if let (Some(gs), Some(vs)) = (g.get_mut(slot), v.get_mut(slot)) {
                    *gs = new_g;
                    *vs = new_v;
                }
            }
        }
        if !rotated {
            break;
        }
    }

    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| {
        let na = g.get(*a).map_or(0.0, |c| c.norm_squared());
        let nb = g.get(*b).map_or(0.0, |c| c.norm_squared());
        nb.total_cmp(&na)
    });
    let [first, second, _] = order;
    let column = |set: &[Vec3; 3], i: usize| set.get(i).copied().unwrap_or(Vec3::ZERO);
    let u1 = column(&g, first)
        .normalized()
        .ok_or(AsteropeError::DegenerateGeometry)?;
    let u2 = column(&g, second)
        .normalized()
        .ok_or(AsteropeError::DegenerateGeometry)?;
    let (v1, v2) = (column(&v, first), column(&v, second));
    let attitude = Mat3::outer(u1, v1)
        .add(&Mat3::outer(u2, v2))
        .add(&Mat3::outer(u1.cross(u2), v1.cross(v2)));
    Quat::from_matrix(&attitude).ok_or(AsteropeError::DegenerateGeometry)
}
