#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(improper_ctypes)] // for u128

#[cfg(feature = "num-complex")]
pub use num_complex::Complex32 as liquid_float_complex;
#[cfg(feature = "num-complex")]
pub use num_complex::Complex64 as liquid_double_complex;

include!(concat!(env!("OUT_DIR"), "/liquid.rs"));
