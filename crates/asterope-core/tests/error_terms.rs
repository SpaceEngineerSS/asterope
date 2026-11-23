//! The attitude error taken apart the way star tracker specifications do
//! (ECSS-E-ST-60-20C, "Star sensor terminology and performance
//! specification"): what changes from frame to frame at a fixed attitude,
//! what changes as the stars move by fractions of a pixel, and what changes
//! as they move across the field.
//!
//! `cargo test --release -p asterope-core --test error_terms -- --nocapture`
//! prints the table quoted in `docs/validation.md`.

use asterope_core::camera::{Camera, Distortion};
use asterope_core::catalog::Catalog;
use asterope_core::detect::Frame;
use asterope_core::index::PairIndex;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::pipeline::{solve_frame, SolverConfig};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, Imperfections, SensorModel};
use asterope_core::track::rotation_increment;

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");
const POINTINGS: u64 = 40;
const FRAMES: u64 = 40;

struct Bench {
    catalog: Catalog,
    index: PairIndex<'static>,
    /// The camera the solver believes in.
    camera: Camera,
}

impl Bench {
    /// Error rotation (sensor axes, arcseconds) of one solved frame, drawn
    /// through `lens`, which may differ from what the solver believes.
    fn error(&self, truth: Quat, lens: &Camera, sensor: &SensorModel, seed: u64) -> Option<Vec3> {
        let image = simulate(self.catalog.stars(), &truth.to_matrix(), lens, sensor, seed).ok()?;
        let view = Frame::new(image.width, image.height, &image.pixels).ok()?;
        let solved = solve_frame(
            &view,
            &self.camera,
            &self.index,
            &self.catalog,
            &SolverConfig::default(),
        )
        .ok()?;
        let q = solved.lis.solution?.q;
        let delta = (q * truth.conjugate()).canonical();
        Some(delta.vector().scale(2.0 / delta.w / ARCSEC))
    }
}

#[derive(Default, Clone, Copy)]
struct Spread {
    cross: f64,
    roll: f64,
    count: f64,
}

impl Spread {
    /// Adds the scatter of `errors` about their own mean.
    fn about_mean(&mut self, errors: &[Vec3]) {
        let n = errors.len() as f64;
        let mean = errors
            .iter()
            .fold(Vec3::ZERO, |sum, e| sum + *e)
            .scale(1.0 / n);
        for e in errors {
            let d = *e - mean;
            self.cross += 0.5 * (d.x * d.x + d.y * d.y);
            self.roll += d.z * d.z;
        }
        self.count += n - 1.0;
    }

    /// Adds `errors` as they are.
    fn about_zero(&mut self, errors: &[Vec3]) {
        for e in errors {
            self.cross += 0.5 * (e.x * e.x + e.y * e.y);
            self.roll += e.z * e.z;
        }
        self.count += errors.len() as f64;
    }

    fn row(&self, name: &str) -> String {
        format!(
            "| {name} | {:.2} | {:.2} |",
            (self.cross / self.count).sqrt(),
            (self.roll / self.count).sqrt()
        )
    }
}

#[test]
fn error_terms_of_the_reference_camera() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let bench = Bench {
        index: PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap(),
        catalog,
        camera,
    };
    let noisy = SensorModel::default();
    let quiet = SensorModel {
        noise: false,
        ..noisy
    };
    let patterned = SensorModel {
        imperfections: Imperfections {
            prnu: 0.02,
            ..Imperfections::default()
        },
        ..quiet
    };
    // A lens whose distortion is not quite what calibration found: the
    // radial term off by 0.002, which moves a corner star 0.3 px.
    let miscalibrated = Camera {
        distortion: Distortion {
            k1: 0.002,
            ..camera.distortion
        },
        ..camera
    };
    let pixel = camera.pixel_scale();

    let (mut temporal, mut pixel_phase, mut pixel_phase_prnu) =
        (Spread::default(), Spread::default(), Spread::default());
    let (mut field, mut field_ideal) = (Spread::default(), Spread::default());
    let mut rng = Rng::new(606);
    for _ in 0..POINTINGS {
        let truth = rng.unit_quat();

        // Temporal noise: one attitude, many frames, scatter about the mean.
        let frames: Vec<Vec3> = (0..FRAMES)
            .filter_map(|seed| bench.error(truth, &camera, &noisy, 1000 + seed))
            .collect();
        temporal.about_mean(&frames);

        // Pixel spatial error: no noise, the stars moved by up to a pixel.
        let mut dithered = |sensor: &SensorModel, spread: &mut Spread| {
            let errors: Vec<Vec3> = (0..FRAMES)
                .filter_map(|_| {
                    let shift = Vec3::new(
                        rng.range(-1.0, 1.0) * pixel,
                        rng.range(-1.0, 1.0) * pixel,
                        0.0,
                    );
                    bench.error(rotation_increment(shift) * truth, &camera, sensor, 1)
                })
                .collect();
            spread.about_mean(&errors);
        };
        dithered(&quiet, &mut pixel_phase);
        dithered(&patterned, &mut pixel_phase_prnu);

        // Field spatial error: no noise, the stars moved across the field.
        let mut swept = |lens: &Camera, spread: &mut Spread| {
            let errors: Vec<Vec3> = (0..FRAMES)
                .filter_map(|_| {
                    let shift =
                        Vec3::new(rng.range(-2.0, 2.0) * DEG, rng.range(-2.0, 2.0) * DEG, 0.0);
                    bench.error(rotation_increment(shift) * truth, lens, &quiet, 1)
                })
                .collect();
            spread.about_zero(&errors);
        };
        swept(&camera, &mut field_ideal);
        swept(&miscalibrated, &mut field);
    }
    println!("| Term | Across the boresight, per axis, RMS (\") | About the boresight, RMS (\") |");
    println!("|---|---|---|");
    println!(
        "{}",
        temporal.row("Temporal noise (noise equivalent angle)")
    );
    println!("{}", pixel_phase.row("Pixel spatial error, ideal detector"));
    println!(
        "{}",
        pixel_phase_prnu.row("Pixel spatial error, 2 % gain pattern")
    );
    println!(
        "{}",
        field_ideal.row("Field spatial error, lens known exactly")
    );
    println!("{}", field.row("Field spatial error, k1 off by 0.002"));
    assert!(temporal.count > 0.9 * (POINTINGS * (FRAMES - 1)) as f64);
    assert!(pixel_phase.cross < temporal.cross);
}
