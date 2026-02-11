use alloc::{boxed::Box, format, rc::Rc};

use midenc_hir::{
    Context, EntityMut, Operation, OperationName, Report, Spanned,
    pass::{OperationPass, Pass, PassExecutionState, PostPassStatus},
    patterns::{self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePatternSet},
};
use midenc_session::diagnostics::Severity;

/// This pass performs various types of canonicalizations over a set of operations by iteratively
/// applying the canonicalization patterns of all loaded dialects until either a fixpoint is reached
/// or the maximum number of iterations/rewrites is exhausted. Canonicalization is best-effort and
/// does not guarantee that the entire IR is in a canonical form after running this pass.
///
/// See the docs for [crate::traits::Canonicalizable] for more details.
pub struct Canonicalizer {
    config: GreedyRewriteConfig,
    rewrites: Option<Rc<FrozenRewritePatternSet>>,
    require_convergence: bool,
}

impl Default for Canonicalizer {
    fn default() -> Self {
        let mut config = GreedyRewriteConfig::default();
        config.with_top_down_traversal(true);
        Self {
            config,
            rewrites: None,
            require_convergence: false,
        }
    }
}

impl Canonicalizer {
    const NAME: &str = "canonicalizer";

    pub fn new(config: GreedyRewriteConfig, require_convergence: bool) -> Self {
        Self {
            config,
            rewrites: None,
            require_convergence,
        }
    }

    /// Creates an instance of this pass, configured with default settings.
    pub fn create() -> Box<dyn OperationPass> {
        Box::new(Self::default())
    }

    /// Creates an instance of this pass with the specified config.
    pub fn create_with_config(config: &GreedyRewriteConfig) -> Box<dyn OperationPass> {
        Box::new(Self {
            config: config.clone(),
            rewrites: None,
            require_convergence: false,
        })
    }
}

impl Pass for Canonicalizer {
    type Target = Operation;

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn argument(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Performs canonicalization over a set of operations"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        log::trace!(target: Self::NAME, "initializing canonicalizer pass");
        let mut rewrites = RewritePatternSet::new(context.clone());

        for dialect in context.registered_dialects().values() {
            for op in dialect.registered_ops().iter() {
                op.populate_canonicalization_patterns(&mut rewrites, context.clone());
            }
        }

        self.rewrites = Some(Rc::new(FrozenRewritePatternSet::new(rewrites)));

        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let Some(rewrites) = self.rewrites.as_ref() else {
            log::debug!(target: Self::NAME, "skipping canonicalization as there are no rewrite patterns to apply");
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        };
        let op = {
            let ptr = op.as_operation_ref();
            drop(op);
            log::debug!(target: Self::NAME, "applying canonicalization to {}", ptr.borrow());
            log::debug!(target: Self::NAME, "  require_convergence = {}", self.require_convergence);
            ptr
        };
        let converged =
            patterns::apply_patterns_and_fold_greedily(op, rewrites.clone(), self.config.clone());
        if self.require_convergence && converged.is_err() {
            log::debug!(target: Self::NAME, "canonicalization could not converge");
            let span = op.borrow().span();
            return Err(state
                .context()
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("canonicalization failed")
                .with_primary_label(
                    span,
                    format!(
                        "canonicalization did not converge{}",
                        self.config
                            .max_iterations()
                            .map(|max| format!(" after {max} iterations"))
                            .unwrap_or_default()
                    ),
                )
                .into_report());
        }

        let op = op.borrow();
        let changed = match converged {
            Ok(changed) => {
                log::debug!(target: Self::NAME, "canonicalization converged for '{}', changed={changed}", op.name());
                changed
            }
            Err(changed) => {
                log::warn!(
                    target: Self::NAME,
                    "canonicalization failed to converge for '{}', changed={changed}",
                    op.name()
                );
                changed
            }
        };
        let ir_changed = changed.into();
        state.set_post_pass_status(ir_changed);

        Ok(())
    }
}
