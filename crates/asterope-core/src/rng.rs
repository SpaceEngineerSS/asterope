use alloc::boxed::Box;

use crate::math::{abs, exp, floor, lgamma, ln, sqrt, Quat};

/// xoshiro256** seeded through splitmix64. Fully deterministic across targets.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
    ziggurat: Option<Box<Ziggurat>>,
    poisson: PoissonTable,
}

/// Covers the mass of every mean below 10 to better than 1e-12.
const POISSON_TABLE_LEN: usize = 48;
const POISSON_GUIDE_LEN: usize = 64;

/// Cumulative distribution of the most recent small Poisson mean, with a
/// guide table that points each uniform deviate near its answer.
#[derive(Clone, Debug)]
struct PoissonTable {
    lambda: f64,
    ready: bool,
    cumulative: [f64; POISSON_TABLE_LEN],
    guide: [u8; POISSON_GUIDE_LEN],
}

impl PoissonTable {
    fn build(&mut self) {
        let mut term = exp(-self.lambda);
        let mut total = term;
        for (k, slot) in self.cumulative.iter_mut().enumerate() {
            *slot = total;
            term *= self.lambda / (k + 1) as f64;
            total += term;
        }
        let mut k = 0usize;
        for (j, slot) in self.guide.iter_mut().enumerate() {
            let level = j as f64 / POISSON_GUIDE_LEN as f64;
            while self.cumulative.get(k).is_some_and(|c| *c < level) {
                k += 1;
            }
            *slot = k as u8;
        }
        self.ready = true;
    }

    fn sample(&self, u: f64) -> f64 {
        let bucket = (u * POISSON_GUIDE_LEN as f64) as usize;
        let mut k = usize::from(self.guide.get(bucket).copied().unwrap_or(0));
        while self.cumulative.get(k).is_some_and(|c| u > *c) {
            k += 1;
        }
        k as f64
    }
}

const ZIGGURAT_LAYERS: usize = 128;
const ZIGGURAT_R: f64 = 3.442_619_855_899;
const ZIGGURAT_V: f64 = 9.912_563_035_262_17e-3;

/// Layer tables of the ziggurat method for the normal distribution
/// (Marsaglia & Tsang 2000, in Doornik's ZIGNOR formulation).
#[derive(Clone, Debug)]
struct Ziggurat {
    x: [f64; ZIGGURAT_LAYERS + 1],
    ratio: [f64; ZIGGURAT_LAYERS],
}

impl Ziggurat {
    fn new() -> Ziggurat {
        let mut x = [0.0; ZIGGURAT_LAYERS + 1];
        let mut f = exp(-0.5 * ZIGGURAT_R * ZIGGURAT_R);
        let mut previous = ZIGGURAT_R;
        for (i, slot) in x.iter_mut().enumerate() {
            *slot = match i {
                0 => ZIGGURAT_V / f,
                1 => ZIGGURAT_R,
                ZIGGURAT_LAYERS => 0.0,
                _ => {
                    let edge = sqrt(-2.0 * ln(ZIGGURAT_V / previous + f));
                    f = exp(-0.5 * edge * edge);
                    previous = edge;
                    edge
                }
            };
        }
        let mut ratio = [0.0; ZIGGURAT_LAYERS];
        for (slot, pair) in ratio.iter_mut().zip(x.windows(2)) {
            if let [lower, upper] = pair {
                *slot = upper / lower;
            }
        }
        Ziggurat { x, ratio }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derives an independent stream seed, e.g. one per Monte Carlo trial.
pub fn derive_seed(master: u64, stream: u64) -> u64 {
    let mut state = master ^ stream.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    let a = splitmix64(&mut state);
    let b = splitmix64(&mut state);
    a ^ b.rotate_left(32)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        let s = [
            splitmix64(&mut state),
            splitmix64(&mut state),
            splitmix64(&mut state),
            splitmix64(&mut state),
        ];
        Self {
            s,
            ziggurat: None,
            poisson: PoissonTable {
                lambda: f64::NAN,
                ready: false,
                cumulative: [1.0; POISSON_TABLE_LEN],
                guide: [0; POISSON_GUIDE_LEN],
            },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.s;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s1 << 17;
        let s2 = s2 ^ s0;
        let s3 = s3 ^ s1;
        let s1 = s1 ^ s2;
        let s0 = s0 ^ s3;
        let s2 = s2 ^ t;
        let s3 = s3.rotate_left(45);
        self.s = [s0, s1, s2, s3];
        result
    }

    /// Uniform in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// Uniform integer in `[0, n)`; returns 0 when `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        (self.uniform() * f64::from(n)) as u32
    }

    /// Uniform in `(0, 1]`, safe as a logarithm argument.
    fn uniform_open(&mut self) -> f64 {
        1.0 - self.uniform()
    }

    /// Standard normal deviate (ziggurat method).
    pub fn normal(&mut self) -> f64 {
        let tables = self
            .ziggurat
            .take()
            .unwrap_or_else(|| Box::new(Ziggurat::new()));
        let value = self.normal_with(&tables);
        self.ziggurat = Some(tables);
        value
    }

    fn normal_with(&mut self, tables: &Ziggurat) -> f64 {
        loop {
            let bits = self.next_u64();
            let layer = (bits & 0x7F) as usize;
            let u = 2.0 * ((bits >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)) - 1.0;
            let (Some(width), Some(ratio)) = (tables.x.get(layer), tables.ratio.get(layer)) else {
                continue;
            };
            if abs(u) < *ratio {
                return u * width;
            }
            if layer == 0 {
                // Base strip: sample the tail beyond R (Marsaglia 1964).
                loop {
                    let x = ln(self.uniform_open()) / ZIGGURAT_R;
                    let y = ln(self.uniform_open());
                    if -2.0 * y >= x * x {
                        return if u < 0.0 {
                            x - ZIGGURAT_R
                        } else {
                            ZIGGURAT_R - x
                        };
                    }
                }
            }
            let x = u * width;
            let inner = tables.x.get(layer + 1).copied().unwrap_or(0.0);
            let f0 = exp(-0.5 * (width * width - x * x));
            let f1 = exp(-0.5 * (inner * inner - x * x));
            if f1 + self.uniform() * (f0 - f1) < 1.0 {
                return x;
            }
        }
    }

    pub fn poisson(&mut self, lambda: f64) -> f64 {
        if lambda.is_nan() || lambda <= 0.0 {
            0.0
        } else if lambda < 10.0 {
            self.poisson_inversion(lambda)
        } else if lambda < 1.0e7 {
            self.poisson_ptrs(lambda)
        } else {
            floor(lambda + sqrt(lambda) * self.normal() + 0.5)
        }
    }

    /// Inversion of the cumulative distribution; one uniform deviate per
    /// sample. A mean seen twice in a row (background pixels) gets a guided
    /// lookup table; other means are searched term by term. Both paths
    /// evaluate the same recurrence, so they return identical counts.
    fn poisson_inversion(&mut self, lambda: f64) -> f64 {
        let u = self.uniform();
        if lambda == self.poisson.lambda {
            if !self.poisson.ready {
                self.poisson.build();
            }
            return self.poisson.sample(u);
        }
        self.poisson.lambda = lambda;
        self.poisson.ready = false;
        let mut k = 0usize;
        let mut term = exp(-lambda);
        let mut cumulative = term;
        while u > cumulative && k < POISSON_TABLE_LEN {
            k += 1;
            term *= lambda / k as f64;
            cumulative += term;
        }
        k as f64
    }

    #[cfg(test)]
    fn poisson_inversion_reset(&mut self) {
        self.poisson.lambda = f64::NAN;
        self.poisson.ready = false;
    }

    /// Hörmann (1993) transformed rejection with squeeze (PTRS).
    fn poisson_ptrs(&mut self, lambda: f64) -> f64 {
        let slam = sqrt(lambda);
        let b = 0.931 + 2.53 * slam;
        let a = -0.059 + 0.02483 * b;
        let inv_alpha = 1.1239 + 1.1328 / (b - 3.4);
        let vr = 0.9277 - 3.6224 / (b - 2.0);
        loop {
            let u = self.uniform() - 0.5;
            let v = self.uniform();
            let us = 0.5 - abs(u);
            let k = floor((2.0 * a / us + b) * u + lambda + 0.43);
            if us >= 0.07 && v <= vr {
                return k;
            }
            if k < 0.0 || (us < 0.013 && v > us) {
                continue;
            }
            if ln(v) + ln(inv_alpha) - ln(a / (us * us) + b)
                <= -lambda + k * ln(lambda) - lgamma(k + 1.0)
            {
                return k;
            }
        }
    }

    /// Uniformly distributed attitude quaternion (Shoemake's subgroup algorithm).
    pub fn unit_quat(&mut self) -> Quat {
        let u1 = self.uniform();
        let u2 = self.uniform() * crate::math::TAU;
        let u3 = self.uniform() * crate::math::TAU;
        let a = sqrt(1.0 - u1);
        let b = sqrt(u1);
        Quat::new(
            a * crate::math::sin(u2),
            a * crate::math::cos(u2),
            b * crate::math::sin(u3),
            b * crate::math::cos(u3),
        )
        .canonical()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_reproducible() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn normal_moments_and_tails() {
        let mut rng = Rng::new(7);
        let n = 2_000_000;
        let (mut sum, mut sum_sq, mut sum_4) = (0.0, 0.0, 0.0);
        let (mut beyond_1, mut beyond_3, mut beyond_r, mut positive) = (0u32, 0u32, 0u32, 0u32);
        for _ in 0..n {
            let z = rng.normal();
            sum += z;
            sum_sq += z * z;
            sum_4 += z * z * z * z;
            beyond_1 += u32::from(z.abs() > 1.0);
            beyond_3 += u32::from(z.abs() > 3.0);
            beyond_r += u32::from(z.abs() > ZIGGURAT_R);
            positive += u32::from(z > 0.0);
        }
        let nf = n as f64;
        let mean = sum / nf;
        let var = sum_sq / nf - mean * mean;
        assert!(mean.abs() < 0.003, "mean {mean}");
        assert!((var - 1.0).abs() < 0.005, "var {var}");
        assert!(
            (sum_4 / nf - 3.0).abs() < 0.03,
            "fourth moment {}",
            sum_4 / nf
        );
        assert!((f64::from(positive) / nf - 0.5).abs() < 0.002);
        assert!((f64::from(beyond_1) / nf - 0.317_310_5).abs() < 0.002);
        assert!((f64::from(beyond_3) / nf / 0.002_699_8 - 1.0).abs() < 0.06);
        assert!((f64::from(beyond_r) / nf / 5.762e-4 - 1.0).abs() < 0.12);
    }

    #[test]
    fn poisson_table_and_search_agree() {
        for &lambda in &[0.2, 3.0, 9.5] {
            let mut repeated = Rng::new(5);
            let mut alternating = Rng::new(5);
            for _ in 0..20_000 {
                let a = repeated.poisson(lambda);
                // A different mean in between forces the term-by-term search.
                alternating.poisson_inversion_reset();
                let b = alternating.poisson(lambda);
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn poisson_moments() {
        for &lambda in &[0.5, 4.0, 9.9, 10.0, 37.0, 500.0, 20_000.0] {
            let mut rng = Rng::new(11);
            let n = 100_000;
            let (mut sum, mut sum_sq) = (0.0, 0.0);
            for _ in 0..n {
                let k = rng.poisson(lambda);
                assert!(k >= 0.0);
                sum += k;
                sum_sq += k * k;
            }
            let mean = sum / n as f64;
            let var = sum_sq / n as f64 - mean * mean;
            let tol = 5.0 * (lambda / n as f64).sqrt();
            assert!((mean - lambda).abs() < tol, "lambda {lambda} mean {mean}");
            assert!(
                (var / lambda - 1.0).abs() < 0.03,
                "lambda {lambda} var {var}"
            );
        }
    }
}
