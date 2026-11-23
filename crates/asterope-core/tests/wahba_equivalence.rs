#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use asterope_core::attitude::{q_method, quest, svd_method, wahba_loss, Observation};
use asterope_core::math::{Mat3, Quat, Vec3, ARCSEC, DEG, TAU};
use asterope_core::rng::Rng;
use asterope_core::validation::wahba_suite;
use nalgebra::{Matrix3, Vector3};

const TOLERANCE_RAD: f64 = 1e-10;

fn to_na(v: Vec3) -> Vector3<f64> {
    Vector3::new(v.x, v.y, v.z)
}

fn profile_matrix(observations: &[Observation]) -> Matrix3<f64> {
    let total: f64 = observations.iter().map(|o| o.weight).sum();
    let mut b = Matrix3::<f64>::zeros();
    for o in observations {
        b += (o.weight / total) * to_na(o.body) * to_na(o.reference).transpose();
    }
    b
}

fn quat_from_na(a: &Matrix3<f64>) -> Quat {
    let m = Mat3::from_rows(
        Vec3::new(a[(0, 0)], a[(0, 1)], a[(0, 2)]),
        Vec3::new(a[(1, 0)], a[(1, 1)], a[(1, 2)]),
        Vec3::new(a[(2, 0)], a[(2, 1)], a[(2, 2)]),
    );
    Quat::from_matrix(&m).unwrap()
}

/// Markley's SVD solution, `A = U diag(1, 1, det(U) det(V)) V^T`, using the
/// library SVD (Golub-Kahan). Kept as a second opinion; see `jacobi_svd_reference`.
fn library_svd_reference(observations: &[Observation]) -> Quat {
    let svd = profile_matrix(observations).svd(true, true);
    let u = svd.u.unwrap();
    let vt = svd.v_t.unwrap();
    let d = u.determinant() * vt.determinant();
    quat_from_na(&(u * Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, d)) * vt))
}

/// Markley's SVD solution with a one-sided (Hestenes) Jacobi SVD, which keeps
/// high relative accuracy for the two small singular values of a narrow-field
/// profile matrix. With `u3 = det(U) u1 x u2` and `v3 = det(V) v1 x v2`,
/// `U diag(1, 1, det(U) det(V)) V^T = u1 v1^T + u2 v2^T + (u1 x u2)(v1 x v2)^T`.
fn jacobi_svd_reference(observations: &[Observation]) -> Quat {
    let mut g = profile_matrix(observations);
    let mut v = Matrix3::<f64>::identity();
    for _sweep in 0..60 {
        let mut rotated = false;
        for p in 0..2 {
            for q in (p + 1)..3 {
                let gp = g.column(p).into_owned();
                let gq = g.column(q).into_owned();
                let (alpha, beta, gamma) = (gp.norm_squared(), gq.norm_squared(), gp.dot(&gq));
                if gamma == 0.0 || gamma.abs() <= f64::EPSILON * (alpha * beta).sqrt() {
                    continue;
                }
                rotated = true;
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = zeta.signum() / (zeta.abs() + (1.0 + zeta * zeta).sqrt());
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                g.set_column(p, &(c * gp - s * gq));
                g.set_column(q, &(s * gp + c * gq));
                let vp = v.column(p).into_owned();
                let vq = v.column(q).into_owned();
                v.set_column(p, &(c * vp - s * vq));
                v.set_column(q, &(s * vp + c * vq));
            }
        }
        if !rotated {
            break;
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| {
        g.column(*b)
            .norm_squared()
            .partial_cmp(&g.column(*a).norm_squared())
            .unwrap()
    });
    let u1 = g.column(order[0]).normalize();
    let u2 = g.column(order[1]).normalize();
    let v1 = v.column(order[0]).into_owned();
    let v2 = v.column(order[1]).into_owned();
    let a = u1 * v1.transpose() + u2 * v2.transpose() + u1.cross(&u2) * v1.cross(&v2).transpose();
    quat_from_na(&a)
}

fn perpendicular(v: Vec3) -> Vec3 {
    let helper = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    v.cross(helper).normalized().unwrap()
}

fn perturb(v: Vec3, sigma: f64, rng: &mut Rng) -> Vec3 {
    let e1 = perpendicular(v);
    let e2 = v.cross(e1);
    (v + e1.scale(sigma * rng.normal()) + e2.scale(sigma * rng.normal()))
        .normalized()
        .unwrap()
}

fn random_case(rng: &mut Rng, truth: Quat) -> Vec<Observation> {
    let count = 3 + rng.below(28) as usize;
    let half_fov = rng.range(4.0, 12.5) * DEG;
    let noise = rng.range(0.0, 60.0) * ARCSEC;
    let attitude = truth.to_matrix();
    let to_reference = attitude.transpose();
    (0..count)
        .map(|_| {
            let off_axis = half_fov * rng.uniform().sqrt();
            let azimuth = rng.uniform() * TAU;
            let body = Vec3::new(
                off_axis.sin() * azimuth.cos(),
                off_axis.sin() * azimuth.sin(),
                off_axis.cos(),
            );
            Observation {
                body: perturb(body, noise, rng),
                reference: to_reference.mul_vec(body),
                weight: rng.range(0.5, 2.0),
            }
        })
        .collect()
}

/// Separation between the two largest eigenvalues of Davenport's K matrix.
/// It bounds how strongly rounding error is amplified into the attitude.
fn eigen_gap(observations: &[Observation]) -> f64 {
    let b = profile_matrix(observations);
    let s = b + b.transpose();
    let sigma = b.trace();
    let z = Vector3::new(
        b[(1, 2)] - b[(2, 1)],
        b[(2, 0)] - b[(0, 2)],
        b[(0, 1)] - b[(1, 0)],
    );
    let mut k = nalgebra::Matrix4::<f64>::zeros();
    k.fixed_view_mut::<3, 3>(0, 0)
        .copy_from(&(s - Matrix3::identity() * sigma));
    k.fixed_view_mut::<3, 1>(0, 3).copy_from(&z);
    k.fixed_view_mut::<1, 3>(3, 0).copy_from(&z.transpose());
    k[(3, 3)] = sigma;
    let mut values: Vec<f64> = k.symmetric_eigenvalues().iter().copied().collect();
    values.sort_by(|a, b| b.partial_cmp(a).unwrap());
    values[0] - values[1]
}

#[test]
fn quest_and_q_method_match_svd_reference() {
    const CASES: usize = 10_000;
    const MIN_EIGEN_GAP: f64 = 1e-5;

    let mut rng = Rng::new(0x0057_4148_4241);
    let mut worst_quest: f64 = 0.0;
    let mut worst_qm: f64 = 0.0;
    let mut worst_library: f64 = 0.0;
    let mut skipped = 0usize;
    let mut smallest_gap = f64::INFINITY;

    for case in 0..CASES {
        let truth = if case % 10 == 0 {
            let axis = Vec3::new(rng.normal(), rng.normal(), rng.normal())
                .normalized()
                .unwrap();
            let half = 0.5 * (180.0 - rng.range(0.0, 1e-3)) * DEG;
            Quat::from_parts(axis.scale(half.sin()), half.cos())
        } else {
            rng.unit_quat()
        };
        let observations = random_case(&mut rng, truth);
        let gap = eigen_gap(&observations);
        if gap < MIN_EIGEN_GAP {
            skipped += 1;
            continue;
        }
        smallest_gap = smallest_gap.min(gap);

        let reference = jacobi_svd_reference(&observations);
        let q_quest = quest(&observations).unwrap().q;
        let (q_qm, _) = q_method(&observations).unwrap();

        let err_quest = q_quest.angle_to(reference);
        let err_qm = q_qm.angle_to(reference);
        worst_library = worst_library.max(library_svd_reference(&observations).angle_to(reference));
        worst_quest = worst_quest.max(err_quest);
        worst_qm = worst_qm.max(err_qm);
        assert!(
            err_quest < TOLERANCE_RAD,
            "case {case}: QUEST differs from SVD by {err_quest:e} rad (gap {gap:e})"
        );
        assert!(
            err_qm < TOLERANCE_RAD,
            "case {case}: q-method differs from SVD by {err_qm:e} rad (gap {gap:e})"
        );

        let err_core_svd = svd_method(&observations).unwrap().angle_to(reference);
        assert!(
            err_core_svd < TOLERANCE_RAD,
            "case {case}: core SVD differs from the test reference by {err_core_svd:e} rad"
        );

        let loss_quest = wahba_loss(&q_quest.to_matrix(), &observations);
        let loss_ref = wahba_loss(&reference.to_matrix(), &observations);
        assert!((loss_quest - loss_ref).abs() < 1e-14);
    }

    println!(
        "wahba equivalence: {CASES} cases, {skipped} skipped (gap < {MIN_EIGEN_GAP:e}), \
         smallest gap {smallest_gap:.3e}, worst QUEST {worst_quest:.3e} rad, \
         worst q-method {worst_qm:.3e} rad, library (Golub-Kahan) SVD vs Jacobi SVD \
         {worst_library:.3e} rad"
    );
    assert!(
        skipped * 100 < CASES,
        "too many ill-conditioned cases skipped: {skipped}"
    );
}

#[test]
fn built_in_suite_meets_the_tolerance() {
    let summary = wahba_suite(10_000, 20_240_229);
    println!(
        "built-in suite: worst QUEST {:.3e} rad, worst q-method {:.3e} rad, {} failures",
        summary.worst_quest, summary.worst_q_method, summary.failures
    );
    assert_eq!(summary.failures, 0);
    assert!(summary.worst_quest < TOLERANCE_RAD);
    assert!(summary.worst_q_method < TOLERANCE_RAD);
}

#[test]
fn two_observations_are_enough() {
    let truth = Quat::new(0.1, -0.2, 0.3, 0.9).normalized().unwrap();
    let a = truth.to_matrix();
    let r1 = Vec3::new(1.0, 0.2, -0.1).normalized().unwrap();
    let r2 = Vec3::new(-0.3, 1.0, 0.4).normalized().unwrap();
    let observations = [
        Observation {
            body: a.mul_vec(r1),
            reference: r1,
            weight: 1.0,
        },
        Observation {
            body: a.mul_vec(r2),
            reference: r2,
            weight: 1.0,
        },
    ];
    assert!(quest(&observations).unwrap().q.angle_to(truth) < 1e-12);
    assert!(q_method(&observations).unwrap().0.angle_to(truth) < 1e-12);
}

#[test]
fn rejects_single_observation() {
    let observations = [Observation {
        body: Vec3::Z,
        reference: Vec3::Z,
        weight: 1.0,
    }];
    assert!(quest(&observations).is_err());
    assert!(q_method(&observations).is_err());
}
