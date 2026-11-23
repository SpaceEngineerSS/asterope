//! Renders the frames the emulated run is given, and solves them here on
//! the build machine so the image can check its own answers bit for bit.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::flight::StarTracker;
use asterope_core::math::{Quat, Vec3, DEG};
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::propagate;

const FRAMES: usize = 2;
const FRAME_PERIOD: f64 = 0.2;

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let catalog_bytes = fs::read(root.join("data/flight-14deg.astc")).unwrap();
    let index_bytes = fs::read(root.join("data/flight-14deg.asti")).unwrap();
    let sky = Catalog::parse(&fs::read(root.join("data/hip65.astc")).unwrap()).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let mut tracker = StarTracker::from_bytes(camera, &catalog_bytes, &index_bytes).unwrap();

    // Orion, turning slowly, as a tracker would see it frame after frame.
    let rate = Vec3::new(0.2 * DEG, -0.1 * DEG, 0.3 * DEG);
    let mut attitude = Quat::from_matrix(&asterope_core::attitude::attitude_from_radec_roll(
        asterope_core::attitude::RaDecRoll {
            ra: 83.8 * DEG,
            dec: -5.4 * DEG,
            roll: 12.0 * DEG,
        },
    ))
    .unwrap();
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut expected = String::from("pub const EXPECTED: [[u64; 4]; FRAMES] = [\n");
    for frame in 0..FRAMES {
        let sensor = SensorModel {
            body_rate: rate,
            ..SensorModel::default()
        };
        let image = simulate(
            sky.stars(),
            &attitude.to_matrix(),
            &camera,
            &sensor,
            40 + frame as u64,
        )
        .unwrap();
        let fix = tracker.update(&image.pixels, FRAME_PERIOD).unwrap();
        let q = fix
            .attitude()
            .expect("the build machine solves every frame");
        writeln!(
            expected,
            "    [{:#x}, {:#x}, {:#x}, {:#x}],",
            q.x.to_bits(),
            q.y.to_bits(),
            q.z.to_bits(),
            q.w.to_bits()
        )
        .unwrap();
        let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        fs::write(out.join(format!("frame{frame}.bin")), pixels).unwrap();
        attitude = propagate(attitude, rate, FRAME_PERIOD);
    }
    expected.push_str("];\n");
    fs::write(
        out.join("expected.rs"),
        format!("pub const FRAMES: usize = {FRAMES};\npub const FRAME_PERIOD: f64 = {FRAME_PERIOD:?};\n{expected}"),
    )
    .unwrap();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=link.x");
    println!(
        "cargo:rustc-link-search={}",
        env::var("CARGO_MANIFEST_DIR").unwrap()
    );
}
