//! Fixture: two functions with the same name in different modules.

/// The first module.
pub mod m1 {
    /// Adds two unsigned 32-bit integers.
    pub fn add(a: u32, b: u32) -> u32 {
        a + b
    }
}

/// The second module.
pub mod m2 {
    /// Adds two unsigned 32-bit integers and 10.
    pub fn add(a: u32, b: u32) -> u32 {
        a + b + 10
    }
}
