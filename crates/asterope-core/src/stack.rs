//! Temporal stacking of a frame sequence.
//!
//! Consecutive frames are registered through the rotation that carries the
//! star vectors of one frame onto the next, estimated without a catalog, and
//! the most recent frames are resampled onto the newest one and averaged.
//! Averaging `n` frames lowers the noise by about `sqrt(n)` while the stars
//! stay in place, so fainter stars cross the detection threshold.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use crate::attitude::{quest, Observation};
use crate::camera::Camera;
use crate::error::AsteropeError;
use crate::math::{cos, floor, round, sqrt, Mat3, Vec3};

/// Brightest detections of each frame considered for registration.
pub const REGISTRATION_STARS: usize = 40;
/// Fewest stars paired between two frames for the rotation to be trusted.
pub const MIN_REGISTRATION_PAIRS: usize = 3;
const MESH_STEP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Registration {
    /// Rotation carrying sensor-frame vectors of the previous frame onto the
    /// current one: `current = rotation * previous`.
    pub rotation: Mat3,
    pub pairs: usize,
    /// RMS angle between paired vectors after the rotation, radians.
    pub rms: f64,
}

fn pair_up(previous: &[Vec3], current: &[Vec3], rotation: &Mat3, radius: f64) -> Vec<Observation> {
    let cos_radius = cos(radius);
    let predicted: Vec<Vec3> = previous.iter().map(|p| rotation.mul_vec(*p)).collect();
    let nearest = |target: Vec3, candidates: &[Vec3]| {
        let mut best: Option<(usize, f64)> = None;
        for (index, candidate) in candidates.iter().enumerate() {
            let alignment = candidate.dot(target);
            if alignment >= cos_radius && best.is_none_or(|(_, a)| alignment > a) {
                best = Some((index, alignment));
            }
        }
        best.map(|(index, _)| index)
    };
    current
        .iter()
        .enumerate()
        .filter_map(|(index, body)| {
            let partner = nearest(*body, &predicted)?;
            let back = nearest(*predicted.get(partner)?, current)?;
            (back == index).then_some(Observation {
                body: *body,
                reference: *previous.get(partner)?,
                weight: 1.0,
            })
        })
        .collect()
}

/// Estimates the rotation between two frames from their detections alone.
///
/// `previous` and `current` are sensor-frame unit vectors, brightest first.
/// Vectors are paired when they lie within `radius` radians of each other
/// after applying `guess`, and only when each is the other's nearest. The
/// pairing is repeated twice with the fitted rotation, each time with a
/// quarter of the radius, which undoes pairs that the first pass crossed
/// between neighboring stars.
pub fn register(
    previous: &[Vec3],
    current: &[Vec3],
    guess: &Mat3,
    radius: f64,
) -> Option<Registration> {
    if !(radius > 0.0) {
        return None;
    }
    let previous = previous.get(..REGISTRATION_STARS).unwrap_or(previous);
    let current = current.get(..REGISTRATION_STARS).unwrap_or(current);
    let mut rotation = *guess;
    let mut pairs = Vec::new();
    for pass_radius in [radius, 0.25 * radius, 0.0625 * radius] {
        pairs = pair_up(previous, current, &rotation, pass_radius);
        if pairs.len() < MIN_REGISTRATION_PAIRS {
            return None;
        }
        rotation = quest(&pairs).ok()?.q.to_matrix();
    }
    let sum_sq: f64 = pairs
        .iter()
        .map(|o| {
            let angle = o.body.angle_to(rotation.mul_vec(o.reference));
            angle * angle
        })
        .sum();
    Some(Registration {
        rotation,
        pairs: pairs.len(),
        rms: sqrt(sum_sq / pairs.len() as f64),
    })
}

struct StackFrame {
    pixels: Vec<u16>,
    /// Sensor-frame unit vectors of the frame's detections, brightest first.
    bodies: Vec<Vec3>,
    /// Rotation carrying this frame's sensor vectors onto the newest frame.
    to_newest: Mat3,
}

/// Mean of the stacked frames, kept at a finer scale than the input.
#[derive(Clone, Debug, PartialEq)]
pub struct Stacked {
    /// `scale` times the mean of the aligned frames.
    pub pixels: Vec<u16>,
    /// A power of two no larger than the number of frames. Averaging lowers
    /// the noise below one input count, so rounding the mean back to whole
    /// counts would undo the gain.
    pub scale: u16,
}

/// The most recent frames of a sequence, each with its rotation to the newest.
pub struct Stacker {
    depth: usize,
    frames: VecDeque<StackFrame>,
    /// Rotation between the last two frames, the first guess for the next.
    motion: Mat3,
}

impl Stacker {
    /// `depth` is the largest number of frames averaged; 1 disables stacking.
    pub fn new(depth: usize) -> Stacker {
        Stacker {
            depth: depth.max(1),
            frames: VecDeque::new(),
            motion: Mat3::IDENTITY,
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.motion = Mat3::IDENTITY;
    }

    /// Adds the newest frame and aligns the held frames with it.
    ///
    /// `bodies` are the sensor-frame unit vectors of the frame's detections,
    /// brightest first. The frame is registered against the previous one
    /// within `radius` radians of the motion seen last time (then of no
    /// motion); each older frame is then registered against the new one
    /// directly, starting from its chained rotation, so that alignment
    /// errors do not accumulate along the sequence. A registration is
    /// trusted when its RMS is at most `max_rms` radians. When the previous
    /// frame cannot be registered the older frames are discarded.
    pub fn push(
        &mut self,
        camera: &Camera,
        pixels: &[u16],
        bodies: &[Vec3],
        radius: f64,
        max_rms: f64,
    ) -> Result<Option<Registration>, AsteropeError> {
        if pixels.len() != camera.pixel_count() {
            return Err(AsteropeError::InvalidFrame);
        }
        let trusted = |r: &Registration| r.rms <= max_rms;
        let registration = self
            .frames
            .back()
            .filter(|previous| previous.pixels.len() == pixels.len())
            .and_then(|previous| {
                [self.motion, Mat3::IDENTITY]
                    .iter()
                    .find_map(|guess| register(&previous.bodies, bodies, guess, radius))
            })
            .filter(trusted);
        match registration {
            Some(found) => {
                let previous = self.frames.len().saturating_sub(1);
                for (position, frame) in self.frames.iter_mut().enumerate() {
                    let chained = found.rotation.mul(&frame.to_newest);
                    frame.to_newest = if position == previous {
                        found.rotation
                    } else {
                        register(&frame.bodies, bodies, &chained, 0.25 * radius)
                            .filter(trusted)
                            .map_or(chained, |direct| direct.rotation)
                    };
                }
                self.motion = found.rotation;
            }
            None => self.clear(),
        }
        while self.frames.len() >= self.depth {
            self.frames.pop_front();
        }
        self.frames.push_back(StackFrame {
            pixels: pixels.to_vec(),
            bodies: bodies.to_vec(),
            to_newest: Mat3::IDENTITY,
        });
        Ok(registration)
    }

    /// Mean of the held frames on the pixel grid of the newest one. Pixels
    /// that an older frame did not cover are averaged over the frames that
    /// did.
    pub fn stacked(&self, camera: &Camera) -> Stacked {
        let Some(newest) = self.frames.back() else {
            return Stacked {
                pixels: Vec::new(),
                scale: 1,
            };
        };
        if self.frames.len() == 1 || newest.pixels.len() != camera.pixel_count() {
            return Stacked {
                pixels: newest.pixels.clone(),
                scale: 1,
            };
        }
        let mut sum: Vec<f32> = newest.pixels.iter().map(|p| f32::from(*p)).collect();
        let mut count = vec![1u16; sum.len()];
        for frame in self.frames.iter().rev().skip(1) {
            add_warped(camera, frame, &mut sum, &mut count);
        }
        let mut peak = 0.0f32;
        for (total, n) in sum.iter_mut().zip(count.iter()) {
            *total /= f32::from((*n).max(1));
            peak = peak.max(*total);
        }
        let mut scale = 1u16;
        while usize::from(scale) * 2 <= self.frames.len()
            && f32::from(scale) * 2.0 * peak <= f32::from(u16::MAX)
        {
            scale *= 2;
        }
        Stacked {
            pixels: sum
                .iter()
                .map(|mean| {
                    round(f64::from(*mean) * f64::from(scale)).clamp(0.0, f64::from(u16::MAX))
                        as u16
                })
                .collect(),
            scale,
        }
    }
}
/// Source-frame coordinates of the newest frame's pixels, on a coarse mesh
/// that is interpolated linearly in between. A node whose line of sight
/// falls behind the source camera is NaN.
fn source_mesh(camera: &Camera, to_newest: &Mat3) -> (Vec<(f64, f64)>, usize) {
    let (width, height) = (camera.width as usize, camera.height as usize);
    let columns = width.div_ceil(MESH_STEP) + 1;
    let rows = height.div_ceil(MESH_STEP) + 1;
    let to_source = to_newest.transpose();
    let mut nodes = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let body = camera.unproject((column * MESH_STEP) as f64, (row * MESH_STEP) as f64);
            nodes.push(
                camera
                    .project(to_source.mul_vec(body))
                    .unwrap_or((f64::NAN, f64::NAN)),
            );
        }
    }
    (nodes, columns)
}

fn sample(pixels: &[u16], width: usize, height: usize, u: f64, v: f64) -> Option<f32> {
    if !(u >= 0.0) || !(v >= 0.0) {
        return None;
    }
    let (x0, y0) = (floor(u), floor(v));
    let (column, row) = (x0 as usize, y0 as usize);
    if column + 1 >= width || row + 1 >= height {
        return None;
    }
    let (fx, fy) = ((u - x0) as f32, (v - y0) as f32);
    let top = pixels.get(row * width + column..row * width + column + 2)?;
    let bottom = pixels.get((row + 1) * width + column..(row + 1) * width + column + 2)?;
    let (&[a, b], &[c, d]) = (top, bottom) else {
        return None;
    };
    let upper = f32::from(a) + (f32::from(b) - f32::from(a)) * fx;
    let lower = f32::from(c) + (f32::from(d) - f32::from(c)) * fx;
    Some(upper + (lower - upper) * fy)
}

fn add_warped(camera: &Camera, frame: &StackFrame, sum: &mut [f32], count: &mut [u16]) {
    let (width, height) = (camera.width as usize, camera.height as usize);
    if width == 0 {
        return;
    }
    let (nodes, columns) = source_mesh(camera, &frame.to_newest);
    let mut row_nodes = vec![(0.0f64, 0.0f64); columns];
    let step = MESH_STEP as f64;
    for (y, (sum_row, count_row)) in sum
        .chunks_exact_mut(width)
        .zip(count.chunks_exact_mut(width))
        .enumerate()
    {
        let band = y / MESH_STEP;
        let t = (y % MESH_STEP) as f64 / step;
        let (Some(upper), Some(lower)) = (
            nodes.get(band * columns..(band + 1) * columns),
            nodes.get((band + 1) * columns..(band + 2) * columns),
        ) else {
            continue;
        };
        for (node, (a, b)) in row_nodes.iter_mut().zip(upper.iter().zip(lower.iter())) {
            *node = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        }
        for ((ends, sums), counts) in row_nodes
            .windows(2)
            .zip(sum_row.chunks_mut(MESH_STEP))
            .zip(count_row.chunks_mut(MESH_STEP))
        {
            let &[left, right] = ends else {
                continue;
            };
            let slope = ((right.0 - left.0) / step, (right.1 - left.1) / step);
            for (offset, (total, n)) in sums.iter_mut().zip(counts.iter_mut()).enumerate() {
                let u = left.0 + slope.0 * offset as f64;
                let v = left.1 + slope.1 * offset as f64;
                if let Some(value) = sample(&frame.pixels, width, height, u, v) {
                    *total += value;
                    *n += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::DEG;
    use crate::rng::Rng;
    use crate::track::rotation_increment;

    /// Random directions near the boresight, no two closer than a blended pair would be.
    fn field(rng: &mut Rng, count: usize) -> Vec<Vec3> {
        let mut stars: Vec<Vec3> = Vec::new();
        while stars.len() < count {
            let star = Vec3::new(rng.range(-0.1, 0.1), rng.range(-0.1, 0.1), 1.0)
                .normalized()
                .unwrap();
            if stars.iter().all(|s| s.angle_to(star) > 0.1 * DEG) {
                stars.push(star);
            }
        }
        stars
    }

    #[test]
    fn registration_recovers_the_rotation_between_frames() {
        let mut rng = Rng::new(5);
        let (mut worst, mut worst_rms) = (0.0f64, 0.0f64);
        for _ in 0..200 {
            let previous = field(&mut rng, 12);
            let theta = Vec3::new(
                rng.range(-0.2, 0.2),
                rng.range(-0.2, 0.2),
                rng.range(-0.5, 0.5),
            )
            .scale(DEG);
            let truth = rotation_increment(theta).to_matrix();
            let mut current: Vec<Vec3> = previous.iter().map(|p| truth.mul_vec(*p)).collect();
            current.swap(0, 7);
            current.truncate(10);
            current.push(Vec3::new(0.3, 0.1, 1.0).normalized().unwrap());
            let found = register(&previous, &current, &Mat3::IDENTITY, 0.6 * DEG).unwrap();
            assert_eq!(found.pairs, 10);
            let error = found.rotation.mul(&truth.transpose());
            let angle = 0.5
                * Vec3::new(
                    error.r2.y - error.r1.z,
                    error.r0.z - error.r2.x,
                    error.r1.x - error.r0.y,
                )
                .norm();
            worst = worst.max(angle);
            worst_rms = worst_rms.max(found.rms);
        }
        println!("worst rotation error {worst:e} rad, worst rms {worst_rms:e} rad");
        assert!(worst < 1e-9, "{worst:e}");
        assert!(worst_rms < 1e-9, "{worst_rms:e}");
    }

    #[test]
    fn registration_needs_enough_common_stars() {
        let mut rng = Rng::new(6);
        let previous = field(&mut rng, 2);
        assert!(register(&previous, &previous, &Mat3::IDENTITY, DEG).is_none());
        let other = field(&mut rng, 12);
        assert!(register(&field(&mut rng, 12), &other, &Mat3::IDENTITY, 0.01 * DEG).is_none());
        assert!(register(&other, &other, &Mat3::IDENTITY, 0.0).is_none());
    }

    #[test]
    fn stacker_keeps_the_newest_frames_and_restarts_without_common_stars() {
        let camera = Camera::from_fov(64, 48, 10.0 * DEG).unwrap();
        let frame = vec![100u16; camera.pixel_count()];
        let mut rng = Rng::new(8);
        let bodies = field(&mut rng, 6);
        let mut stacker = Stacker::new(3);
        assert!(stacker.stacked(&camera).pixels.is_empty());
        for pushed in 0..5 {
            let registration = stacker.push(&camera, &frame, &bodies, DEG, 1e-4).unwrap();
            assert_eq!(registration.is_some(), pushed > 0);
        }
        assert_eq!(stacker.len(), 3);
        let stacked = stacker.stacked(&camera);
        assert_eq!(stacked.scale, 2);
        assert!(stacked.pixels.iter().all(|p| *p == 200));

        let elsewhere = field(&mut rng, 6);
        assert!(stacker
            .push(&camera, &frame, &elsewhere, 0.01 * DEG, 1e-4)
            .unwrap()
            .is_none());
        assert_eq!(stacker.len(), 1);
        assert_eq!(stacker.stacked(&camera).scale, 1);
        assert_eq!(
            stacker
                .push(&camera, &frame[1..], &bodies, DEG, 1e-4)
                .unwrap_err(),
            AsteropeError::InvalidFrame
        );
    }
}
