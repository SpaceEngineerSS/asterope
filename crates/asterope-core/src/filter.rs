//! Attitude filter that joins a rate gyro to the star tracker.
//!
//! A multiplicative extended Kalman filter (Lefferts, Markley & Shuster,
//! "Kalman Filtering for Spacecraft Attitude Estimation", J. Guidance 5,
//! 1982; Markley & Crassidis, *Fundamentals of Spacecraft Attitude
//! Determination and Control*, 2014, chapter 6). The gyro carries the
//! attitude between star fixes and its bias is estimated from them.
//!
//! The state is the attitude quaternion and the gyro bias. The covariance
//! is that of a six-vector: the rotation `dtheta` that takes the estimated
//! attitude to the true one, `q_true = rotation_increment(dtheta) * q_est`,
//! in sensor axes, and the bias error. The gyro is modelled as
//! `measured = rate + bias + white noise`, with the bias a random walk.

use crate::math::{cos, sin, sqrt, Mat3, Quat, Vec3};
use crate::track::rotation_increment;

/// Noise of a rate gyro, as spectral densities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GyroNoise {
    /// Angle random walk, rad / sqrt(s): the white noise of the rate.
    pub angle_random_walk: f64,
    /// Rate random walk, rad / s / sqrt(s): how fast the bias wanders.
    pub rate_random_walk: f64,
}

/// What a star fix did to the filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Innovation {
    /// Rotation from the predicted attitude to the measured one, sensor
    /// axes, radians.
    pub residual: Vec3,
    /// The residual weighed by its predicted covariance. For a consistent
    /// filter it follows a chi-square distribution with three degrees of
    /// freedom, so values far above 3 mark a fix that does not belong.
    pub normalized_squared: f64,
    /// False when the fix was turned away by the gate and nothing changed.
    pub accepted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttitudeFilter {
    q: Quat,
    bias: Vec3,
    /// Covariance blocks: attitude, attitude against bias, bias.
    p_aa: Mat3,
    p_ab: Mat3,
    p_bb: Mat3,
    noise: GyroNoise,
}

fn skew(v: Vec3) -> Mat3 {
    Mat3::from_rows(
        Vec3::new(0.0, -v.z, v.y),
        Vec3::new(v.z, 0.0, -v.x),
        Vec3::new(-v.y, v.x, 0.0),
    )
}

fn symmetric(m: &Mat3) -> Mat3 {
    m.add(&m.transpose()).scale(0.5)
}

impl AttitudeFilter {
    /// Starts from an attitude known to `attitude_sigma` radians about each
    /// axis and a bias taken as zero and known to `bias_sigma` rad/s.
    pub fn new(q: Quat, attitude_sigma: f64, bias_sigma: f64, noise: GyroNoise) -> AttitudeFilter {
        AttitudeFilter {
            q,
            bias: Vec3::ZERO,
            p_aa: Mat3::IDENTITY.scale(attitude_sigma * attitude_sigma),
            p_ab: Mat3::ZERO,
            p_bb: Mat3::IDENTITY.scale(bias_sigma * bias_sigma),
            noise,
        }
    }

    pub fn attitude(&self) -> Quat {
        self.q
    }

    pub fn bias(&self) -> Vec3 {
        self.bias
    }

    /// Body rate from a gyro reading, with the estimated bias taken out.
    pub fn rate(&self, gyro: Vec3) -> Vec3 {
        gyro - self.bias
    }

    /// One-sigma attitude uncertainty about the sensor axes, radians.
    pub fn attitude_sigma(&self) -> Vec3 {
        Vec3::new(
            sqrt(self.p_aa.r0.x.max(0.0)),
            sqrt(self.p_aa.r1.y.max(0.0)),
            sqrt(self.p_aa.r2.z.max(0.0)),
        )
    }

    /// One-sigma bias uncertainty about the sensor axes, rad/s.
    pub fn bias_sigma(&self) -> Vec3 {
        Vec3::new(
            sqrt(self.p_bb.r0.x.max(0.0)),
            sqrt(self.p_bb.r1.y.max(0.0)),
            sqrt(self.p_bb.r2.z.max(0.0)),
        )
    }

    /// Covariance blocks (attitude, attitude against bias, bias).
    pub fn covariance(&self) -> (Mat3, Mat3, Mat3) {
        (self.p_aa, self.p_ab, self.p_bb)
    }

    /// Carries the state across `dt` seconds during which the gyro read
    /// `gyro` (rad/s, sensor axes, taken as constant over the step).
    ///
    /// The error obeys `d(dtheta)/dt = -rate x dtheta - dbias - noise`. For
    /// a constant rate its transition matrix has the closed form used
    /// here, and the process noise over the step is
    /// `[[v dt + u dt^3 / 3, -u dt^2 / 2], [-u dt^2 / 2, u dt]]` times the
    /// identity, with `v` and `u` the squared random walks (Markley &
    /// Crassidis, chapter 6). `tests/attitude_filter.rs` checks the result
    /// against a two-state filter written independently.
    pub fn propagate(&mut self, gyro: Vec3, dt: f64) {
        if !(dt > 0.0) || !dt.is_finite() {
            return;
        }
        let rate = self.rate(gyro);
        self.q = (rotation_increment(rate.scale(dt)) * self.q)
            .normalized()
            .unwrap_or(self.q)
            .canonical();

        let speed = rate.norm();
        let angle = speed * dt;
        let cross = skew(rate);
        let cross_squared = cross.mul(&cross);
        let (f, g) = if angle > 1e-6 {
            let (s, c) = (sin(angle), cos(angle));
            (
                Mat3::IDENTITY
                    .add(&cross.scale(-s / speed))
                    .add(&cross_squared.scale((1.0 - c) / (speed * speed))),
                Mat3::IDENTITY
                    .scale(-dt)
                    .add(&cross.scale((1.0 - c) / (speed * speed)))
                    .add(&cross_squared.scale(-(angle - s) / (speed * speed * speed))),
            )
        } else {
            (
                Mat3::IDENTITY
                    .add(&cross.scale(-dt))
                    .add(&cross_squared.scale(0.5 * dt * dt)),
                Mat3::IDENTITY.scale(-dt).add(&cross.scale(0.5 * dt * dt)),
            )
        };

        let (ft, gt) = (f.transpose(), g.transpose());
        let v = self.noise.angle_random_walk * self.noise.angle_random_walk;
        let u = self.noise.rate_random_walk * self.noise.rate_random_walk;
        let p_aa = f
            .mul(&self.p_aa)
            .mul(&ft)
            .add(&f.mul(&self.p_ab).mul(&gt))
            .add(&g.mul(&self.p_ab.transpose()).mul(&ft))
            .add(&g.mul(&self.p_bb).mul(&gt))
            .add(&Mat3::IDENTITY.scale(v * dt + u * dt * dt * dt / 3.0));
        let p_ab = f
            .mul(&self.p_ab)
            .add(&g.mul(&self.p_bb))
            .add(&Mat3::IDENTITY.scale(-0.5 * u * dt * dt));
        self.p_aa = symmetric(&p_aa);
        self.p_ab = p_ab;
        self.p_bb = symmetric(&self.p_bb.add(&Mat3::IDENTITY.scale(u * dt)));
    }

    /// Takes in a star fix: the measured attitude and the covariance of its
    /// error rotation in sensor axes (rad^2). `gate` is the largest
    /// normalized squared residual accepted; a fix beyond it is reported and
    /// ignored. Returns `None` when the covariances cannot be inverted.
    pub fn update(&mut self, measured: Quat, covariance: &Mat3, gate: f64) -> Option<Innovation> {
        let delta = (measured * self.q.conjugate()).canonical();
        // Twice the vector part over the scalar part is the rotation vector
        // to second order (the Gibbs vector).
        let residual = delta.vector().scale(2.0 / delta.w.max(1e-12));
        let s_inverse = self.p_aa.add(covariance).inverse()?;
        let normalized_squared = residual.dot(s_inverse.mul_vec(residual));
        if !(normalized_squared <= gate) {
            return Some(Innovation {
                residual,
                normalized_squared,
                accepted: false,
            });
        }
        let k_a = self.p_aa.mul(&s_inverse);
        let k_b = self.p_ab.transpose().mul(&s_inverse);

        let correction = k_a.mul_vec(residual);
        self.q = (rotation_increment(correction) * self.q)
            .normalized()
            .unwrap_or(self.q)
            .canonical();
        self.bias = self.bias + k_b.mul_vec(residual);

        // Joseph form, which stays symmetric and positive under rounding.
        let a = Mat3::IDENTITY.add(&k_a.scale(-1.0));
        let (at, k_at, k_bt) = (a.transpose(), k_a.transpose(), k_b.transpose());
        let p_aa = a
            .mul(&self.p_aa)
            .mul(&at)
            .add(&k_a.mul(covariance).mul(&k_at));
        let p_ab = a
            .mul(&self.p_ab)
            .add(&a.mul(&self.p_aa).mul(&k_bt).scale(-1.0))
            .add(&k_a.mul(covariance).mul(&k_bt));
        let p_bb = self
            .p_bb
            .add(&k_b.mul(&self.p_aa).mul(&k_bt))
            .add(&k_b.mul(&self.p_ab).scale(-1.0))
            .add(&self.p_ab.transpose().mul(&k_bt).scale(-1.0))
            .add(&k_b.mul(covariance).mul(&k_bt));
        self.p_aa = symmetric(&p_aa);
        self.p_ab = p_ab;
        self.p_bb = symmetric(&p_bb);
        Some(Innovation {
            residual,
            normalized_squared,
            accepted: true,
        })
    }
}
