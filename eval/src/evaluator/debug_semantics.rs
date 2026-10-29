use alloc::{boxed::Box, collections::BTreeMap};

use midenc_hir::{
    EntityMut, Forward, Op, OperationName, ProgramPoint, RawWalk, Region, Symbol as _, SymbolName,
    SymbolTable,
    diagnostics::Uri,
    dialects::{
        builtin::{Function, FunctionRef, Module, ModuleRef, attributes::U32Attr},
        debuginfo::{
            DebugDeclare, DebugInfoMutation, DebugInfoUpdate, DebugKill, DebugValue,
            attributes::{
                Expression, ExpressionOp, FrameBase, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain,
                InlineCallChainAttr, Variable,
            },
        },
    },
    parse::{ParserConfig, parse},
    pass::{Nesting, OperationPass, Pass, PassExecutionState, PassManager, PostPassStatus},
    patterns::{NoopRewriterListener, RegionSimplificationLevel, RewriterImpl},
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
        ty: Option<Type>,
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
                    ty: debug.variable().as_value().ty.clone(),
                },
            ))
        } else if let Some(debug) = operation.downcast_ref::<DebugDeclare>() {
            Some((
                Binding::new(operation, debug.variable().as_value().clone(), frame),
                Location::Value {
                    seed: None,
                    expression: debug.expression().as_value().clone(),
                    ty: debug.variable().as_value().ty.clone(),
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
                            | ExpressionOp::FrameBase {
                                base: FrameBase::LocalSlot(_),
                                ..
                            }
                            | ExpressionOp::Address { .. }
                            | ExpressionOp::Deref
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
    let Location::Value {
        seed,
        expression,
        ty,
    } = location
    else {
        return Ok(Observation::Unavailable);
    };
    // A source-bearing expression supplies its own input. Otherwise the captured SSA value
    // supplies the one missing operand, exactly as in debug expression lowering.
    let mut depth = 0isize;
    let mut required = 0isize;
    for operator in &expression.operations {
        let (inputs, outputs) = match operator {
            ExpressionOp::Plus | ExpressionOp::Minus => (2, 1),
            ExpressionOp::PlusUConst(_) | ExpressionOp::Deref | ExpressionOp::StackValue => (1, 1),
            _ => (0, 1),
        };
        required = required.max(inputs - depth);
        depth += outputs - inputs;
    }
    let mut values = if expression.is_empty() || required == 1 {
        seed.iter().map(|value| (*value, false)).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let read_local = |index: u32| -> Result<Value, Report> {
        let callee = frame.callee();
        let function = callee.downcast_ref::<Function>().unwrap();
        let local = function.iter_locals().nth(index as usize).ok_or_else(|| {
            Report::msg(format!("debug semantic test references missing local {index}"))
        })?;
        frame.read_local(&local, evaluator.current_span(), &evaluator.context)
    };
    let read_address = |value: Value, ty: &Type| -> Result<Value, Report> {
        let address = u32::try_from(unsigned(value)?)
            .map_err(|_| Report::msg("debug address exceeds u32"))?;
        evaluator.read_memory(address, ty)
    };
    for (index, operator) in expression.operations.iter().enumerate() {
        match operator {
            ExpressionOp::LocalSlot(index) => values.push((read_local(*index)?, false)),
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(index),
                byte_offset,
            } => {
                let base = read_local(*index)?;
                let address = unsigned(base)?.checked_add_signed(*byte_offset);
                values.push((arithmetic_result(address, &base.ty())?, true));
            }
            ExpressionOp::Address { address } => values.push(((*address).into(), true)),
            ExpressionOp::ConstU64(value) => values.push(((*value).into(), false)),
            ExpressionOp::PlusUConst(offset) => {
                let (value, address) =
                    values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let result = unsigned(value)?.checked_add(*offset);
                values.push((arithmetic_result(result, &value.ty())?, address));
            }
            ExpressionOp::Plus | ExpressionOp::Minus => {
                let (rhs, rhs_address) =
                    values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let (lhs, lhs_address) =
                    values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                let (result, address) = if matches!(operator, ExpressionOp::Plus) {
                    if lhs_address && rhs_address {
                        return Err(Report::msg("adding two debug addresses"));
                    }
                    (unsigned(lhs)?.checked_add(unsigned(rhs)?), lhs_address || rhs_address)
                } else {
                    if !lhs_address && rhs_address {
                        return Err(Report::msg("subtracting address from debug value"));
                    }
                    (unsigned(lhs)?.checked_sub(unsigned(rhs)?), lhs_address && !rhs_address)
                };
                values.push((arithmetic_result(result, &lhs.ty())?, address));
            }
            ExpressionOp::Deref => {
                let (value, address) =
                    values.pop().ok_or_else(|| Report::msg("empty expression stack"))?;
                // An intermediate dereference reads a byte-address pointer. A terminal scalar
                // dereference uses the source type. Deref itself preserves the location kind.
                let terminal = index + 1 == expression.operations.len();
                let read_ty = if terminal && !address {
                    ty.as_ref().unwrap_or(&Type::U32)
                } else {
                    &Type::U32
                };
                values.push((read_address(value, read_ty)?, address));
            }
            ExpressionOp::StackValue => {
                values.last_mut().ok_or_else(|| Report::msg("empty expression stack"))?.1 = false;
            }
            _ => unreachable!("expression operators were validated when recorded"),
        }
    }
    match values.as_slice() {
        [(Value::Poison { .. }, _)] => Ok(Observation::Unavailable),
        [(value, true)] => {
            Ok(Observation::Value(read_address(*value, ty.as_ref().unwrap_or(&Type::U32))?))
        }
        [(value, false)] => Ok(Observation::Value(*value)),
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
fn canonicalizer_contracts_preserve_values_and_explicit_kills() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/canonicalizer-debug-contracts.hir");
    for name in ["single_pred", "bool_compare", "bool_trunc", "bool_select"] {
        let scenario = Scenario::new(source, name)?;
        let inputs = [false, true]
            .into_iter()
            .map(|condition| match name {
                "single_pred" => vec![condition.into(), 7u32.into(), u32::MAX.into()],
                "redundant_capture" => vec![condition.into(), u32::MAX.into()],
                _ => vec![condition.into()],
            })
            .collect::<Vec<_>>();
        compare_rewrite(
            &scenario,
            &inputs,
            vec![Box::new(Canonicalizer::new(Default::default(), true))],
            &[],
        )?;
    }
    for name in ["constant_switch", "constant_switch_tracked_local", "switch_unused", "bool_not"] {
        let scenario = Scenario::new(source, name)?;
        let inputs = [0u32, 1, 7, u32::MAX - 1]
            .into_iter()
            .map(|value| match name {
                "constant_switch" => vec![value.into()],
                "constant_switch_tracked_local" => vec![value.into(), 7u32.into()],
                "switch_unused" => vec![value.into(), 5u32.into(), value.into()],
                _ => vec![value.into(), 7u32.into()],
            })
            .collect::<Vec<_>>();
        compare_rewrite(
            &scenario,
            &inputs,
            vec![Box::new(Canonicalizer::new(Default::default(), true))],
            if name == "switch_unused" {
                &["dropped"]
            } else {
                &[]
            },
        )?;
    }
    Ok(())
}

struct AggressiveRegionSimplification;

impl Pass for AggressiveRegionSimplification {
    type Target = Function;

    fn name(&self) -> &'static str {
        "test-aggressive-region-simplification"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        function: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let context = function.as_operation().context_rc();
        let region = function.body().as_region_ref();
        drop(function);
        let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
        Region::simplify_all(&[region], &mut rewriter, RegionSimplificationLevel::Aggressive)
            .map_err(|_| Report::msg("test requires an aggressively simplifiable region"))?;
        state.set_post_pass_status(PostPassStatus::Changed);
        Ok(())
    }
}

#[test]
fn aggressive_region_simplification_preserves_captured_arguments() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/debug-info-aggressive-region.hir");
    let scenario = Scenario::new(source, "entrypoint")?;
    let inputs = [false, true]
        .into_iter()
        .flat_map(|condition| {
            [0u32, 7, u32::MAX]
                .into_iter()
                .map(move |value| vec![condition.into(), value.into()])
        })
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(AggressiveRegionSimplification)], &[])?;
    Ok(())
}

#[test]
fn redundant_yields_invalidate_captures_without_retaining_computation() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/canonicalizer-debug-contracts.hir");
    for targeted in [false, true] {
        let scenario = Scenario::new(source, "redundant_capture")?;
        let inputs = [false, true]
            .into_iter()
            .map(|condition| vec![condition.into(), 7u32.into()])
            .collect::<Vec<_>>();
        let before = inputs
            .iter()
            .map(|args| scenario.evaluate(args))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pass = Canonicalizer::default();
        if targeted {
            Pass::initialize_options(&mut pass, "enable-patterns=fold-redundant-yields")?;
        }
        scenario.rewrite(vec![Box::new(pass)])?;
        let printed = scenario.function.borrow().as_operation().to_string();
        assert!(!printed.contains("scf.if"), "{printed}");
        for (args, expected) in inputs.iter().zip(before) {
            let actual = scenario.evaluate(args)?;
            assert_eq!(expected.results, actual.results);
            assert!(
                actual.snapshots.last().unwrap().frames[0]
                    .variables
                    .iter()
                    .all(|(_, value)| *value == Observation::Unavailable)
            );
        }
        let stripped = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("di."))
            .collect::<Vec<_>>()
            .join("\n");
        let plain = Scenario::new(&stripped, "redundant_capture")?;
        let mut pass = Canonicalizer::default();
        if targeted {
            Pass::initialize_options(&mut pass, "enable-patterns=fold-redundant-yields")?;
        }
        plain.rewrite(vec![Box::new(pass)])?;
        let executable = printed
            .lines()
            .filter(|line| !line.trim_start().starts_with("di."))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(executable, plain.function.borrow().as_operation().to_string());
    }
    Ok(())
}

#[test]
fn redundant_yields_preserve_external_result_captures() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/canonicalizer-debug-contracts.hir")
        .replace(
            "        builtin.ret %result : (u32);",
            "        di.debug_value %result <{ variable = #di.variable<{ name = \"result\", file \
             = \"test.rs\", line = 3 }>, expression = #di.expression<[]> }> : (u32);\n        \
             builtin.ret %result : (u32);",
        );
    let scenario = Scenario::new(&source, "redundant_capture")?;
    let mut pass = Canonicalizer::default();
    Pass::initialize_options(&mut pass, "enable-patterns=fold-redundant-yields")?;
    scenario.rewrite(vec![Box::new(pass)])?;
    let result = scenario.evaluate(&[true.into(), 7u32.into()])?;
    let value = result.snapshots.last().unwrap().frames[0]
        .variables
        .iter()
        .find(|(binding, _)| binding.variable.name.as_str() == "result")
        .unwrap();
    assert_eq!(value.1, Observation::Value(7u32.into()));
    assert!(!scenario.function.borrow().as_operation().to_string().contains("scf.if"));
    Ok(())
}

#[test]
fn cse_coalesces_redundant_nested_captures() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir");
    let scenario = Scenario::new(source, "nested_captures")?;
    let inputs = [false, true]
        .into_iter()
        .map(|condition| vec![condition.into(), 7u32.into(), u32::MAX.into()])
        .collect::<Vec<_>>();
    let before = inputs
        .iter()
        .map(|args| scenario.evaluate(args))
        .collect::<Result<Vec<_>, _>>()?;
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let printed = scenario.function.borrow().as_operation().to_string();
    assert_eq!(printed.matches("scf.if").count(), 1, "{printed}");
    assert_eq!(printed.matches("di.debug_value").count(), 1, "{printed}");
    assert_eq!(printed.matches("di.debug_kill").count(), 1, "{printed}");
    for (args, expected) in inputs.iter().zip(before) {
        expected.compare(&scenario.evaluate(args)?)?;
    }
    Ok(())
}

#[test]
fn cse_preserves_captures_across_unrelated_records() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-unrelated-capture.hir");
    let inputs = [false, true]
        .into_iter()
        .map(|condition| vec![condition.into(), 7u32.into(), 19u32.into()])
        .collect::<Vec<_>>();
    for nested in [false, true] {
        for distinct_frame in [false, true] {
            let record = r#"di.debug_value %right <{ variable = #di.variable<{ name = "other", file = "test.rs", line = 1 }>, expression = #di.expression<[]> }> : (u32);"#;
            let source = if nested {
                source
                    .replacen(
                        record,
                        &format!(
                            "%intervening = scf.if %condition then {{ {record} scf.yield %right : \
                             (u32); }} else {{ scf.yield %left : (u32); }} : (i1) -> u32;"
                        ),
                        1,
                    )
                    .replacen(
                        "builtin.ret %sum",
                        "%with_other = arith.add %sum, %intervening <{ overflow = \
                         #builtin.overflow<wrapping> }>; builtin.ret %with_other",
                        1,
                    )
            } else {
                source.to_string()
            };
            let scenario = Scenario::new(&source, "unrelated")?;
            if distinct_frame {
                // The same name in another record-owned frame does not shadow branch.
                // Executable owners have no inline provenance; the key must come from the record.
                use midenc_hir::dialects::debuginfo::attributes::{InlineCallFrame, VariableAttr};
                scenario.function.as_operation_ref().raw_prewalk_all::<Forward, _>(
                    |mut op: OperationRef| {
                        let variable =
                            midenc_hir::dialects::debuginfo::debug_record_variable(&op.borrow());
                        if let Some(mut variable) =
                            variable.filter(|variable| variable.name.as_str() == "other")
                        {
                            variable.name = "branch".into();
                            let attr =
                                scenario.context.create_attribute::<VariableAttr, _>(variable);
                            op.borrow_mut()
                                .set_property("variable", attr.as_attribute_ref())
                                .unwrap();
                            let chain = scenario
                                .context
                                .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(
                                    vec![InlineCallFrame {
                                        name: "callee".into(),
                                        linkage_name: None,
                                        file: "callee.rs".into(),
                                        line: 1,
                                        column: 1,
                                        call_file: "caller.rs".into(),
                                        call_line: 10,
                                        call_column: 1,
                                    }],
                                ))
                                .as_attribute_ref();
                            op.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
                        }
                    },
                );
            }
            if let Err(error) = compare_rewrite(
                &scenario,
                &inputs,
                vec![Box::new(CommonSubexpressionElimination)],
                &[],
            ) {
                std::eprintln!(
                    "nested={nested} distinct_frame={distinct_frame}: {}",
                    scenario.function.borrow().as_operation()
                );
                return Err(error);
            }
            assert_eq!(
                scenario.function.borrow().as_operation().to_string().matches("scf.if").count(),
                if nested { 2 } else { 1 }
            );
        }
    }
    Ok(())
}

#[test]
fn cse_retains_live_declarations_across_stores() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir");
    let scenario = Scenario::new(source, "nested_declarations")?;
    let inputs = [false, true]
        .into_iter()
        .map(|condition| vec![condition.into(), 7u32.into(), 19u32.into()])
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(CommonSubexpressionElimination)], &[])?;
    assert_eq!(
        scenario.function.borrow().as_operation().to_string().matches("scf.if").count(),
        1
    );
    let result = scenario.evaluate(&inputs[1])?;
    assert_eq!(
        result.snapshots.last().unwrap().frames[0].variables[0].1,
        Observation::Value(19u32.into())
    );
    Ok(())
}

#[test]
fn cse_invalidates_unproved_capture_effects_without_retaining_computation() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-capture-effects.hir");
    for name in ["killed", "rebound", "asymmetric", "unmatched"] {
        let scenario = Scenario::new(source, name)?;
        let inputs = [false, true]
            .into_iter()
            .map(|condition| vec![condition.into(), 7u32.into(), 19u32.into()])
            .collect::<Vec<_>>();
        let before = inputs
            .iter()
            .map(|args| scenario.evaluate(args))
            .collect::<Result<Vec<_>, _>>()?;
        scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
        let printed = scenario.function.borrow().as_operation().to_string();
        assert_eq!(printed.matches("scf.if").count(), 1, "{name}: {printed}");
        for (args, expected) in inputs.iter().zip(before) {
            let actual = scenario.evaluate(args)?;
            assert_eq!(expected.results, actual.results);
            for (binding, value) in &actual.snapshots.last().unwrap().frames[0].variables {
                // A conservative fallback may lose availability, but may never report a stale value.
                if *value != Observation::Unavailable {
                    let expected_value = expected.snapshots.last().unwrap().frames[0]
                        .variables
                        .iter()
                        .find(|(candidate, _)| candidate == binding)
                        .map(|(_, value)| value);
                    assert_eq!(expected_value, Some(value), "{name}: {binding:?}");
                }
            }
        }
    }
    Ok(())
}

#[test]
fn cse_executable_regions_are_independent_of_debug_records() -> Result<(), Report> {
    for (source, names) in [
        (
            include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir"),
            &["nested_captures", "nested_declarations"][..],
        ),
        (
            include_str!("../../../tests/lit/hir-opt/cse-capture-effects.hir"),
            &["killed", "rebound", "asymmetric", "unmatched"][..],
        ),
    ] {
        let stripped = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("di."))
            .collect::<Vec<_>>()
            .join("\n");
        for name in names {
            let recorded = Scenario::new(source, name)?;
            let plain = Scenario::new(&stripped, name)?;
            recorded.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
            plain.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
            let executable = recorded
                .function
                .borrow()
                .as_operation()
                .to_string()
                .lines()
                .filter(|line| !line.trim_start().starts_with("di."))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(executable, plain.function.borrow().as_operation().to_string(), "{name}");
        }
    }
    Ok(())
}

#[test]
fn cse_maps_nested_ssa_captures() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir")
        .replace(
            "            di.debug_value %left",
            "            %inner = arith.add %left, %right <{ overflow = \
             #builtin.overflow<wrapping> }>;\n            di.debug_value %inner",
        )
        .replace("scf.yield %left", "scf.yield %inner");
    // Select only the first function: the declaration fixture has no inner SSA definition.
    let source = format!(
        "{}\n}};",
        source.split("    // CHECK-LABEL: @nested_declarations").next().unwrap()
    );
    let scenario = Scenario::new(&source, "nested_captures")?;
    let inputs = [false, true]
        .into_iter()
        .map(|condition| vec![condition.into(), 7u32.into(), 19u32.into()])
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(CommonSubexpressionElimination)], &[])?;
    assert_eq!(
        scenario.function.borrow().as_operation().to_string().matches("scf.if").count(),
        1
    );
    Ok(())
}

#[test]
fn cse_erases_queued_dead_descendants_with_their_redundant_owner() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir").replace(
        "            di.debug_value %left",
        "            %dead = arith.add %left, %right <{ overflow = #builtin.overflow<wrapping> \
         }>;\n            di.debug_value %left",
    );
    let scenario = Scenario::new(&source, "nested_captures")?;
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let printed = scenario.function.borrow().as_operation().to_string();
    assert_eq!(printed.matches("scf.if").count(), 1, "{printed}");
    assert_eq!(printed.matches("arith.add").count(), 1, "{printed}");
    Ok(())
}

#[test]
fn cse_conditional_shadow_fallback_invalidates_the_previous_binding() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-capture-effects.hir");
    let start = source.find("    builtin.function public extern(\"C\") @asymmetric").unwrap();
    let end = source[start..]
        .find("    // CHECK-LABEL:")
        .map_or(source.len() - 3, |pos| start + pos);
    let function = &source[start..end];
    let (first, second) = function.split_once("        %second").unwrap();
    let second = second.replace("name = \"other\"", "name = \"branch\"");
    let second = second
        .replace("line = 1", "line = 2")
        .lines()
        .filter(|line| !line.trim_start().starts_with("di.debug_kill"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!("builtin.module public @shadow {{\n{first}        %second{second}\n}};");
    let scenario = Scenario::new(&source, "asymmetric")?;
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let actual = scenario.evaluate(&[true.into(), 7u32.into(), 19u32.into()])?;
    for (binding, value) in &actual.snapshots.last().unwrap().frames[0].variables {
        assert_eq!(*value, Observation::Unavailable, "stale shadow binding: {binding:?}");
    }
    Ok(())
}

#[test]
fn cse_preserves_capture_inline_ownership_when_merging_instruction_locations() -> Result<(), Report>
{
    use midenc_hir::dialects::debuginfo::{attributes::InlineCallFrame, is_debug_record};
    for distinct_binding_frames in [false, true] {
        let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir");
        let scenario = Scenario::new(source, "nested_captures")?;
        let mut owners = Vec::new();
        scenario
            .function
            .as_operation_ref()
            .raw_prewalk_all::<Forward, _>(|op: OperationRef| {
                if op.name().is::<midenc_dialect_scf::If>() {
                    owners.push(op);
                }
            });
        let mut first_records: Vec<(OperationRef, midenc_hir::AttributeRef)> = Vec::new();
        for (index, mut owner) in owners.into_iter().enumerate() {
            let chain = scenario
                .context
                .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(vec![
                    InlineCallFrame {
                        name: "inlined".into(),
                        linkage_name: None,
                        file: "callee.rs".into(),
                        line: 1,
                        column: 1,
                        call_file: "caller.rs".into(),
                        call_line: 10 + index as u32,
                        call_column: 1,
                    },
                ]))
                .as_attribute_ref();
            owner.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
            owner.raw_prewalk_all::<Forward, _>(|mut op: OperationRef| {
                if is_debug_record(op) {
                    let binding_chain = if distinct_binding_frames || index == 0 {
                        chain
                    } else {
                        first_records[0].1
                    };
                    op.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, binding_chain);
                    if index == 0 {
                        first_records.push((op, chain));
                    }
                }
            });
        }
        let args = [true.into(), 7u32.into(), 19u32.into()];
        let before = scenario.evaluate(&args)?;
        scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
        assert_eq!(
            scenario.function.borrow().as_operation().to_string().matches("scf.if").count(),
            1
        );
        for (record, chain) in first_records {
            assert!(record.parent().is_some());
            assert_eq!(record.borrow().get_attribute(INLINE_CALL_CHAIN_ATTR_NAME), Some(chain));
        }
        let actual = scenario.evaluate(&args)?;
        if !distinct_binding_frames {
            before.compare(&actual)?;
        } else {
            assert_eq!(
                actual.snapshots.last().unwrap().frames[0].variables[0].1,
                Observation::Value(7u32.into())
            );
            assert_eq!(
                actual.snapshots.last().unwrap().frames[0].variables[1].1,
                Observation::Unavailable
            );
        }
    }
    Ok(())
}

#[test]
fn cse_cross_block_capture_fallback_keeps_the_original_anchor() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-nested-debug-captures.hir")
        .replace("        %second", "        cf.br ^next;\n    ^next:\n        %second");
    let scenario = Scenario::new(&source, "nested_captures")?;
    let args = [true.into(), 7u32.into(), 19u32.into()];
    let before = scenario.evaluate(&args)?;
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let printed = scenario.function.borrow().as_operation().to_string();
    assert_eq!(printed.matches("scf.if").count(), 1, "{printed}");
    let actual = scenario.evaluate(&args)?;
    assert_eq!(before.results, actual.results);
    assert_eq!(
        actual.snapshots.last().unwrap().frames[0].variables[0].1,
        Observation::Unavailable
    );
    Ok(())
}

#[test]
fn cse_capture_invalidation_stays_after_intervening_checkpoints() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-capture-effects.hir");
    let mut marked = String::new();
    for line in source.lines() {
        if line.starts_with("        di.debug_kill") {
            marked.push_str(
                "        hir.store_local %left <{ local = #builtin.local_variable<0, u32> }> \
                 attributes { test.debug_checkpoint = #builtin.u32<1> } : (u32);\n",
            );
            marked.push_str(line);
            marked.push_str(
                "\n        hir.store_local %right <{ local = #builtin.local_variable<0, u32> }> \
                 attributes { test.debug_checkpoint = #builtin.u32<2> } : (u32);\n",
            );
        } else {
            marked.push_str(line);
            marked.push('\n');
        }
    }
    let scenario = Scenario::new(&marked, "killed")?;
    let args = [true.into(), 7u32.into(), 19u32.into()];
    let before = scenario.evaluate(&args)?;
    assert_eq!(before.snapshots.len(), 3);
    assert_eq!(before.snapshots[0].frames[0].variables[0].1, Observation::Value(7u32.into()));
    assert_eq!(before.snapshots[1].frames[0].variables[0].1, Observation::Unavailable);
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let actual = scenario.evaluate(&args)?;
    assert_eq!(actual.results, before.results);
    assert_eq!(actual.snapshots.len(), 3);
    assert_eq!(actual.snapshots[..2], before.snapshots[..2]);
    assert_eq!(actual.snapshots[2].frames[0].variables[0].1, Observation::Unavailable);
    Ok(())
}

#[test]
fn cse_capture_invalidation_preserves_external_result_records() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-capture-effects.hir").replace(
        "        %sum =",
        "        di.debug_value %second <{ variable = #di.variable<{ name = \"result\", file = \
         \"test.rs\", line = 3 }>, expression = #di.expression<[]> }> : (u32);\n        %sum =",
    );
    let scenario = Scenario::new(&source, "killed")?;
    scenario.rewrite(vec![Box::new(CommonSubexpressionElimination)])?;
    let actual = scenario.evaluate(&[true.into(), 7u32.into(), 19u32.into()])?;
    let observed = &actual.snapshots.last().unwrap().frames[0].variables;
    assert_eq!(
        observed
            .iter()
            .find(|(binding, _)| binding.variable.name.as_str() == "result")
            .unwrap()
            .1,
        Observation::Value(7u32.into())
    );
    assert_eq!(
        observed
            .iter()
            .find(|(binding, _)| binding.variable.name.as_str() == "branch")
            .unwrap()
            .1,
        Observation::Unavailable
    );
    assert_eq!(
        scenario.function.borrow().as_operation().to_string().matches("scf.if").count(),
        1
    );
    Ok(())
}

#[test]
fn cse_memory_scan_after_erased_read() -> Result<(), Report> {
    check_cse_memory_scan_after_erased_read(false)
}

#[test]
fn cse_memory_scan_after_erased_read_with_debug_records() -> Result<(), Report> {
    check_cse_memory_scan_after_erased_read(true)
}

fn check_cse_memory_scan_after_erased_read(with_debug_records: bool) -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir-opt/cse-memory-scan-cache.hir");
    let source = source
        .lines()
        .filter(|line| with_debug_records || !line.trim_start().starts_with("di."))
        .collect::<Vec<_>>()
        .join("\n");
    let scenario = Scenario::new(&source, "three_reads")?;
    let values = [(7u32, 19u32), (0, 1), (u32::MAX, 2), (1 << 31, u32::MAX)];
    let inputs = values
        .iter()
        .map(|&(before, after)| vec![before.into(), after.into()])
        .collect::<Vec<_>>();
    for ((before, after), args) in values.iter().zip(&inputs) {
        assert_eq!(
            scenario.evaluate(args)?.results.as_slice(),
            &[before.wrapping_mul(2).wrapping_add(*after).into()]
        );
    }
    compare_rewrite(&scenario, &inputs, vec![Box::new(CommonSubexpressionElimination)], &[])?;
    let printed = scenario.function.borrow().as_operation().to_string();
    assert_eq!(printed.matches("hir.load_local").count(), 2, "{printed}");
    Ok(())
}

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
fn local_promotion_preserves_values_wider_than_one_felt() -> Result<(), Report> {
    let scenario = Scenario::new(SOURCE, "wide_local")?;
    let inputs = [0u128, u64::MAX as u128 + 1, 1 << 127, u128::MAX]
        .into_iter()
        .map(|value| vec![value.into()])
        .collect::<Vec<_>>();
    compare_rewrite(&scenario, &inputs, vec![Box::new(hir::transforms::Local2Reg)], &[])?;
    Ok(())
}

#[test]
fn local_promotion_recovers_at_program_points() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-program-points.hir");
    for name in [
        "with_call",
        "same_join",
        "semantic_join",
        "region_join",
        "unrelated_region",
        "region_loop",
        "matching_branch_bindings",
        "loop_updates",
        "loop_carried",
        "nonentry_pair",
    ] {
        let scenario = Scenario::new(source, name)?;
        compare_rewrite(
            &scenario,
            &[vec![7u32.into(), true.into()], vec![42u32.into(), false.into()]],
            vec![Box::new(hir::transforms::Local2Reg)],
            &[],
        )?;
    }
    Ok(())
}

#[test]
fn local_promotion_preserves_frame_base_address_semantics() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-frame-base.hir");
    for (name, base, first, last) in [
        ("zero", 16u32, 7u32, 99u32),
        ("positive", 12, 7, 99),
        ("negative", 20, 7, 99),
        ("address", 12, 16, 16),
        ("indirect", 8, 7, 99),
        ("indirect_value", 8, 16, 16),
    ] {
        let scenario = Scenario::new(source, name)?;
        let expected = scenario.evaluate(&[base.into()])?;
        assert_eq!(
            expected.snapshots[0].frames[0].variables[0].1,
            Observation::Value(first.into())
        );
        assert_eq!(expected.snapshots[1].frames[0].variables[0].1, Observation::Value(last.into()));
        compare_rewrite(
            &scenario,
            &[vec![base.into()]],
            vec![Box::new(hir::transforms::Local2Reg)],
            &[],
        )?;
    }
    Ok(())
}

#[test]
fn local_promotion_invalidates_ambiguous_joins() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-program-points.hir");
    for name in ["ambiguous_value", "conditional_kill"] {
        let scenario = Scenario::new(source, name)?;
        let inputs = [vec![7u32.into(), true.into()], vec![42u32.into(), false.into()]];
        let before = inputs
            .iter()
            .map(|args| scenario.evaluate(args))
            .collect::<Result<Vec<_>, _>>()?;
        scenario.rewrite(vec![Box::new(hir::transforms::Local2Reg)])?;
        for (args, mut expected) in inputs.iter().zip(before) {
            for snapshot in &mut expected.snapshots {
                for (_, observation) in &mut snapshot.frames[0].variables {
                    *observation = Observation::Unavailable;
                }
            }
            expected.compare(&scenario.evaluate(args)?)?;
        }
    }
    Ok(())
}

#[test]
fn local_promotion_does_not_reuse_previous_iteration_arguments() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-program-points.hir");
    let scenario = Scenario::new(source, "loop_stale_argument")?;
    let args = [3u32.into(), true.into()];
    let mut expected = scenario.evaluate(&args)?;
    scenario.rewrite(vec![Box::new(hir::transforms::Local2Reg)])?;
    let actual = scenario.evaluate(&args)?;
    for (before, after) in expected.snapshots.iter_mut().zip(&actual.snapshots) {
        let observation = &after.frames[0].variables[0].1;
        if *observation == Observation::Unavailable {
            before.frames[0].variables[0].1 = Observation::Unavailable;
        }
    }
    expected.compare(&actual)?;
    assert!(
        actual
            .snapshots
            .iter()
            .filter(|snapshot| snapshot.checkpoint == Checkpoint::Marker(1))
            .all(|snapshot| snapshot.frames[0].variables[0].1 == Observation::Unavailable)
    );
    Ok(())
}

#[test]
fn local_promotion_preserves_binding_lifetimes_and_inline_owners() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-debug-lifetimes.hir");
    for (name, args) in [
        ("after_kill", vec![7u32.into(), 42u32.into()]),
        ("declaration_point", vec![7u32.into()]),
        ("reused_slot", vec![7u32.into(), 42u32.into()]),
        ("value_reassignment", vec![7u32.into(), 42u32.into(), 9u32.into()]),
        ("updates_while_active", vec![7u32.into(), 42u32.into()]),
        ("uninitialized", vec![7u32.into()]),
        ("changes_storage", vec![7u32.into(), 42u32.into(), 9u32.into()]),
        ("value_uses_slot_contents", vec![7u32.into(), 42u32.into(), 9u32.into()]),
        ("inline_bindings", vec![7u32.into(), 42u32.into()]),
        ("shadow_bindings", vec![7u32.into(), 42u32.into()]),
    ] {
        let scenario = Scenario::new(source, name)?;
        compare_rewrite(&scenario, &[args], vec![Box::new(hir::transforms::Local2Reg)], &[])?;
    }
    Ok(())
}

#[test]
fn local_promotion_executable_code_is_independent_of_debug_records() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-program-points.hir");
    let stripped = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("di."))
        .collect::<Vec<_>>()
        .join("\n");
    for name in [
        "with_call",
        "same_join",
        "semantic_join",
        "region_join",
        "unrelated_region",
        "region_loop",
        "matching_branch_bindings",
        "ambiguous_value",
        "conditional_kill",
        "loop_updates",
        "loop_carried",
        "loop_stale_argument",
        "nonentry_pair",
    ] {
        let recorded = Scenario::new(source, name)?;
        let plain = Scenario::new(&stripped, name)?;
        recorded.rewrite(vec![Box::new(hir::transforms::Local2Reg)])?;
        plain.rewrite(vec![Box::new(hir::transforms::Local2Reg)])?;
        let executable = recorded
            .function
            .borrow()
            .as_operation()
            .to_string()
            .lines()
            .filter(|line| !line.trim_start().starts_with("di."))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(executable, plain.function.borrow().as_operation().to_string(), "{name}");
        recorded.function.borrow().as_operation().postwalk_all(|op| {
            assert!(
                !op.is::<hir::StoreLocal>() && !op.is::<hir::LoadLocal>(),
                "{name}: executable local accesses remain"
            );
        });
    }
    Ok(())
}

#[test]
fn local_promotion_resolves_chained_debug_operands_after_each_rewrite() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-chained-locals.hir");
    let scenario = Scenario::new(source, "l2r_chain_debug")?;
    compare_rewrite(
        &scenario,
        &[vec![7u32.into()], vec![u32::MAX.into()]],
        vec![Box::new(hir::transforms::Local2Reg)],
        &[],
    )?;
    Ok(())
}

#[test]
fn local_promotion_preserves_wide_frame_base_reads() -> Result<(), Report> {
    let source = include_str!("../../../tests/lit/hir/transforms/local2reg-frame-base.hir");
    let scenario = Scenario::new(source, "wide")?;
    let value = 0x123456789abcdef0u64;
    let before = compare_rewrite(
        &scenario,
        &[vec![16u32.into(), value.into()]],
        vec![Box::new(hir::transforms::Local2Reg)],
        &[],
    )?;
    assert_eq!(
        before[0].snapshots[0].frames[0].variables[0].1,
        Observation::Value(value.into())
    );
    Ok(())
}
