/// Configuration for the data flow solver and child analyses.
#[derive(Debug, Default, Clone)]
pub struct DataFlowConfig {
    /// Indicates whether the solver should operation interprocedurally
    interprocedural: bool,
    /// Optional limit on queued analysis visits while solving to fixpoint.
    max_worklist_iterations: Option<usize>,
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

    #[inline(always)]
    pub const fn max_worklist_iterations(&self) -> Option<usize> {
        self.max_worklist_iterations
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

    /// Set a maximum number of queued analysis visits while solving to fixpoint.
    ///
    /// This is intended for lint and diagnostic callers that need a bounded analysis result instead
    /// of an unbounded run on large or currently unsupported IR graphs.
    pub fn set_max_worklist_iterations(&mut self, max: Option<usize>) -> &mut Self {
        self.max_worklist_iterations = max;
        self
    }
}
