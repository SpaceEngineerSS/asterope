//! The star tracker as one object: frames in, attitude out.
//!
//! ```no_run
//! use asterope_core::camera::Camera;
//! use asterope_core::flight::StarTracker;
//!
//! static CATALOG: &[u8] = &[]; // contents of camera.astc
//! static INDEX: &[u8] = &[]; // contents of camera.asti
//!
//! # fn main() -> Result<(), asterope_core::AsteropeError> {
//! let camera = Camera::from_fov(1024, 1024, 14.0_f64.to_radians())?;
//! let mut tracker = StarTracker::from_bytes(camera, CATALOG, INDEX)?;
//! let frame = vec![0u16; 1024 * 1024];
//! let fix = tracker.update(&frame, 0.1)?;
//! if let Some(q) = fix.attitude() {
//!     // q rotates ICRS vectors into the sensor frame, scalar last.
//!     let _ = (q.x, q.y, q.z, q.w);
//! }
//! # Ok(())
//! # }
//! ```

use crate::camera::Camera;
use crate::catalog::Catalog;
use crate::detect::{detect, Frame};
use crate::error::AsteropeError;
use crate::filter::{AttitudeFilter, GyroNoise, Innovation};
use crate::index::PairIndex;
use crate::lis::Solution;
use crate::math::{Mat3, Quat, Vec3, ARCSEC};
use crate::pipeline::{identify_detections, RollingShutter, SolverConfig, MAX_PYRAMIDS_PER_FRAME};
use crate::track::{TrackConfig, TrackEvent, TrackScene, Tracker};

/// Result of one frame.
#[derive(Clone, Debug)]
pub struct Fix {
    pub event: TrackEvent,
    pub solution: Option<Solution>,
    /// Star-like blobs found in the frame.
    pub detections: usize,
    /// Estimated body rate about the sensor axes, radians per second.
    pub body_rate: Vec3,
}

impl Fix {
    /// Attitude quaternion, ICRS to sensor, scalar last; `None` without a solution.
    pub fn attitude(&self) -> Option<Quat> {
        self.solution.as_ref().map(|s| s.q)
    }
}

/// A gyro beside the camera: its noise, and how the camera reads out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GyroAiding {
    pub noise: GyroNoise,
    /// Time between rows of a rolling shutter, seconds; zero for a global
    /// shutter. The gyro's rate is used to undo it.
    pub row_time: f64,
    /// Largest normalized squared residual at which a star fix is taken
    /// into the filter. The default of 30 turns away about one honest fix
    /// in a million and any fix that is wrong by more than a few times the
    /// expected error; a wrong identification is off by thousands.
    pub gate: f64,
    /// A fix that disagrees with the gyro this many times in a row is
    /// believed after all, and the filter starts again from it.
    pub rejections_before_restart: u32,
}

impl GyroAiding {
    pub fn new(noise: GyroNoise) -> GyroAiding {
        GyroAiding {
            noise,
            row_time: 0.0,
            gate: 30.0,
            rejections_before_restart: 5,
        }
    }
}

/// Result of one frame with a gyro alongside.
#[derive(Clone, Debug)]
pub struct AidedFix {
    /// What the camera found in this frame.
    pub fix: Fix,
    /// Filtered attitude at the frame, ICRS to sensor; `None` until the
    /// first star fix.
    pub attitude: Option<Quat>,
    /// One-sigma uncertainty of `attitude` about the sensor axes, radians.
    pub sigma: Vec3,
    /// Estimated gyro bias, rad/s.
    pub gyro_bias: Vec3,
    /// How this frame's star fix sat against the gyro's prediction.
    pub innovation: Option<Innovation>,
}

#[derive(Clone, Copy, Debug)]
struct Aided {
    settings: GyroAiding,
    filter: Option<AttitudeFilter>,
    /// Bias-corrected rate from the latest gyro reading.
    rate: Vec3,
    rejections: u32,
}

/// Attitude uncertainty assumed for a first fix that reports none, and the
/// floor under any reported one, radians.
const FIX_SIGMA_FLOOR: f64 = 0.2 * ARCSEC;
const FIRST_FIX_SIGMA: f64 = 60.0 * ARCSEC;
/// Widest window the gyro's prediction is searched in, pixels. Beyond it a
/// pattern search is the safer way to find the stars again.
const MAX_AIDED_WINDOW_PX: f64 = 40.0;
/// Bias of an uncalibrated gyro, rad/s: 0.1 degrees per second.
const INITIAL_BIAS_SIGMA: f64 = 1.745e-3;

/// Camera, catalog, pair index, settings and tracking state.
///
/// The index is borrowed, so it can stay in read-only storage; the catalog
/// is decoded into memory once.
pub struct StarTracker<'a> {
    camera: Camera,
    catalog: Catalog,
    index: PairIndex<'a>,
    solver: SolverConfig,
    track: TrackConfig,
    tracker: Tracker,
    /// Observer velocity over the speed of light already applied to the catalog.
    beta: Vec3,
    aided: Option<Aided>,
}

impl<'a> StarTracker<'a> {
    /// Fails with `IndexCatalogMismatch` when the index was not built from
    /// this catalog or from a deeper version of it.
    pub fn new(
        camera: Camera,
        catalog: Catalog,
        index: PairIndex<'a>,
    ) -> Result<StarTracker<'a>, AsteropeError> {
        let bound = index.stars().all(|star| {
            catalog
                .stars()
                .get(star.catalog_index as usize)
                .is_some_and(|s| s.hip == star.hip)
        });
        if !bound || index.star_count() == 0 {
            return Err(AsteropeError::IndexCatalogMismatch);
        }
        Ok(StarTracker {
            camera,
            catalog,
            index,
            solver: SolverConfig::default(),
            track: TrackConfig::default(),
            tracker: Tracker::new(),
            beta: Vec3::ZERO,
            aided: None,
        })
    }

    /// Builds a tracker from a `.astc` catalog and a `.asti` index. The index
    /// bytes are verified against their checksum and then used in place.
    pub fn from_bytes(
        camera: Camera,
        catalog: &[u8],
        index: &'a [u8],
    ) -> Result<StarTracker<'a>, AsteropeError> {
        StarTracker::new(
            camera,
            Catalog::parse(catalog)?,
            PairIndex::from_bytes(index)?,
        )
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn index(&self) -> &PairIndex<'a> {
        &self.index
    }

    pub fn solver(&self) -> &SolverConfig {
        &self.solver
    }

    pub fn set_solver(&mut self, solver: SolverConfig) {
        self.solver = solver;
    }

    pub fn set_track_config(&mut self, track: TrackConfig) {
        self.track = track;
    }

    /// Corrects for stellar aberration at the observer's velocity relative
    /// to the solar-system barycenter, km/s in the ICRS axes: the Earth's
    /// velocity (`ephem::earth_velocity_km_s`) plus the orbital velocity.
    /// Solved attitudes then refer to the ICRS axes instead of the apparent
    /// sky, a difference of up to 25 arcseconds. Call it again when the
    /// velocity has changed noticeably; a low Earth orbit turns its 5
    /// arcseconds around every orbit.
    pub fn set_velocity(&mut self, velocity_km_s: Vec3) {
        let beta = crate::ephem::beta(velocity_km_s);
        self.catalog.aberrate(beta - self.beta);
        self.beta = beta;
    }

    /// Forgets the tracked attitude; the next frame starts with a search.
    pub fn reset(&mut self) {
        self.tracker.reset();
        if let Some(aided) = &mut self.aided {
            aided.filter = None;
            aided.rejections = 0;
        }
    }

    pub fn is_tracking(&self) -> bool {
        self.tracker.is_tracking()
    }

    /// Turns on gyro aiding. Feed readings with [`StarTracker::gyro`] and
    /// frames with [`StarTracker::update_aided`].
    pub fn enable_gyro(&mut self, settings: GyroAiding) {
        self.aided = Some(Aided {
            settings,
            filter: None,
            rate: Vec3::ZERO,
            rejections: 0,
        });
    }

    /// Takes a gyro reading that covers the `dt` seconds since the previous
    /// one (rad/s about the sensor axes). Call it for every reading; the
    /// frames come in between with [`StarTracker::update_aided`].
    pub fn gyro(&mut self, reading: Vec3, dt: f64) {
        if let Some(aided) = &mut self.aided {
            match &mut aided.filter {
                Some(filter) => {
                    filter.propagate(reading, dt);
                    aided.rate = filter.rate(reading);
                }
                None => aided.rate = reading,
            }
        }
    }

    /// Processes a frame taken at the time of the latest gyro reading. The
    /// gyro's attitude is the prediction the frame is solved from, its rate
    /// undoes a rolling shutter, and the star fix corrects the gyro. Without
    /// [`StarTracker::enable_gyro`] this is [`StarTracker::update`] with no
    /// motion assumed.
    pub fn update_aided(&mut self, pixels: &[u16]) -> Result<AidedFix, AsteropeError> {
        let Some(mut aided) = self.aided else {
            let fix = self.update(pixels, 0.0)?;
            return Ok(AidedFix {
                attitude: fix.attitude(),
                sigma: Vec3::ZERO,
                gyro_bias: Vec3::ZERO,
                innovation: None,
                fix,
            });
        };
        let detected = detect(&self.frame(pixels)?, &self.solver.detect)?;
        let solver = SolverConfig {
            rolling: (aided.settings.row_time > 0.0 && aided.filter.is_some()).then_some(
                RollingShutter {
                    row_time: aided.settings.row_time,
                    rate: aided.rate,
                },
            ),
            ..self.solver
        };
        // The gyro carries the attitude through a frame without stars, so
        // the tracker need not coast: when the prediction finds nothing it
        // searches at once. The window for the prediction is opened to
        // three sigma of what the filter says it knows.
        let mut track = TrackConfig {
            max_misses: 0,
            ..self.track
        };
        if let Some(filter) = &aided.filter {
            self.tracker.prime(filter.attitude(), aided.rate);
            let sigma = filter.attitude_sigma();
            let three_sigma_px =
                3.0 * sigma.x.max(sigma.y).max(sigma.z) / self.camera.pixel_scale();
            track.window_px = track.window_px.max(three_sigma_px).min(MAX_AIDED_WINDOW_PX);
        }
        let scene = TrackScene {
            camera: &self.camera,
            index: &self.index,
            catalog: &self.catalog,
            config: &solver,
            track: &track,
        };
        let update = self.tracker.update(&detected.detections, &scene, 0.0);

        let mut innovation = None;
        if let Some(solution) = &update.solution {
            let sigma = solution.sigma.map_or(
                Vec3::new(FIRST_FIX_SIGMA, FIRST_FIX_SIGMA, FIRST_FIX_SIGMA),
                |s| Vec3::new(s.about_x, s.about_y, s.about_boresight),
            );
            let variance = |s: f64| s.max(FIX_SIGMA_FLOOR) * s.max(FIX_SIGMA_FLOOR);
            let covariance = Mat3::diagonal(Vec3::new(
                variance(sigma.x),
                variance(sigma.y),
                variance(sigma.z),
            ));
            let start = |q: Quat| {
                AttitudeFilter::new(
                    q,
                    sigma.x.max(sigma.y).max(sigma.z).max(FIX_SIGMA_FLOOR),
                    INITIAL_BIAS_SIGMA,
                    aided.settings.noise,
                )
            };
            match &mut aided.filter {
                None => aided.filter = Some(start(solution.q)),
                Some(filter) => {
                    innovation = filter.update(solution.q, &covariance, aided.settings.gate);
                    if innovation.is_some_and(|i| i.accepted) {
                        aided.rejections = 0;
                    } else {
                        aided.rejections += 1;
                        if aided.rejections >= aided.settings.rejections_before_restart {
                            aided.filter = Some(start(solution.q));
                            aided.rejections = 0;
                        }
                    }
                }
            }
        }
        self.aided = Some(aided);
        let filter = aided.filter;
        Ok(AidedFix {
            fix: Fix {
                event: update.event,
                solution: update.solution,
                detections: detected.detections.len(),
                body_rate: aided.rate,
            },
            attitude: filter.map(|f| f.attitude()),
            sigma: filter.map_or(Vec3::ZERO, |f| f.attitude_sigma()),
            gyro_bias: filter.map_or(Vec3::ZERO, |f| f.bias()),
            innovation,
        })
    }

    fn frame<'p>(&self, pixels: &'p [u16]) -> Result<Frame<'p>, AsteropeError> {
        Frame::new(self.camera.width, self.camera.height, pixels)
    }

    /// Processes the frame taken `dt` seconds after the previous one:
    /// solved from the predicted attitude when one is known, by a
    /// lost-in-space search otherwise. `pixels` is row-major, top row first.
    pub fn update(&mut self, pixels: &[u16], dt: f64) -> Result<Fix, AsteropeError> {
        let detected = detect(&self.frame(pixels)?, &self.solver.detect)?;
        let scene = TrackScene {
            camera: &self.camera,
            index: &self.index,
            catalog: &self.catalog,
            config: &self.solver,
            track: &self.track,
        };
        let update = self.tracker.update(&detected.detections, &scene, dt);
        Ok(Fix {
            event: update.event,
            solution: update.solution,
            detections: detected.detections.len(),
            body_rate: self.tracker.rate(),
        })
    }

    /// Solves one frame on its own by a lost-in-space search, leaving the
    /// tracking state untouched.
    pub fn solve(&self, pixels: &[u16]) -> Result<Fix, AsteropeError> {
        let detected = detect(&self.frame(pixels)?, &self.solver.detect)?;
        let lis = identify_detections(
            &detected.detections,
            &self.camera,
            &self.index,
            &self.catalog,
            &self.solver,
            0,
            MAX_PYRAMIDS_PER_FRAME,
        );
        Ok(Fix {
            event: if lis.solution.is_some() {
                TrackEvent::Acquired
            } else {
                TrackEvent::Lost
            },
            solution: lis.solution,
            detections: detected.detections.len(),
            body_rate: Vec3::ZERO,
        })
    }
}
