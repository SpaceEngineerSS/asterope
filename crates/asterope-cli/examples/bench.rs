use std::hint::black_box;
use std::time::Instant;

use asterope_core::detect::{detect, DetectConfig, Frame};
use asterope_core::rng::Rng;
use asterope_core::sim::{render_sources, SensorModel};

fn time(label: &str, mut f: impl FnMut()) {
    let start = Instant::now();
    f();
    println!(
        "{label:<28} {:>8.2} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
}

fn main() {
    const N: usize = 1 << 20;
    let mut rng = Rng::new(1);
    time("uniform x 1M", || {
        let mut s = 0.0;
        for _ in 0..N {
            s += rng.uniform();
        }
        black_box(s);
    });
    time("normal x 1M", || {
        let mut s = 0.0;
        for _ in 0..N {
            s += rng.normal();
        }
        black_box(s);
    });
    for lambda in [3.0, 30.0, 300.0] {
        time(&format!("poisson({lambda}) x 1M"), || {
            let mut s = 0.0;
            for _ in 0..N {
                s += rng.poisson(lambda);
            }
            black_box(s);
        });
    }
    for (label, f) in [
        ("sqrt x 1M", asterope_core::math::sqrt as fn(f64) -> f64),
        ("ln x 1M", asterope_core::math::ln),
        ("exp x 1M", asterope_core::math::exp),
        ("floor x 1M", asterope_core::math::floor),
        ("erf x 1M", asterope_core::math::erf),
    ] {
        time(label, || {
            let mut s = 0.0;
            for i in 0..N {
                s += f(black_box(0.5 + i as f64 * 1e-6));
            }
            black_box(s);
        });
    }
    let quiet = SensorModel {
        noise: false,
        ..SensorModel::default()
    };
    time("render 1024^2 noiseless", || {
        black_box(render_sources(1024, 1024, &[], &quiet, 1).unwrap());
    });
    let noisy = SensorModel::default();
    let mut pixels = Vec::new();
    time("render 1024^2 noisy", || {
        pixels = render_sources(1024, 1024, &[], &noisy, 1).unwrap();
    });
    time("detect 1024^2", || {
        let frame = Frame::new(1024, 1024, &pixels).unwrap();
        black_box(detect(&frame, &DetectConfig::default()).unwrap());
    });
}
