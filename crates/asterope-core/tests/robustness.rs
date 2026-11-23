//! Hostile-input tests: every entry point must return a value, never panic.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use asterope_core::attitude::{q_method, quest, svd_method, Observation};
use asterope_core::calib::{solve_with_scale_search, ScaleSearch};
use asterope_core::camera::{Camera, Distortion};
use asterope_core::catalog::{Catalog, Star, HEADER_LEN};
use asterope_core::detect::{detect, CentroidMethod, DetectConfig, Detection, Frame};
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::lis::{identify, LisConfig};
use asterope_core::math::{Mat3, Vec3, DEG};
use asterope_core::pipeline::SolverConfig;
use asterope_core::rng::Rng;
use asterope_core::sim::{
    render_sources, simulate, Imperfections, PointSource, SensorModel, StrayLight,
};

const CATALOG: &[u8] = include_bytes!("../../../data/hip65.astc");

/// A value that is usually ordinary and sometimes hostile.
fn wild(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    match rng.below(12) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => 0.0,
        4 => -rng.range(lo, hi),
        5 => rng.range(lo, hi) * 1e12,
        _ => rng.range(lo, hi),
    }
}

#[test]
fn catalog_parser_survives_corruption() {
    let mut rng = Rng::new(1);
    let mut accepted = 0u32;
    for case in 0..100_000 {
        let mut bytes = CATALOG[..HEADER_LEN + 24 * (1 + rng.below(40) as usize)].to_vec();
        match case % 4 {
            0 => {
                for _ in 0..1 + rng.below(6) {
                    let at = rng.below(bytes.len() as u32) as usize;
                    bytes[at] = rng.next_u64() as u8;
                }
            }
            1 => bytes.truncate(rng.below(bytes.len() as u32 + 1) as usize),
            2 => bytes.iter_mut().for_each(|b| *b = rng.next_u64() as u8),
            _ => {
                // A consistent header that promises far more records than exist.
                bytes[8..12].copy_from_slice(&(rng.next_u64() as u32).to_le_bytes());
            }
        }
        accepted += u32::from(Catalog::parse(&bytes).is_ok());
    }
    // Truncating the payload breaks the checksum, so almost nothing parses.
    assert!(
        accepted < 100,
        "{accepted} corrupted catalogs were accepted"
    );
}

#[test]
fn detection_survives_arbitrary_frames_and_settings() {
    let mut rng = Rng::new(2);
    for case in 0..4_000 {
        let width = 16 + rng.below(80);
        let height = 16 + rng.below(80);
        let style = case % 6;
        let pixels: Vec<u16> = (0..width * height)
            .map(|i| match style {
                0 => rng.next_u64() as u16,
                1 => u16::MAX,
                2 => 0,
                3 => (i % 7) as u16 * 9000,
                4 => {
                    if rng.below(50) == 0 {
                        u16::MAX
                    } else {
                        100
                    }
                }
                _ => (i / width) as u16 * 600,
            })
            .collect();
        let config = DetectConfig {
            cell_size: rng.below(200),
            segment_sigma: wild(&mut rng, 0.5, 6.0),
            peak_sigma: wild(&mut rng, 0.5, 12.0),
            min_area: rng.below(8),
            max_area: rng.below(5000),
            max_sharpness: wild(&mut rng, 0.1, 1.0),
            max_elongation: wild(&mut rng, 0.5, 6.0),
            saturation: rng.next_u64() as u16,
            method: if rng.below(2) == 0 {
                CentroidMethod::CenterOfGravity
            } else {
                CentroidMethod::IterativeWeighted
            },
            max_detections: rng.below(100) as usize,
            trails: rng.below(2) == 0,
            max_runs: rng.below(500_000) as usize,
        };
        let frame = Frame::new(width, height, &pixels).unwrap();
        if let Ok(output) = detect(&frame, &config) {
            for d in &output.detections {
                assert!(
                    d.x.is_finite() && d.y.is_finite(),
                    "case {case}: centroid {d:?}"
                );
                assert!((0.0..=1.0).contains(&d.stellarity()), "case {case}");
            }
        }
    }
    assert!(Frame::new(4, 4, &[0; 16]).is_err());
    assert!(Frame::new(64, 64, &[0; 10]).is_err());
    assert!(Frame::new(u32::MAX, u32::MAX, &[]).is_err());
}

#[test]
fn simulator_survives_arbitrary_sensors() {
    let mut rng = Rng::new(3);
    for _ in 0..3_000 {
        let sensor = SensorModel {
            zero_point: wild(&mut rng, 0.0, 1e7),
            exposure: wild(&mut rng, 0.001, 2.0),
            psf_sigma: wild(&mut rng, 0.2, 5.0),
            sky_background: wild(&mut rng, 0.0, 1e4),
            dark_current: wild(&mut rng, 0.0, 1e3),
            read_noise: wild(&mut rng, 0.0, 100.0),
            gain: wild(&mut rng, 0.1, 20.0),
            bias: wild(&mut rng, 0.0, 500.0),
            full_well: wild(&mut rng, 100.0, 1e5),
            bit_depth: rng.below(40),
            hot_pixel_fraction: wild(&mut rng, 0.0, 0.1),
            hot_pixel_rate: wild(&mut rng, 0.0, 1e5),
            hot_pixel_seed: rng.next_u64(),
            cosmic_ray_mean: wild(&mut rng, 0.0, 50.0),
            stray_light: StrayLight {
                intensity: wild(&mut rng, 0.0, 1e5),
                azimuth: wild(&mut rng, 0.0, 7.0),
                falloff: wild(&mut rng, 0.05, 1.0),
            },
            noise: rng.below(2) == 0,
            body_rate: Vec3::new(
                wild(&mut rng, 0.0, 0.5),
                wild(&mut rng, 0.0, 0.5),
                wild(&mut rng, 0.0, 0.5),
            ),
            color_term: wild(&mut rng, -1.0, 1.0),
            imperfections: Imperfections {
                vignetting: wild(&mut rng, 0.0, 1.0),
                prnu: wild(&mut rng, 0.0, 0.2),
                dsnu: wild(&mut rng, 0.0, 1e3),
                column_noise: wild(&mut rng, 0.0, 50.0),
                psf_growth: wild(&mut rng, 0.0, 4.0),
                coma: wild(&mut rng, 0.0, 0.5),
                row_time: wild(&mut rng, 0.0, 1e-3),
                nonlinearity: wild(&mut rng, 0.0, 0.5),
                blooming: wild(&mut rng, 0.0, 1.0),
                pattern_seed: rng.next_u64(),
            },
        };
        let star = [Star {
            hip: 1,
            dir: Vec3::new(wild(&mut rng, -0.1, 0.1), wild(&mut rng, -0.1, 0.1), 1.0)
                .normalized()
                .unwrap_or(Vec3::Z),
            pm_ra: 0.0,
            pm_dec: 0.0,
            mag: wild(&mut rng, -2.0, 8.0),
            b_v: (rng.below(2) == 0).then(|| wild(&mut rng, -0.5, 2.0)),
        }];
        if let Ok(camera) = Camera::from_fov(48, 48, 0.2) {
            if let Ok(frame) = simulate(&star, &Mat3::IDENTITY, &camera, &sensor, rng.next_u64()) {
                assert_eq!(frame.pixels.len(), 48 * 48);
            }
        }
        let sources = [PointSource {
            x: wild(&mut rng, -10.0, 60.0),
            y: wild(&mut rng, -10.0, 60.0),
            electrons: wild(&mut rng, 0.0, 1e6),
        }];
        if let Ok(pixels) = render_sources(48, 48, &sources, &sensor, rng.next_u64()) {
            assert_eq!(pixels.len(), 48 * 48);
        }
    }
    assert!(render_sources(0, 10, &[], &SensorModel::default(), 1).is_err());
}

#[test]
fn cameras_reject_or_handle_extreme_parameters() {
    let mut rng = Rng::new(4);
    for _ in 0..20_000 {
        let Ok(camera) =
            Camera::from_fov(rng.below(3000), rng.below(3000), wild(&mut rng, 0.01, 3.0))
        else {
            continue;
        };
        let camera = camera.with_distortion(Distortion {
            k1: wild(&mut rng, -1.0, 1.0),
            k2: wild(&mut rng, -1.0, 1.0),
            k3: wild(&mut rng, -1.0, 1.0),
            p1: wild(&mut rng, -0.1, 0.1),
            p2: wild(&mut rng, -0.1, 0.1),
        });
        let direction = camera.unproject(
            wild(&mut rng, -100.0, 3000.0),
            wild(&mut rng, -100.0, 3000.0),
        );
        let _ = camera.project(direction);
        let _ = (
            camera.fov_diagonal(),
            camera.half_diagonal(),
            camera.solid_angle(),
        );
    }
}

#[test]
fn wahba_solvers_survive_degenerate_observations() {
    let mut rng = Rng::new(5);
    for _ in 0..20_000 {
        let count = rng.below(6) as usize;
        let observations: Vec<Observation> = (0..count)
            .map(|_| {
                let mut vector = || {
                    Vec3::new(
                        wild(&mut rng, -1.0, 1.0),
                        wild(&mut rng, -1.0, 1.0),
                        wild(&mut rng, -1.0, 1.0),
                    )
                };
                let (body, reference) = (vector(), vector());
                Observation {
                    body,
                    reference,
                    weight: wild(&mut rng, 0.0, 2.0),
                }
            })
            .collect();
        for q in [
            quest(&observations).ok().map(|s| s.q),
            q_method(&observations).ok().map(|s| s.0),
            svd_method(&observations).ok(),
        ]
        .into_iter()
        .flatten()
        {
            // A returned quaternion may be meaningless for garbage input, but
            // it must never be the result of a panic path.
            let _ = q.to_matrix();
        }
    }
}

#[test]
fn identification_survives_garbage_detections() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let camera = Camera::from_fov(256, 256, 20.0 * DEG).unwrap();
    let index = PairIndex::for_camera(&catalog, &camera, 25.0, 4.0).unwrap();
    let mut rng = Rng::new(6);

    for _ in 0..300 {
        let bodies: Vec<Vec3> = (0..rng.below(24))
            .map(|_| match rng.below(8) {
                0 => Vec3::ZERO,
                1 => Vec3::new(f64::NAN, 0.0, 1.0),
                2 => Vec3::Z,
                _ => camera.unproject(rng.range(0.0, 255.0), rng.range(0.0, 255.0)),
            })
            .collect();
        let config = LisConfig {
            tolerance: wild(&mut rng, 1e-5, 5e-3),
            match_radius: wild(&mut rng, 1e-5, 5e-3),
            pattern_stars: rng.below(30) as usize,
            max_triads: rng.below(400) as usize,
            min_matches: rng.below(8) as usize,
            max_chance_probability: wild(&mut rng, 0.0, 1.0),
            prior_pyramids: rng.below(10),
            max_pyramids: rng.below(300),
            field_radius: wild(&mut rng, 0.01, 1.0),
            field_solid_angle: wild(&mut rng, 0.001, 1.0),
            verify_mag_limit: wild(&mut rng, 0.0, 7.0),
        };
        let output = identify(&bodies, &index, &catalog, &config);
        assert!(output.pyramids_tested <= config.max_pyramids.max(1) + 1);
    }

    let garbage: Vec<Detection> = Vec::new();
    let search = ScaleSearch {
        span: 0.2,
        refine: true,
    };
    let solved = solve_with_scale_search(
        &garbage,
        &camera,
        &index,
        &catalog,
        &SolverConfig::default(),
        &search,
    );
    assert!(solved.lis.solution.is_none());
}

#[test]
fn index_builder_rejects_bad_configurations() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let mut rng = Rng::new(7);
    for _ in 0..200 {
        let config = IndexConfig {
            star_count: rng.below(400) as usize,
            max_angle: wild(&mut rng, 0.01, 0.5),
            min_angle: wild(&mut rng, 0.0, 0.01),
            blend_radius: wild(&mut rng, 0.0, 0.01),
            max_pairs: rng.below(50_000) as usize,
        };
        if let Ok(index) = PairIndex::build(&catalog, &config) {
            let _ = index.range(wild(&mut rng, 0.0, 0.5), wild(&mut rng, 0.0, 0.5));
        }
    }
}
