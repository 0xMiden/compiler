//! The invariant every `scf.while`-reshaping canonicalization owes an early exit that addresses
//! the loop it is reshaping.
//!
//! Four patterns in this module rebuild an `scf.while` with a different *interface*: a shorter
//! result list ([`WhileUnusedResult`](super::WhileUnusedResult),
//! [`WhileRemoveDuplicatedResults`](super::WhileRemoveDuplicatedResults)) or a shorter iter-arg
//! list ([`WhileRemoveUnusedArgs`](super::WhileRemoveUnusedArgs),
//! [`RemoveLoopInvariantArgsFromBeforeBlock`](super::RemoveLoopInvariantArgsFromBeforeBlock)),
//! all through [`rebuild_while`](super::while_rebuild::rebuild_while). It rewrites the two
//! terminators it knows about — `scf.condition` and `scf.yield` — and neither of those is the
//! only op that can carry values into a loop's interface any more.
//!
//! A `scf.break` nested anywhere inside the loop hands its operands to the loop's **results**
//! (verifier rule 4); a `scf.continue` hands its operands to the loop's **iter-args** (rule 5).
//! Both may sit arbitrarily deep — inside an `scf.if`, inside an inner loop addressing this one
//! by level count — where no accessor a pattern reaches for will find them. Reshaping the
//! interface out from under such an exit produces IR that this dialect's own verifier rejects.
//!
//! So `rebuild_while` bails, and with it the four patterns. Reshaping a loop with a nested exit
//! is not *wrong* in principle — the exits' operand lists would have to be rewritten in step —
//! but the rebuild does not do that work, and a rewrite that cannot maintain an invariant must
//! not be the one to break it.

use midenc_hir::HasBreakingControlFlowOpInterface;

use crate::While;

/// Does any early exit address `while_op`, making its interface unsafe to reshape?
///
/// This is the first consumer of
/// [`HasBreakingControlFlowOpInterface::has_nested_predecessors`], and it is exactly the question
/// that method was declared to answer: a loop with no nested exit pays a walk of its own two
/// regions and is reshaped as before, and a loop with one is left alone.
///
/// The predicate is deliberately BOTH kinds. A `break` invalidates a reshaped result list and a
/// `continue` invalidates a reshaped iter-arg list, but every one of these patterns builds a
/// whole new `scf.while` and moves the body into it, so no pattern here is in a position to
/// claim that only one half of its interface moved.
pub(super) fn addressed_by_a_nested_exit(while_op: &While) -> bool {
    while_op.has_nested_predecessors()
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc, string::String};

    use midenc_hir::{
        Context,
        patterns::{
            self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePattern, RewritePatternSet,
        },
        testing::parse_function_fixpoint,
    };

    use crate::canonicalization::{
        RemoveLoopInvariantArgsFromBeforeBlock, WhileRemoveDuplicatedResults,
        WhileRemoveUnusedArgs, WhileUnusedResult,
    };

    /// Run exactly ONE pattern over `source`; report whether it fired and what the IR it left
    /// behind says when it is verified.
    ///
    /// One pattern rather than the dialect's whole `While` set, deliberately: the greedy driver
    /// panics with an `AliasingViolation` on the full set over a two-result loop, and it does so
    /// with no early exit anywhere in the program. That is a pre-existing defect (`Q8-10`),
    /// routed rather than chased, and a single-pattern run keeps it out of the question these
    /// witnesses ask.
    #[track_caller]
    fn run_one(
        name: &str,
        source: &str,
        make: fn(Rc<Context>) -> Box<dyn RewritePattern>,
    ) -> (bool, Result<(), String>) {
        let context = Rc::new(Context::default());
        let (function, _) = parse_function_fixpoint(&context, name, source).unwrap_or_else(|err| {
            panic!("'{name}' must parse AND verify before any pattern runs: {err}")
        });
        let op = function.as_operation_ref();
        let pattern_set = RewritePatternSet::from_iter(context.clone(), [make(context.clone())]);
        let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));
        let changed = patterns::apply_patterns_and_fold_greedily(
            op,
            rewrites,
            GreedyRewriteConfig::default(),
        )
        .expect("expected the canonicalizer to converge");
        // The report is rendered here, while `context` is still alive, so the caller never holds
        // a diagnostic that outlives the entities it points at.
        let verified = function
            .as_operation_ref()
            .borrow()
            .recursively_verify()
            .map_err(|err| format!("{err}"));
        (changed, verified)
    }

    /// The matched pair, for one pattern.
    ///
    /// * `with_exit` — the loop with an early exit addressing it, nested in an `scf.if`. The
    ///   pattern must BAIL, and the IR must still verify. Without the guard the pattern fires and
    ///   its own output is what `recursively_verify` rejects.
    /// * `control` — the same program with the exit replaced by an ordinary yield. The pattern
    ///   must still FIRE. This half is what keeps the guard narrow: a pattern disabled outright
    ///   would pass the first assertion and fail this one.
    #[track_caller]
    fn matched_pair(
        name: &str,
        with_exit: &str,
        control: &str,
        make: fn(Rc<Context>) -> Box<dyn RewritePattern>,
    ) {
        let (changed, verified) = run_one(&format!("{name}_with_exit.hir"), with_exit, make);
        if let Err(err) = verified {
            panic!(
                "'{name}' reshaped a loop addressed by a nested early exit, and its own output no \
                 longer verifies:\n{err}"
            );
        }
        assert!(
            !changed,
            "'{name}' must bail on a loop addressed by a nested early exit, not reshape it"
        );

        let (changed, verified) = run_one(&format!("{name}_control.hir"), control, make);
        verified.unwrap_or_else(|err| panic!("'{name}' control output does not verify:\n{err}"));
        assert!(
            changed,
            "'{name}' must still fire on the same loop WITHOUT a nested early exit — the guard is \
             supposed to be narrow, not a disable switch"
        );
    }

    /// `WhileUnusedResult` drops a result whose after-block argument is also unused, so the
    /// loop's result list shrinks. A nested `scf.break` still carries the OLD list — rule 4.
    #[test]
    fn while_unused_result_bails_on_a_loop_a_nested_break_addresses() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r0, %r1 = scf.while %zero, %zero before {
    ^head(%i: u32, %k: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i, %k : (i1, u32, u32);
    } after {
    ^body(%j: u32, %m: u32):
        %flag = arith.gt %j, %n;
        %picked = scf.if %flag then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %picked;
        scf.yield %next, %next : (u32, u32);
    } : (u32, u32) -> (u32, u32);
    builtin.ret %r0 : (u32);
};";

        matched_pair(
            "while_unused_result",
            &PROGRAM.replace("EXIT", "scf.break 1 %j, %j : (u32, u32);"),
            &PROGRAM.replace("EXIT", "scf.yield %j : (u32);"),
            |context| Box::new(WhileUnusedResult::new(context)),
        );
    }

    /// `WhileRemoveDuplicatedResults` collapses two results forwarded from the same value, so the
    /// loop's result list shrinks. A nested `scf.break` still carries the OLD list — rule 4.
    #[test]
    fn while_remove_duplicated_results_bails_on_a_loop_a_nested_break_addresses() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r0, %r1 = scf.while %zero, %zero before {
    ^head(%i: u32, %k: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i, %i : (i1, u32, u32);
    } after {
    ^body(%j: u32, %m: u32):
        %flag = arith.gt %j, %n;
        %picked = scf.if %flag then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %picked;
        scf.yield %next, %next : (u32, u32);
    } : (u32, u32) -> (u32, u32);
    builtin.ret %r0 : (u32);
};";

        matched_pair(
            "while_remove_duplicated_results",
            &PROGRAM.replace("EXIT", "scf.break 1 %j, %m : (u32, u32);"),
            &PROGRAM.replace("EXIT", "scf.yield %j : (u32);"),
            |context| Box::new(WhileRemoveDuplicatedResults::new(context)),
        );
    }

    /// `WhileRemoveUnusedArgs` erases a before-block argument nothing reads, so the loop's
    /// iter-arg list shrinks. A nested `scf.continue` still carries the OLD list — rule 5.
    #[test]
    fn while_remove_unused_args_bails_on_a_loop_a_nested_continue_addresses() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero, %zero before {
    ^head(%i: u32, %k: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %flag = arith.gt %j, %n;
        %picked = scf.if %flag then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %picked;
        scf.yield %next, %next : (u32, u32);
    } : (u32, u32) -> (u32);
    builtin.ret %r : (u32);
};";

        matched_pair(
            "while_remove_unused_args",
            &PROGRAM.replace("EXIT", "scf.continue 1 %j, %j : (u32, u32);"),
            &PROGRAM.replace("EXIT", "scf.yield %j : (u32);"),
            |context| Box::new(WhileRemoveUnusedArgs::new(context)),
        );
    }

    /// `RemoveLoopInvariantArgsFromBeforeBlock` drops an iter-arg whose yielded value is its own
    /// initial value, so the loop's iter-arg list shrinks. A nested `scf.continue` still carries
    /// the OLD list — rule 5. The pattern rebuilds the loop through `rebuild_while`, whose guard
    /// is the one that bails.
    #[test]
    fn remove_loop_invariant_args_bails_on_a_loop_a_nested_continue_addresses() {
        const PROGRAM: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %r = scf.while %zero, %zero before {
    ^head(%i: u32, %k: u32):
        %more = arith.lt %i, %n;
        scf.condition %more, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %flag = arith.gt %j, %n;
        %picked = scf.if %flag then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %picked;
        scf.yield %next, %zero : (u32, u32);
    } : (u32, u32) -> (u32);
    builtin.ret %r : (u32);
};";

        matched_pair(
            "remove_loop_invariant_args",
            &PROGRAM.replace("EXIT", "scf.continue 1 %j, %j : (u32, u32);"),
            &PROGRAM.replace("EXIT", "scf.yield %j : (u32);"),
            |context| Box::new(RemoveLoopInvariantArgsFromBeforeBlock::new(context)),
        );
    }
}
