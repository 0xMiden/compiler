//! Fixture: a function with a parameter type the Rust MIR frontend cannot map.

/// Returns its argument unchanged.
pub fn identity(a: f64) -> f64 {
    a
}
