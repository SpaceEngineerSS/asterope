use super::wahba::{attitude_profile, Observation, Profile};
use crate::error::AsteropeError;
use crate::math::{abs, solve_linear, sqrt, Mat3, Quat, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct QuestSolution {
    pub q: Quat,
    pub lambda_max: f64,
    pub iterations: u32,
}

struct Terms {
    s: Mat3,
    z: Vec3,
    sigma: f64,
    kappa: f64,
    delta: f64,
}

fn terms(profile: &Profile) -> Terms {
    let s = profile.s();
    let kappa = s.r0.x * s.r1.y - s.r0.y * s.r1.x + s.r0.x * s.r2.z - s.r0.z * s.r2.x
        + s.r1.y * s.r2.z
        - s.r1.z * s.r2.y;
    Terms {
        s,
        z: profile.z(),
        sigma: profile.sigma(),
        kappa,
        delta: s.det(),
    }
}

/// Largest root of the QUEST characteristic quartic
/// `l^4 - (a + b) l^2 - c l + (a b + c sigma - d) = 0`, Newton-Raphson from `l = 1`.
fn lambda_max(t: &Terms) -> (f64, u32) {
    let sz = t.s.mul_vec(t.z);
    let a = t.sigma * t.sigma - t.kappa;
    let b = t.sigma * t.sigma + t.z.norm_squared();
    let c = t.delta + t.z.dot(sz);
    let d = sz.norm_squared();
    let constant = a * b + c * t.sigma - d;

    let mut lambda = 1.0;
    let mut iterations = 0;
    for _ in 0..64 {
        let l2 = lambda * lambda;
        let f = l2 * l2 - (a + b) * l2 - c * lambda + constant;
        let df = 4.0 * l2 * lambda - 2.0 * (a + b) * lambda - c;
        if df == 0.0 || !df.is_finite() {
            break;
        }
        let step = f / df;
        lambda -= step;
        iterations += 1;
        if abs(step) <= 4.0 * f64::EPSILON * abs(lambda) {
            break;
        }
    }
    (lambda, iterations)
}

/// Unnormalized quaternion `[x; gamma]` from the Gibbs-vector form of QUEST.
fn gibbs_form(t: &Terms, lambda: f64) -> (Vec3, f64) {
    let alpha = lambda * lambda - t.sigma * t.sigma + t.kappa;
    let beta = lambda - t.sigma;
    let gamma = (lambda + t.sigma) * alpha - t.delta;
    let sz = t.s.mul_vec(t.z);
    let x = t.z.scale(alpha) + sz.scale(beta) + t.s.mul_vec(sz);
    (x, gamma)
}

const FRAME_FLIPS: [Vec3; 4] = [
    Vec3::new(1.0, 1.0, 1.0),
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
];

const FLIP_QUATS: [Quat; 4] = [
    Quat::new(0.0, 0.0, 0.0, 1.0),
    Quat::new(1.0, 0.0, 0.0, 0.0),
    Quat::new(0.0, 1.0, 0.0, 0.0),
    Quat::new(0.0, 0.0, 1.0, 0.0),
];

fn rayleigh_quotient(k: &[[f64; 4]; 4], q: Quat) -> f64 {
    let v = [q.x, q.y, q.z, q.w];
    k.iter()
        .zip(v.iter())
        .map(|(row, vi)| vi * row.iter().zip(v.iter()).map(|(a, b)| a * b).sum::<f64>())
        .sum()
}

/// One inverse-iteration step on `K - lambda I`, which pulls `q` toward the
/// eigenvector of the eigenvalue closest to `lambda`.
fn inverse_iteration(k: &[[f64; 4]; 4], lambda: f64, q: Quat) -> Option<Quat> {
    let mut shifted = *k;
    for (i, row) in shifted.iter_mut().enumerate() {
        if let Some(diagonal) = row.get_mut(i) {
            *diagonal -= lambda;
        }
    }
    let [x, y, z, w] = solve_linear(shifted, [q.x, q.y, q.z, q.w])?;
    let refined = Quat::new(x, y, z, w).normalized()?;
    let aligned = refined.x * q.x + refined.y * q.y + refined.z * q.z + refined.w * q.w;
    Some(if aligned < 0.0 {
        Quat::new(-refined.x, -refined.y, -refined.z, -refined.w)
    } else {
        refined
    })
}

/// QUEST (Shuster & Oh 1981).
///
/// The Gibbs-vector step is singular for 180 degree rotations. Following
/// Shuster's method of sequential rotations, the problem is also solved in
/// reference frames rotated by 180 degrees about each axis and the frame with
/// the largest scalar part is kept.
///
/// The closed-form eigenvector is built from adjugate terms that cancel
/// heavily when the star field is narrow (the two largest eigenvalues of K
/// nearly coincide). Two inverse-iteration steps on `K - lambda_max I` restore
/// the lost digits; they do not change which solution is selected.
pub fn quest(observations: &[Observation]) -> Result<QuestSolution, AsteropeError> {
    let profile = attitude_profile(observations)?;
    let (lambda, iterations) = lambda_max(&terms(&profile));
    if !lambda.is_finite() {
        return Err(AsteropeError::DegenerateGeometry);
    }
    let k = profile.davenport_k();
    let mut q = solve_for_lambda(&profile, lambda)?;
    for _ in 0..2 {
        if let Some(refined) = inverse_iteration(&k, lambda, q) {
            q = refined;
        }
    }
    let q = q.canonical();
    Ok(QuestSolution {
        q,
        lambda_max: rayleigh_quotient(&k, q),
        iterations,
    })
}

fn solve_for_lambda(profile: &Profile, lambda: f64) -> Result<Quat, AsteropeError> {
    let mut best: Option<(f64, Quat)> = None;
    for (flip, flip_quat) in FRAME_FLIPS.iter().zip(FLIP_QUATS.iter()) {
        let rotated = Profile {
            b: profile.b.mul(&Mat3::diagonal(*flip)),
            weight_sum: profile.weight_sum,
        };
        let (x, gamma) = gibbs_form(&terms(&rotated), lambda);
        let norm = sqrt(gamma * gamma + x.norm_squared());
        if norm <= 0.0 || !norm.is_finite() {
            continue;
        }
        let scalar = abs(gamma) / norm;
        if best.is_none_or(|(s, _)| scalar > s) {
            let q_rotated = Quat::from_parts(x.scale(1.0 / norm), gamma / norm);
            best = Some((scalar, q_rotated * *flip_quat));
        }
    }

    let (_, q) = best.ok_or(AsteropeError::DegenerateGeometry)?;
    Ok(q.normalized()
        .ok_or(AsteropeError::DegenerateGeometry)?
        .canonical())
}
