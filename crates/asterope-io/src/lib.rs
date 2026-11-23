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

pub mod fits;

pub use fits::{encode_fits_u16, parse_fits, parse_fits_plane, FitsError, FitsImage};
