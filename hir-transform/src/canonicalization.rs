use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};
use core::fmt;

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
/// See the docs for [midenc_hir::traits::Canonicalizable] for more details.
///
/// # Options
///
/// The set of patterns applied by the pass can be narrowed from a pass pipeline string with
/// `enable-patterns` (only the listed patterns are applied) and `disable-patterns` (the listed
/// patterns are skipped). Both take a non-empty `;`-separated list of pattern names, which must
/// be quoted in a pipeline string when it has more than one name, e.g.
/// `canonicalizer{enable-patterns="fold-redundant-yields;while-unused-result"}`. A name that
/// matches no registered pattern is an error, and so is a name that appears in both lists. The
/// greedy rewrite driver still folds operations, erases trivially dead ones and, when the rewrite
/// config asks for it, simplifies regions whatever the filter says.
pub struct Canonicalizer {
    config: GreedyRewriteConfig,
    rewrites: Option<Rc<FrozenRewritePatternSet>>,
    require_convergence: bool,
    /// If non-empty, only patterns with these names are applied.
    enabled_patterns: Vec<String>,
    /// Patterns with these names are never applied.
    disabled_patterns: Vec<String>,
}

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<Canonicalizer>(
    Canonicalizer::NAME,
    "canonicalization"
));

impl Default for Canonicalizer {
    fn default() -> Self {
        let mut config = GreedyRewriteConfig::default();
        config.with_top_down_traversal(true);
        Self {
            config,
            rewrites: None,
            require_convergence: false,
            enabled_patterns: Vec::new(),
            disabled_patterns: Vec::new(),
        }
    }
}

impl Canonicalizer {
    const NAME: &str = "canonicalizer";

    /// Creates an instance of this pass with the given rewrite config; `require_convergence`
    /// turns a failure to reach a fixpoint into an error.
    pub fn new(config: GreedyRewriteConfig, require_convergence: bool) -> Self {
        Self {
            config,
            require_convergence,
            ..Self::default()
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
            ..Self::default()
        })
    }

    /// Returns true if the pattern named `name` passes the `enable-patterns`/`disable-patterns`
    /// filter.
    fn is_pattern_enabled(&self, name: &str) -> bool {
        (self.enabled_patterns.is_empty() || self.enabled_patterns.iter().any(|p| p == name))
            && !self.disabled_patterns.iter().any(|p| p == name)
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

    /// Parses `enable-patterns` and `disable-patterns` (see the [Canonicalizer] docs) from the
    /// `key=value, key=value` string of the pass pipeline.
    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        for option in options.split(',').map(str::trim).filter(|opt| !opt.is_empty()) {
            let Some((key, value)) = option.split_once('=').map(|(k, v)| (k.trim(), v)) else {
                return Err(Report::msg(format!(
                    "invalid option '{option}' for pass '{}': expected 'key=value'",
                    Self::NAME
                )));
            };
            let names = value
                .split(';')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .collect::<Vec<_>>();
            let list = match key {
                "enable-patterns" => &mut self.enabled_patterns,
                "disable-patterns" => &mut self.disabled_patterns,
                _ => {
                    return Err(Report::msg(format!(
                        "invalid option '{key}' for pass '{}'",
                        Self::NAME
                    )));
                }
            };
            // An empty list would silently mean "no filter", the opposite of what was asked.
            if names.is_empty() {
                return Err(Report::msg(format!(
                    "option '{key}' of pass '{}' has no pattern names",
                    Self::NAME
                )));
            }
            if !list.is_empty() {
                return Err(Report::msg(format!(
                    "option '{key}' of pass '{}' is given more than once",
                    Self::NAME
                )));
            }
            *list = names;
        }
        // A name in both lists is a contradiction rather than a filter.
        if let Some(name) =
            self.enabled_patterns.iter().find(|n| self.disabled_patterns.contains(n))
        {
            return Err(Report::msg(format!(
                "pattern '{name}' is both enabled and disabled in the options of pass '{}'",
                Self::NAME
            )));
        }
        Ok(())
    }

    /// Prints the pass with its pattern filter, the only state settable from pipeline options;
    /// the rewrite config given to [Canonicalizer::new] has no textual form.
    fn print_as_textual_pipeline(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", Self::NAME)?;
        if self.enabled_patterns.is_empty() && self.disabled_patterns.is_empty() {
            return Ok(());
        }
        f.write_str("{")?;
        let mut sep = "";
        for (key, names) in [
            ("enable-patterns", &self.enabled_patterns),
            ("disable-patterns", &self.disabled_patterns),
        ] {
            if !names.is_empty() {
                write!(f, "{sep}{key}=\"{}\"", names.join(";"))?;
                sep = " ";
            }
        }
        f.write_str("}")
    }

    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        log::trace!(target: Self::NAME, "initializing canonicalizer pass");
        let mut rewrites = RewritePatternSet::new(context.clone());

        for dialect in context.registered_dialects().values() {
            for op in dialect.registered_ops().iter() {
                op.populate_canonicalization_patterns(&mut rewrites, context.clone());
            }
        }

        // A filter naming a pattern that does not exist would silently apply no pattern at all,
        // so reject it.
        let is_registered =
            |name: &String| rewrites.patterns().iter().any(|pattern| pattern.name() == name);
        if let Some(name) = self
            .enabled_patterns
            .iter()
            .chain(&self.disabled_patterns)
            .find(|n| !is_registered(n))
        {
            return Err(Report::msg(format!(
                "unknown canonicalization pattern '{name}' in the options of pass '{}'",
                Self::NAME
            )));
        }
        rewrites.retain(|pattern| self.is_pattern_enabled(pattern.name()));

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

/// Tests of the option parsing and the textual pipeline form of [Canonicalizer].
#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    /// Renders a pass through [Pass::print_as_textual_pipeline].
    struct Textual<'a>(&'a Canonicalizer);

    impl fmt::Display for Textual<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            Pass::print_as_textual_pipeline(self.0, f)
        }
    }

    /// The pattern lists parse from the `key=value, key=value` form and print back quoted.
    #[test]
    fn options_print_as_textual_pipeline() {
        let mut pass = Canonicalizer::default();
        assert_eq!(Textual(&pass).to_string(), "canonicalizer");

        Pass::initialize_options(&mut pass, "enable-patterns=a;b, disable-patterns=c").unwrap();
        assert_eq!(
            Textual(&pass).to_string(),
            "canonicalizer{enable-patterns=\"a;b\" disable-patterns=\"c\"}"
        );
        assert_eq!(pass.enabled_patterns, ["a", "b"]);
        assert_eq!(pass.disabled_patterns, ["c"]);
    }

    /// Unknown keys, missing or empty lists, a name in both lists and a repeated key are errors.
    #[test]
    fn invalid_options_are_rejected() {
        for options in [
            "bogus=1",
            "enable-patterns",
            "enable-patterns=",
            "enable-patterns=;",
            "enable-patterns=a, disable-patterns=a",
            "enable-patterns=a, enable-patterns=b",
        ] {
            let mut pass = Canonicalizer::default();
            assert!(
                Pass::initialize_options(&mut pass, options).is_err(),
                "expected '{options}' to be rejected"
            );
        }
    }
}
