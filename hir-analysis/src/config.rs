/// Configuration for the data flow solver and child analyses.
#[derive(Debug, Default, Clone)]
pub struct DataFlowConfig {
    /// Indicates whether the solver should operation interprocedurally
    interprocedural: bool,
}

impl DataFlowConfig {
    /// Get a new, default configuration
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub const fn is_interprocedural(&self) -> bool {
        self.interprocedural
    }

    /// Set whether the solver should operate interprocedurally, i.e. enter the callee body when
    /// available.
    ///
    /// Interprocedural analyses may be more precise, but also more expensive as more states need to
    /// be computed and the fixpoint convergence takes longer.
    pub fn set_interprocedural(&mut self, yes: bool) -> &mut Self {
        self.interprocedural = yes;
        self
    }
}
