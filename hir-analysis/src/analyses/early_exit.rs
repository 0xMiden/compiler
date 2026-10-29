//! Regression tests for successor discovery and liveness across structured early exits.

use alloc::{rc::Rc, string::String};

use midenc_hir::{
    Context, Op, Operation, OperationRef, ProgramPoint, RegionBranchPoint, RegionSuccessorInfo,
    Report, Value as _, ValueRef, dialects::builtin::FunctionRef, pass::AnalysisManager,
    testing::parse_function_fixpoint,
};

use crate::analyses::{LivenessAnalysis, dce::PredecessorState};

/// The skeleton, with `EXIT` standing in for the operation that terminates the `then` branch.
///
/// * `%seed` is defined **before** the loop and used **after** it.
/// * the loop's condition is a constant `true`, so the only way out is whatever `EXIT` is.
const SKELETON: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %seed = arith.constant 7 : u32;
    %always = arith.constant 1 : i1;
    %r = scf.while %zero before {
    ^head(%i: u32):
        scf.condition %always, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %hit = arith.gt %j, %n;
        %k = scf.if %hit then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.incr %k;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    %sum = arith.add %r, %seed <{ overflow = #builtin.overflow<unchecked> }>;
    builtin.ret %sum : (u32);
};";

/// The program whose loop exits through an `scf.break`.
fn with_break() -> String {
    SKELETON.replace("EXIT", "scf.break 1 %j : (u32);")
}

/// The control: the same program whose `then` branch yields instead, so the loop never exits.
fn without_break() -> String {
    SKELETON.replace("EXIT", "scf.yield %j : (u32);")
}

struct Program {
    #[allow(unused)]
    context: Rc<Context>,
    function: FunctionRef,
}

impl Program {
    fn parse(name: &str, source: &str) -> Result<Self, Report> {
        let context = Rc::new(Context::default());
        let (function, _) = parse_function_fixpoint(&context, name, source)?;
        Ok(Self { context, function })
    }

    /// The single `scf.while` in the program.
    fn loop_op(&self) -> OperationRef {
        self.find(|op| op.name().name() == "while").expect("expected an scf.while")
    }

    /// The `arith.constant 7` defining `%seed`.
    fn seed(&self) -> ValueRef {
        let op = self
            .find(|op| {
                op.name().name() == "constant"
                    && op.num_results() == 1
                    && op.results()[0].borrow().ty().is_integer()
                    && alloc::format!("{op}").contains(" 7 ")
            })
            .expect("expected the seed constant");
        op.borrow().results()[0].borrow().as_value_ref()
    }

    fn find(&self, mut predicate: impl FnMut(&Operation) -> bool) -> Option<OperationRef> {
        let mut found = None;
        self.function.borrow().as_operation().prewalk_all(|op: &Operation| {
            if found.is_none() && predicate(op) {
                found = Some(op.as_operation_ref());
            }
        });
        found
    }

    fn liveness(&self) -> Result<Rc<LivenessAnalysis>, Report> {
        let am = AnalysisManager::new(self.function.as_operation_ref(), None);
        am.get_analysis_for::<LivenessAnalysis, Operation>()
    }
}

/// A break is a predecessor of the point after its target loop, even when nested in an if.
/// Without the break, the constant-true loop has no reachable exit.
#[test]
fn n1_dce_records_the_break_as_a_predecessor_after_the_loop() -> Result<(), Report> {
    let broken = Program::parse("n1_with_break.hir", &with_break())?;
    let control = Program::parse("n1_without_break.hir", &without_break())?;

    let predecessors_after_loop = |program: &Program| -> alloc::vec::Vec<String> {
        let liveness = program.liveness().expect("liveness should run");
        let point = ProgramPoint::after(program.loop_op());
        liveness
            .solver()
            .get::<PredecessorState, _>(&point)
            .map(|state| {
                state
                    .known_predecessors()
                    .iter()
                    .map(|op| alloc::string::ToString::to_string(&op.borrow().name()))
                    .collect()
            })
            .unwrap_or_default()
    };

    let with = predecessors_after_loop(&broken);
    let without = predecessors_after_loop(&control);

    assert!(
        with.iter().any(|name| name == "scf.break"),
        "the break must be recorded as a predecessor of the point after the loop it exits, but \
         the predecessors were {with:?}"
    );
    assert!(
        !without.iter().any(|name| name == "scf.break"),
        "the control has no break, so it cannot have one as a predecessor: {without:?}"
    );
    assert_ne!(with, without, "the break must change the set of predecessors after the loop");

    Ok(())
}

/// A value defined before the loop and used after it remains live across a nested break.
#[test]
fn n2_liveness_carries_a_use_after_the_loop_back_across_the_break_edge() -> Result<(), Report> {
    let broken = Program::parse("n2_with_break.hir", &with_break())?;

    let seed = broken.seed();
    let break_op = broken.find(|op| op.name().name() == "break").expect("expected an scf.break");
    let liveness = broken.liveness()?;

    let result = broken.loop_op().borrow().results()[0].borrow().as_value_ref();
    assert!(
        !liveness.is_live_after(result, &break_op.borrow()),
        "the loop result is defined by receiving the break, not live on the exit edge"
    );
    assert!(
        !liveness.is_live_before(result, &break_op.borrow()),
        "the loop result cannot be live before its definition"
    );

    assert!(
        liveness.is_live_after(seed, &break_op.borrow()),
        "`%seed` is used after the loop, and the break exits to that use, so it must still be \
         live at the point after the break"
    );

    let body_entry = {
        let loop_op = broken.loop_op();
        let loop_op = loop_op.borrow();
        loop_op.region(1).entry_block_ref().expect("the loop body has an entry block")
    };
    assert!(
        liveness.is_live_at_start(seed, body_entry),
        "`%seed` must be live across the loop body, which nothing in the body mentions"
    );

    Ok(())
}

/// A body containing a break to its loop has a returning edge in addition to the back edge.
#[test]
fn n6_the_loop_body_reports_an_exit_when_a_nested_break_targets_it() -> Result<(), Report> {
    let broken = Program::parse("n6_with_break.hir", &with_break())?;
    let control = Program::parse("n6_without_break.hir", &without_break())?;

    let body_exits = |program: &Program| -> usize {
        let loop_op = program.loop_op();
        let loop_op = loop_op.borrow();
        let branch = loop_op
            .as_trait::<dyn midenc_hir::RegionBranchOpInterface>()
            .expect("scf.while implements RegionBranchOpInterface");
        let body = loop_op.region(1).as_region_ref();
        branch
            .get_successor_regions(RegionBranchPoint::Child(body))
            .into_successor_infos()
            .iter()
            .filter(|info| matches!(info, RegionSuccessorInfo::Returning(_)))
            .count()
    };

    let with = body_exits(&broken);
    let without = body_exits(&control);

    assert_eq!(
        with, 1,
        "the loop body exits the loop via the break, and that edge must be there"
    );
    assert_eq!(without, 0, "without a break the body's only successor is the header");
    assert_ne!(with, without, "the break must add a returning edge to the loop body");

    Ok(())
}

/// The skeleton for the BACKWARD framework's own edge, with `EXIT` standing in for the operation
/// that terminates the `then` branch.
///
/// `%q` is defined **inside the loop body, before the `scf.if`**, and its ONLY use is **after the
/// `scf.if`, still inside the body**. So at the point after the `scf.break`, `%q` is dead:
/// control leaves the loop entirely and never reaches that use. At the point after the `scf.if`,
/// it is live. **The two points are one op apart and the correct answers are opposite**, which is
/// what makes this program able to say which one the backward walk anchored the break's edge at.
const BODY_LOCAL_SKELETON: &str = "\
builtin.function public extern(\"C\") @f(%n: u32) -> u32 {
    %zero = arith.constant 0 : u32;
    %always = arith.constant 1 : i1;
    %r = scf.while %zero before {
    ^head(%i: u32):
        scf.condition %always, %i : (i1, u32);
    } after {
    ^body(%j: u32):
        %q = arith.constant 9 : u32;
        %hit = arith.gt %j, %n;
        %k = scf.if %hit then {
            EXIT
        } else {
            scf.yield %j : (u32);
        } : (i1) -> (u32);
        %next = arith.add %k, %q <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %next : (u32);
    } : (u32) -> u32;
    builtin.ret %r : (u32);
};";

/// The break bypasses a use after its enclosing if, so that body-local value is dead after
/// the break while remaining live after the if.
#[test]
fn the_backward_walk_anchors_a_breaks_edge_after_the_loop_not_after_the_if() -> Result<(), Report> {
    let broken = Program::parse(
        "backward_edge_with_break.hir",
        &BODY_LOCAL_SKELETON.replace("EXIT", "scf.break 1 %j : (u32);"),
    )?;

    let q = broken
        .find(|op| {
            op.name().name() == "constant"
                && op.num_results() == 1
                && op.results()[0].borrow().ty().is_integer()
                && alloc::format!("{op}").contains(" 9 ")
        })
        .expect("expected the %q constant");
    let q = q.borrow().results()[0].borrow().as_value_ref();

    let break_op = broken.find(|op| op.name().name() == "break").expect("expected an scf.break");
    let if_op = broken.find(|op| op.name().name() == "if").expect("expected an scf.if");
    let liveness = broken.liveness()?;

    assert!(
        liveness.is_live_after(q, &if_op.borrow()),
        "`%q`'s only use follows the `scf.if`, so it must be live at the point after it — if this \
         fails the program no longer says what it was built to say"
    );
    assert!(
        !liveness.is_live_after(q, &break_op.borrow()),
        "`%q` is used only after the `scf.if`, and the break leaves the LOOP — it never reaches \
         that use. Reporting `%q` live here means the backward walk anchored the break's edge at \
         `after(scf.if)` instead of `after(loop)`."
    );

    Ok(())
}
