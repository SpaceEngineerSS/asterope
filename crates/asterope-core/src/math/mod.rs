pub mod eig;
pub mod linsolve;
pub mod mat3;
pub mod quat;
pub mod vec3;

pub use eig::{jacobi_eigen, SymEigen};
pub use linsolve::solve_linear;
pub use mat3::Mat3;
pub use quat::Quat;
pub use vec3::Vec3;

pub const PI: f64 = core::f64::consts::PI;
pub const TAU: f64 = core::f64::consts::TAU;
pub const DEG: f64 = PI / 180.0;
pub const ARCSEC: f64 = DEG / 3600.0;

#[inline]
pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}
#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}
#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}
#[inline]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}
/// Integer-cast floor: identical on every target and much faster than a
/// software `floor` in per-pixel loops.
#[inline]
pub fn floor(x: f64) -> f64 {
    if !(abs(x) < 9.0e15) {
        return x;
    }
    let truncated = x as i64 as f64;
    if truncated > x {
        truncated - 1.0
    } else {
        truncated
    }
}
#[inline]
pub fn ceil(x: f64) -> f64 {
    -floor(-x)
}
/// Rounds half toward positive infinity.
#[inline]
pub fn round(x: f64) -> f64 {
    floor(x + 0.5)
}
#[inline]
pub fn abs(x: f64) -> f64 {
    libm::fabs(x)
}
#[inline]
pub fn erf(x: f64) -> f64 {
    libm::erf(x)
}
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}
#[inline]
pub fn lgamma(x: f64) -> f64 {
    libm::lgamma(x)
}

#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

pub fn wrap_two_pi(angle: f64) -> f64 {
    let wrapped = angle - TAU * floor(angle / TAU);
    if wrapped >= TAU {
        0.0
    } else {
        wrapped
    }
}

pub fn wrap_pi(angle: f64) -> f64 {
    let wrapped = wrap_two_pi(angle + PI) - PI;
    if wrapped <= -PI {
        PI
    } else {
        wrapped
    }
}
