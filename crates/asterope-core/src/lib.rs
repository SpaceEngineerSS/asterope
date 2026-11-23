#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    clippy::indexing_slicing
)]
// `!(a > b)` is used on purpose: it is also true when either side is NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

extern crate alloc;

pub mod attitude;
mod bytes;
pub mod calib;
pub mod calibrate;
pub mod camera;
pub mod catalog;
pub mod design;
pub mod detect;
pub mod ephem;
pub mod error;
pub mod filter;
pub mod flight;
pub mod index;
pub mod lis;
pub mod math;
pub mod orbit;
pub mod pipeline;
pub mod rng;
#[cfg(feature = "sim")]
pub mod sim;
pub mod solar;
pub mod stack;
pub mod track;
#[cfg(feature = "validation")]
pub mod validation;

pub use error::AsteropeError;
