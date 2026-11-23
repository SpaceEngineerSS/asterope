#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use asterope_core::detect::{detect, CentroidMethod, DetectConfig, Frame};
use asterope_core::sim::{render_sources, SensorModel};
use asterope_core::validation::centroid_suite;

const PHASES: u32 = 20;
const SEEDS_PER_PHASE: u32 = 25;
const PASS_LIMIT_PX: f64 = 0.05;
const PSF_SIGMAS: [f64; 4] = [0.8, 1.0, 1.5, 2.0];
const METHODS: [CentroidMethod; 2] = [
    CentroidMethod::IterativeWeighted,
    CentroidMethod::CenterOfGravity,
];

#[test]
fn noiseless_centroids_are_sub_pixel() {
    for sigma in PSF_SIGMAS {
        for method in METHODS {
            let s = centroid_suite(sigma, None, PHASES, 1, method, 1).unwrap();
            println!(
                "noiseless sigma {sigma:.1} {method:?}: mean {:.5} px, max {:.5} px, missed {}",
                s.mean_error, s.max_error, s.missed
            );
            assert_eq!(s.missed, 0);
            if method == DetectConfig::default().method {
                assert!(
                    s.mean_error < PASS_LIMIT_PX,
                    "sigma {sigma}: mean error {} px",
                    s.mean_error
                );
            }
        }
    }
}

/// The pass limit applies at peak SNR 60. At peak SNR 30 the Cramer-Rao bound
/// itself is about 0.05 px, so that row is reported without a pass criterion.
#[test]
fn noisy_centroids_are_sub_pixel() {
    for (peak_snr, enforced) in [(30.0, false), (60.0, true)] {
        for sigma in PSF_SIGMAS {
            for method in METHODS {
                let is_default = method == DetectConfig::default().method;
                let seeds = if is_default {
                    SEEDS_PER_PHASE
                } else {
                    SEEDS_PER_PHASE / 5
                };
                let s = centroid_suite(sigma, Some(peak_snr), PHASES, seeds, method, 1).unwrap();
                println!(
                    "peak SNR {peak_snr:.0} sigma {sigma:.1} ({:.0} e-) {method:?}: mean {:.4} px, \
                     max {:.4} px, missed {} of {}, Cramer-Rao ~{:.4} px",
                    s.electrons,
                    s.mean_error,
                    s.max_error,
                    s.missed,
                    s.samples + s.missed,
                    s.bound
                );
                if is_default {
                    assert_eq!(s.missed, 0);
                    if enforced {
                        assert!(
                            s.mean_error < PASS_LIMIT_PX,
                            "sigma {sigma}: mean error {} px",
                            s.mean_error
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn error_versus_snr_curve() {
    for snr in [5.0, 8.0, 12.0, 20.0, 30.0, 60.0, 120.0] {
        let s = centroid_suite(
            1.2,
            Some(snr),
            PHASES,
            10,
            DetectConfig::default().method,
            1,
        )
        .unwrap();
        println!(
            "sigma 1.2 peak SNR {snr:>5.0}: mean {:.4} px, detected {:.1} %",
            s.mean_error,
            100.0 * f64::from(s.samples) / f64::from(s.samples + s.missed)
        );
    }
}

#[test]
fn hot_pixels_and_cosmic_rays_are_rejected() {
    let sensor = SensorModel {
        hot_pixel_fraction: 2e-3,
        cosmic_ray_mean: 6.0,
        ..SensorModel::default()
    };
    let (mut false_stars, mut hot, mut cosmic) = (0u32, 0u32, 0u32);
    for seed in 0..40 {
        let pixels = render_sources(256, 256, &[], &sensor, seed).unwrap();
        let output = detect(
            &Frame::new(256, 256, &pixels).unwrap(),
            &DetectConfig::default(),
        )
        .unwrap();
        false_stars += output.detections.len() as u32;
        hot += output.stats.hot_pixels;
        cosmic += output.stats.cosmic_rays;
    }
    println!("40 empty frames: {false_stars} false stars, {hot} hot pixels and {cosmic} cosmic rays rejected");
    assert!(hot > 1000, "hot pixels rejected: {hot}");
    assert!(cosmic > 100, "cosmic rays rejected: {cosmic}");
    assert!(false_stars < 80, "false stars: {false_stars}");
}
