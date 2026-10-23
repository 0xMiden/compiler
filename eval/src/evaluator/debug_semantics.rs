use alloc::{boxed::Box, collections::BTreeMap};

use midenc_hir::{
    EntityMut, Forward, Op, OperationName, ProgramPoint, RawWalk, Symbol as _, SymbolName,
    SymbolTable,
    diagnostics::Uri,
    dialects::{
        builtin::{Function, FunctionRef, Module, ModuleRef, attributes::U32Attr},
        debuginfo::{
            DebugDeclare, DebugInfoMutation, DebugInfoUpdate, DebugKill, DebugValue,
            attributes::{
                Expression, ExpressionOp, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain,
                InlineCallChainAttr, Variable,
            },
        },
    },
    parse::{ParserConfig, parse},
    pass::{Nesting, OperationPass, Pass, PassExecutionState, PassManager, PostPassStatus},
    patterns::{NoopRewriterListener, RewriterImpl},
    testing::Test,
};
use midenc_hir_transform::{
    Canonicalizer, CommonSubexpressionElimination, DeadCodeElimination, SinkOperandDefs,
};

use super::*;

const CHECKPOINT_ATTR: &str = "test.debug_checkpoint";
const STEP_LIMIT: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    function: String,
    variable: Variable,
    inline_calls: Option<InlineCallChain>,
}

impl Binding {
    fn new(operation: &Operation, variable: Variable, frame: &CallFrame) -> Self {
        Self {
            function: frame.symbol_path().unwrap().to_string(),
            variable,
            inline_calls: operation
                .get_typed_attribute::<InlineCallChainAttr>(INLINE_CALL_CHAIN_ATTR_NAME)
                .map(|attribute| attribute.borrow().as_value().clone()),
        }
    }
}

#[derive(Clone)]
enum Location {
    Value {
        seed: Option<Value>,
        expression: Expression,
    },
    Killed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    Value(Value),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FrameSnapshot {
    function: String,
    variables: Vec<(Binding, Observation)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Checkpoint {
    Marker(u32),
    Return,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    checkpoint: Checkpoint,
    frames: Vec<FrameSnapshot>,
}

#[derive(Default)]
pub(super) struct DebugTrace {
    frames: Vec<Vec<(Binding, Location)>>,
    snapshots: Vec<Snapshot>,
    steps: usize,
}

impl DebugTrace {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe(
        &mut self,
        evaluator: &HirEvaluator,
        operation: &Operation,
    ) -> Result<(), Report> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            return Err(Report::msg("debug semantic test exceeded its instruction limit"));
        }
        let depth = evaluator.call_stack.len();
        self.frames.resize_with(depth, Vec::new);
        let frame = evaluator.current_frame();
        let record = if let Some(debug) = operation.downcast_ref::<DebugValue>() {
            Some((
                Binding::new(operation, debug.variable().as_value().clone(), frame),
                Location::Value {
                    seed: Some(evaluator.get_value(&debug.value().as_value_ref())?),
                    expression: debug.expression().as_value().clone(),
                },
            ))
        } else if let Some(debug) = operation.downcast_ref::<DebugDeclare>() {
            Some((
                Binding::new(operation, debug.variable().as_value().clone(), frame),
                Location::Value {
                    seed: None,
                    expression: debug.expression().as_value().clone(),
                },
            ))
        } else {
            operation.downcast_ref::<DebugKill>().map(|debug| {
                (
                    Binding::new(operation, debug.variable().as_value().clone(), frame),
                    Location::Killed,
                )
            })
        };
        if let Some((binding, location)) = record {
            if let Location::Value { expression, .. } = &location {
                for operator in &expression.operations {
                    if !matches!(
                        operator,
                        ExpressionOp::LocalSlot(_)
                            | ExpressionOp::ConstU64(_)
                            | ExpressionOp::PlusUConst(_)
                            | ExpressionOp::Plus
                            | ExpressionOp::Minus
                            | ExpressionOp::StackValue
                    ) {
                        return Err(Report::msg(format!(
                            "debug semantic test cannot evaluate expression operator {operator:?}"
                        )));
                    }
                }
            }
            let variables = self.frames.last_mut().unwrap();
            if let Some((_, tracked)) = variables.iter_mut().find(|(key, _)| *key == binding) {
                *tracked = location;
            } else {
                variables.push((binding, location));
            }
        }
        let checkpoint = operation
            .get_typed_attribute::<U32Attr>(CHECKPOINT_ATTR)
            .map(|attribute| Checkpoint::Marker(*attribute.borrow().as_value()))
            .or_else(|| {
                (operation.is::<builtin::Ret>() || operation.is::<builtin::RetImm>())
                    .then_some(Checkpoint::Return)
            });
        if let Some(checkpoint) = checkpoint {
            let mut frames = Vec::new();
            for (frame, variables) in evaluator.call_stack.iter().zip(&self.frames) {
                let mut observed = Vec::new();
                for (binding, location) in variables {
                    observed.push((binding.clone(), resolve(location, frame, evaluator)?));
                }
                frames.push(FrameSnapshot {
                    function: frame.symbol_path().unwrap().to_string(),
                    variables: observed,
                });
            }
            self.snapshots.push(Snapshot { checkpoint, frames });
        }
        Ok(())
    }
}

fn resolve(
    location: &Location,
    frame: &CallFrame,
    evaluator: &HirEvaluator,
) -> Result<Observation, Report> {
    let Location::Value { seed, expression } = location else {
        return Ok(Observation::Unavailable);
    };
    let mut values = seed.iter().copied().collect::<Vec<_>>();
    for operator in &expression.operations {
        match operator {
            ExpressionOp::LocalSlot(index) => {
                let callee = frame.callee();
                let function = callee.downcast_ref::<Function>().unwrap();
                let local = function.iter_locals().nth(*index as usize).ok_or_else(|| {
                    Report::msg(format!("debug semantic test references missing local {index}"))
                })?;
                values.push(frame.read_local(
                    &local,
                    evaluator.current_span(),
                    &evaluator.context,
                )?);
            }
            ExpressionOp::ConstU64(value) => values.push((*value).into()),
            ExpressionOp::PlusUConst(offset) => {
                let value = values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let result = unsigned(value)?.checked_add(*offset);
                values.push(arithmetic_result(result, &value.ty())?);
            }
            ExpressionOp::Plus | ExpressionOp::Minus => {
                let rhs = values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let lhs = values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let result = if matches!(operator, ExpressionOp::Plus) {
                    unsigned(lhs)?.checked_add(unsigned(rhs)?)
                } else {
                    unsigned(lhs)?.checked_sub(unsigned(rhs)?)
                };
                values.push(arithmetic_result(result, &lhs.ty())?);
            }
            ExpressionOp::StackValue => (),
            _ => unreachable!("expression operators were validated when recorded"),
        }
    }
    match values.as_slice() {
        [Value::Immediate(value)] => Ok(Observation::Value((*value).into())),
        [Value::Poison { .. }] => Ok(Observation::Unavailable),
        _ => Err(Report::msg("debug semantic test expression must produce exactly one value")),
    }
}

fn unsigned(value: Value) -> Result<u64, Report> {
    match value {
        Value::Immediate(value) => value.as_u64().ok_or_else(|| {
            Report::msg("debug semantic test requires an unsigned expression value")
        }),
        Value::Poison { .. } => Err(Report::msg("poison in debug recovery expression")),
    }
}

fn arithmetic_result(value: Option<u64>, ty: &Type) -> Result<Value, Report> {
    let value = value.ok_or_else(|| Report::msg("debug recovery expression overflow"))?;
    Value::cast_immediate(Immediate::U64(value), ty)
        .map(Value::Immediate)
        .map_err(Report::new)
}

#[derive(Clone)]
struct Evaluation {
    results: SmallVec<[Value; 1]>,
    snapshots: Vec<Snapshot>,
}

impl Evaluation {
    fn compare(&self, actual: &Self) -> Result<(), Report> {
        if self.results != actual.results {
            return Err(Report::msg("debug semantic test changed program results"));
        }
        if self.snapshots.is_empty() || self.snapshots.len() != actual.snapshots.len() {
            return Err(Report::msg("debug semantic test changed the checkpoint sequence"));
        }
        for (expected, actual) in self.snapshots.iter().zip(&actual.snapshots) {
            if expected.checkpoint != actual.checkpoint
                || expected.frames.len() != actual.frames.len()
            {
                return Err(Report::msg("debug semantic test changed the checkpoint sequence"));
            }
            for (expected_frame, actual_frame) in expected.frames.iter().zip(&actual.frames) {
                if expected_frame.function != actual_frame.function
                    || expected_frame.variables.len() != actual_frame.variables.len()
                {
                    return Err(Report::msg(format!(
                        "debug semantic test changed bindings at {:?}",
                        expected.checkpoint
                    )));
                }
                for (binding, value) in &expected_frame.variables {
                    let actual_value =
                        actual_frame.variables.iter().find_map(|(actual_binding, value)| {
                            (actual_binding == binding).then_some(value)
                        });
                    if actual_value != Some(value) {
                        return Err(Report::msg(format!(
                            "debug semantic mismatch for '{}' at {:?} in {}: expected {value:?}, \
                             got {actual_value:?}",
                            binding.variable.name, expected.checkpoint, expected_frame.function
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn expect_unavailable(&mut self, name: &str) {
        let bindings = self
            .snapshots
            .iter()
            .flat_map(|snapshot| &snapshot.frames)
            .flat_map(|frame| &frame.variables)
            .filter(|(binding, _)| binding.variable.name.as_str() == name)
            .map(|(binding, _)| binding.clone())
            .collect::<Vec<_>>();
        let binding = bindings.first().expect("expected a tracked binding");
        assert!(
            bindings.iter().all(|candidate| candidate == binding),
            "an invalidation must identify one unambiguous binding"
        );
        self.expect_binding_unavailable(binding);
    }

    fn expect_binding_unavailable(&mut self, expected: &Binding) {
        let mut changed = 0;
        for snapshot in &mut self.snapshots {
            for frame in &mut snapshot.frames {
                for (binding, value) in &mut frame.variables {
                    if binding == expected && matches!(value, Observation::Value(_)) {
                        *value = Observation::Unavailable;
                        changed += 1;
                    }
                }
            }
        }
        assert!(changed > 0, "the expected invalidation must affect an observed value");
    }
}

struct Scenario {
    context: Rc<Context>,
    module: ModuleRef,
    function: FunctionRef,
}

impl Scenario {
    fn new(source: &str, name: &str) -> Result<Self, Report> {
        let test = Test::default();
        let context = test.context_rc();
        context.get_or_register_dialect::<debuginfo::DebugInfoDialect>();
        context.get_or_register_dialect::<arith::ArithDialect>();
        context.get_or_register_dialect::<cf::ControlFlowDialect>();
        context.get_or_register_dialect::<scf::ScfDialect>();
        context.get_or_register_dialect::<hir::HirDialect>();
        let module = parse::<Module>(
            ParserConfig::new(context.clone()),
            Uri::new("debug_semantics.hir"),
            source,
        )?;
        let function = module
            .borrow()
            .get(SymbolName::intern(name))
            .unwrap()
            .borrow()
            .as_operation_ref();
        let function = function.borrow().downcast_ref::<Function>().unwrap().as_function_ref();
        bind_locals(function);
        Ok(Self {
            context,
            module,
            function,
        })
    }

    fn evaluate(&self, args: &[Value]) -> Result<Evaluation, Report> {
        let mut evaluator = HirEvaluator::new(self.context.clone());
        evaluator.debug_trace = Some(DebugTrace::default());
        let path = self.function.borrow().path();
        let results =
            evaluator.call(self.module.borrow().as_operation(), &path, args.iter().copied())?;
        Ok(Evaluation {
            results,
            snapshots: evaluator.debug_trace.take().unwrap().snapshots,
        })
    }

    fn rewrite(&self, passes: Vec<Box<dyn OperationPass>>) -> Result<(), Report> {
        let mut manager = PassManager::on::<Function>(self.context.clone(), Nesting::Implicit);
        for pass in passes {
            manager.add_pass(pass);
        }
        manager.enable_verifier(true);
        manager.run(self.function.as_operation_ref())
    }
}

fn bind_locals(mut function: FunctionRef) {
    let mut slots = BTreeMap::new();
    let mut operations = Vec::new();
    function
        .as_operation_ref()
        .raw_prewalk_all::<Forward, _>(|operation: OperationRef| {
            let op = operation.borrow();
            if let Some(local) =
                op.get_typed_attribute::<builtin::attributes::LocalVariableAttr>("local")
            {
                let local = local.borrow();
                if !local.as_value().is_bound() {
                    slots.insert(local.as_value().as_usize(), local.local_type());
                    operations.push(operation);
                }
            }
        });
    for (index, ty) in slots {
        assert_eq!(function.borrow().num_locals(), index);
        function.borrow_mut().alloc_local(ty);
    }
    for mut operation in operations {
        let index = operation
            .borrow()
            .get_typed_attribute::<builtin::attributes::LocalVariableAttr>("local")
            .unwrap()
            .borrow()
            .as_value()
            .as_usize();
        let local = function.borrow().iter_locals().nth(index).unwrap();
        let mut op = operation.borrow_mut();
        if let Some(store) = op.downcast_mut::<hir::StoreLocal>() {
            store.set_local(local);
        } else if let Some(load) = op.downcast_mut::<hir::LoadLocal>() {
            load.set_local(local);
        } else {
            panic!("unsupported local operation in semantic fixture");
        }
    }
}

fn compare_rewrite(
    scenario: &Scenario,
    inputs: &[Vec<Value>],
    passes: Vec<Box<dyn OperationPass>>,
    unavailable: &[&str],
) -> Result<Vec<Evaluation>, Report> {
    assert!(!inputs.is_empty());
    assert!(!passes.is_empty());
    let before = inputs
        .iter()
        .map(|args| scenario.evaluate(args))
        .collect::<Result<Vec<_>, _>>()?;
    for pass in passes {
        let name = pass.name();
        let original = scenario.function.borrow().as_operation().to_string();
        scenario.rewrite(vec![pass])?;
        assert_ne!(
            original,
            scenario.function.borrow().as_operation().to_string(),
            "pass '{name}' did not exercise a rewrite"
        );
        for (args, expected) in inputs.iter().zip(&before) {
            let actual = scenario.evaluate(args)?;
            let mut expected = expected.clone();
            for name in unavailable {
                expected.expect_unavailable(name);
            }
            expected.compare(&actual).map_err(|error| {
                Report::msg(format!("after pass '{name}', inputs {args:?}: {error}"))
            })?;
        }
    }
    Ok(before)
}

const SOURCE: &str = include_str!("../../../tests/lit/hir-opt/debug-info-semantic-values.hir");

#[test]
fn cse_preserves_observed_values_across_edge_inputs() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    let edges = [0u32, 1, 7, u32::MAX, 1 << 31];
    let mut inputs = edges
        .iter()
        .flat_map(|lhs| edges.iter().map(move |rhs| vec![(*lhs).into(), (*rhs).into()]))
        .collect::<Vec<_>>();
    inputs
        .extend((0u32..16).flat_map(|lhs| (0u32..16).map(move |rhs| vec![lhs.into(), rhs.into()])));
    let before =
        compare_rewrite(&scenario, &inputs, vec![Box::new(CommonSubexpressionElimination)], &[])?;
    for (args, observed) in inputs.iter().zip(before) {
        let expected = unsigned(args[0])? as u32 ^ unsigned(args[1])? as u32;
        assert_eq!(observed.results.as_slice(), &[expected.wrapping_add(expected).into()]);
        assert!(
            observed.snapshots.last().unwrap().frames[0]
                .variables
                .iter()
                .all(|(_, value)| *value == Observation::Value(expected.into()))
        );
    }
    Ok(())
}

#[test]
fn canonicalization_preserves_path_specific_debug_values() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "select_values")?;
    let inputs = [true, false]
        .into_iter()
        .flat_map(|condition| {
            [0u32, 7, u32::MAX]
                .into_iter()
                .map(move |value| vec![value.into(), 42u32.into(), condition.into(), 0u32.into()])
        })
        .collect::<Vec<_>>();
    let mut pass = Canonicalizer::default();
    Pass::initialize_options(&mut pass, "enable-patterns=convert-trivial-if-to-select")?;
    let before = compare_rewrite(&scenario, &inputs, vec![Box::new(pass)], &[])?;
    for (args, observed) in inputs.iter().zip(before) {
        let expected = if args[2] == true.into() {
            args[0]
        } else {
            args[1]
        };
        assert_eq!(observed.results.as_slice(), &[expected]);
        let snapshots = observed.snapshots;
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].frames[0].variables[0].1, Observation::Value(expected));
        assert_eq!(
            snapshots[1].frames[0].variables.last().unwrap().1,
            Observation::Value(expected)
        );
    }
    Ok(())
}

#[test]
fn local_promotion_preserves_value_updates() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "promoted_locals")?;
    let inputs = [0u32, 1, 42, u32::MAX]
        .into_iter()
        .map(|value| vec![value.into()])
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(hir::transforms::Local2Reg)], &[])?;
    Ok(())
}

#[test]
fn sinking_requires_explicitly_expected_unavailability() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "sunk_value")?;
    compare_rewrite(
        &scenario,
        &[vec![11u32.into()]],
        vec![Box::new(SinkOperandDefs)],
        &["constant"],
    )?;
    Ok(())
}

#[test]
fn dead_values_require_explicitly_expected_unavailability() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "dead_value")?;
    compare_rewrite(
        &scenario,
        &[vec![11u32.into()]],
        vec![Box::new(DeadCodeElimination)],
        &["dead"],
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Fault {
    WrongEquivalent,
    RestoreEquivalent,
    RecoverOffset,
    WrongRecovery,
    UnsupportedRecovery,
    DropValue,
    LateCapture,
    MissingCheckpoint,
}

struct FaultyRewrite(Fault);

impl Pass for FaultyRewrite {
    type Target = Function;

    fn name(&self) -> &'static str {
        "test-faulty-debug-semantics"
    }

    fn can_schedule_on(&self, _: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        function: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        let replacement = function.entry_block().borrow().arguments()[1] as ValueRef;
        drop(function);
        let mut record = None;
        let mut checkpoint = None;
        root.raw_prewalk_all::<Forward, _>(|operation: OperationRef| {
            if record.is_none() && operation.borrow().is::<DebugValue>() {
                record = Some(operation);
            }
            if operation.borrow().get_attribute(CHECKPOINT_ATTR).is_some() {
                checkpoint = Some(operation);
            }
        });
        let mut record = record.unwrap();
        match self.0 {
            Fault::WrongEquivalent | Fault::RestoreEquivalent => {
                let _contract = DebugInfoMutation::new(record, DebugInfoUpdate::Equivalent);
                let replacement = if matches!(self.0, Fault::RestoreEquivalent) {
                    let mut original = None;
                    root.raw_prewalk_all::<Forward, _>(|operation: OperationRef| {
                        if original.is_none() && operation.borrow().is::<arith::Bxor>() {
                            original = Some(operation.borrow().results()[0] as ValueRef);
                        }
                    });
                    original.unwrap()
                } else {
                    replacement
                };
                let mut operand = record.borrow().operands()[0];
                operand.borrow_mut().set(replacement);
            }
            Fault::RecoverOffset => {
                use midenc_dialect_arith::ArithOpBuilder;
                use midenc_hir::Builder;
                let value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
                rewriter.set_insertion_point_before(record);
                let offset = rewriter.u32(5, SourceSpan::UNKNOWN);
                let relocated = rewriter.add_wrapping(value, offset, SourceSpan::UNKNOWN)?;
                debuginfo::transform::salvage_debug_info(
                    &value,
                    &debuginfo::transform::SalvageAction::OffsetBy {
                        new_value: relocated,
                        offset: 5,
                    },
                    &mut rewriter,
                );
            }
            Fault::WrongRecovery | Fault::UnsupportedRecovery => {
                let _contract = DebugInfoMutation::new(record, DebugInfoUpdate::Salvage);
                let operator = if matches!(self.0, Fault::WrongRecovery) {
                    ExpressionOp::PlusUConst(1)
                } else {
                    ExpressionOp::Unsupported(midenc_hir::interner::Symbol::intern("DW_OP_unknown"))
                };
                record
                    .borrow_mut()
                    .downcast_mut::<DebugValue>()
                    .unwrap()
                    .set_expression(Expression::with_ops(vec![operator]));
            }
            Fault::DropValue => {
                let value =
                    record.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
                debuginfo::transform::erase_debug_info_with(&value, &mut rewriter);
            }
            Fault::LateCapture => {
                let _contract = DebugInfoMutation::new(record, DebugInfoUpdate::Relocate);
                record.borrow_mut().move_to(ProgramPoint::after(checkpoint.unwrap()));
            }
            Fault::MissingCheckpoint => {
                checkpoint.unwrap().borrow_mut().remove_attribute(CHECKPOINT_ATTR);
            }
        }
        state.set_post_pass_status(PostPassStatus::Changed);
        Ok(())
    }
}

#[test]
fn false_equivalence_is_detected_even_when_results_and_audit_pass() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    let inputs = [1u32.into(), 0u32.into()];
    let expected = scenario.evaluate(&inputs)?;
    scenario.rewrite(vec![Box::new(FaultyRewrite(Fault::WrongEquivalent))])?;
    let actual = scenario.evaluate(&inputs)?;
    assert_eq!(expected.results, actual.results);
    let error = expected.compare(&actual).unwrap_err();
    assert!(error.to_string().contains("debug semantic mismatch for 'value'"), "{error}");
    assert!(error.to_string().contains("Marker(1)"), "{error}");
    Ok(())
}

#[test]
fn incorrect_salvage_expressions_are_detected() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    let inputs = [7u32.into(), 2u32.into()];
    let expected = scenario.evaluate(&inputs)?;
    scenario.rewrite(vec![Box::new(FaultyRewrite(Fault::WrongRecovery))])?;
    let actual = scenario.evaluate(&inputs)?;
    assert_eq!(expected.results, actual.results);
    assert!(
        expected
            .compare(&actual)
            .unwrap_err()
            .to_string()
            .contains("debug semantic mismatch")
    );
    Ok(())
}

#[test]
fn correct_inverse_recovery_expressions_preserve_observed_values() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    compare_rewrite(
        &scenario,
        &[vec![7u32.into(), 2u32.into()]],
        vec![Box::new(FaultyRewrite(Fault::RecoverOffset))],
        &[],
    )?;
    Ok(())
}

#[test]
fn a_later_repair_cannot_hide_an_incorrect_intermediate_pass() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    let expected = scenario.evaluate(&[1u32.into(), 0u32.into()])?;
    let error = compare_rewrite(
        &scenario,
        &[vec![1u32.into(), 0u32.into()]],
        vec![
            Box::new(FaultyRewrite(Fault::WrongEquivalent)),
            Box::new(FaultyRewrite(Fault::RestoreEquivalent)),
        ],
        &[],
    )
    .err()
    .unwrap();
    assert!(
        error.to_string().contains("after pass 'test-faulty-debug-semantics'"),
        "{error}"
    );
    scenario.rewrite(vec![Box::new(FaultyRewrite(Fault::RestoreEquivalent))])?;
    expected.compare(&scenario.evaluate(&[1u32.into(), 0u32.into()])?)?;
    Ok(())
}

#[test]
fn unavailable_values_require_a_binding_specific_expectation() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    let inputs = [7u32.into(), 2u32.into()];
    let mut expected = scenario.evaluate(&inputs)?;
    let binding = expected.snapshots[0].frames[0].variables[0].0.clone();
    scenario.rewrite(vec![Box::new(FaultyRewrite(Fault::DropValue))])?;
    let actual = scenario.evaluate(&inputs)?;
    assert!(expected.compare(&actual).is_err());
    expected.expect_binding_unavailable(&binding);
    expected.compare(&actual)?;
    Ok(())
}

#[test]
fn moved_captures_and_missing_checkpoints_are_detected() -> Result<(), Report> {
    for fault in [Fault::LateCapture, Fault::MissingCheckpoint] {
        let scenario = Scenario::new(SOURCE, "cse_values")?;
        let inputs = [7u32.into(), 2u32.into()];
        let expected = scenario.evaluate(&inputs)?;
        scenario.rewrite(vec![Box::new(FaultyRewrite(fault))])?;
        let actual = scenario.evaluate(&inputs)?;
        assert_eq!(expected.results, actual.results);
        assert!(expected.compare(&actual).is_err());
    }
    Ok(())
}

#[test]
fn unsupported_expressions_do_not_silently_pass_the_oracle() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "cse_values")?;
    scenario.rewrite(vec![Box::new(FaultyRewrite(Fault::UnsupportedRecovery))])?;
    let error = scenario.evaluate(&[7u32.into(), 2u32.into()]).err().unwrap();
    assert!(error.to_string().contains("cannot evaluate expression operator"), "{error}");
    Ok(())
}

#[test]
fn loops_snapshot_values_instead_of_retaining_mutable_register_references() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "loop_values")?;
    let observed = scenario.evaluate(&[3u32.into()])?;
    let values = observed
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.checkpoint == Checkpoint::Marker(1))
        .map(|snapshot| snapshot.frames[0].variables[0].1.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            Observation::Value(0u32.into()),
            Observation::Value(1u32.into()),
            Observation::Value(2u32.into())
        ]
    );
    Ok(())
}

#[test]
fn local_declarations_read_storage_at_each_checkpoint() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "local_updates")?;
    for value in [0u32, 7, u32::MAX] {
        let observed = scenario.evaluate(&[value.into()])?;
        assert_eq!(
            observed.snapshots[0].frames[0].variables[0].1,
            Observation::Value(value.into())
        );
        assert_eq!(
            observed.snapshots[1].frames[0].variables[0].1,
            Observation::Value(value.wrapping_add(1).into())
        );
    }
    Ok(())
}

#[test]
fn repeated_calls_do_not_leak_variables_between_invocations() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "nested_values")?;
    let observed = scenario.evaluate(&[7u32.into()])?;
    let snapshots = &observed.snapshots;
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].frames[0].variables[0].1, Observation::Value(7u32.into()));
    assert_eq!(snapshots[0].frames[1].variables[0].1, Observation::Value(7u32.into()));
    assert!(snapshots[1].frames[1].variables.is_empty());
    assert_eq!(snapshots[2].frames[0].variables[0].1, Observation::Value(7u32.into()));
    Ok(())
}

#[test]
#[should_panic(expected = "an invalidation must identify one unambiguous binding")]
fn expected_unavailability_cannot_mask_another_functions_variable() {
    let scenario = Scenario::new(SOURCE, "nested_values").unwrap();
    let mut observed = scenario.evaluate(&[7u32.into()]).unwrap();
    observed.expect_unavailable("shared");
}

#[test]
fn empty_observations_and_unbounded_execution_are_rejected() -> Result<(), Report> {
    let empty = Evaluation {
        results: SmallVec::new(),
        snapshots: Vec::new(),
    };
    assert!(empty.compare(&empty).is_err());
    let scenario = Scenario::new(
        r#"
builtin.module public @test {
    builtin.function public extern("C") @forever() -> u32 {
        cf.br ^repeat;
    ^repeat:
        cf.br ^repeat;
    };
};
"#,
        "forever",
    )?;
    let error = scenario.evaluate(&[]).err().unwrap();
    assert!(error.to_string().contains("instruction limit"), "{error}");
    Ok(())
}

#[test]
fn spill_reload_and_phi_rewrites_preserve_concrete_debug_values() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/transform-spills-debug-values.hir");
    for name in ["tracked_reload", "tracked_join", "tracked_phi"] {
        let scenario = Scenario::new(source, name)?;
        let mut inputs = Vec::new();
        for kept in [0u32, 1, u32::MAX - 42] {
            for wide in [0u128, 7, 1u128 << 120] {
                if name == "tracked_reload" {
                    inputs.push(vec![kept.into(), wide.into()]);
                } else {
                    for condition in [true, false] {
                        inputs.push(vec![condition.into(), wide.into(), kept.into()]);
                    }
                }
            }
        }
        let unavailable = match name {
            "tracked_reload" => &["after"][..],
            "tracked_join" => &["kept"][..],
            _ => &[],
        };
        compare_rewrite(
            &scenario,
            &inputs,
            vec![Box::new(hir::transforms::TransformSpills)],
            unavailable,
        )?;
    }
    Ok(())
}

#[test]
fn local_promotion_preserves_values_wider_than_one_felt() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "wide_local")?;
    let inputs = [0u128, u64::MAX as u128 + 1, 1 << 127, u128::MAX]
        .into_iter()
        .map(|value| vec![value.into()])
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(hir::transforms::Local2Reg)], &[])?;
    Ok(())
}
