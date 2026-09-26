//! Fixture: the smallest arithmetic function the Rust MIR frontend must translate.

/// Adds two unsigned 32-bit integers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
