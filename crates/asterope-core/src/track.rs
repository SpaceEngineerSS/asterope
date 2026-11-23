//! Frame-to-frame tracking: once an attitude is known, the next frame is
//! solved from its prediction instead of a lost-in-space search.

use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::detect::Detection;
use crate::index::PairIndex;
use crate::lis::{solve_from_prior, Solution};
use crate::math::{atan2, cos, sin, Quat, Vec3};
use crate::pipeline::{
    centroid_weights, identify_detections, SolverConfig, MAX_PYRAMIDS_PER_FRAME,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackConfig {
    /// Radius around a predicted star position searched for its detection, pixels.
    pub window_px: f64,
    /// Consecutive frames without a solution tolerated before a new
    /// lost-in-space search.
    pub max_misses: u32,
    /// A solution from the prediction is checked by a pattern search when it
    /// keeps less than this fraction of the previous frame's matches.
    pub min_match_fraction: f64,
    /// A solution from the prediction is checked by a pattern search when it
    /// matches less than this fraction of the brightest detections (the
    /// `pattern_stars` a search would use). Leftovers of the previous frame
    /// (image lag, video compression) sit exactly where a stationary
    /// prediction expects stars; the true stars are then the brighter,
    /// unmatched ones.
    pub min_bright_fraction: f64,
}

impl Default for TrackConfig {
    fn default() -> Self {
        TrackConfig {
            window_px: 12.0,
            max_misses: 3,
            min_match_fraction: 0.5,
            min_bright_fraction: 0.5,
        }
    }
}

/// Everything a tracking step needs besides the detections.
#[derive(Clone, Copy)]
pub struct TrackScene<'a> {
    pub camera: &'a Camera,
    pub index: &'a PairIndex<'a>,
    pub catalog: &'a Catalog,
    pub config: &'a SolverConfig,
    pub track: &'a TrackConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackEvent {
    /// Solved by a lost-in-space search.
    Acquired,
    /// Solved from the predicted attitude.
    Tracked,
    /// No solution this frame; the prediction is carried forward.
    Coasting,
    /// No solution and no usable prediction.
    Lost,
}

#[derive(Clone, Debug)]
pub struct TrackUpdate {
    pub event: TrackEvent,
    pub solution: Option<Solution>,
    /// Pattern-search effort spent on this frame (zero while tracking).
    pub triads_tested: u32,
    pub pyramids_tested: u32,
}

/// Rotation of the sensor frame by the rotation vector `theta` (radians,
/// sensor axes), as an attitude quaternion increment: `q_new = delta * q_old`.
pub fn rotation_increment(theta: Vec3) -> Quat {
    let angle = theta.norm();
    match theta.normalized() {
        Some(axis) => Quat::from_parts(axis.scale(sin(0.5 * angle)), cos(0.5 * angle)),
        None => Quat::IDENTITY,
    }
}

/// Attitude after rotating for `dt` seconds at a constant body rate (rad/s).
pub fn propagate(q: Quat, body_rate: Vec3, dt: f64) -> Quat {
    (rotation_increment(body_rate.scale(dt)) * q)
        .normalized()
        .unwrap_or(q)
        .canonical()
}

/// Mean body rate (rad/s) that carries attitude `from` to `to` in `dt` seconds.
pub fn rate_between(from: Quat, to: Quat, dt: f64) -> Vec3 {
    if !(dt > 0.0) {
        return Vec3::ZERO;
    }
    let delta = (to * from.conjugate()).canonical();
    let vector = delta.vector();
    let angle = 2.0 * atan2(vector.norm(), delta.w);
    vector
        .normalized()
        .map_or(Vec3::ZERO, |axis| axis.scale(angle / dt))
}

#[derive(Clone, Debug, Default)]
pub struct Tracker {
    attitude: Option<Quat>,
    rate: Vec3,
    /// The rate comes from two solved frames rather than the zero default.
    rate_known: bool,
    misses: u32,
    /// Stars matched in the last solved frame.
    matches: usize,
}

impl Tracker {
    pub fn new() -> Tracker {
        Tracker::default()
    }

    pub fn reset(&mut self) {
        *self = Tracker::default();
    }

    /// Estimated body rate in radians per second.
    pub fn rate(&self) -> Vec3 {
        self.rate
    }

    pub fn is_tracking(&self) -> bool {
        self.attitude.is_some()
    }

    /// Replaces the prediction for the next frame with an attitude and a
    /// rate known from elsewhere, such as a gyro. The next `update` should
    /// then be given a `dt` of zero.
    pub fn prime(&mut self, attitude: Quat, rate: Vec3) {
        self.attitude = Some(attitude);
        self.rate = rate;
        self.rate_known = true;
    }

    /// A solution from the prediction that should not be accepted without a
    /// second opinion from a pattern search.
    fn is_suspect(
        &self,
        solution: &Solution,
        detections: &[Detection],
        config: &SolverConfig,
        track: &TrackConfig,
    ) -> bool {
        let matched = solution.matches.len() as f64;
        let brightest = detections.len().min(config.pattern_stars);
        let bright_matched = solution
            .matches
            .iter()
            .filter(|m| (m.detection as usize) < brightest)
            .count() as f64;
        matched < track.min_match_fraction * self.matches as f64
            || bright_matched < track.min_bright_fraction * brightest as f64
    }

    fn follow(&mut self, prior: Quat, solution: Solution, dt: f64) -> TrackUpdate {
        self.rate = rate_between(prior, solution.q, dt);
        self.rate_known = true;
        self.attitude = Some(solution.q);
        self.misses = 0;
        self.matches = solution.matches.len();
        TrackUpdate {
            event: TrackEvent::Tracked,
            solution: Some(solution),
            triads_tested: 0,
            pyramids_tested: 0,
        }
    }

    /// Processes the detections of the frame taken `dt` seconds after the
    /// previous one.
    pub fn update(&mut self, detections: &[Detection], scene: &TrackScene, dt: f64) -> TrackUpdate {
        let TrackScene {
            camera,
            index,
            catalog,
            config,
            track,
        } = *scene;
        let previous = self.attitude;
        let mut suspect = None;
        if let Some(prior) = previous {
            let predicted = propagate(prior, self.rate, dt);
            let bodies = config.vectors(camera, detections);
            let lis = config.lis_config(camera, catalog, 0, MAX_PYRAMIDS_PER_FRAME);
            let window = track.window_px * camera.pixel_scale();
            match solve_from_prior(&bodies, catalog, &predicted.to_matrix(), window, &lis) {
                Some(solution) if self.is_suspect(&solution, detections, config, track) => {
                    suspect = Some((prior, solution));
                }
                Some(mut solution) => {
                    if config.weighted {
                        solution.reweigh(&bodies, catalog, &centroid_weights(detections));
                    }
                    return self.follow(prior, solution, dt);
                }
                // With a measured rate a dropped frame is bridged by the
                // prediction. Without one, the window was centered on a
                // guess, so a fresh search is the only way to learn the motion.
                None if self.rate_known && self.misses < track.max_misses => {
                    self.misses += 1;
                    self.attitude = Some(predicted);
                    return TrackUpdate {
                        event: TrackEvent::Coasting,
                        solution: None,
                        triads_tested: 0,
                        pyramids_tested: 0,
                    };
                }
                None => {}
            }
        }

        let output = identify_detections(
            detections,
            camera,
            index,
            catalog,
            config,
            0,
            MAX_PYRAMIDS_PER_FRAME,
        );
        self.misses = 0;
        match (&output.solution, suspect) {
            (Some(solution), second_opinion) => {
                match previous {
                    // Two consecutive solutions bracket the motion: after a
                    // search that overruled the prediction, and when two
                    // searches in a row had to stand in for tracking.
                    Some(prior) if second_opinion.is_some() || !self.rate_known => {
                        self.rate = rate_between(prior, solution.q, dt);
                        self.rate_known = true;
                    }
                    Some(_) => {}
                    None => {
                        self.rate = Vec3::ZERO;
                        self.rate_known = false;
                    }
                }
                self.attitude = Some(solution.q);
                self.matches = solution.matches.len();
            }
            // The search found nothing better, so the doubtful solution stands.
            (None, Some((prior, solution))) => {
                let mut update = self.follow(prior, solution, dt);
                update.triads_tested = output.triads_tested;
                update.pyramids_tested = output.pyramids_tested;
                return update;
            }
            (None, None) => self.reset(),
        }
        TrackUpdate {
            event: if output.solution.is_some() {
                TrackEvent::Acquired
            } else {
                TrackEvent::Lost
            },
            triads_tested: output.triads_tested,
            pyramids_tested: output.pyramids_tested,
            solution: output.solution,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{ARCSEC, DEG};

    #[test]
    fn propagation_and_rate_are_inverse() {
        let q = Quat::new(0.2, -0.4, 0.1, 0.88).normalized().unwrap();
        let rate = Vec3::new(0.5, -1.2, 3.0).scale(DEG);
        let next = propagate(q, rate, 0.25);
        let recovered = rate_between(q, next, 0.25);
        assert!((recovered - rate).norm() < 1e-12);
        assert_eq!(rate_between(q, next, 0.0), Vec3::ZERO);
        assert!(propagate(q, Vec3::ZERO, 1.0).angle_to(q) < 1e-15);
    }

    #[test]
    fn rate_about_boresight_keeps_the_boresight_fixed() {
        let q = Quat::new(0.1, 0.3, -0.2, 0.9).normalized().unwrap();
        let next = propagate(q, Vec3::new(0.0, 0.0, 5.0 * DEG), 1.0);
        let moved = q.to_matrix().r2.angle_to(next.to_matrix().r2);
        assert!(moved < 1e-12);
        assert!((next.angle_to(q) - 5.0 * DEG).abs() < 1e-12);
        // Stars drift at -rate x body: a +Y rate moves a boresight star to -X.
        let star = q.to_matrix().r2;
        let after = propagate(q, Vec3::new(0.0, 100.0 * ARCSEC, 0.0), 1.0)
            .to_matrix()
            .mul_vec(star);
        assert!(after.x < 0.0 && after.y.abs() < 1e-12);
    }
}
