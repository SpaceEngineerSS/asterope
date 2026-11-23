//! Star detection: local background mesh, run-based connected components,
//! false-star rejection and sub-pixel centroiding.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::AsteropeError;
use crate::math::{ceil, exp, floor, ln, sqrt};

#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    width: usize,
    height: usize,
    pixels: &'a [u16],
}

impl<'a> Frame<'a> {
    pub fn new(width: u32, height: u32, pixels: &'a [u16]) -> Result<Self, AsteropeError> {
        let (w, h) = (width as usize, height as usize);
        if w < 16 || h < 16 || w.checked_mul(h) != Some(pixels.len()) {
            return Err(AsteropeError::InvalidFrame);
        }
        Ok(Frame {
            width: w,
            height: h,
            pixels,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    fn at(&self, x: usize, y: usize) -> Option<u16> {
        if x < self.width {
            self.pixels.get(y * self.width + x).copied()
        } else {
            None
        }
    }
}

/// A frame flipped left to right. Optics with an odd number of mirrors
/// show the sky reversed, and a reversed sky matches no catalog: every
/// triangle in it is the mirror image of a real one, which the handedness
/// check of the search turns away. Flip such a frame before solving it; the
/// attitude found is then that of the flipped image.
pub fn mirrored(width: usize, pixels: &[u16]) -> Vec<u16> {
    let mut flipped = Vec::with_capacity(pixels.len());
    for row in pixels.chunks(width.max(1)) {
        flipped.extend(row.iter().rev());
    }
    flipped
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CentroidMethod {
    /// Intensity-weighted center of gravity over a hard circular window.
    CenterOfGravity,
    /// Center of gravity with an iteratively re-centered Gaussian window.
    IterativeWeighted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectConfig {
    /// Background mesh cell size in pixels.
    pub cell_size: u32,
    /// Segmentation threshold in background sigmas.
    pub segment_sigma: f64,
    /// Minimum peak height in background sigmas.
    pub peak_sigma: f64,
    pub min_area: u32,
    pub max_area: u32,
    /// Largest accepted ratio of peak pixel to summed blob flux.
    pub max_sharpness: f64,
    /// Largest accepted major/minor axis ratio.
    pub max_elongation: f64,
    /// Pixel value treated as saturated.
    pub saturation: u16,
    pub method: CentroidMethod,
    pub max_detections: usize,
    /// Keep stars drawn out into trails by the camera's rotation instead of
    /// rejecting everything elongated. A trail is told from a cosmic-ray
    /// track by its width, which is that of a star image.
    pub trails: bool,
    /// Most runs of bright pixels a frame may hold; one with more is
    /// refused as `InvalidFrame`. Detection keeps a record of about 100
    /// bytes for every run, so this is what bounds its memory: a sky frame
    /// of a megapixel has a few thousand runs, one with 2 % of hot pixels
    /// twenty thousand.
    pub max_runs: usize,
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            cell_size: 32,
            segment_sigma: 3.0,
            peak_sigma: 5.0,
            min_area: 3,
            max_area: 600,
            max_sharpness: 0.7,
            max_elongation: 2.5,
            saturation: 4095,
            method: CentroidMethod::IterativeWeighted,
            max_detections: 64,
            trails: false,
            max_runs: 400_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    HotPixel,
    CosmicRay,
    TooLarge,
    Edge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub x: f64,
    pub y: f64,
    /// Background-subtracted aperture sum in digital numbers.
    pub flux: f64,
    pub peak: f64,
    pub snr: f64,
    pub area: u32,
    pub saturated: bool,
    /// Estimated PSF standard deviation in pixels.
    pub sigma: f64,
    pub shape: BlobShape,
}

impl Detection {
    /// How star-like the blob is, from 0 to 1. A star peaks at its centroid;
    /// nebula knots, lettering and merged pairs do not. Measured on real
    /// photographs, stars give a core ratio of 0.94-1.00, watermark letters
    /// mostly 0.5-0.9 and nebulosity 0.2-0.55; the eighth power turns that
    /// into a steep penalty.
    pub fn stellarity(&self) -> f64 {
        let core = self.shape.core.clamp(0.0, 1.0);
        let squared = core * core;
        let fourth = squared * squared;
        fourth * fourth
    }

    /// Ordering key for pattern matching: bright and star-like first.
    pub fn rank(&self) -> f64 {
        self.flux * self.stellarity()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlobShape {
    /// Major over minor axis of the intensity distribution.
    pub axis_ratio: f64,
    /// Brightest pixel within one pixel of the centroid over the blob peak.
    pub core: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rejected {
    pub x: f64,
    pub y: f64,
    pub area: u32,
    pub reason: RejectReason,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DetectStats {
    pub background: f64,
    pub noise: f64,
    pub blobs: u32,
    pub noise_blobs: u32,
    pub hot_pixels: u32,
    pub cosmic_rays: u32,
    pub too_large: u32,
    pub edge: u32,
    pub stars: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DetectOutput {
    /// Accepted blobs ordered by `Detection::rank`, best pattern stars first.
    pub detections: Vec<Detection>,
    pub rejected: Vec<Rejected>,
    pub stats: DetectStats,
}

struct Mesh {
    cols: usize,
    rows: usize,
    cell: usize,
    level: Vec<f32>,
    sigma: Vec<f32>,
}

/// Sigma-clipped mean and standard deviation of sorted samples.
fn clipped_stats(sorted: &[u16]) -> (f32, f32) {
    let n = sorted.len();
    let pick = |fraction: f64| -> f64 {
        let index = ((n as f64 - 1.0) * fraction) as usize;
        sorted.get(index).map_or(0.0, |v| f64::from(*v))
    };
    let mut center = pick(0.5);
    let mut spread = (0.5 * (pick(0.84) - pick(0.16))).max(0.5);
    for _ in 0..3 {
        let lo = center - 3.0 * spread;
        let hi = center + 3.0 * spread;
        let start = sorted.partition_point(|v| f64::from(*v) < lo);
        let end = sorted.partition_point(|v| f64::from(*v) <= hi);
        let Some(kept) = sorted.get(start..end) else {
            break;
        };
        if kept.is_empty() {
            break;
        }
        let count = kept.len() as f64;
        let mean = kept.iter().map(|v| f64::from(*v)).sum::<f64>() / count;
        let variance = kept
            .iter()
            .map(|v| {
                let d = f64::from(*v) - mean;
                d * d
            })
            .sum::<f64>()
            / count;
        center = mean;
        // 1/12 accounts for quantization so a constant cell keeps a non-zero sigma.
        spread = sqrt(variance + 1.0 / 12.0);
    }
    (center as f32, spread as f32)
}

fn median3x3(values: &[f32], cols: usize, rows: usize) -> Vec<f32> {
    if cols < 3 || rows < 3 {
        return values.to_vec();
    }
    let mut out = Vec::with_capacity(values.len());
    for r in 0..rows {
        for c in 0..cols {
            let mut window = [0.0f32; 9];
            let mut count = 0;
            for rr in r.saturating_sub(1)..=(r + 1).min(rows - 1) {
                for cc in c.saturating_sub(1)..=(c + 1).min(cols - 1) {
                    if let (Some(slot), Some(v)) =
                        (window.get_mut(count), values.get(rr * cols + cc))
                    {
                        *slot = *v;
                        count += 1;
                    }
                }
            }
            let used = window.get_mut(..count).unwrap_or(&mut []);
            used.sort_unstable_by(f32::total_cmp);
            out.push(used.get(count / 2).copied().unwrap_or(0.0));
        }
    }
    out
}

impl Mesh {
    fn build(frame: &Frame, cell: usize) -> Mesh {
        let cols = frame.width.div_ceil(cell).max(1);
        let rows = frame.height.div_ceil(cell).max(1);
        let stride = if cell >= 16 { 2 } else { 1 };
        let mut level = Vec::with_capacity(cols * rows);
        let mut sigma = Vec::with_capacity(cols * rows);
        let mut samples: Vec<u16> = Vec::with_capacity(cell * cell);
        for r in 0..rows {
            for c in 0..cols {
                samples.clear();
                let x0 = c * cell;
                let x1 = (x0 + cell).min(frame.width);
                for y in ((r * cell)..((r + 1) * cell).min(frame.height)).step_by(stride) {
                    if let Some(row) = frame.pixels.get(y * frame.width + x0..y * frame.width + x1)
                    {
                        samples.extend(row.iter().step_by(stride));
                    }
                }
                samples.sort_unstable();
                let (l, s) = clipped_stats(&samples);
                level.push(l);
                sigma.push(s);
            }
        }
        Mesh {
            level: median3x3(&level, cols, rows),
            sigma: median3x3(&sigma, cols, rows),
            cols,
            rows,
            cell,
        }
    }

    fn knot_position(&self, pixel: f64, count: usize) -> (usize, usize, f32) {
        let offset = 0.5 * (self.cell as f64 - 1.0);
        let position = ((pixel - offset) / self.cell as f64).clamp(0.0, (count - 1) as f64);
        let lower = floor(position) as usize;
        let upper = (lower + 1).min(count - 1);
        (lower, upper, (position - lower as f64) as f32)
    }

    fn fill_row(&self, y: usize, level: &mut [f32], sigma: &mut [f32], scratch: &mut MeshRow) {
        let (r0, r1, t) = self.knot_position(y as f64, self.rows);
        let lerp_rows = |values: &[f32], out: &mut Vec<f32>| {
            out.clear();
            let a = values
                .get(r0 * self.cols..(r0 + 1) * self.cols)
                .unwrap_or(&[]);
            let b = values
                .get(r1 * self.cols..(r1 + 1) * self.cols)
                .unwrap_or(&[]);
            out.extend(a.iter().zip(b.iter()).map(|(p, q)| p + (q - p) * t));
        };
        lerp_rows(&self.level, &mut scratch.level);
        lerp_rows(&self.sigma, &mut scratch.sigma);
        let expand = |knots: &[f32], out: &mut [f32]| {
            for (target, (c0, c1, tx)) in out.iter_mut().zip(scratch.columns.iter()) {
                let a = knots.get(*c0 as usize).copied().unwrap_or(0.0);
                let b = knots.get(*c1 as usize).copied().unwrap_or(a);
                *target = a + (b - a) * tx;
            }
        };
        expand(&scratch.level, level);
        expand(&scratch.sigma, sigma);
    }

    fn eval(&self, x: f64, y: f64) -> (f64, f64) {
        let (c0, c1, tx) = self.knot_position(x, self.cols);
        let (r0, r1, ty) = self.knot_position(y, self.rows);
        let sample = |values: &[f32]| -> f64 {
            let get = |r: usize, c: usize| values.get(r * self.cols + c).copied().unwrap_or(0.0);
            let top = get(r0, c0) + (get(r0, c1) - get(r0, c0)) * tx;
            let bottom = get(r1, c0) + (get(r1, c1) - get(r1, c0)) * tx;
            f64::from(top + (bottom - top) * ty)
        };
        (sample(&self.level), sample(&self.sigma))
    }

    fn mean(values: &[f32]) -> f64 {
        if values.is_empty() {
            0.0
        } else {
            values.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64
        }
    }
}

struct MeshRow {
    columns: Vec<(u32, u32, f32)>,
    level: Vec<f32>,
    sigma: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
struct BlobStats {
    area: u32,
    sum: f64,
    sum_x: f64,
    sum_y: f64,
    sum_xx: f64,
    sum_yy: f64,
    sum_xy: f64,
    peak: f64,
    peak_snr: f64,
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
    saturated: bool,
}

impl BlobStats {
    fn empty() -> Self {
        BlobStats {
            area: 0,
            sum: 0.0,
            sum_x: 0.0,
            sum_y: 0.0,
            sum_xx: 0.0,
            sum_yy: 0.0,
            sum_xy: 0.0,
            peak: 0.0,
            peak_snr: 0.0,
            min_x: u32::MAX,
            max_x: 0,
            min_y: u32::MAX,
            max_y: 0,
            saturated: false,
        }
    }

    fn add_pixel(&mut self, x: usize, y: usize, value: f64, snr: f64, saturated: bool) {
        let (fx, fy) = (x as f64, y as f64);
        self.area += 1;
        self.sum += value;
        self.sum_x += value * fx;
        self.sum_y += value * fy;
        self.sum_xx += value * fx * fx;
        self.sum_yy += value * fy * fy;
        self.sum_xy += value * fx * fy;
        if value > self.peak {
            self.peak = value;
        }
        if snr > self.peak_snr {
            self.peak_snr = snr;
        }
        self.min_x = self.min_x.min(x as u32);
        self.max_x = self.max_x.max(x as u32);
        self.min_y = self.min_y.min(y as u32);
        self.max_y = self.max_y.max(y as u32);
        self.saturated |= saturated;
    }

    fn merge(&mut self, o: &BlobStats) {
        self.area += o.area;
        self.sum += o.sum;
        self.sum_x += o.sum_x;
        self.sum_y += o.sum_y;
        self.sum_xx += o.sum_xx;
        self.sum_yy += o.sum_yy;
        self.sum_xy += o.sum_xy;
        self.peak = self.peak.max(o.peak);
        self.peak_snr = self.peak_snr.max(o.peak_snr);
        self.min_x = self.min_x.min(o.min_x);
        self.max_x = self.max_x.max(o.max_x);
        self.min_y = self.min_y.min(o.min_y);
        self.max_y = self.max_y.max(o.max_y);
        self.saturated |= o.saturated;
    }

    fn centroid(&self) -> (f64, f64) {
        if self.sum > 0.0 {
            (self.sum_x / self.sum, self.sum_y / self.sum)
        } else {
            (
                0.5 * (f64::from(self.min_x) + f64::from(self.max_x)),
                0.5 * (f64::from(self.min_y) + f64::from(self.max_y)),
            )
        }
    }

    /// Unit vector along the major axis of the second-moment matrix.
    fn major_axis(&self) -> (f64, f64) {
        if self.sum <= 0.0 {
            return (1.0, 0.0);
        }
        let (cx, cy) = self.centroid();
        let mxx = self.sum_xx / self.sum - cx * cx;
        let myy = self.sum_yy / self.sum - cy * cy;
        let mxy = self.sum_xy / self.sum - cx * cy;
        let angle = 0.5 * crate::math::atan2(2.0 * mxy, mxx - myy);
        (crate::math::cos(angle), crate::math::sin(angle))
    }

    /// Eigenvalues (major, minor) of the intensity-weighted second-moment matrix.
    fn moments(&self) -> (f64, f64) {
        if self.sum <= 0.0 {
            return (0.0, 0.0);
        }
        let (cx, cy) = self.centroid();
        let mxx = (self.sum_xx / self.sum - cx * cx).max(0.0);
        let myy = (self.sum_yy / self.sum - cy * cy).max(0.0);
        let mxy = self.sum_xy / self.sum - cx * cy;
        let mean = 0.5 * (mxx + myy);
        let diff = 0.5 * (mxx - myy);
        let root = sqrt(diff * diff + mxy * mxy);
        (mean + root, (mean - root).max(0.0))
    }
}

struct Run {
    x0: u32,
    x1: u32,
    parent: u32,
    stats: BlobStats,
}

fn find_root(runs: &mut [Run], mut index: u32) -> u32 {
    loop {
        let Some(parent) = runs.get(index as usize).map(|r| r.parent) else {
            return index;
        };
        if parent == index {
            return index;
        }
        let grandparent = runs.get(parent as usize).map_or(parent, |r| r.parent);
        if let Some(run) = runs.get_mut(index as usize) {
            run.parent = grandparent;
        }
        index = grandparent;
    }
}

fn union(runs: &mut [Run], a: u32, b: u32) {
    let ra = find_root(runs, a);
    let rb = find_root(runs, b);
    if ra == rb {
        return;
    }
    let (keep, drop) = if ra < rb { (ra, rb) } else { (rb, ra) };
    if let Some(run) = runs.get_mut(drop as usize) {
        run.parent = keep;
    }
}

/// Appends a run. The list grows by doubling, but never past what
/// `max_runs` and one more row can hold, so that the limit bounds the
/// memory and not only the count.
fn push_run(runs: &mut Vec<Run>, run: Run, most: usize) {
    if runs.len() == runs.capacity() {
        let room = most.saturating_sub(runs.len()).max(1);
        runs.reserve_exact(runs.len().max(256).min(room));
    }
    runs.push(run);
}

fn segment(frame: &Frame, mesh: &Mesh, config: &DetectConfig) -> Result<Vec<Run>, AsteropeError> {
    let width = frame.width;
    let mut scratch = MeshRow {
        columns: (0..width)
            .map(|x| {
                let (c0, c1, t) = mesh.knot_position(x as f64, mesh.cols);
                (c0 as u32, c1 as u32, t)
            })
            .collect(),
        level: Vec::with_capacity(mesh.cols),
        sigma: Vec::with_capacity(mesh.cols),
    };
    let mut level_row = vec![0.0f32; width];
    let mut sigma_row = vec![0.0f32; width];
    let mut runs: Vec<Run> = Vec::new();
    let mut previous = 0..0;
    let threshold = config.segment_sigma as f32;
    // A row can add half its width in runs before the limit is looked at.
    let most = config.max_runs.saturating_add(width / 2 + 1);

    for (y, row) in frame.pixels.chunks_exact(width).enumerate() {
        mesh.fill_row(y, &mut level_row, &mut sigma_row, &mut scratch);
        let row_start = runs.len();
        let mut open: Option<(usize, BlobStats)> = None;
        let samples = row.iter().zip(level_row.iter().zip(sigma_row.iter()));
        for (x, (value, (level, sigma))) in samples.enumerate() {
            let excess = f32::from(*value) - level;
            if excess > threshold * sigma {
                let (_, stats) = open.get_or_insert_with(|| (x, BlobStats::empty()));
                stats.add_pixel(
                    x,
                    y,
                    f64::from(excess),
                    f64::from(excess / sigma),
                    *value >= config.saturation,
                );
            } else if let Some((x0, stats)) = open.take() {
                let parent = runs.len() as u32;
                let run = Run {
                    x0: x0 as u32,
                    x1: x as u32,
                    parent,
                    stats,
                };
                push_run(&mut runs, run, most);
            }
        }
        if let Some((x0, stats)) = open.take() {
            let parent = runs.len() as u32;
            let run = Run {
                x0: x0 as u32,
                x1: width as u32,
                parent,
                stats,
            };
            push_run(&mut runs, run, most);
        }
        if runs.len() > config.max_runs {
            return Err(AsteropeError::InvalidFrame);
        }

        // 8-connectivity: runs touch when their column ranges overlap or abut diagonally.
        let mut cursor = previous.start;
        for current in row_start..runs.len() {
            let Some((x0, x1)) = runs.get(current).map(|r| (r.x0, r.x1)) else {
                continue;
            };
            while runs
                .get(cursor)
                .is_some_and(|p| cursor < previous.end && p.x1 < x0)
            {
                cursor += 1;
            }
            let mut probe = cursor;
            while probe < previous.end && runs.get(probe).is_some_and(|p| p.x0 <= x1) {
                union(&mut runs, current as u32, probe as u32);
                probe += 1;
            }
        }
        previous = row_start..runs.len();
    }
    Ok(runs)
}

fn window_sums(
    frame: &Frame,
    mesh: &Mesh,
    cx: f64,
    cy: f64,
    radius: f64,
    weight: impl Fn(f64) -> f64,
) -> (f64, f64, f64) {
    let x_lo = floor(cx - radius).max(0.0) as usize;
    let y_lo = floor(cy - radius).max(0.0) as usize;
    let x_hi = (ceil(cx + radius).max(0.0) as usize).min(frame.width.saturating_sub(1));
    let y_hi = (ceil(cy + radius).max(0.0) as usize).min(frame.height.saturating_sub(1));
    let (mut sum, mut sum_x, mut sum_y) = (0.0, 0.0, 0.0);
    for y in y_lo..=y_hi {
        for x in x_lo..=x_hi {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let r2 = dx * dx + dy * dy;
            if r2 > radius * radius {
                continue;
            }
            let Some(value) = frame.at(x, y) else {
                continue;
            };
            let (level, _) = mesh.eval(x as f64, y as f64);
            let w = weight(r2) * (f64::from(value) - level);
            sum += w;
            sum_x += w * dx;
            sum_y += w * dy;
        }
    }
    (sum, sum_x, sum_y)
}

/// Narrowest trail taken for a star, as the variance across it in square
/// pixels (the pixel's own 1/12 included). A cosmic-ray track is one pixel
/// wide and measures 0.08 to 0.2; a star image narrower than 0.55 px RMS is
/// not one this detector is meant for.
const TRAIL_MIN_WIDTH_SQUARED: f64 = 0.3;

/// Center of gravity and flux of a trail: the light inside an ellipse laid
/// along it, re-centered a few times. A hard-edged window weighs every part
/// of the trail the same, so the result is the middle of the trail, which
/// is where the star stood at the middle of the exposure.
fn trail_centroid(
    frame: &Frame,
    mesh: &Mesh,
    start: (f64, f64),
    axis: (f64, f64),
    half_length: f64,
    half_width: f64,
) -> (f64, f64, f64, f64) {
    let (mut cx, mut cy) = start;
    let reach = half_length.max(half_width);
    let (mut flux, mut pixels) = (0.0, 0.0);
    for _ in 0..4 {
        let x_lo = floor(cx - reach).max(0.0) as usize;
        let y_lo = floor(cy - reach).max(0.0) as usize;
        let x_hi = (ceil(cx + reach).max(0.0) as usize).min(frame.width.saturating_sub(1));
        let y_hi = (ceil(cy + reach).max(0.0) as usize).min(frame.height.saturating_sub(1));
        let (mut sum, mut sum_x, mut sum_y, mut count) = (0.0, 0.0, 0.0, 0.0);
        for y in y_lo..=y_hi {
            for x in x_lo..=x_hi {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let along = (dx * axis.0 + dy * axis.1) / half_length;
                let across = (dy * axis.0 - dx * axis.1) / half_width;
                if along * along + across * across > 1.0 {
                    continue;
                }
                let Some(value) = frame.at(x, y) else {
                    continue;
                };
                let (level, _) = mesh.eval(x as f64, y as f64);
                let signal = f64::from(value) - level;
                sum += signal;
                sum_x += signal * dx;
                sum_y += signal * dy;
                count += 1.0;
            }
        }
        (flux, pixels) = (sum, count);
        if sum <= 0.0 {
            break;
        }
        cx += sum_x / sum;
        cy += sum_y / sum;
    }
    let moved = crate::math::hypot(cx - start.0, cy - start.1);
    if moved.is_finite() && moved <= reach {
        (cx, cy, flux, pixels)
    } else {
        (start.0, start.1, flux, pixels)
    }
}

/// Largest background-subtracted value in the 3x3 pixels around a position.
fn core_brightness(frame: &Frame, mesh: &Mesh, x: f64, y: f64) -> f64 {
    let (cx, cy) = (crate::math::round(x), crate::math::round(y));
    let mut best = 0.0f64;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (px, py) = (cx + f64::from(dx), cy + f64::from(dy));
            if px < 0.0 || py < 0.0 {
                continue;
            }
            if let Some(value) = frame.at(px as usize, py as usize) {
                let (level, _) = mesh.eval(px, py);
                best = best.max(f64::from(value) - level);
            }
        }
    }
    best
}

/// Gaussian width from the footprint above threshold: for a Gaussian of peak
/// `P`, the area above level `T` is `2 pi sigma^2 ln(P / T)`.
fn estimate_sigma(stats: &BlobStats, segment_sigma: f64) -> f64 {
    let ratio = stats.peak_snr / segment_sigma;
    let estimate = if ratio > 1.3 {
        sqrt(f64::from(stats.area) / (core::f64::consts::TAU * ln(ratio)))
    } else {
        1.0
    };
    estimate.clamp(0.7, 6.0)
}

fn refine_centroid(
    frame: &Frame,
    mesh: &Mesh,
    start: (f64, f64),
    sigma: f64,
    method: CentroidMethod,
) -> (f64, f64) {
    let (mut cx, mut cy) = start;
    let limit = 3.0 * sigma + 2.0;
    match method {
        CentroidMethod::CenterOfGravity => {
            let radius = 2.5 * sigma + 0.5;
            for _ in 0..4 {
                let (sum, sum_x, sum_y) = window_sums(frame, mesh, cx, cy, radius, |_| 1.0);
                if sum <= 0.0 {
                    break;
                }
                cx += sum_x / sum;
                cy += sum_y / sum;
            }
        }
        CentroidMethod::IterativeWeighted => {
            let radius = 3.0 * sigma + 1.0;
            let inv = 1.0 / (2.0 * sigma * sigma);
            for _ in 0..12 {
                let (sum, sum_x, sum_y) =
                    window_sums(frame, mesh, cx, cy, radius, |r2| exp(-r2 * inv));
                if sum <= 0.0 {
                    break;
                }
                // A Gaussian window as wide as the PSF halves the measured offset.
                let (step_x, step_y) = (2.0 * sum_x / sum, 2.0 * sum_y / sum);
                cx += step_x.clamp(-1.0, 1.0);
                cy += step_y.clamp(-1.0, 1.0);
                if step_x * step_x + step_y * step_y < 1e-8 {
                    break;
                }
            }
        }
    }
    let moved = crate::math::hypot(cx - start.0, cy - start.1);
    if moved.is_finite() && moved <= limit {
        (cx, cy)
    } else {
        start
    }
}

pub fn detect(frame: &Frame, config: &DetectConfig) -> Result<DetectOutput, AsteropeError> {
    if config.cell_size < 4
        || !(config.segment_sigma > 0.0)
        || !(config.peak_sigma >= config.segment_sigma)
        || !(config.max_elongation >= 1.0)
        || config.min_area == 0
    {
        return Err(AsteropeError::InvalidParameter);
    }
    let mesh = Mesh::build(frame, config.cell_size as usize);
    let mut runs = segment(frame, &mesh, config)?;

    for index in 0..runs.len() {
        let root = find_root(&mut runs, index as u32) as usize;
        if root == index {
            continue;
        }
        let Some(stats) = runs.get(index).map(|r| r.stats) else {
            continue;
        };
        if let Some(target) = runs.get_mut(root) {
            target.stats.merge(&stats);
        }
    }

    let mut output = DetectOutput {
        stats: DetectStats {
            background: Mesh::mean(&mesh.level),
            noise: Mesh::mean(&mesh.sigma),
            ..DetectStats::default()
        },
        ..DetectOutput::default()
    };
    let last_x = (frame.width - 1) as u32;
    let last_y = (frame.height - 1) as u32;

    for (index, run) in runs.iter().enumerate() {
        if run.parent as usize != index {
            continue;
        }
        let stats = &run.stats;
        output.stats.blobs += 1;
        let (bx, by) = stats.centroid();
        let reject = |output: &mut DetectOutput, reason: RejectReason| {
            output.rejected.push(Rejected {
                x: bx,
                y: by,
                area: stats.area,
                reason,
            });
        };

        if stats.peak_snr < config.peak_sigma {
            output.stats.noise_blobs += 1;
            continue;
        }
        if stats.area > config.max_area {
            output.stats.too_large += 1;
            reject(&mut output, RejectReason::TooLarge);
            continue;
        }
        if stats.area < config.min_area || stats.peak > config.max_sharpness * stats.sum {
            output.stats.hot_pixels += 1;
            reject(&mut output, RejectReason::HotPixel);
            continue;
        }
        let (major, minor) = stats.moments();
        let stretched = stats.area >= 5
            && sqrt((major + 1.0 / 12.0) / (minor + 1.0 / 12.0)) > config.max_elongation;
        // A bright star always lights pixels on both sides of any line through it.
        let collinear = minor < 0.02 && stats.peak_snr > 12.0;
        let trail = config.trails
            && stretched
            && !collinear
            && minor + 1.0 / 12.0 >= TRAIL_MIN_WIDTH_SQUARED;
        if (stretched && !trail) || collinear {
            output.stats.cosmic_rays += 1;
            reject(&mut output, RejectReason::CosmicRay);
            continue;
        }
        if stats.min_x == 0 || stats.min_y == 0 || stats.max_x >= last_x || stats.max_y >= last_y {
            output.stats.edge += 1;
            reject(&mut output, RejectReason::Edge);
            continue;
        }

        let (sigma, x, y, flux, aperture_pixels) = if trail {
            // The footprint above the threshold is narrower and shorter than
            // the trail, so the window is opened by the same three widths
            // a round star gets.
            let width = sqrt(minor + 1.0 / 12.0);
            let length = sqrt(12.0 * (major - minor).max(0.0));
            let (x, y, flux, pixels) = trail_centroid(
                frame,
                &mesh,
                (bx, by),
                stats.major_axis(),
                0.5 * length + 3.0 * width + 1.0,
                3.0 * width + 1.0,
            );
            (width, x, y, flux, pixels.max(1.0))
        } else {
            let sigma = estimate_sigma(stats, config.segment_sigma);
            let (x, y) = refine_centroid(frame, &mesh, (bx, by), sigma, config.method);
            let aperture = 3.0 * sigma + 1.0;
            let (flux, _, _) = window_sums(frame, &mesh, x, y, aperture, |_| 1.0);
            (
                sigma,
                x,
                y,
                flux,
                core::f64::consts::PI * aperture * aperture,
            )
        };
        let (_, noise) = mesh.eval(x, y);
        let shape = BlobShape {
            axis_ratio: sqrt((major + 1.0 / 12.0) / (minor + 1.0 / 12.0)),
            core: core_brightness(frame, &mesh, x, y) / stats.peak.max(1e-9),
        };
        output.detections.push(Detection {
            shape,
            x,
            y,
            flux,
            peak: stats.peak,
            snr: if noise > 0.0 {
                flux / (noise * sqrt(aperture_pixels))
            } else {
                0.0
            },
            area: stats.area,
            saturated: stats.saturated,
            sigma,
        });
    }

    output
        .detections
        .sort_by(|a, b| b.rank().total_cmp(&a.rank()));
    output.detections.truncate(config.max_detections);
    output.stats.stars = output.detections.len() as u32;
    Ok(output)
}
