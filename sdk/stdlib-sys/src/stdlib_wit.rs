#[cfg(feature = "wit")]
pub const STDLIB_WIT: &str = include_str!("../wit/miden-core-stdlib.wit");

#[cfg(feature = "wit")]
pub const INTRINSICS_WIT: &str = include_str!("../wit/miden-core-intrinsics.wit");
