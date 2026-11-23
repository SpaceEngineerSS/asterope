//! Target-independent engine behind the WebAssembly bindings.
//!
//! Results cross the boundary as flat `f64` buffers; the layouts below are
//! mirrored by `web/src/engine/decode.ts`.

use asterope_core::attitude::{attitude_from_radec_roll, radec_roll_from_attitude, RaDecRoll};
use asterope_core::calib::{
    auto_tolerances, ladder_camera, ladder_models, widest_camera, ImageSolution, ScaleSearch,
    ScaleSearchState, FOV_LADDER_DEG, LADDER_RUNG_BUDGET, LADDER_SPAN,
};
use asterope_core::camera::{Camera, Distortion, Projection};
use asterope_core::catalog::Catalog;
use asterope_core::design::{evaluate, Exposure, Optics};
use asterope_core::detect::{
    detect, CentroidMethod, DetectConfig, DetectOutput, Frame, RejectReason,
};
use asterope_core::index::PairIndex;
use asterope_core::lis::LisOutput;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::orbit::{Blinder, Exclusion, Orbit, Pointing, Surroundings};
use asterope_core::pipeline::MAX_PYRAMIDS_PER_FRAME;
use asterope_core::pipeline::{detection_vectors, SolverConfig};
use asterope_core::rng::{derive_seed, Rng};
use asterope_core::sim::{simulate, Imperfections, SensorModel, StrayLight, TruthStar};
use asterope_core::stack::Stacker;
use asterope_core::track::{propagate, TrackConfig, TrackEvent, TrackScene, Tracker};
use asterope_core::validation::{
    attitude_error, centroid_suite, disturbed_sensor, run_trial, wahba_suite, Scenario,
    StressParameter, TrialClass,
};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const STARS_PER_FIELD: f64 = 25.0;
const BLEND_PIXELS: f64 = 4.0;
/// Largest star displacement between video frames, beyond the predicted
/// motion, that registration still follows (pixels).
const REGISTRATION_RADIUS_PX: f64 = 24.0;
/// A registration whose paired stars disagree by more than this is discarded (pixels).
const REGISTRATION_MAX_RMS_PX: f64 = 1.0;

pub const SENSOR_PARAMS: usize = 31;
const SENSOR_HOT_PIXEL_SEED: usize = 17;
pub const SOLVER_PARAMS: usize = 18;
pub const DETECT_HEADER: usize = 10;
pub const DETECTION_FIELDS: usize = 9;
pub const REJECTED_FIELDS: usize = 4;
pub const SOLUTION_HEADER: usize = 33;
pub const MATCH_FIELDS: usize = 7;
pub const FIELD_STAR_FIELDS: usize = 4;
pub const TRUTH_FIELDS: usize = 5;
pub const TRIAL_FIELDS: usize = 12;
pub const DESIGN_PARAMS: usize = 15;
pub const DESIGN_HEADER: usize = 20;
pub const MISSION_PARAMS: usize = 15;
pub const MISSION_HEADER: usize = 8;
pub const MISSION_FIELDS: usize = 9;
/// Most samples a mission may hold, so a small step cannot exhaust memory.
const MISSION_MAX_SAMPLES: usize = 20_000;

type EngineResult<T> = Result<T, String>;

fn text(error: impl core::fmt::Display) -> String {
    error.to_string()
}

fn put(buffer: &mut [f64], index: usize, value: f64) {
    if let Some(slot) = buffer.get_mut(index) {
        *slot = value;
    }
}

fn param(values: &[f64], index: usize) -> EngineResult<f64> {
    values
        .get(index)
        .copied()
        .ok_or_else(|| format!("parameter {index} is missing"))
}

pub fn sensor_from(values: &[f64]) -> EngineResult<SensorModel> {
    if values.len() != SENSOR_PARAMS {
        return Err(format!("expected {SENSOR_PARAMS} sensor parameters"));
    }
    Ok(SensorModel {
        zero_point: param(values, 0)?,
        exposure: param(values, 1)?,
        psf_sigma: param(values, 2)?,
        sky_background: param(values, 3)?,
        dark_current: param(values, 4)?,
        read_noise: param(values, 5)?,
        gain: param(values, 6)?,
        bias: param(values, 7)?,
        full_well: param(values, 8)?,
        bit_depth: param(values, 9)? as u32,
        hot_pixel_fraction: param(values, 10)?,
        hot_pixel_rate: param(values, 11)?,
        cosmic_ray_mean: param(values, 12)?,
        stray_light: StrayLight {
            intensity: param(values, 13)?,
            azimuth: param(values, 14)? * DEG,
            falloff: param(values, 15)?,
        },
        noise: param(values, 16)? != 0.0,
        hot_pixel_seed: param(values, 17)? as u64,
        body_rate: Vec3::new(
            param(values, 18)? * DEG,
            param(values, 19)? * DEG,
            param(values, 20)? * DEG,
        ),
        color_term: param(values, 21)?,
        imperfections: Imperfections {
            vignetting: param(values, 22)?,
            prnu: param(values, 23)?,
            dsnu: param(values, 24)?,
            column_noise: param(values, 25)?,
            psf_growth: param(values, 26)?,
            coma: param(values, 27)?,
            row_time: param(values, 28)? * 1e-6,
            nonlinearity: param(values, 29)?,
            blooming: param(values, 30)?,
            ..Imperfections::default()
        },
    })
}

fn sensor_to(sensor: &SensorModel) -> Vec<f64> {
    vec![
        sensor.zero_point,
        sensor.exposure,
        sensor.psf_sigma,
        sensor.sky_background,
        sensor.dark_current,
        sensor.read_noise,
        sensor.gain,
        sensor.bias,
        sensor.full_well,
        f64::from(sensor.bit_depth),
        sensor.hot_pixel_fraction,
        sensor.hot_pixel_rate,
        sensor.cosmic_ray_mean,
        sensor.stray_light.intensity,
        sensor.stray_light.azimuth / DEG,
        sensor.stray_light.falloff,
        f64::from(u8::from(sensor.noise)),
        0.0,
        sensor.body_rate.x / DEG,
        sensor.body_rate.y / DEG,
        sensor.body_rate.z / DEG,
        sensor.color_term,
        sensor.imperfections.vignetting,
        sensor.imperfections.prnu,
        sensor.imperfections.dsnu,
        sensor.imperfections.column_noise,
        sensor.imperfections.psf_growth,
        sensor.imperfections.coma,
        sensor.imperfections.row_time * 1e6,
        sensor.imperfections.nonlinearity,
        sensor.imperfections.blooming,
    ]
}

pub fn solver_from(values: &[f64]) -> EngineResult<SolverConfig> {
    if values.len() != SOLVER_PARAMS {
        return Err(format!("expected {SOLVER_PARAMS} solver parameters"));
    }
    Ok(SolverConfig {
        detect: DetectConfig {
            cell_size: param(values, 0)? as u32,
            segment_sigma: param(values, 1)?,
            peak_sigma: param(values, 2)?,
            min_area: param(values, 3)? as u32,
            max_area: param(values, 4)? as u32,
            max_sharpness: param(values, 5)?,
            max_elongation: param(values, 6)?,
            saturation: param(values, 7)? as u16,
            method: if param(values, 8)? == 0.0 {
                CentroidMethod::CenterOfGravity
            } else {
                CentroidMethod::IterativeWeighted
            },
            max_detections: param(values, 9)? as usize,
            trails: param(values, 17)? != 0.0,
            ..DetectConfig::default()
        },
        tolerance_px: param(values, 10)?,
        match_radius_px: param(values, 11)?,
        pattern_stars: param(values, 12)? as usize,
        max_triads: param(values, 13)? as usize,
        min_matches: param(values, 14)? as usize,
        max_chance_probability: param(values, 15)?,
        weighted: param(values, 16)? != 0.0,
        rolling: None,
    })
}

#[derive(Clone, Copy, PartialEq)]
struct ContextKey {
    width: u32,
    height: u32,
    fov_deg: f64,
    distortion: Distortion,
    projection: Projection,
    scale_span: f64,
    current_epoch: bool,
}

struct Context {
    key: ContextKey,
    camera: Camera,
    index: PairIndex<'static>,
    scale_span: f64,
    current_epoch: bool,
    pixels: Vec<u16>,
    /// Factor by which a stacked frame exceeds the scale of its input.
    stack_scale: u16,
    truth: Vec<TruthStar>,
    detections: DetectOutput,
}

/// An identification in progress, advanced a few focal lengths at a time.
struct Session {
    config: SolverConfig,
    /// Searching a ladder of fields of view rather than the configured one.
    ladder: bool,
    /// Fields of view to try, degrees.
    rungs: Vec<(f64, Projection)>,
    rung: usize,
    state: Option<ScaleSearchState>,
    /// Pair index of the current ladder rung; the context index otherwise.
    index: Option<PairIndex<'static>>,
    pyramids: u32,
    triads: u32,
    scales: u32,
    /// Scales tried and available in the current rung.
    rung_scales: (u32, u32),
    result: Option<ImageSolution>,
}

pub struct Engine {
    catalog: Catalog,
    catalog_now: Catalog,
    context: Option<Context>,
    session: Option<Session>,
    tracker: Tracker,
    video: VideoStack,
}

/// The stacked frames of a video and the camera they were taken with.
struct VideoStack {
    stacker: Stacker,
    camera: Option<Camera>,
}

impl VideoStack {
    fn new() -> VideoStack {
        VideoStack {
            stacker: Stacker::new(1),
            camera: None,
        }
    }
}

impl Engine {
    /// `epoch_year` is the observation epoch used for real photographs.
    pub fn new(epoch_year: f64) -> EngineResult<Engine> {
        let catalog = Catalog::parse(CATALOG).map_err(text)?;
        let catalog_now = catalog.at_epoch(epoch_year.clamp(1900.0, 2200.0));
        Ok(Engine {
            catalog,
            catalog_now,
            context: None,
            session: None,
            tracker: Tracker::new(),
            video: VideoStack::new(),
        })
    }

    pub fn catalog_size(&self) -> u32 {
        self.catalog.len() as u32
    }

    fn context(&self) -> EngineResult<&Context> {
        self.context
            .as_ref()
            .ok_or_else(|| "engine is not configured".to_string())
    }

    fn context_mut(&mut self) -> EngineResult<&mut Context> {
        self.context
            .as_mut()
            .ok_or_else(|| "engine is not configured".to_string())
    }

    fn catalog_for(&self, current_epoch: bool) -> &Catalog {
        if current_epoch {
            &self.catalog_now
        } else {
            &self.catalog
        }
    }

    /// Sets the camera and (re)builds the pair index when anything changed.
    /// `distortion` is `[k1, k2, k3, p1, p2]` and may carry a sixth value,
    /// 1 for an equidistant lens.
    /// Returns `[index stars, index pairs, index magnitude limit, fov x (deg),
    /// fov y (deg), pixel scale (arcsec), rebuilt]`.
    pub fn configure(
        &mut self,
        width: u32,
        height: u32,
        fov_deg: f64,
        distortion: &[f64],
        scale_span: f64,
        current_epoch: bool,
    ) -> EngineResult<Vec<f64>> {
        let projection = projection_from(distortion.get(5).copied().unwrap_or(0.0));
        let distortion = Distortion {
            k1: param(distortion, 0)?,
            k2: param(distortion, 1)?,
            k3: param(distortion, 2)?,
            p1: param(distortion, 3)?,
            p2: param(distortion, 4)?,
        };
        if !(0.0..=0.5).contains(&scale_span) {
            return Err("scale span must be between 0 and 0.5".to_string());
        }
        let key = ContextKey {
            width,
            height,
            fov_deg,
            distortion,
            projection,
            scale_span,
            current_epoch,
        };
        let rebuilt = self.context.as_ref().is_none_or(|c| c.key != key);
        if rebuilt {
            let camera = ladder_camera(width, height, fov_deg, projection)
                .map_err(text)?
                .with_distortion(distortion);
            let search = ScaleSearch {
                span: scale_span,
                refine: false,
            };
            let index = PairIndex::for_camera(
                self.catalog_for(current_epoch),
                &widest_camera(&camera, &search),
                STARS_PER_FIELD,
                BLEND_PIXELS,
            )
            .map_err(text)?;
            let previous = self.context.take();
            let keep_frame = previous
                .as_ref()
                .is_some_and(|p| p.key.width == width && p.key.height == height);
            let (pixels, truth) = match previous {
                Some(p) if keep_frame => (p.pixels, p.truth),
                _ => (Vec::new(), Vec::new()),
            };
            self.context = Some(Context {
                key,
                camera,
                index,
                scale_span,
                current_epoch,
                pixels,
                stack_scale: 1,
                truth,
                detections: DetectOutput::default(),
            });
        }
        let context = self.context()?;
        Ok(vec![
            context.index.star_count() as f64,
            context.index.pair_count() as f64,
            context.index.mag_limit(),
            context.camera.fov_x() / DEG,
            context.camera.fov_y() / DEG,
            context.camera.pixel_scale() / ARCSEC,
            f64::from(u8::from(rebuilt)),
        ])
    }

    /// Renders a synthetic frame at the given pointing (degrees).
    pub fn render(
        &mut self,
        ra_deg: f64,
        dec_deg: f64,
        roll_deg: f64,
        sensor: &[f64],
        seed: f64,
    ) -> EngineResult<()> {
        let sensor = sensor_from(sensor)?;
        let attitude = attitude_from_radec_roll(RaDecRoll {
            ra: ra_deg * DEG,
            dec: dec_deg * DEG,
            roll: roll_deg * DEG,
        });
        let frame = {
            let context = self.context()?;
            simulate(
                self.catalog_for(context.current_epoch).stars(),
                &attitude,
                &context.camera,
                &sensor,
                seed as u64,
            )
            .map_err(text)?
        };
        let context = self.context_mut()?;
        context.pixels = frame.pixels;
        context.stack_scale = 1;
        context.truth = frame.truth;
        Ok(())
    }

    /// Renders the frame of one Monte Carlo trial so it can be inspected.
    /// Returns `[ra, dec, roll (deg)]` followed by the sensor parameters.
    pub fn render_trial(
        &mut self,
        master_seed: f64,
        trial: f64,
        sensor: &[f64],
        disturbances: bool,
    ) -> EngineResult<Vec<f64>> {
        let base = sensor_from(sensor)?;
        let mut rng = Rng::new(derive_seed(master_seed as u64, trial as u64));
        let attitude = rng.unit_quat().to_matrix();
        let sensor = if disturbances {
            disturbed_sensor(&base, &mut rng)
        } else {
            base
        };
        let frame = {
            let context = self.context()?;
            simulate(
                self.catalog_for(context.current_epoch).stars(),
                &attitude,
                &context.camera,
                &sensor,
                rng.next_u64(),
            )
            .map_err(text)?
        };
        let context = self.context_mut()?;
        context.pixels = frame.pixels;
        context.stack_scale = 1;
        context.truth = frame.truth;
        let pointing = asterope_core::attitude::radec_roll_from_attitude(&attitude);
        let mut out = vec![pointing.ra / DEG, pointing.dec / DEG, pointing.roll / DEG];
        out.extend(sensor_to(&sensor));
        Ok(out)
    }

    pub fn load_frame(&mut self, pixels: &[u16]) -> EngineResult<()> {
        let context = self.context_mut()?;
        if pixels.len() != context.camera.pixel_count() {
            return Err("frame size does not match the configured camera".to_string());
        }
        context.pixels = pixels.to_vec();
        context.stack_scale = 1;
        context.truth.clear();
        Ok(())
    }

    /// Follows a camera round a circular orbit and reports what stands
    /// near its boresight.
    ///
    /// Input: `[altitude (km), inclination, node (deg), Julian date (TT),
    /// boresight x, y, z in the orbit's frame (along track, orbit normal,
    /// up), spin rate x, y, z (deg/s; all zero keeps the camera fixed to
    /// the orbit), exclusion angles from the Sun, the Earth's limb and the
    /// Moon (deg), number of orbits, step (s)]`.
    ///
    /// Output header (8 values): `0` orbital period (s), `1` samples,
    /// `2..5` share of samples blinded by the Sun, the Earth and the Moon,
    /// `5` share in the Earth's shadow, `6` longest blinded stretch (s),
    /// `7` body rate (deg/s). Then nine values per sample: time (s),
    /// angles from the boresight to the Sun, the Earth's limb and the Moon
    /// (deg), eclipse flag, blinder (0 none, 1 Sun, 2 Earth, 3 Moon), and
    /// the boresight's right ascension, declination and roll (deg).
    pub fn mission(&self, spec: &[f64]) -> EngineResult<Vec<f64>> {
        if spec.len() != MISSION_PARAMS {
            return Err(format!("expected {MISSION_PARAMS} mission parameters"));
        }
        let orbit = Orbit::circular(
            param(spec, 0)?,
            param(spec, 1)? * DEG,
            param(spec, 2)? * DEG,
            param(spec, 3)?,
        );
        let boresight = Vec3::new(param(spec, 4)?, param(spec, 5)?, param(spec, 6)?);
        let spin = Vec3::new(param(spec, 7)?, param(spec, 8)?, param(spec, 9)?).scale(DEG);
        let exclusion = Exclusion {
            sun: param(spec, 10)? * DEG,
            earth_limb: param(spec, 11)? * DEG,
            moon: param(spec, 12)? * DEG,
        };
        let (orbits, step) = (param(spec, 13)?, param(spec, 14)?);
        let valid = spec.iter().all(|v| v.is_finite())
            && (150.0..=40_000.0).contains(&param(spec, 0)?)
            && orbits > 0.0
            && step > 0.0
            && (boresight.norm() > 1e-6 || spin.norm() > 0.0);
        if !valid {
            return Err("mission parameters out of range".into());
        }
        let span = orbit.period() * orbits;
        let samples = ((span / step) as usize).clamp(1, MISSION_MAX_SAMPLES);
        let step = span / samples as f64;
        let pointing = if spin.norm() > 0.0 {
            Pointing::Spinning {
                start: Quat::IDENTITY,
                rate: spin,
            }
        } else {
            Pointing::Local {
                boresight,
                roll: 0.0,
            }
        };
        let mut out = vec![0.0; MISSION_HEADER];
        out.reserve(samples * MISSION_FIELDS);
        let (mut blinded, mut eclipsed) = ([0.0f64; 4], 0.0f64);
        let (mut stretch, mut longest, mut rate_sum) = (0.0f64, 0.0f64, 0.0f64);
        for sample in 0..samples {
            let time = sample as f64 * step;
            let (attitude, rate) = pointing.attitude(&orbit, time);
            let matrix = attitude.to_matrix();
            let (position, _) = orbit.state(time);
            let around = Surroundings::of(matrix.r2, position, orbit.julian_date(time));
            let code = match around.blinder(&exclusion) {
                None => 0usize,
                Some(Blinder::Sun) => 1,
                Some(Blinder::Earth) => 2,
                Some(Blinder::Moon) => 3,
            };
            if let Some(slot) = blinded.get_mut(code) {
                *slot += 1.0;
            }
            eclipsed += f64::from(u8::from(around.in_eclipse));
            stretch = if code == 0 { 0.0 } else { stretch + step };
            longest = longest.max(stretch);
            rate_sum += rate.norm();
            let sky = radec_roll_from_attitude(&matrix);
            out.extend_from_slice(&[
                time,
                around.sun_angle / DEG,
                around.earth_limb_angle / DEG,
                around.moon_angle / DEG,
                f64::from(u8::from(around.in_eclipse)),
                code as f64,
                sky.ra / DEG,
                sky.dec / DEG,
                sky.roll / DEG,
            ]);
        }
        let n = samples as f64;
        let [_, sun, earth, moon] = blinded;
        let header = [
            orbit.period(),
            n,
            sun / n,
            earth / n,
            moon / n,
            eclipsed / n,
            longest,
            rate_sum / n / DEG,
        ];
        for (slot, value) in out.iter_mut().zip(header) {
            *slot = value;
        }
        Ok(out)
    }

    /// Predicts the performance of a camera design.
    ///
    /// Input: `[focal length (mm), aperture (mm), pixel (um), width, height,
    /// transmission, quantum efficiency, bandwidth (nm), exposure (s), PSF
    /// sigma (px), read noise (e-), dark current (e-/px/s), sky (e-/px/s),
    /// body rate (deg/s), peak threshold (sigma)]`.
    ///
    /// Output header (20 values): `0..2` field of view x / y (deg), `2`
    /// pixel scale (arcsec), `3` zero point (e-/s at V = 0), `4` noise (e-),
    /// `5` trail (px), `6` limiting magnitude, `7` catalog limited, `8`
    /// stars per field, `9..11` one-sigma cross-boresight / roll (arcsec),
    /// `11..13` expected median cross-boresight / roll error (arcsec), `13`
    /// streak rate (deg/s), `14..17` sky fraction with at least 4 / 5 / 8
    /// stars, `17` fewest and `18` median stars in a field, `19` focal
    /// ratio. Then the star count of each cell of the 72 x 36 sky grid,
    /// from declination +90 down.
    pub fn design(&self, spec: &[f64]) -> EngineResult<Vec<f64>> {
        let optics = Optics {
            focal_length_mm: param(spec, 0)?,
            aperture_mm: param(spec, 1)?,
            pixel_um: param(spec, 2)?,
            width: param(spec, 3)? as u32,
            height: param(spec, 4)? as u32,
            transmission: param(spec, 5)?,
            quantum_efficiency: param(spec, 6)?,
            bandwidth_nm: param(spec, 7)?,
        };
        let exposure = Exposure {
            seconds: param(spec, 8)?,
            psf_sigma_px: param(spec, 9)?,
            read_noise: param(spec, 10)?,
            dark_current: param(spec, 11)?,
            sky_background: param(spec, 12)?,
            body_rate: param(spec, 13)? * DEG,
            peak_sigma: param(spec, 14)?,
        };
        let report = evaluate(&self.catalog, &optics, &exposure).map_err(text)?;
        let [four, six, ten] = report.coverage.fractions;
        let mut out = vec![
            report.camera.fov_x() / DEG,
            report.camera.fov_y() / DEG,
            report.camera.pixel_scale() / ARCSEC,
            report.zero_point,
            report.noise,
            report.trail_px,
            report.limiting_magnitude,
            f64::from(u8::from(report.catalog_limited)),
            report.stars_per_field,
            report.sigma_cross / ARCSEC,
            report.sigma_roll / ARCSEC,
            report.median_cross_arcsec(),
            report.median_roll_arcsec(),
            report.streak_rate / DEG,
            four,
            six,
            ten,
            f64::from(report.coverage.minimum),
            f64::from(report.coverage.median),
            optics.focal_ratio(),
        ];
        out.extend(report.coverage.counts.iter().map(|c| f64::from(*c)));
        Ok(out)
    }

    /// Starts a new video: the next `stack_frame` has nothing to align with.
    pub fn stack_reset(&mut self) {
        self.video = VideoStack::new();
    }

    /// Loads the next frame of a video. With `depth` above 1 the frame is
    /// registered against the previous one through its stars, and the stored
    /// frame becomes the mean of the last `depth` aligned frames.
    ///
    /// Returns `[frames averaged, stars paired with the previous frame,
    /// registration RMS (px), displacement at the frame center (px), scale]`;
    /// the stored pixels are `scale` times the mean (see `Stacked`).
    pub fn stack_frame(
        &mut self,
        pixels: &[u16],
        depth: u32,
        solver: &[f64],
    ) -> EngineResult<Vec<f64>> {
        let depth = depth.clamp(1, 64) as usize;
        if depth == 1 {
            self.stack_reset();
            self.load_frame(pixels)?;
            return Ok(vec![1.0, 0.0, 0.0, 0.0, 1.0]);
        }
        let config = solver_from(solver)?;
        let camera = self.context()?.camera;
        if pixels.len() != camera.pixel_count() {
            return Err("frame size does not match the configured camera".to_string());
        }
        let mut video = core::mem::replace(&mut self.video, VideoStack::new());
        if video.camera != Some(camera) || video.stacker.depth() != depth {
            video = VideoStack {
                stacker: Stacker::new(depth),
                camera: Some(camera),
            };
        }
        let frame = Frame::new(camera.width, camera.height, pixels).map_err(text)?;
        let detections = detect(&frame, &config.detect).map_err(text)?;
        let bodies = detection_vectors(&camera, &detections.detections);
        let scale = camera.pixel_scale();
        let registration = video
            .stacker
            .push(
                &camera,
                pixels,
                &bodies,
                REGISTRATION_RADIUS_PX * scale,
                REGISTRATION_MAX_RMS_PX * scale,
            )
            .map_err(text)?;
        let stacked = video.stacker.stacked(&camera);
        let report = vec![
            video.stacker.len() as f64,
            registration.map_or(0.0, |r| r.pairs as f64),
            registration.map_or(0.0, |r| r.rms / scale),
            registration.map_or(0.0, |r| {
                r.rotation.mul_vec(Vec3::Z).angle_to(Vec3::Z) / scale
            }),
            f64::from(stacked.scale),
        ];
        self.video = video;
        let context = self.context_mut()?;
        context.pixels = stacked.pixels;
        context.stack_scale = stacked.scale;
        context.truth.clear();
        Ok(report)
    }
    pub fn frame(&self) -> EngineResult<&[u16]> {
        Ok(&self.context()?.pixels)
    }

    /// `[hip, x, y, magnitude, electrons]` per catalog star rendered in the frame.
    pub fn truth(&self) -> EngineResult<Vec<f64>> {
        Ok(self
            .context()?
            .truth
            .iter()
            .flat_map(|t| [f64::from(t.hip), t.x, t.y, t.mag, t.electrons])
            .collect())
    }

    /// Runs detection on the stored frame.
    ///
    /// Layout: `[detections, rejected, background, noise, blobs, noise blobs,
    /// hot pixels, cosmic rays, too large, edge]`, then per detection
    /// `[x, y, flux, peak, snr, area, saturated, sigma, stellarity]`, then per rejected
    /// blob `[x, y, area, reason]` (0 hot pixel, 1 cosmic ray, 2 too large, 3 edge).
    pub fn detect(&mut self, solver: &[f64]) -> EngineResult<Vec<f64>> {
        let mut config = solver_from(solver)?;
        self.session = None;
        let context = self.context_mut()?;
        config.detect.saturation = config.detect.saturation.saturating_mul(context.stack_scale);
        let frame = Frame::new(context.camera.width, context.camera.height, &context.pixels)
            .map_err(text)?;
        let output = detect(&frame, &config.detect).map_err(text)?;
        let stats = output.stats;
        let mut out = vec![
            output.detections.len() as f64,
            output.rejected.len() as f64,
            stats.background,
            stats.noise,
            f64::from(stats.blobs),
            f64::from(stats.noise_blobs),
            f64::from(stats.hot_pixels),
            f64::from(stats.cosmic_rays),
            f64::from(stats.too_large),
            f64::from(stats.edge),
        ];
        for d in &output.detections {
            out.extend([
                d.x,
                d.y,
                d.flux,
                d.peak,
                d.snr,
                f64::from(d.area),
                f64::from(u8::from(d.saturated)),
                d.sigma,
                d.stellarity(),
            ]);
        }
        for r in &output.rejected {
            let reason = match r.reason {
                RejectReason::HotPixel => 0.0,
                RejectReason::CosmicRay => 1.0,
                RejectReason::TooLarge => 2.0,
                RejectReason::Edge => 3.0,
            };
            out.extend([r.x, r.y, f64::from(r.area), reason]);
        }
        context.detections = output;
        Ok(out)
    }

    /// Identifies the stored detections and solves the attitude.
    ///
    /// Header (32 values): `0` solved, `1` triads, `2` pyramids, `3` scales
    /// tried, `4` calibration refined, `5..9` quaternion (x, y, z, w),
    /// `9..12` RA / Dec / roll (deg), `12` RMS residual (arcsec), `13`
    /// lambda max, `14` QUEST vs q-method (rad), `15` chance probability,
    /// `16..19` one-sigma about x / y / boresight (arcsec), `19..23` pyramid
    /// detection indices, `23` match count, `24..28` fx, fy, cx, cy, `28` k1,
    /// `29` horizontal field of view (deg), `30` QUEST vs SVD (rad), `31`
    /// tracking event (`track_step` only), `32` lens model (0 pinhole, 1
    /// equidistant).
    /// Then per match `[detection, hip, magnitude, residual (arcsec),
    /// predicted x, predicted y, catalog index]`.
    pub fn identify(&mut self, solver: &[f64]) -> EngineResult<Vec<f64>> {
        self.begin_search(solver, false)?;
        loop {
            let progress = self.step_search(64)?;
            if progress.first().copied() == Some(1.0) {
                return self.search_result();
            }
        }
    }

    /// Starts an identification that is advanced with `step_search`.
    ///
    /// With `unknown_fov` the configured field of view is only a hint: a
    /// ladder of fields from 6 to 100 degrees is searched, nearest first,
    /// each with its own pair index and tolerances. Returns the number of
    /// ladder rungs (1 when the field of view is known).
    pub fn begin_search(&mut self, solver: &[f64], unknown_fov: bool) -> EngineResult<f64> {
        let config = solver_from(solver)?;
        let context = self.context()?;
        let rungs = if unknown_fov {
            let hint = context.camera.fov_x() / DEG;
            let mut ladder = FOV_LADDER_DEG.to_vec();
            ladder.sort_by(|a, b| (a / hint).ln().abs().total_cmp(&(b / hint).ln().abs()));
            ladder
                .iter()
                .flat_map(|fov| {
                    ladder_models(*fov)
                        .into_iter()
                        .flatten()
                        .map(move |projection| (*fov, projection))
                })
                .collect()
        } else {
            vec![(context.camera.fov_x() / DEG, context.camera.projection)]
        };
        let count = rungs.len() as f64;
        self.session = Some(Session {
            config,
            ladder: unknown_fov,
            rungs,
            rung: 0,
            state: None,
            index: None,
            pyramids: 0,
            triads: 0,
            scales: 0,
            rung_scales: (0, 0),
            result: None,
        });
        Ok(count)
    }

    /// Tries up to `max_scales` more candidate focal lengths.
    ///
    /// Returns `[done, rung (1-based), rungs, rung field of view (deg),
    /// scales tried in rung, scales in rung, pyramids, triads]`.
    pub fn step_search(&mut self, max_scales: u32) -> EngineResult<Vec<f64>> {
        let mut session = self.session.take().ok_or("no search in progress")?;
        let outcome = self.advance_session(&mut session, max_scales);
        let progress = vec![
            f64::from(u8::from(session.result.is_some())),
            (session.rung + 1).min(session.rungs.len()) as f64,
            session.rungs.len() as f64,
            session
                .rungs
                .get(session.rung)
                .or(session.rungs.last())
                .map_or(f64::NAN, |rung| rung.0),
            f64::from(session.rung_scales.0),
            f64::from(session.rung_scales.1),
            f64::from(session.pyramids),
            f64::from(session.triads),
        ];
        self.session = Some(session);
        outcome.map(|()| progress)
    }

    fn advance_session(&self, session: &mut Session, max_scales: u32) -> EngineResult<()> {
        if session.result.is_some() {
            return Ok(());
        }
        let context = self.context()?;
        let catalog = self.catalog_for(context.current_epoch);
        let mut state = match session.state.take() {
            Some(state) => state,
            None => {
                let (fov, projection) = session
                    .rungs
                    .get(session.rung)
                    .copied()
                    .ok_or("search has no field of view to try")?;
                let (camera, config, search, budget) = if session.ladder {
                    let camera =
                        ladder_camera(context.camera.width, context.camera.height, fov, projection)
                            .map_err(text)?;
                    let (tolerance_px, match_radius_px) = auto_tolerances(&camera);
                    let config = SolverConfig {
                        tolerance_px,
                        match_radius_px,
                        ..session.config
                    };
                    let search = ScaleSearch {
                        span: LADDER_SPAN,
                        refine: true,
                    };
                    session.index = Some(
                        PairIndex::for_camera(
                            catalog,
                            &widest_camera(&camera, &search),
                            STARS_PER_FIELD,
                            BLEND_PIXELS,
                        )
                        .map_err(text)?,
                    );
                    (camera, config, search, LADDER_RUNG_BUDGET)
                } else {
                    let search = ScaleSearch {
                        span: context.scale_span,
                        refine: context.scale_span > 0.0,
                    };
                    (
                        context.camera,
                        session.config,
                        search,
                        MAX_PYRAMIDS_PER_FRAME,
                    )
                };
                ScaleSearchState::new(&camera, &config, &search, session.pyramids, budget)
            }
        };

        let index = session.index.as_ref().unwrap_or(&context.index);
        let done = state.advance(&context.detections.detections, index, catalog, max_scales);
        session.rung_scales = (state.scales_tried(), state.total_scales());
        if !done {
            session.pyramids = state.pyramids();
            session.state = Some(state);
            return Ok(());
        }
        let mut solved = state.finish();
        session.triads = session.triads.saturating_add(solved.lis.triads_tested);
        session.pyramids = solved.lis.pyramids_tested;
        session.scales = session.scales.saturating_add(solved.scales_tried);
        session.index = None;
        let last = session.rung + 1 >= session.rungs.len();
        if solved.lis.solution.is_some() || last {
            solved.lis.triads_tested = session.triads;
            solved.scales_tried = session.scales;
            session.result = Some(solved);
        } else {
            session.rung += 1;
        }
        Ok(())
    }

    /// Result of the current search in the layout documented on `identify`.
    /// A search that has not finished is abandoned and reports no lock.
    pub fn search_result(&mut self) -> EngineResult<Vec<f64>> {
        let session = self.session.take().ok_or("no search in progress")?;
        let context = self.context()?;
        let catalog = self.catalog_for(context.current_epoch);
        let solved = match (session.result, session.state) {
            (Some(solved), _) => solved,
            (None, Some(state)) => {
                let mut partial = state.finish();
                partial.lis.triads_tested =
                    session.triads.saturating_add(partial.lis.triads_tested);
                partial.scales_tried = session.scales.saturating_add(partial.scales_tried);
                partial
            }
            (None, None) => ImageSolution {
                camera: context.camera,
                lis: LisOutput {
                    solution: None,
                    triads_tested: session.triads,
                    pyramids_tested: session.pyramids,
                },
                scales_tried: session.scales,
                refined: false,
            },
        };
        Ok(encode_solution(&solved, catalog))
    }

    /// Forgets the tracked attitude; the next `track_step` starts with a search.
    pub fn track_reset(&mut self) {
        self.tracker.reset();
    }

    /// Solves the stored detections as the next frame of a sequence taken
    /// `dt` seconds after the previous one: from the predicted attitude when
    /// one is known, by a lost-in-space search otherwise.
    ///
    /// A frame that cannot be solved from the prediction is coasted over for
    /// up to `max_misses` frames before a new search; 0 searches at once.
    ///
    /// Returns the `identify` layout with the tracking event in header slot
    /// 31: 0 acquired by search, 1 tracked, 2 coasting, 3 lost.
    pub fn track_step(
        &mut self,
        solver: &[f64],
        dt: f64,
        max_misses: u32,
    ) -> EngineResult<Vec<f64>> {
        let config = solver_from(solver)?;
        let mut tracker = core::mem::take(&mut self.tracker);
        let context = self.context()?;
        let catalog = self.catalog_for(context.current_epoch);
        let scene = TrackScene {
            camera: &context.camera,
            index: &context.index,
            catalog,
            config: &config,
            track: &TrackConfig {
                max_misses,
                ..TrackConfig::default()
            },
        };
        let update = tracker.update(&context.detections.detections, &scene, dt);
        let event = match update.event {
            TrackEvent::Acquired => 0.0,
            TrackEvent::Tracked => 1.0,
            TrackEvent::Coasting => 2.0,
            TrackEvent::Lost => 3.0,
        };
        let solved = ImageSolution {
            camera: context.camera,
            lis: LisOutput {
                solution: update.solution,
                triads_tested: update.triads_tested,
                pyramids_tested: update.pyramids_tested,
            },
            scales_tried: 1,
            refined: false,
        };
        let mut out = encode_solution(&solved, catalog);
        put(&mut out, 31, event);
        self.tracker = tracker;
        Ok(out)
    }
}

/// `[ra, dec, roll]` (degrees) after rotating for `dt` seconds at a body rate
/// given in degrees per second about the sensor axes.
pub fn advance_pointing(pointing: &[f64], rate: &[f64], dt: f64) -> EngineResult<Vec<f64>> {
    let attitude = attitude_from_radec_roll(RaDecRoll {
        ra: param(pointing, 0)? * DEG,
        dec: param(pointing, 1)? * DEG,
        roll: param(pointing, 2)? * DEG,
    });
    let q = Quat::from_matrix(&attitude).ok_or("pointing is not a valid attitude")?;
    let body_rate = Vec3::new(param(rate, 0)?, param(rate, 1)?, param(rate, 2)?).scale(DEG);
    let next = radec_roll_from_attitude(&propagate(q, body_rate, dt).to_matrix());
    Ok(vec![next.ra / DEG, next.dec / DEG, next.roll / DEG])
}

fn projection_from(code: f64) -> Projection {
    if code == 1.0 {
        Projection::Equidistant
    } else {
        Projection::Gnomonic
    }
}

fn projection_code(projection: Projection) -> f64 {
    match projection {
        Projection::Gnomonic => 0.0,
        Projection::Equidistant => 1.0,
    }
}

fn encode_solution(solved: &ImageSolution, catalog: &Catalog) -> Vec<f64> {
    {
        let mut out = vec![f64::NAN; SOLUTION_HEADER];
        let camera = solved.camera;
        let mut set = |index: usize, value: f64| put(&mut out, index, value);
        set(0, 0.0);
        set(1, f64::from(solved.lis.triads_tested));
        set(2, f64::from(solved.lis.pyramids_tested));
        set(3, f64::from(solved.scales_tried));
        set(4, f64::from(u8::from(solved.refined)));
        set(23, 0.0);
        set(24, camera.fx);
        set(25, camera.fy);
        set(26, camera.cx);
        set(27, camera.cy);
        set(28, camera.distortion.k1);
        set(29, camera.fov_x() / DEG);
        set(32, projection_code(camera.projection));
        let Some(solution) = &solved.lis.solution else {
            return out;
        };
        let mut set = |index: usize, value: f64| put(&mut out, index, value);
        set(0, 1.0);
        set(5, solution.q.x);
        set(6, solution.q.y);
        set(7, solution.q.z);
        set(8, solution.q.w);
        set(9, solution.pointing.ra / DEG);
        set(10, solution.pointing.dec / DEG);
        set(11, solution.pointing.roll / DEG);
        set(12, solution.rms_arcsec);
        set(13, solution.lambda_max);
        set(14, solution.quest_q_method_delta);
        set(15, solution.chance_probability);
        if let Some(sigma) = solution.sigma {
            set(16, sigma.about_x / ARCSEC);
            set(17, sigma.about_y / ARCSEC);
            set(18, sigma.about_boresight / ARCSEC);
        }
        for (offset, detection) in solution.pyramid.iter().flatten().enumerate() {
            set(19 + offset, f64::from(*detection));
        }
        set(23, solution.matches.len() as f64);
        set(30, solution.quest_svd_delta);

        for m in &solution.matches {
            let predicted = catalog
                .stars()
                .get(m.catalog_index as usize)
                .and_then(|star| camera.project(solution.attitude.mul_vec(star.dir)))
                .unwrap_or((f64::NAN, f64::NAN));
            out.extend([
                f64::from(m.detection),
                f64::from(m.hip),
                m.mag,
                m.residual_arcsec,
                predicted.0,
                predicted.1,
                f64::from(m.catalog_index),
            ]);
        }
        out
    }
}

impl Engine {
    /// Catalog stars that a camera with the given intrinsics would see at
    /// attitude `q`: `[hip, x, y, magnitude]` each.
    pub fn field(&self, q: &[f64], intrinsics: &[f64], mag_limit: f64) -> EngineResult<Vec<f64>> {
        let context = self.context()?;
        let attitude = Quat::new(param(q, 0)?, param(q, 1)?, param(q, 2)?, param(q, 3)?)
            .normalized()
            .ok_or("quaternion has zero norm")?
            .to_matrix();
        let camera = Camera {
            fx: param(intrinsics, 0)?,
            fy: param(intrinsics, 1)?,
            cx: param(intrinsics, 2)?,
            cy: param(intrinsics, 3)?,
            distortion: Distortion {
                k1: param(intrinsics, 4)?,
                ..context.camera.distortion
            },
            projection: intrinsics
                .get(5)
                .map_or(context.camera.projection, |code| projection_from(*code)),
            ..context.camera
        };
        let cos_limit = asterope_core::math::cos(camera.half_diagonal().min(1.5));
        let mut out = Vec::new();
        for star in self.catalog_for(context.current_epoch).stars() {
            if star.mag > mag_limit {
                break;
            }
            if star.dir.dot(attitude.r2) < cos_limit {
                continue;
            }
            if let Some((x, y)) = camera.project(attitude.mul_vec(star.dir)) {
                if camera.contains(x, y, 0.0) {
                    out.extend([f64::from(star.hip), x, y, star.mag]);
                }
            }
        }
        Ok(out)
    }

    /// Runs `count` lost-in-space trials starting at `first_trial`.
    ///
    /// Per trial: `[class, cross-boresight (arcsec), roll (arcsec), total
    /// (arcsec), catalog stars in frame, detections, rejected, matches,
    /// triads, ra, dec, roll (deg)]`; class 0 success, 1 inaccurate, 2 false
    /// positive, 3 no solution.
    pub fn monte_carlo(
        &self,
        master_seed: f64,
        first_trial: f64,
        count: u32,
        sensor: &[f64],
        solver: &[f64],
        disturbances: bool,
    ) -> EngineResult<Vec<f64>> {
        let context = self.context()?;
        let scenario = Scenario {
            camera: context.camera,
            sensor: sensor_from(sensor)?,
            solver: solver_from(solver)?,
            disturbances,
        };
        let catalog = self.catalog_for(context.current_epoch);
        let mut out = Vec::with_capacity(count as usize * TRIAL_FIELDS);
        for offset in 0..count {
            let trial = first_trial as u64 + u64::from(offset);
            let o = run_trial(
                catalog,
                &context.index,
                &scenario,
                master_seed as u64,
                trial,
            )
            .map_err(text)?;
            let class = match o.class {
                TrialClass::Success => 0.0,
                TrialClass::Inaccurate => 1.0,
                TrialClass::FalsePositive => 2.0,
                TrialClass::NoSolution => 3.0,
            };
            out.extend([
                class,
                o.cross_boresight_arcsec,
                o.roll_arcsec,
                o.total_arcsec,
                f64::from(o.truth_stars),
                f64::from(o.detections),
                f64::from(o.rejected),
                f64::from(o.matches),
                f64::from(o.triads),
                o.truth.ra / DEG,
                o.truth.dec / DEG,
                o.truth.roll / DEG,
            ]);
        }
        Ok(out)
    }
}

/// `[samples, missed, mean error (px), max error (px), electrons, bound (px)]`.
/// A negative `peak_snr` renders without noise.
pub fn centroid_validation(
    psf_sigma: f64,
    peak_snr: f64,
    phases: u32,
    seeds: u32,
    method: u32,
    seed: f64,
) -> EngineResult<Vec<f64>> {
    let method = if method == 0 {
        CentroidMethod::CenterOfGravity
    } else {
        CentroidMethod::IterativeWeighted
    };
    let snr = (peak_snr > 0.0).then_some(peak_snr);
    let s = centroid_suite(psf_sigma, snr, phases, seeds, method, seed as u64).map_err(text)?;
    Ok(vec![
        f64::from(s.samples),
        f64::from(s.missed),
        s.mean_error,
        s.max_error,
        s.electrons,
        s.bound,
    ])
}

/// `[cases, worst QUEST (rad), worst q-method (rad), failures]`.
pub fn wahba_validation(cases: u32, seed: f64) -> Vec<f64> {
    let s = wahba_suite(cases, seed as u64);
    vec![
        f64::from(s.cases),
        s.worst_quest,
        s.worst_q_method,
        f64::from(s.failures),
    ]
}

/// Attitude quaternion `[x, y, z, w]` (ICRS -> sensor) of a pointing in degrees.
pub fn pointing_quaternion(ra_deg: f64, dec_deg: f64, roll_deg: f64) -> Vec<f64> {
    let attitude = attitude_from_radec_roll(RaDecRoll {
        ra: ra_deg * DEG,
        dec: dec_deg * DEG,
        roll: roll_deg * DEG,
    });
    let q = Quat::from_matrix(&attitude).unwrap_or(Quat::IDENTITY);
    vec![q.x, q.y, q.z, q.w]
}

/// `[cross-boresight, roll, total]` error in arcseconds between two quaternions.
pub fn attitude_error_arcsec(estimate: &[f64], truth: &[f64]) -> EngineResult<Vec<f64>> {
    let quat = |v: &[f64]| -> EngineResult<Quat> {
        Quat::new(param(v, 0)?, param(v, 1)?, param(v, 2)?, param(v, 3)?)
            .normalized()
            .ok_or_else(|| "quaternion has zero norm".to_string())
    };
    let (cross, roll, total) = attitude_error(quat(estimate)?, quat(truth)?);
    Ok(vec![cross / ARCSEC, roll / ARCSEC, total / ARCSEC])
}

fn stress_parameter(index: u32) -> EngineResult<StressParameter> {
    StressParameter::ALL
        .get(index as usize)
        .copied()
        .ok_or_else(|| format!("unknown stress parameter {index}"))
}

/// Values a stress sweep visits for parameter `index` (order of
/// `StressParameter::ALL`: read noise, stray light, hot pixels, cosmic rays,
/// PSF sigma, exposure, body rate).
pub fn stress_values(index: u32) -> EngineResult<Vec<f64>> {
    Ok(stress_parameter(index)?.values().to_vec())
}

/// Sensor parameters with stress parameter `index` set to `value`.
pub fn stress_sensor(sensor: &[f64], index: u32, value: f64) -> EngineResult<Vec<f64>> {
    let base = sensor_from(sensor)?;
    let mut out = sensor_to(&stress_parameter(index)?.apply(&base, value));
    put(
        &mut out,
        SENSOR_HOT_PIXEL_SEED,
        param(sensor, SENSOR_HOT_PIXEL_SEED)?,
    );
    Ok(out)
}

/// Hex digest of a fixed synthetic frame and its solution (FNV-1a over the
/// pixel values and the bit patterns of every solution value). Native and
/// WebAssembly builds must agree on it; `data/parity.txt` holds the value.
pub fn parity_digest() -> EngineResult<String> {
    let mut engine = Engine::new(2000.0)?;
    engine.configure(512, 512, 14.0, &[-0.05, 0.01, 0.0, 2e-4, -1e-4], 0.0, false)?;
    let mut sensor = default_sensor();
    for (index, value) in [(10, 5e-4), (12, 3.0), (13, 400.0), (14, 30.0)] {
        put(&mut sensor, index, value);
    }
    engine.render(83.822, -5.391, 12.0, &sensor, 42.0)?;
    let solver = default_solver();
    let detected = engine.detect(&solver)?;
    let solution = engine.identify(&solver)?;

    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
    };
    for pixel in engine.frame()? {
        feed(&pixel.to_le_bytes());
    }
    for value in detected.iter().chain(solution.iter()) {
        feed(&value.to_bits().to_le_bytes());
    }
    Ok(format!("{hash:016x}"))
}

pub fn default_sensor() -> Vec<f64> {
    let mut values = sensor_to(&SensorModel::default());
    put(&mut values, SENSOR_HOT_PIXEL_SEED, 1.0);
    values
}

pub fn default_solver() -> Vec<f64> {
    let c = SolverConfig::default();
    vec![
        f64::from(c.detect.cell_size),
        c.detect.segment_sigma,
        c.detect.peak_sigma,
        f64::from(c.detect.min_area),
        f64::from(c.detect.max_area),
        c.detect.max_sharpness,
        c.detect.max_elongation,
        f64::from(c.detect.saturation),
        match c.detect.method {
            CentroidMethod::CenterOfGravity => 0.0,
            CentroidMethod::IterativeWeighted => 1.0,
        },
        c.detect.max_detections as f64,
        c.tolerance_px,
        c.match_radius_px,
        c.pattern_stars as f64,
        c.max_triads as f64,
        c.min_matches as f64,
        c.max_chance_probability,
        if c.weighted { 1.0 } else { 0.0 },
        if c.detect.trails { 1.0 } else { 0.0 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> Engine {
        let mut engine = Engine::new(2026.9).unwrap();
        let info = engine
            .configure(512, 512, 14.0, &[0.0; 5], 0.0, false)
            .unwrap();
        assert_eq!(info.len(), 7);
        assert!(info[0] > 1000.0 && info[6] == 1.0);
        engine
    }

    #[test]
    fn parameter_arrays_round_trip() {
        assert_eq!(default_sensor().len(), SENSOR_PARAMS);
        assert_eq!(default_solver().len(), SOLVER_PARAMS);
        let sensor = sensor_from(&default_sensor()).unwrap();
        assert_eq!(sensor.psf_sigma, SensorModel::default().psf_sigma);
        assert_eq!(
            solver_from(&default_solver()).unwrap(),
            SolverConfig::default()
        );
        assert!(sensor_from(&[0.0; 3]).is_err());
    }

    #[test]
    fn render_detect_identify_round_trip() {
        let mut engine = configured();
        engine
            .render(83.822, -5.391, 12.0, &default_sensor(), 42.0)
            .unwrap();
        assert_eq!(engine.frame().unwrap().len(), 512 * 512);
        assert!(engine.truth().unwrap().len() >= 5 * TRUTH_FIELDS);

        let solver = default_solver();
        let detected = engine.detect(&solver).unwrap();
        let (n_det, n_rej) = (detected[0] as usize, detected[1] as usize);
        assert_eq!(
            detected.len(),
            DETECT_HEADER + n_det * DETECTION_FIELDS + n_rej * REJECTED_FIELDS
        );

        let solution = engine.identify(&solver).unwrap();
        assert_eq!(solution[0], 1.0, "Orion must lock");
        let matches = solution[23] as usize;
        assert_eq!(solution.len(), SOLUTION_HEADER + matches * MATCH_FIELDS);
        assert!((solution[9] - 83.822).abs() < 0.02 && (solution[10] + 5.391).abs() < 0.02);
        assert!((solution[11] - 12.0).abs() < 0.05);

        let field = engine
            .field(&solution[5..9], &solution[24..29], 6.5)
            .unwrap();
        assert!(field.len() / FIELD_STAR_FIELDS >= matches);

        assert_eq!(
            engine
                .configure(512, 512, 14.0, &[0.0; 5], 0.0, false)
                .unwrap()[6],
            0.0
        );
    }

    #[test]
    fn image_mode_solves_with_an_approximate_field_of_view() {
        let mut engine = Engine::new(2000.0).unwrap();
        engine
            .configure(512, 512, 14.0, &[0.0; 5], 0.0, true)
            .unwrap();
        engine
            .render(200.0, 30.0, 75.0, &default_sensor(), 5.0)
            .unwrap();
        let pixels = engine.frame().unwrap().to_vec();

        engine
            .configure(512, 512, 14.8, &[0.0; 5], 0.1, true)
            .unwrap();
        engine.load_frame(&pixels).unwrap();
        let mut solver = default_solver();
        solver[10] = 2.0;
        solver[11] = 3.0;
        engine.detect(&solver).unwrap();
        let solution = engine.identify(&solver).unwrap();
        assert_eq!(solution[0], 1.0);
        assert!(
            (solution[29] - 14.0).abs() < 0.05,
            "refined FOV {}",
            solution[29]
        );
        assert!(engine.load_frame(&pixels[..10]).is_err());
    }

    #[test]
    fn unknown_field_of_view_is_found_by_the_ladder() {
        let mut engine = Engine::new(2000.0).unwrap();
        engine
            .configure(512, 512, 17.0, &[0.0; 5], 0.0, true)
            .unwrap();
        engine
            .render(120.0, -40.0, 200.0, &default_sensor(), 9.0)
            .unwrap();
        let pixels = engine.frame().unwrap().to_vec();

        // The hint is 75 % off; only the ladder can recover the field.
        engine
            .configure(512, 512, 30.0, &[0.0; 5], 0.0, true)
            .unwrap();
        engine.load_frame(&pixels).unwrap();
        let solver = default_solver();
        engine.detect(&solver).unwrap();
        assert_eq!(engine.begin_search(&solver, true).unwrap(), 10.0);
        let mut steps = 0;
        loop {
            let progress = engine.step_search(8).unwrap();
            assert_eq!(progress.len(), 8);
            steps += 1;
            if progress[0] == 1.0 {
                println!("ladder finished after {steps} steps: {progress:?}");
                break;
            }
            assert!(steps < 10_000, "search did not terminate");
        }
        let solution = engine.search_result().unwrap();
        assert_eq!(solution[0], 1.0);
        assert!(
            (solution[29] - 17.0).abs() < 0.1,
            "recovered FOV {}",
            solution[29]
        );
        assert!((solution[9] - 120.0).abs() < 0.05 && (solution[10] + 40.0).abs() < 0.05);
        assert!(engine.search_result().is_err(), "the session is consumed");
    }

    #[test]
    fn stress_helpers_change_one_parameter() {
        let base = default_sensor();
        for index in 0..7 {
            let values = stress_values(index).unwrap();
            assert_eq!(values.len(), 10);
            let changed = stress_sensor(&base, index, values[9]).unwrap();
            let differing = base.iter().zip(&changed).filter(|(a, b)| a != b).count();
            assert_eq!(differing, 1, "parameter {index}");
            assert!(sensor_from(&changed).is_ok());
        }
        assert!(stress_values(7).is_err());
    }

    #[test]
    fn sequence_acquires_then_tracks() {
        let mut engine = configured();
        let (mut sensor, solver) = (default_sensor(), default_solver());
        let rate = [0.2, -0.3, 0.4];
        sensor[18..21].copy_from_slice(&rate);
        let mut pointing = vec![150.0, 35.0, 10.0];
        let mut events = Vec::new();
        engine.track_reset();
        for frame in 0..8 {
            engine
                .render(
                    pointing[0],
                    pointing[1],
                    pointing[2],
                    &sensor,
                    f64::from(frame),
                )
                .unwrap();
            engine.detect(&solver).unwrap();
            let solution = engine.track_step(&solver, 0.25, 3).unwrap();
            assert_eq!(solution[0], 1.0, "frame {frame} must solve");
            assert!((solution[9] - pointing[0]).abs() < 0.02);
            events.push(solution[31]);
            pointing = advance_pointing(&pointing, &rate, 0.25).unwrap();
        }
        assert_eq!(events[0], 0.0);
        assert!(events[1..].iter().all(|e| *e == 1.0), "{events:?}");
        assert!(engine.track_step(&solver[..3], 0.25, 3).is_err());
    }

    #[test]
    fn parity_digest_matches_the_recorded_value() {
        let digest = parity_digest().unwrap();
        println!("parity digest {digest}");
        assert_eq!(digest, include_str!("../../../data/parity.txt").trim());
    }

    #[test]
    fn abandoned_search_reports_no_lock() {
        let mut engine = configured();
        engine
            .render(10.0, 10.0, 0.0, &default_sensor(), 1.0)
            .unwrap();
        let solver = default_solver();
        engine.detect(&solver).unwrap();
        engine.begin_search(&solver, true).unwrap();
        let solution = engine.search_result().unwrap();
        assert_eq!(solution[0], 0.0);
        assert!(engine.step_search(1).is_err());
    }

    #[test]
    fn monte_carlo_and_replay_agree() {
        let mut engine = configured();
        let (sensor, solver) = (default_sensor(), default_solver());
        let rows = engine
            .monte_carlo(1.0, 0.0, 4, &sensor, &solver, true)
            .unwrap();
        assert_eq!(rows.len(), 4 * TRIAL_FIELDS);
        let replay = engine.render_trial(1.0, 2.0, &sensor, true).unwrap();
        assert_eq!(replay.len(), 3 + SENSOR_PARAMS);
        assert!((replay[0] - rows[2 * TRIAL_FIELDS + 9]).abs() < 1e-9);
        engine.detect(&solver).unwrap();
        let solution = engine.identify(&solver).unwrap();
        assert_eq!(solution[0] == 1.0, rows[2 * TRIAL_FIELDS] < 3.0);
    }

    #[test]
    fn validation_suites_report() {
        let wahba = wahba_validation(200, 3.0);
        assert!(wahba[1] < 1e-10 && wahba[2] < 1e-10 && wahba[3] == 0.0);
        let centroid = centroid_validation(1.2, -1.0, 4, 1, 1, 1.0).unwrap();
        assert!(centroid[0] == 16.0 && centroid[2] < 0.05);
    }

    #[test]
    fn unconfigured_engine_reports_errors() {
        let mut engine = Engine::new(2026.0).unwrap();
        assert!(engine.frame().is_err());
        assert!(engine.detect(&default_solver()).is_err());
        assert!(engine.configure(4, 4, 14.0, &[0.0; 5], 0.0, false).is_err());
    }

    #[test]
    fn mission_reports_what_blinds_a_camera_along_an_orbit() {
        let engine = Engine::new(2000.0).unwrap();
        let away = [
            550.0,
            53.0,
            40.0,
            2_460_400.5,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            35.0,
            25.0,
            10.0,
            1.0,
            10.0,
        ];
        let report = engine.mission(&away).unwrap();
        let samples = report[1] as usize;
        assert_eq!(report.len(), MISSION_HEADER + samples * MISSION_FIELDS);
        assert!(
            (report[0] / 60.0 - 95.6).abs() < 0.5,
            "period {}",
            report[0]
        );
        // The numbers of `tests/orbit_availability.rs` for the same orbit.
        assert!((report[2] - 0.132).abs() < 0.01 && report[3] == 0.0);
        assert!((report[4] - 0.040).abs() < 0.01);
        assert!((report[7] - 360.0 / report[0]).abs() < 0.001);
        let mut normal = away;
        normal[4..7].copy_from_slice(&[0.0, 1.0, 0.0]);
        assert_eq!(engine.mission(&normal).unwrap()[3], 1.0);
        let mut tumbling = away;
        tumbling[7] = 0.4;
        assert!(engine.mission(&tumbling).unwrap()[3] > 0.3);
        let mut bad = away;
        bad[14] = 0.0;
        assert!(engine.mission(&bad).is_err());
        assert!(engine.mission(&away[..5]).is_err());
    }

    #[test]
    fn design_of_the_reference_camera_is_consistent_with_the_simulator() {
        let engine = Engine::new(2000.0).unwrap();
        let spec = [
            25.0, 14.53, 6.0, 1024.0, 1024.0, 0.8, 0.5, 300.0, 0.1, 1.2, 5.0, 10.0, 20.0, 0.0, 5.0,
        ];
        let report = engine.design(&spec).unwrap();
        assert_eq!(report.len(), DESIGN_HEADER + 72 * 36);
        println!("design header: {:?}", &report[..DESIGN_HEADER]);
        let default = SensorModel::default();
        assert!((report[3] / default.zero_point - 1.0).abs() < 0.01);
        assert!((report[0] - 14.0).abs() < 0.1);
        assert!(report[6] > 5.0 && report[6] < 8.5);
        assert!(report[13] > 1.0 && report[13] < 1.5);
        assert!(engine.design(&spec[..5]).is_err());
    }
}
