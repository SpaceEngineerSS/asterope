//! Synthetic star-tracker frame generator.

use alloc::vec;
use alloc::vec::Vec;

use crate::camera::Camera;
use crate::catalog::Star;
use crate::error::AsteropeError;
use crate::math::{ceil, cos, erf, exp, floor, ln, pow, round, sin, sqrt, Mat3, Vec3, TAU};
use crate::rng::Rng;

/// A rate gyro: the true rate plus a bias that wanders and white noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gyro {
    pub noise: crate::filter::GyroNoise,
    /// Present bias, rad/s about the sensor axes.
    pub bias: Vec3,
}

impl Gyro {
    /// The reading over a step of `dt` seconds during which the true rate
    /// was `rate`: its mean over the step, as an integrating gyro reports.
    pub fn read(&mut self, rate: Vec3, dt: f64, rng: &mut Rng) -> Vec3 {
        let mut draw =
            |sigma: f64| Vec3::new(rng.normal(), rng.normal(), rng.normal()).scale(sigma);
        let before = self.bias;
        self.bias = self.bias + draw(self.noise.rate_random_walk * sqrt(dt));
        rate + (before + self.bias).scale(0.5) + draw(self.noise.angle_random_walk / sqrt(dt))
    }
}

/// Out-of-field glare from the Sun or Moon, modelled as a veiling gradient
/// that decays exponentially away from the sensor edge facing the source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrayLight {
    /// Electrons per pixel per second at the brightest edge.
    pub intensity: f64,
    /// Image-plane direction toward the source, radians from +X toward +Y.
    pub azimuth: f64,
    /// E-folding length as a fraction of the sensor diagonal.
    pub falloff: f64,
}

/// Departures of a real lens and detector from the ideal ones. Everything is
/// zero by default, which leaves the ideal sensor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Imperfections {
    /// Share of the natural cos^4 illumination falloff that is present: 0
    /// for a flat field, 1 for an uncorrected lens. Needs a camera, so it
    /// acts in `simulate` and not in `render_sources`.
    pub vignetting: f64,
    /// Photo-response non-uniformity: RMS of the fixed gain pattern, as a
    /// fraction of the signal.
    pub prnu: f64,
    /// Dark-signal non-uniformity: RMS of the fixed dark-current pattern,
    /// electrons per pixel per second.
    pub dsnu: f64,
    /// RMS of an offset common to a column, drawn anew for every frame,
    /// electrons.
    pub column_noise: f64,
    /// Growth of the PSF width toward the corner: the width there is
    /// `1 + psf_growth` times the central one, rising with the square of
    /// the field radius (defocus from field curvature).
    pub psf_growth: f64,
    /// Share of a corner star's light thrown into a flare 2.5 PSF widths
    /// farther from the center, rising with the square of the field radius.
    /// The asymmetry moves the centroid outward, as coma does.
    pub coma: f64,
    /// Time between the readout of consecutive rows, seconds; zero for a
    /// global shutter. With a body rate the rows see different attitudes.
    pub row_time: f64,
    /// Shortfall of the response at full well: a pixel holding `e`
    /// electrons reads `e (1 - nonlinearity * e / full_well)`.
    pub nonlinearity: f64,
    /// Share of the charge above full well that spills into the two
    /// neighbors in the column instead of being lost.
    pub blooming: f64,
    /// Seed of the fixed gain and dark-current patterns.
    pub pattern_seed: u64,
}

impl Default for Imperfections {
    fn default() -> Self {
        Imperfections {
            vignetting: 0.0,
            prnu: 0.0,
            dsnu: 0.0,
            column_noise: 0.0,
            psf_growth: 0.0,
            coma: 0.0,
            row_time: 0.0,
            nonlinearity: 0.0,
            blooming: 0.0,
            pattern_seed: 0x0A57_E209,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensorModel {
    /// Electrons per second collected from a magnitude-0 star.
    pub zero_point: f64,
    /// Exposure time in seconds.
    pub exposure: f64,
    /// Gaussian PSF standard deviation in pixels.
    pub psf_sigma: f64,
    /// Sky background, electrons per pixel per second.
    pub sky_background: f64,
    /// Dark current, electrons per pixel per second.
    pub dark_current: f64,
    /// Read noise, electrons RMS.
    pub read_noise: f64,
    /// Electrons per digital number.
    pub gain: f64,
    /// Offset added before quantization, in digital numbers.
    pub bias: f64,
    /// Pixel saturation level in electrons.
    pub full_well: f64,
    pub bit_depth: u32,
    /// Fraction of pixels with elevated dark current.
    pub hot_pixel_fraction: f64,
    /// Mean hot-pixel rate, electrons per second (exponentially distributed).
    pub hot_pixel_rate: f64,
    /// Seed of the fixed hot-pixel pattern; independent of the frame seed.
    pub hot_pixel_seed: u64,
    /// Mean number of cosmic-ray streaks per frame.
    pub cosmic_ray_mean: f64,
    pub stray_light: StrayLight,
    /// Disables photon shot noise and read noise when false.
    pub noise: bool,
    /// Angular rate of the sensor about its own axes during the exposure,
    /// radians per second. `simulate` takes the mid-exposure attitude.
    pub body_rate: Vec3,
    /// Colour term of the detector band: the instrumental magnitude is
    /// `V + color_term * (B - V)`. Zero reproduces the V band; silicon
    /// sensors without a filter sit near -0.3 (red stars look brighter).
    pub color_term: f64,
    pub imperfections: Imperfections,
}

/// B-V assumed for the few catalog stars without a measured colour.
const DEFAULT_B_V: f64 = 0.6;
/// Largest rotation during one exposure the trail model accepts, radians.
const MAX_EXPOSURE_ROTATION: f64 = 0.35;

impl Default for SensorModel {
    fn default() -> Self {
        SensorModel {
            zero_point: 2.0e6,
            exposure: 0.1,
            psf_sigma: 1.2,
            sky_background: 20.0,
            dark_current: 10.0,
            read_noise: 5.0,
            gain: 4.0,
            bias: 64.0,
            full_well: 16_000.0,
            bit_depth: 12,
            hot_pixel_fraction: 0.0,
            hot_pixel_rate: 5_000.0,
            hot_pixel_seed: 0x5EED_0407,
            cosmic_ray_mean: 0.0,
            stray_light: StrayLight {
                intensity: 0.0,
                azimuth: 0.0,
                falloff: 0.35,
            },
            noise: true,
            body_rate: Vec3::ZERO,
            color_term: 0.0,
            imperfections: Imperfections::default(),
        }
    }
}

impl SensorModel {
    pub fn max_dn(&self) -> u16 {
        let bits = self.bit_depth.clamp(8, 16);
        ((1u32 << bits) - 1) as u16
    }

    pub fn star_electrons(&self, mag: f64) -> f64 {
        self.zero_point * self.exposure * pow(10.0, -0.4 * mag)
    }

    /// Magnitude of a star in the detector band.
    pub fn instrumental_magnitude(&self, star: &Star) -> f64 {
        star.mag + self.color_term * star.b_v.unwrap_or(DEFAULT_B_V)
    }

    fn validate(&self) -> Result<(), AsteropeError> {
        let finite = [
            self.zero_point,
            self.exposure,
            self.psf_sigma,
            self.sky_background,
            self.dark_current,
            self.read_noise,
            self.gain,
            self.bias,
            self.full_well,
            self.hot_pixel_fraction,
            self.hot_pixel_rate,
            self.cosmic_ray_mean,
            self.stray_light.intensity,
            self.stray_light.azimuth,
            self.stray_light.falloff,
            self.color_term,
            self.imperfections.dsnu,
            self.imperfections.column_noise,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= -1e300)
            && self.body_rate.norm() * self.exposure <= MAX_EXPOSURE_ROTATION
            && crate::math::abs(self.color_term) <= 2.0;
        let positive = self.exposure > 0.0
            && self.psf_sigma >= 0.2
            && self.psf_sigma <= 20.0
            && self.gain > 0.0
            && self.full_well > 0.0
            && self.stray_light.falloff > 0.0;
        let non_negative = self.zero_point >= 0.0
            && self.sky_background >= 0.0
            && self.dark_current >= 0.0
            && self.read_noise >= 0.0
            && self.bias >= 0.0
            && (0.0..=0.2).contains(&self.hot_pixel_fraction)
            && self.hot_pixel_rate >= 0.0
            && (0.0..=1000.0).contains(&self.cosmic_ray_mean)
            && self.stray_light.intensity >= 0.0
            && self.imperfections.dsnu >= 0.0
            && self.imperfections.column_noise >= 0.0;
        let flaws = &self.imperfections;
        let bounded = (0.0..=1.0).contains(&flaws.vignetting)
            && (0.0..=0.2).contains(&flaws.prnu)
            && (0.0..=4.0).contains(&flaws.psf_growth)
            && (0.0..=0.5).contains(&flaws.coma)
            && (0.0..=1.0).contains(&flaws.row_time)
            && (0.0..=0.5).contains(&flaws.nonlinearity)
            && (0.0..=1.0).contains(&flaws.blooming);
        if finite && positive && non_negative && bounded {
            Ok(())
        } else {
            Err(AsteropeError::InvalidParameter)
        }
    }
}

/// Photon shot noise is sampled from the exact Poisson distribution below
/// this many expected electrons. Above it, shot and read noise are drawn
/// together from a normal distribution with the same variance; the Poisson
/// skewness there is under 0.1.
pub const GAUSSIAN_SHOT_NOISE_ABOVE: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSource {
    pub x: f64,
    pub y: f64,
    pub electrons: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TruthStar {
    pub catalog_index: u32,
    pub hip: u32,
    pub x: f64,
    pub y: f64,
    pub mag: f64,
    pub electrons: f64,
}

#[derive(Clone, Debug)]
pub struct SimFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u16>,
    pub truth: Vec<TruthStar>,
}

/// A point source with the width of its image.
#[derive(Clone, Copy)]
struct Blur {
    source: PointSource,
    sigma: f64,
}

fn add_psf(mean: &mut [f32], width: usize, height: usize, source: &PointSource, sigma: f64) {
    let reach = ceil(4.5 * sigma) + 1.0;
    let x_lo = floor(source.x - reach).max(0.0) as usize;
    let y_lo = floor(source.y - reach).max(0.0) as usize;
    let x_hi = (ceil(source.x + reach).max(0.0) as usize).min(width.saturating_sub(1));
    let y_hi = (ceil(source.y + reach).max(0.0) as usize).min(height.saturating_sub(1));
    if source.x + reach < 0.0 || source.y + reach < 0.0 || x_lo > x_hi || y_lo > y_hi {
        return;
    }
    let scale = 1.0 / (sigma * core::f64::consts::SQRT_2);
    let pixel_integral = |index: usize, center: f64| {
        let d = index as f64 - center;
        0.5 * (erf((d + 0.5) * scale) - erf((d - 0.5) * scale))
    };
    let weights_x: Vec<f64> = (x_lo..=x_hi).map(|x| pixel_integral(x, source.x)).collect();
    for y in y_lo..=y_hi {
        let wy = pixel_integral(y, source.y) * source.electrons;
        let start = y * width + x_lo;
        if let Some(row) = mean.get_mut(start..start + weights_x.len()) {
            for (pixel, wx) in row.iter_mut().zip(weights_x.iter()) {
                *pixel += (wx * wy) as f32;
            }
        }
    }
}

fn stray_light_profile(length: usize, direction: f64, scale: f64) -> Vec<f32> {
    let peak = if direction >= 0.0 {
        direction * (length.saturating_sub(1)) as f64
    } else {
        0.0
    };
    (0..length)
        .map(|i| exp((i as f64 * direction - peak) / scale) as f32)
        .collect()
}

fn add_cosmic_ray(extra: &mut [f32], width: usize, height: usize, rng: &mut Rng) {
    let mut x = rng.range(0.0, width as f64);
    let mut y = rng.range(0.0, height as f64);
    let angle = rng.range(0.0, TAU);
    let length = (2.0 - 6.0 * ln(1.0 - rng.uniform())).min(60.0);
    let per_pixel = rng.range(400.0, 4000.0);
    let (dx, dy) = (0.5 * cos(angle), 0.5 * sin(angle));
    let steps = ceil(length * 2.0) as usize;
    for _ in 0..steps {
        if x >= 0.0 && y >= 0.0 && (x as usize) < width && (y as usize) < height {
            if let Some(pixel) = extra.get_mut(y as usize * width + x as usize) {
                *pixel += (0.5 * per_pixel) as f32;
            }
        }
        x += dx;
        y += dy;
    }
}

/// Charge above full well moves along the column: of the spilled share,
/// half goes to each neighbor, which may overflow in turn. A few rounds
/// carry a saturated star's excess several pixels up and down.
fn bloom(electrons: &mut [f32], width: usize, full_well: f32, share: f32) {
    const ROUNDS: usize = 256;
    let height = electrons.len() / width.max(1);
    let mut spill = vec![0.0f32; height];
    for column in 0..width {
        for _ in 0..ROUNDS {
            let mut any = false;
            for (row, slot) in spill.iter_mut().enumerate() {
                let held = electrons.get(row * width + column).copied().unwrap_or(0.0);
                *slot = share * (held - full_well).max(0.0);
                any |= *slot > 0.5;
            }
            if !any {
                break;
            }
            for row in 0..height {
                let above = row
                    .checked_sub(1)
                    .and_then(|r| spill.get(r))
                    .copied()
                    .unwrap_or(0.0);
                let below = spill.get(row + 1).copied().unwrap_or(0.0);
                let own = spill.get(row).copied().unwrap_or(0.0);
                if let Some(pixel) = electrons.get_mut(row * width + column) {
                    *pixel += 0.5 * (above + below) - own;
                }
            }
        }
    }
}

/// Renders point sources at given pixel positions through the sensor model.
pub fn render_sources(
    width: u32,
    height: u32,
    sources: &[PointSource],
    sensor: &SensorModel,
    seed: u64,
) -> Result<Vec<u16>, AsteropeError> {
    let blurred: Vec<Blur> = sources
        .iter()
        .map(|source| Blur {
            source: *source,
            sigma: sensor.psf_sigma,
        })
        .collect();
    render(width, height, &blurred, sensor, seed, None)
}

fn render(
    width: u32,
    height: u32,
    sources: &[Blur],
    sensor: &SensorModel,
    seed: u64,
    camera: Option<&Camera>,
) -> Result<Vec<u16>, AsteropeError> {
    sensor.validate()?;
    let flaws = &sensor.imperfections;
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return Err(AsteropeError::InvalidFrame);
    }
    let t = sensor.exposure;
    let base = ((sensor.sky_background + sensor.dark_current) * t) as f32;
    let mut mean = vec![base; w * h];

    if sensor.stray_light.intensity > 0.0 {
        let diagonal = crate::math::hypot(w as f64, h as f64);
        let scale = diagonal * sensor.stray_light.falloff;
        let cols = stray_light_profile(w, cos(sensor.stray_light.azimuth), scale);
        let rows = stray_light_profile(h, sin(sensor.stray_light.azimuth), scale);
        let amplitude = (sensor.stray_light.intensity * t) as f32;
        const VEILING_FLOOR: f32 = 0.1;
        for (row, row_factor) in mean.chunks_exact_mut(w).zip(rows.iter()) {
            for (pixel, col_factor) in row.iter_mut().zip(cols.iter()) {
                *pixel +=
                    amplitude * (VEILING_FLOOR + (1.0 - VEILING_FLOOR) * row_factor * col_factor);
            }
        }
    }

    for Blur { source, sigma } in sources {
        if source.electrons > 0.0 && source.x.is_finite() && source.y.is_finite() {
            add_psf(&mut mean, w, h, source, *sigma);
        }
    }

    // Light is scaled by the lens and by each pixel's own gain; the dark
    // current underneath is not.
    let dark = (sensor.dark_current * t) as f32;
    if let (Some(camera), true) = (camera, flaws.vignetting > 0.0) {
        for (y, row) in mean.chunks_exact_mut(w).enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                let cosine = camera.unproject(x as f64, y as f64).z.clamp(0.0, 1.0);
                let falloff = 1.0 - flaws.vignetting * (1.0 - pow(cosine, 4.0));
                *pixel = dark + (*pixel - dark) * falloff as f32;
            }
        }
    }
    if flaws.prnu > 0.0 {
        let mut pattern = Rng::new(flaws.pattern_seed);
        for pixel in mean.iter_mut() {
            let gain = (1.0 + flaws.prnu * pattern.normal()).max(0.0);
            *pixel = dark + (*pixel - dark) * gain as f32;
        }
    }
    if flaws.dsnu > 0.0 {
        let mut pattern = Rng::new(flaws.pattern_seed ^ 0x9E37_79B9_7F4A_7C15);
        for pixel in mean.iter_mut() {
            *pixel += (flaws.dsnu * t * crate::math::abs(pattern.normal())) as f32;
        }
    }

    if sensor.hot_pixel_fraction > 0.0 {
        let mut pattern = Rng::new(sensor.hot_pixel_seed);
        let count = round(sensor.hot_pixel_fraction * (w * h) as f64) as usize;
        for _ in 0..count {
            let index = pattern.below((w * h) as u32) as usize;
            let rate = -ln(1.0 - pattern.uniform()) * sensor.hot_pixel_rate;
            if let Some(pixel) = mean.get_mut(index) {
                *pixel += (rate * t) as f32;
            }
        }
    }

    let mut rng = Rng::new(seed);
    if sensor.noise {
        let read_variance = sensor.read_noise * sensor.read_noise;
        for pixel in mean.iter_mut() {
            let expected = f64::from(*pixel);
            let electrons = if expected >= GAUSSIAN_SHOT_NOISE_ABOVE {
                expected + sqrt(expected + read_variance) * rng.normal()
            } else {
                rng.poisson(expected) + sensor.read_noise * rng.normal()
            };
            *pixel = electrons as f32;
        }
    }

    if sensor.cosmic_ray_mean > 0.0 {
        let count = rng.poisson(sensor.cosmic_ray_mean) as usize;
        for _ in 0..count {
            add_cosmic_ray(&mut mean, w, h, &mut rng);
        }
    }

    if flaws.column_noise > 0.0 {
        let offsets: Vec<f32> = (0..w)
            .map(|_| (flaws.column_noise * rng.normal()) as f32)
            .collect();
        for row in mean.chunks_exact_mut(w) {
            for (pixel, offset) in row.iter_mut().zip(offsets.iter()) {
                *pixel += offset;
            }
        }
    }
    if flaws.blooming > 0.0 {
        bloom(&mut mean, w, sensor.full_well as f32, flaws.blooming as f32);
    }

    let max_dn = f64::from(sensor.max_dn());
    let pixels = mean
        .iter()
        .map(|electrons| {
            let held = f64::from(*electrons).min(sensor.full_well);
            let clipped = held * (1.0 - flaws.nonlinearity * held.max(0.0) / sensor.full_well);
            let dn = round(sensor.bias + clipped / sensor.gain);
            dn.clamp(0.0, max_dn) as u16
        })
        .collect();
    Ok(pixels)
}

/// Renders the sky seen by `camera` at `attitude` (ICRS -> sensor).
pub fn simulate(
    stars: &[Star],
    attitude: &Mat3,
    camera: &Camera,
    sensor: &SensorModel,
    seed: u64,
) -> Result<SimFrame, AsteropeError> {
    sensor.validate()?;
    let boresight = attitude.r2;
    let smear = 0.75 * sensor.body_rate.norm() * sensor.exposure / camera.pixel_scale();
    let reach = 4.5 * sensor.psf_sigma + 2.0 + smear;
    let cos_limit = cos((camera.half_diagonal() + reach * camera.pixel_scale()).min(1.5));

    let flaws = &sensor.imperfections;
    let corner = crate::math::hypot(
        (camera.cx).max(f64::from(camera.width) - camera.cx),
        (camera.cy).max(f64::from(camera.height) - camera.cy),
    );
    let mut sources: Vec<Blur> = Vec::new();
    let mut truth = Vec::new();
    for (index, star) in stars.iter().enumerate() {
        if star.dir.dot(boresight) < cos_limit {
            continue;
        }
        let mut body = attitude.mul_vec(star.dir);
        let Some((mut x, mut y)) = camera.project(body) else {
            continue;
        };
        if flaws.row_time > 0.0 {
            // A row is read `row_time` after the one above it, and in the
            // sensor frame a star moves at `-rate x body`. Two rounds settle
            // which row the star is on when that row is read.
            let still = body;
            for _ in 0..2 {
                let delay = (y - 0.5 * (f64::from(camera.height) - 1.0)) * flaws.row_time;
                let moved = still - sensor.body_rate.cross(still).scale(delay);
                let Some(direction) = moved.normalized() else {
                    break;
                };
                let Some(position) = camera.project(direction) else {
                    break;
                };
                body = direction;
                (x, y) = position;
            }
        }
        if !camera.contains(x, y, -reach) {
            continue;
        }
        let radius = crate::math::hypot(x - camera.cx, y - camera.cy) / corner.max(1.0);
        let sigma = sensor.psf_sigma * (1.0 + flaws.psf_growth * radius * radius);
        let flare = (flaws.coma * radius * radius).min(0.5);
        let electrons = sensor.star_electrons(sensor.instrumental_magnitude(star));
        // Stars drift across the sensor at `-rate x body`; the exposure is
        // centered on the given attitude, so the trail is centered on (x, y).
        let drift = sensor.body_rate.cross(body).scale(0.5 * sensor.exposure);
        let ends = (
            (body + drift).normalized().and_then(|b| camera.project(b)),
            (body - drift).normalized().and_then(|b| camera.project(b)),
        );
        let trail = match ends {
            (Some(start), Some(end)) => {
                let length = crate::math::hypot(end.0 - start.0, end.1 - start.1);
                (length >= 0.25).then_some((start, end, length))
            }
            _ => None,
        };
        let outward = if radius > 0.0 {
            let reach = 2.5 * sigma / (radius * corner);
            ((x - camera.cx) * reach, (y - camera.cy) * reach)
        } else {
            (0.0, 0.0)
        };
        let mut place = |x: f64, y: f64, electrons: f64| {
            sources.push(Blur {
                source: PointSource {
                    x,
                    y,
                    electrons: electrons * (1.0 - flare),
                },
                sigma,
            });
            if flare > 0.0 {
                sources.push(Blur {
                    source: PointSource {
                        x: x + outward.0,
                        y: y + outward.1,
                        electrons: electrons * flare,
                    },
                    sigma: 1.6 * sigma,
                });
            }
        };
        match trail {
            None => place(x, y, electrons),
            Some((start, end, length)) => {
                let samples = ceil(2.0 * length / sensor.psf_sigma).clamp(2.0, 96.0);
                let count = samples as usize;
                for k in 0..count {
                    let t = (k as f64 + 0.5) / samples;
                    place(
                        start.0 + (end.0 - start.0) * t,
                        start.1 + (end.1 - start.1) * t,
                        electrons / samples,
                    );
                }
            }
        }
        if camera.contains(x, y, 0.0) {
            truth.push(TruthStar {
                catalog_index: index as u32,
                hip: star.hip,
                x,
                y,
                mag: star.mag,
                electrons,
            });
        }
    }

    let pixels = render(
        camera.width,
        camera.height,
        &sources,
        sensor,
        seed,
        Some(camera),
    )?;
    Ok(SimFrame {
        width: camera.width,
        height: camera.height,
        pixels,
        truth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_sensor() -> SensorModel {
        SensorModel {
            noise: false,
            ..SensorModel::default()
        }
    }

    #[test]
    fn psf_conserves_flux() {
        let sensor = SensorModel {
            sky_background: 0.0,
            dark_current: 0.0,
            ..quiet_sensor()
        };
        let source = PointSource {
            x: 31.37,
            y: 30.81,
            electrons: 8000.0,
        };
        let pixels = render_sources(64, 64, &[source], &sensor, 1).unwrap();
        let total: f64 = pixels.iter().map(|p| f64::from(*p) - sensor.bias).sum();
        let ratio = total * sensor.gain / 8000.0;
        assert!((ratio - 1.0).abs() < 0.02, "flux ratio {ratio}");
    }

    #[test]
    fn same_seed_gives_identical_frames() {
        let sensor = SensorModel {
            cosmic_ray_mean: 3.0,
            hot_pixel_fraction: 1e-3,
            ..SensorModel::default()
        };
        let source = PointSource {
            x: 20.0,
            y: 20.0,
            electrons: 5000.0,
        };
        let a = render_sources(48, 48, &[source], &sensor, 99).unwrap();
        let b = render_sources(48, 48, &[source], &sensor, 99).unwrap();
        let c = render_sources(48, 48, &[source], &sensor, 100).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn background_noise_matches_model() {
        let sensor = SensorModel::default();
        let pixels = render_sources(256, 256, &[], &sensor, 5).unwrap();
        let n = pixels.len() as f64;
        let mean = pixels.iter().map(|p| f64::from(*p)).sum::<f64>() / n;
        let var = pixels
            .iter()
            .map(|p| (f64::from(*p) - mean).powi(2))
            .sum::<f64>()
            / n;
        let electrons = (sensor.sky_background + sensor.dark_current) * sensor.exposure;
        let expected_mean = sensor.bias + electrons / sensor.gain;
        let expected_var = (electrons + sensor.read_noise * sensor.read_noise)
            / (sensor.gain * sensor.gain)
            + 1.0 / 12.0;
        assert!(
            (mean - expected_mean).abs() < 0.05,
            "mean {mean} vs {expected_mean}"
        );
        assert!(
            (var / expected_var - 1.0).abs() < 0.05,
            "var {var} vs {expected_var}"
        );
    }

    #[test]
    fn stray_light_is_brightest_toward_the_source() {
        let sensor = SensorModel {
            stray_light: StrayLight {
                intensity: 4000.0,
                azimuth: 0.0,
                falloff: 0.3,
            },
            ..quiet_sensor()
        };
        let pixels = render_sources(64, 64, &[], &sensor, 1).unwrap();
        let left = pixels[32 * 64];
        let right = pixels[32 * 64 + 63];
        assert!(right > left + 20, "left {left} right {right}");
    }

    fn one_star(mag: f64, b_v: Option<f64>) -> Vec<Star> {
        vec![Star {
            hip: 1,
            dir: Vec3::Z,
            pm_ra: 0.0,
            pm_dec: 0.0,
            mag,
            b_v,
        }]
    }

    fn moments(pixels: &[u16], width: usize, bias: f64) -> (f64, f64, f64, f64) {
        let (mut sum, mut sx, mut sy) = (0.0, 0.0, 0.0);
        for (i, p) in pixels.iter().enumerate() {
            let v = f64::from(*p) - bias;
            sum += v;
            sx += v * (i % width) as f64;
            sy += v * (i / width) as f64;
        }
        let (cx, cy) = (sx / sum, sy / sum);
        let mut sxx = 0.0;
        for (i, p) in pixels.iter().enumerate() {
            let dx = (i % width) as f64 - cx;
            sxx += (f64::from(*p) - bias) * dx * dx;
        }
        (sum, cx, cy, sxx / sum)
    }

    #[test]
    fn body_rate_smears_stars_without_losing_flux() {
        let camera = Camera::from_fov(128, 128, 0.2).unwrap();
        let still = SensorModel {
            sky_background: 0.0,
            dark_current: 0.0,
            ..quiet_sensor()
        };
        // A rate about +Y moves stars along image x; 12 px over the exposure.
        let rate = 12.0 * camera.pixel_scale() / still.exposure;
        let moving = SensorModel {
            body_rate: Vec3::new(0.0, rate, 0.0),
            ..still
        };
        let stars = one_star(1.0, None);
        let a = simulate(&stars, &Mat3::IDENTITY, &camera, &still, 1).unwrap();
        let b = simulate(&stars, &Mat3::IDENTITY, &camera, &moving, 1).unwrap();
        let (flux_a, cx_a, _, var_a) = moments(&a.pixels, 128, still.bias);
        let (flux_b, cx_b, _, var_b) = moments(&b.pixels, 128, still.bias);
        assert!(
            (flux_b / flux_a - 1.0).abs() < 0.02,
            "flux ratio {}",
            flux_b / flux_a
        );
        assert!(
            (cx_b - cx_a).abs() < 0.05,
            "trail is centered: {cx_a} vs {cx_b}"
        );
        // A uniform trail of length L adds L^2 / 12 to the variance.
        assert!(
            (var_b - var_a - 12.0).abs() < 1.0,
            "variance {var_a} -> {var_b}"
        );
        assert_eq!(a.truth[0].x, b.truth[0].x);
    }

    #[test]
    fn color_term_brightens_red_stars() {
        let camera = Camera::from_fov(64, 64, 0.2).unwrap();
        let sensor = SensorModel {
            color_term: -0.3,
            ..quiet_sensor()
        };
        let red = simulate(
            &one_star(4.0, Some(1.5)),
            &Mat3::IDENTITY,
            &camera,
            &sensor,
            1,
        )
        .unwrap();
        let blue = simulate(
            &one_star(4.0, Some(-0.2)),
            &Mat3::IDENTITY,
            &camera,
            &sensor,
            1,
        )
        .unwrap();
        let ratio = red.truth[0].electrons / blue.truth[0].electrons;
        let expected = 10f64.powf(-0.4 * -0.3 * (1.5 + 0.2));
        assert!((ratio / expected - 1.0).abs() < 1e-12);
    }

    fn star_at(camera: &Camera, x: f64, y: f64, mag: f64) -> Vec<Star> {
        vec![Star {
            hip: 1,
            dir: camera.unproject(x, y),
            pm_ra: 0.0,
            pm_dec: 0.0,
            mag,
            b_v: None,
        }]
    }

    fn dark_sensor(imperfections: Imperfections) -> SensorModel {
        SensorModel {
            sky_background: 0.0,
            dark_current: 0.0,
            imperfections,
            ..quiet_sensor()
        }
    }

    #[test]
    fn vignetting_follows_the_fourth_power_of_the_cosine() {
        let camera = Camera::from_fov(128, 128, 1.0).unwrap();
        let sensor = SensorModel {
            sky_background: 20_000.0,
            dark_current: 0.0,
            imperfections: Imperfections {
                vignetting: 1.0,
                ..Imperfections::default()
            },
            ..quiet_sensor()
        };
        let frame = simulate(&[], &Mat3::IDENTITY, &camera, &sensor, 1).unwrap();
        let level = |x: usize, y: usize| f64::from(frame.pixels[y * 128 + x]) - sensor.bias;
        let cosine = camera.unproject(2.0, 2.0).z;
        let ratio = level(2, 2) / level(64, 64);
        assert!(
            (ratio - cosine.powi(4)).abs() < 0.01,
            "{ratio} against {}",
            cosine.powi(4)
        );
        assert!(cosine.powi(4) < 0.7);
    }

    #[test]
    fn fixed_patterns_have_the_stated_spread_and_repeat() {
        let flat = SensorModel {
            sky_background: 20_000.0,
            dark_current: 0.0,
            ..quiet_sensor()
        };
        let uneven = SensorModel {
            imperfections: Imperfections {
                prnu: 0.02,
                ..Imperfections::default()
            },
            ..flat
        };
        let spread = |sensor: &SensorModel, seed: u64| {
            let pixels = render_sources(128, 128, &[], sensor, seed).unwrap();
            let n = pixels.len() as f64;
            let mean = pixels.iter().map(|p| f64::from(*p)).sum::<f64>() / n;
            let var = pixels
                .iter()
                .map(|p| (f64::from(*p) - mean).powi(2))
                .sum::<f64>()
                / n;
            (var.sqrt() / (mean - sensor.bias), pixels)
        };
        let (relative, first) = spread(&uneven, 1);
        assert!((relative - 0.02).abs() < 0.002, "{relative}");
        assert_eq!(first, spread(&uneven, 2).1);
        assert!(spread(&flat, 1).0 < 0.001);

        let warm = SensorModel {
            sky_background: 0.0,
            dark_current: 0.0,
            exposure: 1.0,
            imperfections: Imperfections {
                dsnu: 400.0,
                ..Imperfections::default()
            },
            ..quiet_sensor()
        };
        let pixels = render_sources(128, 128, &[], &warm, 1).unwrap();
        let mean = pixels
            .iter()
            .map(|p| f64::from(*p) - warm.bias)
            .sum::<f64>()
            / pixels.len() as f64;
        // The mean of the absolute value of a normal variable is sqrt(2 / pi) sigma.
        let expected = 400.0 * (2.0 / core::f64::consts::PI).sqrt() / warm.gain;
        assert!(
            (mean / expected - 1.0).abs() < 0.03,
            "{mean} against {expected}"
        );
    }

    #[test]
    fn field_aberrations_widen_and_shift_a_corner_star() {
        let camera = Camera::from_fov(128, 128, 0.2).unwrap();
        let stars = star_at(&camera, 100.0, 100.0, 1.0);
        let image = |imperfections: Imperfections| {
            let sensor = dark_sensor(imperfections);
            let frame = simulate(&stars, &Mat3::IDENTITY, &camera, &sensor, 1).unwrap();
            moments(&frame.pixels, 128, sensor.bias)
        };
        let ideal = image(Imperfections::default());
        let soft = image(Imperfections {
            psf_growth: 1.0,
            ..Imperfections::default()
        });
        let radius = (36.5f64 * 36.5 * 2.0).sqrt() / (64.0f64 * 64.0 * 2.0).sqrt();
        let widening = (1.0 + radius * radius).powi(2);
        assert!(
            (soft.3 / ideal.3 / widening - 1.0).abs() < 0.08,
            "{} against {widening}",
            soft.3 / ideal.3
        );
        assert!((soft.0 / ideal.0 - 1.0).abs() < 0.01);

        let comatic = image(Imperfections {
            coma: 0.2,
            ..Imperfections::default()
        });
        let flare = 0.2 * radius * radius;
        let expected = flare * 2.5 * 1.2 / 2.0f64.sqrt();
        assert!(
            (comatic.1 - ideal.1 - expected).abs() < 0.03,
            "{} against {expected}",
            comatic.1 - ideal.1
        );
        assert!((comatic.2 - ideal.2 - expected).abs() < 0.03);
        assert!((comatic.0 / ideal.0 - 1.0).abs() < 0.01);
    }

    #[test]
    fn rolling_shutter_shifts_rows_by_their_readout_delay() {
        let camera = Camera::from_fov(128, 128, 0.2).unwrap();
        let rate = 1000.0 * camera.pixel_scale();
        let sensor = SensorModel {
            body_rate: Vec3::new(0.0, rate, 0.0),
            exposure: 1e-4,
            zero_point: 2.0e9,
            imperfections: Imperfections {
                row_time: 1e-4,
                ..Imperfections::default()
            },
            ..dark_sensor(Imperfections::default())
        };
        let shift_at = |row: f64| {
            let frame = simulate(
                &star_at(&camera, 64.0, row, 1.0),
                &Mat3::IDENTITY,
                &camera,
                &sensor,
                1,
            )
            .unwrap();
            frame.truth[0].x - 64.0
        };
        // 1000 px/s across the sensor and 0.1 ms a row: 0.1 px a row, with
        // the sign of a star moving toward +x under a rate about +y.
        let (top, middle, bottom) = (shift_at(13.5), shift_at(63.5), shift_at(113.5));
        assert!(middle.abs() < 1e-6, "{middle}");
        assert!(
            (bottom - top).abs() > 9.0 && (bottom - top).abs() < 11.0,
            "{top} {bottom}"
        );
        assert!((bottom + top).abs() < 0.05);
        let moving = simulate(
            &star_at(&camera, 64.0, 113.5, 1.0),
            &Mat3::IDENTITY,
            &camera,
            &sensor,
            1,
        )
        .unwrap();
        let (_, cx, _, _) = moments(&moving.pixels, 128, sensor.bias);
        assert!((cx - moving.truth[0].x).abs() < 0.1);
    }

    #[test]
    fn response_falls_short_near_full_well_and_charge_blooms() {
        let source = PointSource {
            x: 32.0,
            y: 32.0,
            electrons: 1_000_000.0,
        };
        let ideal = dark_sensor(Imperfections::default());
        let compressed = dark_sensor(Imperfections {
            nonlinearity: 0.1,
            ..Imperfections::default()
        });
        let peak = |sensor: &SensorModel| {
            let pixels = render_sources(64, 64, &[source], sensor, 1).unwrap();
            f64::from(pixels[32 * 64 + 32]) - sensor.bias
        };
        // The central pixel is saturated; at full well the response is 10 % short.
        assert!((peak(&compressed) / peak(&ideal) - 0.9).abs() < 0.005);

        let blooming = dark_sensor(Imperfections {
            blooming: 1.0,
            ..Imperfections::default()
        });
        let total = |sensor: &SensorModel| {
            let pixels = render_sources(64, 64, &[source], sensor, 1).unwrap();
            let column: f64 = (0..64)
                .map(|y| f64::from(pixels[y * 64 + 32]) - sensor.bias)
                .sum();
            let all: f64 = pixels.iter().map(|p| f64::from(*p) - sensor.bias).sum();
            (column, all)
        };
        let (clipped_column, clipped_all) = total(&ideal);
        let (bloomed_column, bloomed_all) = total(&blooming);
        assert!(
            bloomed_all > clipped_all * 1.2,
            "{clipped_all} -> {bloomed_all}"
        );
        // The charge runs along the columns that overflowed and nowhere else.
        assert!(bloomed_column > 2.0 * clipped_column);
        let far = |sensor: &SensorModel, x: usize| {
            let pixels = render_sources(64, 64, &[source], sensor, 1).unwrap();
            f64::from(pixels[39 * 64 + x]) - sensor.bias
        };
        assert!(
            far(&ideal, 32) < 2.0 && far(&blooming, 32) > 1000.0,
            "{} {} {clipped_all} {bloomed_all}",
            far(&ideal, 32),
            far(&blooming, 32)
        );
        assert!(far(&blooming, 44) < 2.0);
        assert!((bloomed_all * ideal.gain / 1_000_000.0 - 1.0).abs() < 0.05);
    }

    #[test]
    fn rejects_invalid_sensor() {
        let sensor = SensorModel {
            gain: 0.0,
            ..SensorModel::default()
        };
        assert!(render_sources(32, 32, &[], &sensor, 1).is_err());
    }
}
