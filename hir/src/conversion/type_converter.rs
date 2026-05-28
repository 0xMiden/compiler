/// Converts source IR types and values to target IR types and values.
///
/// This is a placeholder for the Phase 5 implementation. It is introduced now
/// so conversion pattern APIs can be typed without pulling type conversion into
/// the initial pattern-registration work.
pub struct TypeConverter {
    _private: (),
}

impl TypeConverter {
    #[inline]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for TypeConverter {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
