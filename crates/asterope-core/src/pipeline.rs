//! End-to-end frame solving: detection, identification, attitude.

use alloc::vec::Vec;

use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::detect::{detect, DetectConfig, DetectOutput, Detection, Frame};
use crate::error::AsteropeError;
use crate::index::PairIndex;
use crate::lis::{identify, LisConfig, LisOutput};
use crate::math::Vec3;

/// Search budget for one frame, counted across every candidate focal length.
pub const MAX_PYRAMIDS_PER_FRAME: u32 = 2000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolverConfig {
    pub detect: DetectConfig,
    /// Tolerance on inter-star distances, pixels.
    pub tolerance_px: f64,
    /// Radius for associating detections with predicted stars, pixels.
    pub match_radius_px: f64,
    pub pattern_stars: usize,
    pub max_triads: usize,
    pub min_matches: usize,
    pub max_chance_probability: f64,
    /// Weigh each star by the expected accuracy of its centroid when
    /// fitting the attitude, instead of counting every star equally.
    pub weighted: bool,
    /// Readout of a rolling shutter to undo, when the rate is known.
    pub rolling: Option<RollingShutter>,
}

/// A rolling shutter reads the rows one after another, so in a rotating
/// camera each row shows a slightly different attitude. With the rate
/// known, from a gyro or from the frames before, every star can be moved to
/// where it stood when the middle row was read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollingShutter {
    /// Time between consecutive rows, seconds.
    pub row_time: f64,
    /// Angular rate of the sensor about its own axes, radians per second.
    pub rate: Vec3,
}

impl Default for SolverConfig {
    fn default() -> Self {
        SolverConfig {
            detect: DetectConfig::default(),
            tolerance_px: 0.75,
            match_radius_px: 1.5,
            pattern_stars: 14,
            max_triads: 2000,
            min_matches: 5,
            max_chance_probability: 1e-3,
            weighted: true,
            rolling: None,
        }
    }
}

impl SolverConfig {
    /// Settings for photographs and video instead of tracker frames: the
    /// full 16-bit range, larger and softer star images, more pattern stars.
    /// The tolerances suit a narrow field; `calib::auto_tolerances` gives
    /// the values for a particular camera.
    pub fn photograph() -> SolverConfig {
        SolverConfig {
            detect: DetectConfig {
                cell_size: 64,
                peak_sigma: 8.0,
                min_area: 5,
                max_area: 3000,
                max_elongation: 3.0,
                saturation: u16::MAX,
                max_detections: 80,
                ..DetectConfig::default()
            },
            tolerance_px: 2.0,
            match_radius_px: 2.8,
            pattern_stars: 18,
            // The lens model of a photograph is fitted, not known, and its
            // error grows with the brightness-independent distance from the
            // center, so signal-to-noise says little about a star's error.
            weighted: false,
            ..SolverConfig::default()
        }
    }

    pub fn lis_config(
        &self,
        camera: &Camera,
        catalog: &Catalog,
        prior_pyramids: u32,
        pyramid_cap: u32,
    ) -> LisConfig {
        let scale = camera.pixel_scale();
        LisConfig {
            tolerance: self.tolerance_px * scale,
            match_radius: self.match_radius_px * scale,
            pattern_stars: self.pattern_stars,
            max_triads: self.max_triads,
            min_matches: self.min_matches,
            max_chance_probability: self.max_chance_probability,
            prior_pyramids,
            max_pyramids: pyramid_cap,
            field_radius: camera.half_diagonal(),
            field_solid_angle: camera.solid_angle(),
            verify_mag_limit: catalog.mag_limit(),
        }
    }
}

pub fn detection_vectors(camera: &Camera, detections: &[Detection]) -> Vec<Vec3> {
    detections
        .iter()
        .map(|d| camera.unproject(d.x, d.y))
        .collect()
}

impl SolverConfig {
    /// Sensor-frame directions of the detections, referred to the instant
    /// the middle row was read when a rolling shutter is to be undone. In
    /// the sensor frame a star moves at `-rate x direction`, so a star read
    /// `delay` late is put back by `rate x direction * delay`.
    pub fn vectors(&self, camera: &Camera, detections: &[Detection]) -> Vec<Vec3> {
        let Some(RollingShutter { row_time, rate }) = self.rolling else {
            return detection_vectors(camera, detections);
        };
        let middle = 0.5 * (f64::from(camera.height) - 1.0);
        detections
            .iter()
            .map(|d| {
                let seen = camera.unproject(d.x, d.y);
                let delay = (d.y - middle) * row_time;
                (seen + rate.cross(seen).scale(delay))
                    .normalized()
                    .unwrap_or(seen)
            })
            .collect()
    }
}

/// Identifies detections with a known camera. `prior_pyramids` is the number
/// of pyramids already verified for the same frame with other cameras; the
/// search stops once the running total would exceed `pyramid_cap`.
pub fn identify_detections(
    detections: &[Detection],
    camera: &Camera,
    index: &PairIndex,
    catalog: &Catalog,
    config: &SolverConfig,
    prior_pyramids: u32,
    pyramid_cap: u32,
) -> LisOutput {
    let bodies = config.vectors(camera, detections);
    let mut output = identify(
        &bodies,
        index,
        catalog,
        &config.lis_config(camera, catalog, prior_pyramids, pyramid_cap),
    );
    if config.weighted {
        if let Some(solution) = &mut output.solution {
            solution.reweigh(&bodies, catalog, &centroid_weights(detections));
        }
    }
    output
}

/// Centroid error of a star that is not limited by noise, pixels: what
/// pixel sampling leaves at high signal (section 1 of `docs/validation.md`).
const CENTROID_FLOOR_PX: f64 = 0.03;
/// Centroid error assumed for a saturated star, pixels.
const SATURATED_CENTROID_PX: f64 = 0.3;

/// Inverse variance of each centroid, in pixels^-2. The noise part is the
/// width of the star image over its signal-to-noise ratio.
pub fn centroid_weights(detections: &[Detection]) -> Vec<f64> {
    detections
        .iter()
        .map(|d| {
            let noise = if d.snr > 0.0 {
                d.sigma.max(0.5) / d.snr
            } else {
                1.0
            };
            let floor = if d.saturated {
                SATURATED_CENTROID_PX
            } else {
                CENTROID_FLOOR_PX
            };
            1.0 / (floor * floor + noise * noise)
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct FrameResult {
    pub detect: DetectOutput,
    pub lis: LisOutput,
}

pub fn solve_frame(
    frame: &Frame,
    camera: &Camera,
    index: &PairIndex,
    catalog: &Catalog,
    config: &SolverConfig,
) -> Result<FrameResult, AsteropeError> {
    if frame.width() != camera.width as usize || frame.height() != camera.height as usize {
        return Err(AsteropeError::InvalidFrame);
    }
    let detect = detect(frame, &config.detect)?;
    let lis = identify_detections(
        &detect.detections,
        camera,
        index,
        catalog,
        config,
        0,
        MAX_PYRAMIDS_PER_FRAME,
    );
    Ok(FrameResult { detect, lis })
}
