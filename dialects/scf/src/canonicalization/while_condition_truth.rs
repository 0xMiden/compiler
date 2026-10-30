use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Replace uses of the condition of a [While] operation within its do block with true, since
/// otherwise the block would not be evaluated.
///
/// Before:
///
/// ```text,ignore
/// scf.while (..) : (i1, ...) -> ... {
///    %condition = call @evaluate_condition() : () -> i1
///    scf.condition(%condition) %condition : i1, ...
/// } do {
/// ^bb0(%arg0: i1, ...):
///    use(%arg0)
///    ...
/// ```
///
/// After:
///
/// ```text,ignore
/// scf.while (..) : (i1, ...) -> ... {
///    %condition = call @evaluate_condition() : () -> i1
///    scf.condition(%condition) %condition : i1, ...
/// } do {
/// ^bb0(%arg0: i1, ...):
///    use(%true)
///    ...
/// ```
pub struct WhileConditionTruth {
    info: PatternInfo,
}

impl WhileConditionTruth {
    pub fn new(context: Rc<Context>) -> Self {
        let scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let while_op = scf_dialect.registered_name::<While>().expect("scf.while is not registered");
        Self {
            info: PatternInfo::new(
                context,
                "while-condition-truth",
                PatternKind::Operation(while_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for WhileConditionTruth {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for WhileConditionTruth {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let op = op.borrow();
        let Some(while_op) = op.downcast_ref::<While>() else {
            return Ok(false);
        };

        // A header terminated by `scf.break` has no `scf.condition` whose truth to inspect.
        let Some(condition_op) = while_op.condition_op() else {
            return Ok(false);
        };
        let span = while_op.span();
        let (condition, forwarded) = {
            let condition_op = condition_op.borrow();
            (
                condition_op.condition().as_value_ref(),
                condition_op
                    .forwarded()
                    .iter()
                    .map(|v| v.borrow().as_value_ref())
                    .collect::<SmallVec<[ValueRef; 4]>>(),
            )
        };
        let after_args = while_op
            .after()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.borrow().as_value_ref())
            .collect::<SmallVec<[ValueRef; 4]>>();
        // Only `ValueRef`s are kept from here on: a borrow of the loop, of its after block or of
        // a block argument must not be alive while the rewriter replaces the uses of that argument.
        drop(op);

        // Prevents creating duplicate constants
        let mut constant_true = None;
        let mut replaced = false;
        for (forwarded, after_arg) in forwarded.into_iter().zip(after_args) {
            if forwarded == condition && after_arg.borrow().has_real_uses() {
                let constant = *constant_true.get_or_insert_with(|| rewriter.i1(true, span));
                rewriter.replace_all_uses_of_value_with(after_arg, constant);
                replaced = true;
            }
        }

        Ok(replaced)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, string::String};

    use midenc_hir::{
        Context,
        patterns::{self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePatternSet},
        testing::parse_function_fixpoint,
    };

    use super::WhileConditionTruth;

    /// **F-19 — the Sigma frontend's `loop` SHAPE, round-tripped through
    /// this pattern.**
    ///
    /// `loop { … break v; … }` lowers to an `scf.while` whose header is a
    /// constant `true` and whose only exit is an `scf.break` carrying the
    /// loop's result. This pattern is keyed on `scf.while` and inspects a
    /// constant condition, so it is the canonicalization in the best
    /// position to eat that header — and a loop whose header disappeared
    /// would be a silent miscompile rather than a verifier rejection, which
    /// is why the shape is witnessed HERE and not only at emission.
    ///
    /// The assertion is what makes this more than a smoke test: the loop
    /// must still be an `scf.while`, its `scf.break` must still be there,
    /// and the result must still verify. The pattern legitimately has
    /// nothing to rewrite in this shape — the constant is not itself
    /// forwarded into the body — so "it left the loop alone" is the correct
    /// outcome and the IR is what proves it.
    ///
    /// The frontend's own loop carries a poison filler on the forwarded
    /// edge rather than an iteration argument; that difference is
    /// immaterial to this pattern, which keys on the CONDITION, and the
    /// executed half of F-19 covers the emitted shape end to end — the
    /// default pipeline runs `Canonicalizer` twice over every function, so
    /// every `loop` in `sigma-exec-tests` has been through this pattern
    /// before it reaches the VM.
    #[test]
    fn a_constant_true_header_with_a_break_survives_intact() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero before {
    ^head(%i: u32):
        %t = arith.constant 1 : i1;
        scf.condition %t, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %flag = arith.gt %j, %n;
        %picked = scf.if %flag then {
            scf.break 1 %j : (u32);
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %picked;
        scf.yield %next : (u32);
    } : (u32) -> (u32);
    builtin.ret %r : (u32);
};";

        let context = Rc::new(Context::default());
        let (function, _) = parse_function_fixpoint(&context, "loop_shape.hir", PROGRAM)
            .unwrap_or_else(|err| {
                panic!("the `loop` shape must parse AND verify before the pattern runs: {err}")
            });
        let op = function.as_operation_ref();
        let pattern_set = RewritePatternSet::from_iter(
            context.clone(),
            [alloc::boxed::Box::new(WhileConditionTruth::new(context.clone()))
                as alloc::boxed::Box<dyn patterns::RewritePattern>],
        );
        let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));
        patterns::apply_patterns_and_fold_greedily(op, rewrites, GreedyRewriteConfig::default())
            .expect("expected the canonicalizer to converge");

        let verified: Result<(), String> =
            op.borrow().recursively_verify().map_err(|err| alloc::format!("{err}"));
        if let Err(err) = verified {
            panic!("`while-condition-truth` left a `loop` shape that no longer verifies:\n{err}");
        }
        let rendered = alloc::format!("{}", op.borrow());
        assert!(
            rendered.contains("scf.while"),
            "the `loop`'s header was eaten by a canonicalization:\n{rendered}"
        );
        assert!(
            rendered.contains("scf.break"),
            "the `loop`'s only exit did not survive canonicalization:\n{rendered}"
        );
    }
}
