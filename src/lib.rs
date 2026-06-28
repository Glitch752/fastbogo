#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod benchmark;
pub mod kernel;
#[cfg(target_feature = "avx2")]
pub mod kernel_simd;
pub(self) mod libdivide;