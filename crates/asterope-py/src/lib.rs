//! Python interface to the Asterope star tracker.
//!
//! Frames are two-dimensional `numpy` arrays of `uint16`, row-major with the
//! top row first. Angles cross the boundary in degrees, except residuals and
//! uncertainties, which are in arcseconds.

use asterope_core::calib::solve_unknown_fov;
use asterope_core::camera::{Camera, Distortion, Projection};
use asterope_core::catalog::{Catalog, Designation};
use asterope_core::detect::{detect, Frame};
use asterope_core::filter::GyroNoise;
use asterope_core::flight::{Fix as CoreFix, GyroAiding, StarTracker as CoreTracker};
use asterope_core::index::PairIndex;
use asterope_core::lis::Solution;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::SensorModel;
use asterope_core::track::TrackEvent;
use asterope_core::AsteropeError;
use numpy::{PyArray2, PyReadonlyArray2, PyUntypedArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// The Hipparcos stars to V 6.5, built into the module.
static CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");

fn invalid(error: AsteropeError) -> PyErr {
    PyValueError::new_err(format!("{error:?}"))
}

fn pixels<'a>(frame: &'a PyReadonlyArray2<'_, u16>) -> PyResult<(u32, u32, &'a [u16])> {
    let [height, width] = [frame.shape()[0], frame.shape()[1]];
    let data = frame
        .as_slice()
        .map_err(|_| PyValueError::new_err("the frame must be a C-contiguous uint16 array"))?;
    Ok((width as u32, height as u32, data))
}

fn camera_for(
    width: u32,
    height: u32,
    fov_deg: f64,
    k1: f64,
    equidistant: bool,
) -> PyResult<Camera> {
    let mut camera = if equidistant {
        Camera::from_fov_equidistant(width, height, fov_deg * DEG)
    } else {
        Camera::from_fov(width, height, fov_deg * DEG)
    }
    .map_err(invalid)?;
    camera.distortion = Distortion {
        k1,
        ..camera.distortion
    };
    Ok(camera)
}

/// One matched star.
#[pyclass(get_all, frozen)]
#[derive(Clone)]
struct Match {
    /// Position of the star in the frame's list of detections.
    detection: u32,
    /// "HIP 27989" or "TYC 1-2-3".
    star: String,
    magnitude: f64,
    residual_arcsec: f64,
}

/// What one frame gave.
#[pyclass(get_all, frozen)]
struct Fix {
    /// A solution was found.
    locked: bool,
    /// "acquired", "tracked", "coasting" or "lost".
    event: &'static str,
    /// (x, y, z, w): ICRS to sensor, scalar last. None without a solution.
    quaternion: Option<(f64, f64, f64, f64)>,
    ra_deg: Option<f64>,
    dec_deg: Option<f64>,
    roll_deg: Option<f64>,
    matches: Vec<Match>,
    rms_arcsec: Option<f64>,
    /// One sigma about the sensor x and y axes and the boresight.
    sigma_arcsec: Option<(f64, f64, f64)>,
    /// Bound on the probability that the solution is a coincidence.
    chance_probability: Option<f64>,
    detections: usize,
    /// Body rate about the sensor axes, degrees per second.
    body_rate_deg_s: (f64, f64, f64),
}

fn event_name(event: TrackEvent) -> &'static str {
    match event {
        TrackEvent::Acquired => "acquired",
        TrackEvent::Tracked => "tracked",
        TrackEvent::Coasting => "coasting",
        TrackEvent::Lost => "lost",
    }
}

fn fix_from(event: TrackEvent, solution: Option<&Solution>, detections: usize, rate: Vec3) -> Fix {
    let tuple = |q: Quat| (q.x, q.y, q.z, q.w);
    Fix {
        locked: solution.is_some(),
        event: event_name(event),
        quaternion: solution.map(|s| tuple(s.q)),
        ra_deg: solution.map(|s| s.pointing.ra / DEG),
        dec_deg: solution.map(|s| s.pointing.dec / DEG),
        roll_deg: solution.map(|s| s.pointing.roll / DEG),
        matches: solution
            .map(|s| {
                s.matches
                    .iter()
                    .map(|m| Match {
                        detection: m.detection,
                        star: Designation::of(m.hip).to_string(),
                        magnitude: m.mag,
                        residual_arcsec: m.residual_arcsec,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        rms_arcsec: solution.map(|s| s.rms_arcsec),
        sigma_arcsec: solution.and_then(|s| s.sigma).map(|s| {
            (
                s.about_x / ARCSEC,
                s.about_y / ARCSEC,
                s.about_boresight / ARCSEC,
            )
        }),
        chance_probability: solution.map(|s| s.chance_probability),
        detections,
        body_rate_deg_s: (rate.x / DEG, rate.y / DEG, rate.z / DEG),
    }
}

fn fix_of(fix: &CoreFix) -> Fix {
    fix_from(
        fix.event,
        fix.solution.as_ref(),
        fix.detections,
        fix.body_rate,
    )
}

/// A camera with its catalog and pair index, and the state that carries a
/// solution from one frame to the next.
#[pyclass]
struct StarTracker {
    inner: CoreTracker<'static>,
    width: u32,
    height: u32,
}

impl StarTracker {
    fn checked<'a>(&self, frame: &'a PyReadonlyArray2<'_, u16>) -> PyResult<&'a [u16]> {
        let (width, height, data) = pixels(frame)?;
        if (width, height) != (self.width, self.height) {
            return Err(PyValueError::new_err(format!(
                "the frame is {width} x {height}, the camera {} x {}",
                self.width, self.height
            )));
        }
        Ok(data)
    }
}

#[pymethods]
impl StarTracker {
    /// A tracker for a camera of the given size and horizontal field of
    /// view. The built-in catalog is used unless `catalog` gives the bytes
    /// of another `.astc` file; the pair index is built here, which takes
    /// a moment. `saturation` is the largest value the sensor delivers.
    #[new]
    #[pyo3(signature = (width, height, fov_deg, *, k1=0.0, equidistant=false, catalog=None, stars_per_field=25.0, saturation=4095, trails=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: u32,
        height: u32,
        fov_deg: f64,
        k1: f64,
        equidistant: bool,
        catalog: Option<&[u8]>,
        stars_per_field: f64,
        saturation: u16,
        trails: bool,
    ) -> PyResult<StarTracker> {
        let camera = camera_for(width, height, fov_deg, k1, equidistant)?;
        let catalog = Catalog::parse(catalog.unwrap_or(CATALOG)).map_err(invalid)?;
        let index =
            PairIndex::for_camera(&catalog, &camera, stars_per_field, 4.0).map_err(invalid)?;
        let mut inner = CoreTracker::new(camera, catalog, index).map_err(invalid)?;
        let mut solver = SolverConfig::default();
        solver.detect.saturation = saturation;
        solver.detect.trails = trails;
        inner.set_solver(solver);
        Ok(StarTracker {
            inner,
            width,
            height,
        })
    }

    /// A tracker from the bytes of an onboard catalog and index, as written
    /// by `asterope build-index`.
    #[staticmethod]
    #[pyo3(signature = (width, height, fov_deg, catalog, index, *, k1=0.0, equidistant=false))]
    fn from_bytes(
        width: u32,
        height: u32,
        fov_deg: f64,
        catalog: &[u8],
        index: &[u8],
        k1: f64,
        equidistant: bool,
    ) -> PyResult<StarTracker> {
        let camera = camera_for(width, height, fov_deg, k1, equidistant)?;
        let catalog = Catalog::parse(catalog).map_err(invalid)?;
        let index = PairIndex::from_bytes(index).map_err(invalid)?.into_owned();
        Ok(StarTracker {
            inner: CoreTracker::new(camera, catalog, index).map_err(invalid)?,
            width,
            height,
        })
    }

    /// Solves one frame on its own, by a lost-in-space search.
    fn solve(&self, frame: PyReadonlyArray2<'_, u16>) -> PyResult<Fix> {
        let data = self.checked(&frame)?;
        self.inner
            .solve(data)
            .map(|fix| fix_of(&fix))
            .map_err(invalid)
    }

    /// Processes the frame taken `dt` seconds after the previous one:
    /// tracked from the last solution when there is one.
    fn update(&mut self, frame: PyReadonlyArray2<'_, u16>, dt: f64) -> PyResult<Fix> {
        let data = self.checked(&frame)?;
        self.inner
            .update(data, dt)
            .map(|fix| fix_of(&fix))
            .map_err(invalid)
    }

    /// Forgets the tracked attitude.
    fn reset(&mut self) {
        self.inner.reset();
    }

    /// Corrects for stellar aberration at this velocity relative to the
    /// solar-system barycenter, km/s in the ICRS axes.
    fn set_velocity(&mut self, velocity_km_s: (f64, f64, f64)) {
        let (x, y, z) = velocity_km_s;
        self.inner.set_velocity(Vec3::new(x, y, z));
    }

    /// Turns on gyro aiding. The random walks are in degrees per root
    /// second and degrees per second per root second; `row_time` is the
    /// rolling-shutter delay between rows in seconds, zero for a global
    /// shutter.
    #[pyo3(signature = (angle_random_walk, rate_random_walk, *, row_time=0.0))]
    fn enable_gyro(&mut self, angle_random_walk: f64, rate_random_walk: f64, row_time: f64) {
        self.inner.enable_gyro(GyroAiding {
            row_time,
            ..GyroAiding::new(GyroNoise {
                angle_random_walk: angle_random_walk * DEG,
                rate_random_walk: rate_random_walk * DEG,
            })
        });
    }

    /// Takes a gyro reading in degrees per second about the sensor axes,
    /// covering the `dt` seconds since the previous one.
    fn gyro(&mut self, rate_deg_s: (f64, f64, f64), dt: f64) {
        let (x, y, z) = rate_deg_s;
        self.inner.gyro(Vec3::new(x, y, z).scale(DEG), dt);
    }

    /// Processes a frame taken at the time of the latest gyro reading.
    /// Returns the frame's own fix, the filtered quaternion (None before
    /// the first star fix), its one-sigma uncertainty in arcseconds and the
    /// estimated gyro bias in degrees per second.
    #[allow(clippy::type_complexity)]
    fn update_aided(
        &mut self,
        frame: PyReadonlyArray2<'_, u16>,
    ) -> PyResult<(
        Fix,
        Option<(f64, f64, f64, f64)>,
        (f64, f64, f64),
        (f64, f64, f64),
    )> {
        let data = self.checked(&frame)?;
        let aided = self.inner.update_aided(data).map_err(invalid)?;
        let per = |v: Vec3, unit: f64| (v.x / unit, v.y / unit, v.z / unit);
        Ok((
            fix_of(&aided.fix),
            aided.attitude.map(|q| (q.x, q.y, q.z, q.w)),
            per(aided.sigma, ARCSEC),
            per(aided.gyro_bias, DEG),
        ))
    }
}

/// A photograph solved without knowing its field of view.
#[pyclass(get_all, frozen)]
struct ImageFix {
    fix: Py<Fix>,
    /// Horizontal field of view found, degrees.
    fov_deg: f64,
    k1: f64,
    /// The lens maps angle, not its tangent, to distance from the center.
    equidistant: bool,
}

/// Solves a photograph of the night sky whose field of view is not known:
/// fields from 6 to 100 degrees are tried, those nearest `hint_fov_deg`
/// first, and the lens is fitted. `catalog` gives the bytes of another
/// `.astc` file.
#[pyfunction]
#[pyo3(signature = (frame, *, hint_fov_deg=20.0, catalog=None))]
fn solve_image(
    py: Python<'_>,
    frame: PyReadonlyArray2<'_, u16>,
    hint_fov_deg: f64,
    catalog: Option<&[u8]>,
) -> PyResult<ImageFix> {
    let (width, height, data) = pixels(&frame)?;
    let catalog = Catalog::parse(catalog.unwrap_or(CATALOG)).map_err(invalid)?;
    let solver = SolverConfig::photograph();
    let view = Frame::new(width, height, data).map_err(invalid)?;
    let detected = detect(&view, &solver.detect).map_err(invalid)?;
    let found = solve_unknown_fov(
        &detected.detections,
        (width, height),
        &catalog,
        &solver,
        hint_fov_deg * DEG,
        (25.0, 4.0),
    )
    .map_err(invalid)?;
    let event = if found.lis.solution.is_some() {
        TrackEvent::Acquired
    } else {
        TrackEvent::Lost
    };
    let fix = fix_from(
        event,
        found.lis.solution.as_ref(),
        detected.detections.len(),
        Vec3::ZERO,
    );
    Ok(ImageFix {
        fix: Py::new(py, fix)?,
        fov_deg: found.camera.fov_x() / DEG,
        k1: found.camera.distortion.k1,
        equidistant: found.camera.projection == Projection::Equidistant,
    })
}

/// Renders the sky a camera would see at a pointing: a `uint16` frame from
/// the simulator's default 12-bit sensor. For trying the solver without a
/// camera.
#[pyfunction]
#[pyo3(signature = (width, height, fov_deg, ra_deg, dec_deg, roll_deg=0.0, *, seed=0, exposure=0.1, rate_deg_s=(0.0, 0.0, 0.0)))]
#[allow(clippy::too_many_arguments)]
fn simulate<'py>(
    py: Python<'py>,
    width: u32,
    height: u32,
    fov_deg: f64,
    ra_deg: f64,
    dec_deg: f64,
    roll_deg: f64,
    seed: u64,
    exposure: f64,
    rate_deg_s: (f64, f64, f64),
) -> PyResult<Bound<'py, PyArray2<u16>>> {
    let camera = Camera::from_fov(width, height, fov_deg * DEG).map_err(invalid)?;
    let catalog = Catalog::parse(CATALOG).map_err(invalid)?;
    let attitude =
        asterope_core::attitude::attitude_from_radec_roll(asterope_core::attitude::RaDecRoll {
            ra: ra_deg * DEG,
            dec: dec_deg * DEG,
            roll: roll_deg * DEG,
        });
    let sensor = SensorModel {
        exposure,
        body_rate: Vec3::new(rate_deg_s.0, rate_deg_s.1, rate_deg_s.2).scale(DEG),
        ..SensorModel::default()
    };
    let image = asterope_core::sim::simulate(catalog.stars(), &attitude, &camera, &sensor, seed)
        .map_err(invalid)?;
    let rows: Vec<Vec<u16>> = image
        .pixels
        .chunks(width as usize)
        .map(<[u16]>::to_vec)
        .collect();
    PyArray2::from_vec2(py, &rows).map_err(|error| PyValueError::new_err(error.to_string()))
}

#[pymodule]
fn asterope(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add_class::<StarTracker>()?;
    module.add_class::<Fix>()?;
    module.add_class::<Match>()?;
    module.add_class::<ImageFix>()?;
    module.add_function(wrap_pyfunction!(solve_image, module)?)?;
    module.add_function(wrap_pyfunction!(simulate, module)?)?;
    Ok(())
}
