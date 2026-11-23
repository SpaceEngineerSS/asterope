//! Camera calibration from several solved frames.
//!
//! The focal length, principal point and Brown-Conrady coefficients are
//! fitted together with a small rotation of every frame, by
//! Levenberg-Marquardt on the pixel residuals of the matched stars. The
//! normal equations have an arrowhead shape (each frame's rotation couples
//! only to the intrinsics), so the frame rotations are eliminated block by
//! block and a 7 x 7 system remains.
//!
//! A shift of the principal point is, to first order, the same as tilting
//! every frame, so it is separated from the attitudes only by the curvature
//! of the projection and by the distortion being centered on it. On a narrow
//! field that is a weak handle: read the reported uncertainty before
//! trusting the value. Tangential distortion that is present but not fitted
//! is absorbed by the principal point and biases it by several pixels.

use alloc::vec::Vec;

use crate::camera::{Camera, Distortion};
use crate::error::AsteropeError;
use crate::math::{cos, sin, solve_linear, sqrt, Mat3, Vec3};

/// One matched star of a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationStar {
    /// Measured centroid, pixels.
    pub u: f64,
    pub v: f64,
    /// Catalog direction, ICRS unit vector.
    pub reference: Vec3,
}

#[derive(Clone, Debug)]
pub struct CalibrationFrame {
    /// Attitude from the solver, ICRS to sensor; refined by the fit.
    pub attitude: Mat3,
    pub stars: Vec<CalibrationStar>,
}

/// Which parameters are fitted; the rest keep their starting values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationModel {
    pub principal_point: bool,
    pub k2: bool,
    /// The tangential terms `p1` and `p2`.
    pub tangential: bool,
}

impl Default for CalibrationModel {
    fn default() -> Self {
        CalibrationModel {
            principal_point: true,
            k2: false,
            tangential: false,
        }
    }
}

pub const PARAMETERS: usize = 7;
/// Names of the fitted parameters, in the order of `Calibration::sigma`.
pub const PARAMETER_NAMES: [&str; PARAMETERS] = ["f", "cx", "cy", "k1", "k2", "p1", "p2"];

#[derive(Clone, Debug)]
pub struct Calibration {
    pub camera: Camera,
    /// RMS residual per star after the fit, pixels.
    pub rms_px: f64,
    /// RMS residual per star before the fit, pixels.
    pub initial_rms_px: f64,
    /// One-sigma uncertainty of each parameter in `PARAMETER_NAMES` order,
    /// from the residuals and the curvature of the fit with the frame
    /// rotations marginalized; zero for parameters that were held fixed.
    pub sigma: [f64; PARAMETERS],
    pub frames: usize,
    pub stars: usize,
    pub iterations: u32,
}

type Params = [f64; PARAMETERS];

const STEPS: Params = [1e-2, 1e-3, 1e-3, 1e-6, 1e-6, 1e-7, 1e-7];
const ROTATION_STEP: f64 = 1e-7;
const MAX_ITERATIONS: u32 = 40;

fn params_of(camera: &Camera) -> Params {
    let d = camera.distortion;
    [camera.fx, camera.cx, camera.cy, d.k1, d.k2, d.p1, d.p2]
}

fn camera_with(base: &Camera, p: &Params) -> Camera {
    let [f, cx, cy, k1, k2, p1, p2] = *p;
    Camera {
        fx: f,
        fy: f * base.fy / base.fx,
        cx,
        cy,
        distortion: Distortion {
            k1,
            k2,
            k3: base.distortion.k3,
            p1,
            p2,
        },
        ..*base
    }
}

/// Active rotation by the rotation vector `delta` (Rodrigues).
fn rotation(delta: Vec3) -> Mat3 {
    let angle = delta.norm();
    let (a, b) = if angle < 1e-8 {
        (1.0, 0.5)
    } else {
        (sin(angle) / angle, (1.0 - cos(angle)) / (angle * angle))
    };
    let column = |e: Vec3| {
        let first = delta.cross(e);
        e + first.scale(a) + delta.cross(first).scale(b)
    };
    Mat3::from_cols(column(Vec3::X), column(Vec3::Y), column(Vec3::Z))
}

fn residual(camera: &Camera, body: Vec3, star: &CalibrationStar) -> Option<(f64, f64)> {
    let (u, v) = camera.project(body)?;
    Some((star.u - u, star.v - v))
}

fn cost(base: &Camera, p: &Params, frames: &[CalibrationFrame]) -> Option<f64> {
    let camera = camera_with(base, p);
    let mut sum = 0.0;
    for frame in frames {
        for star in &frame.stars {
            let (du, dv) = residual(&camera, frame.attitude.mul_vec(star.reference), star)?;
            sum += du * du + dv * dv;
        }
    }
    Some(sum)
}

/// Accumulated normal equations with the frame rotations eliminated.
struct Reduced {
    matrix: [[f64; PARAMETERS]; PARAMETERS],
    rhs: Params,
    /// Per frame: inverse of its damped rotation block, its coupling to the
    /// intrinsics and its gradient.
    blocks: Vec<(Mat3, [Vec3; PARAMETERS], Vec3)>,
}

fn reduce(
    base: &Camera,
    p: &Params,
    frames: &[CalibrationFrame],
    fitted: &[bool; PARAMETERS],
    damping: f64,
) -> Option<Reduced> {
    let camera = camera_with(base, p);
    let mut matrix = [[0.0; PARAMETERS]; PARAMETERS];
    let mut rhs = [0.0; PARAMETERS];
    let mut blocks = Vec::with_capacity(frames.len());
    for frame in frames {
        let mut rotation_block = Mat3::ZERO;
        let mut coupling = [Vec3::ZERO; PARAMETERS];
        let mut gradient = Vec3::ZERO;
        for star in &frame.stars {
            let body = frame.attitude.mul_vec(star.reference);
            let (eu, ev) = residual(&camera, body, star)?;

            // Derivatives of the predicted pixel, by central differences.
            let mut intrinsic = [(0.0, 0.0); PARAMETERS];
            for (index, (slot, step)) in intrinsic.iter_mut().zip(STEPS).enumerate() {
                if !fitted.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let shifted = |sign: f64| {
                    let mut q = *p;
                    if let Some(value) = q.get_mut(index) {
                        *value += sign * step;
                    }
                    camera_with(base, &q).project(body)
                };
                let (plus, minus) = (shifted(1.0)?, shifted(-1.0)?);
                *slot = (
                    (plus.0 - minus.0) / (2.0 * step),
                    (plus.1 - minus.1) / (2.0 * step),
                );
            }
            let mut turn = [(0.0, 0.0); 3];
            for (slot, axis) in turn.iter_mut().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
                let moved =
                    |sign: f64| camera.project(body + axis.cross(body).scale(sign * ROTATION_STEP));
                let (plus, minus) = (moved(1.0)?, moved(-1.0)?);
                *slot = (
                    (plus.0 - minus.0) / (2.0 * ROTATION_STEP),
                    (plus.1 - minus.1) / (2.0 * ROTATION_STEP),
                );
            }
            let [tx, ty, tz] = turn;
            let (turn_u, turn_v) = (Vec3::new(tx.0, ty.0, tz.0), Vec3::new(tx.1, ty.1, tz.1));

            for (i, (ui, vi)) in intrinsic.iter().enumerate() {
                if let Some(value) = rhs.get_mut(i) {
                    *value += ui * eu + vi * ev;
                }
                if let Some(row) = matrix.get_mut(i) {
                    for (entry, (uj, vj)) in row.iter_mut().zip(intrinsic.iter()) {
                        *entry += ui * uj + vi * vj;
                    }
                }
                if let Some(entry) = coupling.get_mut(i) {
                    *entry = *entry + turn_u.scale(*ui) + turn_v.scale(*vi);
                }
            }
            rotation_block = rotation_block
                .add(&Mat3::outer(turn_u, turn_u))
                .add(&Mat3::outer(turn_v, turn_v));
            gradient = gradient + turn_u.scale(eu) + turn_v.scale(ev);
        }
        let damped = rotation_block.add(&Mat3::diagonal(Vec3::new(
            damping * rotation_block.r0.x,
            damping * rotation_block.r1.y,
            damping * rotation_block.r2.z,
        )));
        let inverse = damped.inverse()?;
        for (i, ci) in coupling.iter().enumerate() {
            let solved = inverse.mul_vec(*ci);
            if let Some(value) = rhs.get_mut(i) {
                *value -= solved.dot(gradient);
            }
            if let Some(row) = matrix.get_mut(i) {
                for (entry, cj) in row.iter_mut().zip(coupling.iter()) {
                    *entry -= solved.dot(*cj);
                }
            }
        }
        blocks.push((inverse, coupling, gradient));
    }
    Some(Reduced {
        matrix,
        rhs,
        blocks,
    })
}

/// Solves the reduced system for the fitted parameters, scaled to unit
/// diagonal; fixed parameters get a zero step. With `damping` zero and a
/// unit right-hand side this also yields columns of the covariance.
fn solve_reduced(
    matrix: &[[f64; PARAMETERS]; PARAMETERS],
    rhs: &Params,
    fitted: &[bool; PARAMETERS],
    damping: f64,
) -> Option<Params> {
    let mut scale = [1.0; PARAMETERS];
    for (i, s) in scale.iter_mut().enumerate() {
        let diagonal = matrix
            .get(i)
            .and_then(|row| row.get(i))
            .copied()
            .unwrap_or(0.0);
        if fitted.get(i).copied().unwrap_or(false) && diagonal > 0.0 {
            *s = 1.0 / sqrt(diagonal);
        }
    }
    let mut a = [[0.0; PARAMETERS]; PARAMETERS];
    let mut b = [0.0; PARAMETERS];
    for i in 0..PARAMETERS {
        let active_i = fitted.get(i).copied().unwrap_or(false);
        for j in 0..PARAMETERS {
            let active = active_i && fitted.get(j).copied().unwrap_or(false);
            let value = if active {
                matrix
                    .get(i)
                    .and_then(|row| row.get(j))
                    .copied()
                    .unwrap_or(0.0)
                    * scale.get(i).copied().unwrap_or(1.0)
                    * scale.get(j).copied().unwrap_or(1.0)
            } else {
                0.0
            };
            let identity = if i == j {
                if active_i {
                    damping
                } else {
                    1.0
                }
            } else {
                0.0
            };
            if let Some(entry) = a.get_mut(i).and_then(|row| row.get_mut(j)) {
                *entry = value + identity;
            }
        }
        if let Some(entry) = b.get_mut(i) {
            *entry = if active_i {
                rhs.get(i).copied().unwrap_or(0.0) * scale.get(i).copied().unwrap_or(1.0)
            } else {
                0.0
            };
        }
    }
    let mut x = solve_linear(a, b)?;
    for (value, s) in x.iter_mut().zip(scale) {
        *value *= s;
    }
    Some(x)
}

/// Fits the camera to the matched stars of `frames`, starting from
/// `initial` (its `k3` and pixel aspect ratio are kept).
pub fn calibrate(
    initial: &Camera,
    frames: &[CalibrationFrame],
    model: &CalibrationModel,
) -> Result<Calibration, AsteropeError> {
    let stars: usize = frames.iter().map(|f| f.stars.len()).sum();
    let fitted = [
        true,
        model.principal_point,
        model.principal_point,
        true,
        model.k2,
        model.tangential,
        model.tangential,
    ];
    let unknowns = fitted.iter().filter(|f| **f).count() + 3 * frames.len();
    if frames.is_empty() || frames.iter().any(|f| f.stars.len() < 3) || 2 * stars <= unknowns + 4 {
        return Err(AsteropeError::TooFewObservations);
    }

    let mut frames: Vec<CalibrationFrame> = frames.to_vec();
    let mut p = params_of(initial);
    let mut current = cost(initial, &p, &frames).ok_or(AsteropeError::DegenerateGeometry)?;
    let initial_rms_px = sqrt(current / stars as f64);
    let mut damping = 1e-3;
    let mut iterations = 0;
    while iterations < MAX_ITERATIONS {
        iterations += 1;
        let reduced = reduce(initial, &p, &frames, &fitted, damping)
            .ok_or(AsteropeError::DegenerateGeometry)?;
        let step = solve_reduced(&reduced.matrix, &reduced.rhs, &fitted, damping)
            .ok_or(AsteropeError::DegenerateGeometry)?;
        let mut trial = p;
        for (value, delta) in trial.iter_mut().zip(step) {
            *value += delta;
        }
        let mut turned = frames.clone();
        for (frame, (inverse, coupling, gradient)) in turned.iter_mut().zip(&reduced.blocks) {
            let mut carried = *gradient;
            for (c, delta) in coupling.iter().zip(step) {
                carried = carried - c.scale(delta);
            }
            frame.attitude = rotation(inverse.mul_vec(carried)).mul(&frame.attitude);
        }
        match cost(initial, &trial, &turned) {
            Some(next) if next < current => {
                let gain = (current - next) / current.max(f64::MIN_POSITIVE);
                p = trial;
                frames = turned;
                current = next;
                damping = (damping * 0.3).max(1e-9);
                if gain < 1e-10 {
                    break;
                }
            }
            _ => {
                damping *= 10.0;
                if damping > 1e8 {
                    break;
                }
            }
        }
    }

    let camera = camera_with(initial, &p);
    if !(camera.fx > 0.0) || !camera.fx.is_finite() {
        return Err(AsteropeError::DegenerateGeometry);
    }
    let degrees_of_freedom = (2 * stars).saturating_sub(unknowns).max(1) as f64;
    let variance = current / degrees_of_freedom;
    let reduced =
        reduce(initial, &p, &frames, &fitted, 0.0).ok_or(AsteropeError::DegenerateGeometry)?;
    let mut sigma = [0.0; PARAMETERS];
    for (i, slot) in sigma.iter_mut().enumerate() {
        if !fitted.get(i).copied().unwrap_or(false) {
            continue;
        }
        let mut unit = [0.0; PARAMETERS];
        if let Some(value) = unit.get_mut(i) {
            *value = 1.0;
        }
        let column = solve_reduced(&reduced.matrix, &unit, &fitted, 0.0)
            .ok_or(AsteropeError::DegenerateGeometry)?;
        *slot = sqrt((variance * column.get(i).copied().unwrap_or(0.0)).max(0.0));
    }
    Ok(Calibration {
        camera,
        rms_px: sqrt(current / stars as f64),
        initial_rms_px,
        sigma,
        frames: frames.len(),
        stars,
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attitude::{quest, Observation};
    use crate::math::DEG;
    use crate::rng::Rng;

    fn truth(width: u32, fov_deg: f64) -> Camera {
        let mut camera = Camera::from_fov(width, width, fov_deg * DEG).unwrap();
        camera.fx *= 1.013;
        camera.fy = camera.fx;
        camera.cx += 6.0;
        camera.cy -= 4.0;
        camera.with_distortion(Distortion {
            k1: -0.08,
            k2: 0.02,
            k3: 0.0,
            p1: 5e-4,
            p2: -3e-4,
        })
    }

    /// Frames of random stars seen through `camera` with centroid noise,
    /// each with the attitude a solver would find using `assumed`.
    fn observe(
        camera: &Camera,
        assumed: &Camera,
        frames: usize,
        stars: usize,
        noise_px: f64,
        rng: &mut Rng,
    ) -> Vec<CalibrationFrame> {
        (0..frames)
            .map(|_| {
                let attitude = rng.unit_quat().to_matrix();
                let mut seen = Vec::new();
                while seen.len() < stars {
                    let u = rng.range(8.0, f64::from(camera.width) - 8.0);
                    let v = rng.range(8.0, f64::from(camera.height) - 8.0);
                    seen.push(CalibrationStar {
                        u: u + noise_px * rng.normal(),
                        v: v + noise_px * rng.normal(),
                        reference: attitude.transpose().mul_vec(camera.unproject(u, v)),
                    });
                }
                let observations: Vec<Observation> = seen
                    .iter()
                    .map(|s| Observation {
                        body: assumed.unproject(s.u, s.v),
                        reference: s.reference,
                        weight: 1.0,
                    })
                    .collect();
                CalibrationFrame {
                    attitude: quest(&observations).unwrap().q.to_matrix(),
                    stars: seen,
                }
            })
            .collect()
    }

    const FULL: CalibrationModel = CalibrationModel {
        principal_point: true,
        k2: true,
        tangential: true,
    };

    #[test]
    fn wide_field_parameters_are_recovered_within_their_uncertainty() {
        let camera = truth(1024, 60.0);
        let nominal = Camera::from_fov(1024, 1024, 60.0 * DEG).unwrap();
        let mut rng = Rng::new(17);
        let frames = observe(&camera, &nominal, 8, 40, 0.1, &mut rng);
        let fit = calibrate(&nominal, &frames, &FULL).unwrap();
        assert!(fit.initial_rms_px > 2.0, "{}", fit.initial_rms_px);
        assert!(
            (fit.rms_px - 0.1 * 2f64.sqrt()).abs() < 0.03,
            "{}",
            fit.rms_px
        );
        let (found, expected) = (params_of(&fit.camera), params_of(&camera));
        for ((name, sigma), (value, target)) in PARAMETER_NAMES
            .iter()
            .zip(fit.sigma)
            .zip(found.iter().zip(expected))
        {
            println!("{name}: {value:.6} +/- {sigma:.6} (truth {target:.6})");
            assert!(sigma > 0.0);
            assert!((value - target).abs() < 4.0 * sigma, "{name}");
        }
        assert!((found[0] - expected[0]).abs() < 0.3);
        assert!((found[1] - expected[1]).abs() < 0.5 && (found[2] - expected[2]).abs() < 0.5);
        assert!((found[3] - expected[3]).abs() < 2e-3);
    }

    #[test]
    fn narrow_field_reports_a_weak_principal_point() {
        let mut camera = truth(1024, 14.0);
        camera.distortion = Distortion {
            k1: -0.08,
            ..Distortion::default()
        };
        let nominal = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
        let mut rng = Rng::new(19);
        let frames = observe(&camera, &nominal, 8, 30, 0.05, &mut rng);
        let fit = calibrate(&nominal, &frames, &CalibrationModel::default()).unwrap();
        println!(
            "f {:.3} +/- {:.3}, cx {:.2} +/- {:.2}, cy {:.2} +/- {:.2}, rms {:.3} px",
            fit.camera.fx,
            fit.sigma[0],
            fit.camera.cx,
            fit.sigma[1],
            fit.camera.cy,
            fit.sigma[2],
            fit.rms_px
        );
        assert!((fit.camera.fx - camera.fx).abs() < 4.0 * fit.sigma[0].max(0.05));
        assert!(fit.sigma[1] > 3.0 * fit.sigma[0]);
        assert!((fit.camera.cy - camera.cy).abs() < 4.0 * fit.sigma[2]);
        assert!((fit.camera.cx - camera.cx).abs() < 4.0 * fit.sigma[1]);
        assert_eq!(fit.sigma[4], 0.0);
        assert!(fit.rms_px < 0.15);
    }

    #[test]
    fn too_little_data_is_refused() {
        let camera = Camera::from_fov(512, 512, 30.0 * DEG).unwrap();
        let mut rng = Rng::new(23);
        let frames = observe(&camera, &camera, 1, 4, 0.0, &mut rng);
        assert_eq!(
            calibrate(&camera, &frames, &FULL).unwrap_err(),
            AsteropeError::TooFewObservations
        );
        assert_eq!(
            calibrate(&camera, &[], &FULL).unwrap_err(),
            AsteropeError::TooFewObservations
        );
    }
}
