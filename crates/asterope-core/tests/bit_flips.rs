//! Single bits flipped in stored files and in frames, as a particle would.
//!
//! `cargo test --release -p asterope-core --test bit_flips -- --nocapture`
//! prints the counts quoted in `docs/validation.md`.

use asterope_core::camera::Camera;
use asterope_core::catalog::Catalog;
use asterope_core::detect::Frame;
use asterope_core::index::PairIndex;
use asterope_core::math::{ARCSEC, DEG};
use asterope_core::pipeline::{solve_frame, SolverConfig};
use asterope_core::rng::Rng;
use asterope_core::sim::{simulate, SensorModel};

const CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
const INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");
const SKY: &[u8] = include_bytes!("../../../data/hip65.astc");

/// Every bit of the header, then `extra` bits anywhere, each flipped alone.
fn flips(len: usize, header: usize, extra: usize, seed: u64) -> impl Iterator<Item = (usize, u8)> {
    let mut rng = Rng::new(seed);
    (0..header * 8)
        .map(|bit| (bit / 8, 1u8 << (bit % 8)))
        .chain((0..extra).map(move |_| (rng.below(len as u32) as usize, 1u8 << rng.below(8))))
}

#[test]
fn a_flipped_bit_in_a_stored_file_is_always_noticed() {
    let mut catalog = CATALOG.to_vec();
    let mut noticed = 0;
    for (byte, mask) in flips(catalog.len(), 32, 20_000, 1) {
        catalog[byte] ^= mask;
        assert!(
            Catalog::parse(&catalog).is_err(),
            "catalog byte {byte} mask {mask:#x}"
        );
        catalog[byte] ^= mask;
        noticed += 1;
    }
    println!("catalog: {noticed} single-bit flips, all refused");
    assert!(Catalog::parse(&catalog).is_ok());

    let mut index = INDEX.to_vec();
    let mut noticed = 0;
    for (byte, mask) in flips(index.len(), 80, 20_000, 2) {
        index[byte] ^= mask;
        assert!(
            PairIndex::from_bytes(&index).is_err(),
            "index byte {byte} mask {mask:#x}"
        );
        index[byte] ^= mask;
        noticed += 1;
    }
    println!("index: {noticed} single-bit flips, all refused");
    assert!(PairIndex::from_bytes(&index).is_ok());
}

#[test]
fn flipped_bits_in_a_frame_do_not_move_the_attitude_far() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let index = PairIndex::from_bytes(INDEX).unwrap();
    let sky = Catalog::parse(SKY).unwrap();
    let camera = Camera::from_fov(1024, 1024, 14.0 * DEG).unwrap();
    let mut rng = Rng::new(9);
    println!(
        "| Bits flipped per frame | Frames | Still solved | Largest change of attitude (\") |"
    );
    println!("|---|---|---|---|");
    for count in [1usize, 10, 100, 1000, 10_000] {
        let (mut solved, mut worst) = (0, 0.0f64);
        let frames = 60;
        for _ in 0..frames {
            let truth = rng.unit_quat();
            let image = simulate(
                sky.stars(),
                &truth.to_matrix(),
                &camera,
                &SensorModel::default(),
                rng.next_u64(),
            )
            .unwrap();
            let clean = solve_frame(
                &Frame::new(1024, 1024, &image.pixels).unwrap(),
                &camera,
                &index,
                &catalog,
                &SolverConfig::default(),
            )
            .unwrap();
            let Some(before) = clean.lis.solution else {
                continue;
            };
            let mut struck = image.pixels.clone();
            for _ in 0..count {
                let pixel = rng.below(struck.len() as u32) as usize;
                // Any of the twelve bits the sensor delivers.
                struck[pixel] ^= 1 << rng.below(12);
            }
            let after = solve_frame(
                &Frame::new(1024, 1024, &struck).unwrap(),
                &camera,
                &index,
                &catalog,
                &SolverConfig::default(),
            )
            .unwrap();
            if let Some(after) = after.lis.solution {
                solved += 1;
                worst = worst.max(after.q.angle_to(before.q) / ARCSEC);
            }
        }
        println!("| {count} | {frames} | {solved} | {worst:.2} |");
        // A hundred flips are far more than a frame collects in orbit; ten
        // thousand, one pixel in a hundred, are there to find the limit.
        if count <= 100 {
            assert!(solved >= frames - 1);
            assert!(
                worst < 5.0,
                "{count} flips moved the attitude {worst} arcsec"
            );
        }
        assert!(worst < 0.1 * DEG / ARCSEC);
    }
}
