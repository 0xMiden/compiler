use alloc::{format, rc::Rc, string::String, vec, vec::Vec};

use smallvec::SmallVec;

use super::{
    ConversionPattern, ConversionPatternRewriter, ConversionPatternSet, ConversionTarget,
    ConvertedOperands, FrozenConversionPatternSet, Legality, LegalizationGraph, TrackedMutations,
    TypeConverter,
};
use crate::{
    OperationName, OperationRef, Report, Spanned, ValueRef, WalkResult,
    dialects::builtin::UnrealizedConversionCast,
};

/// Dialect conversion mode.
///
/// Only full conversion is implemented in Phase 4. The additional variants reserve the API shape
/// for the partial and analysis modes described by the design document.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ConversionMode {
    Full,
    Partial,
    Analysis,
}

/// Configuration for the dialect conversion driver.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ConversionConfig {
    mode: ConversionMode,
    reconcile_unrealized_casts: bool,
    verify_after_conversion: bool,
    max_iterations: usize,
}

impl ConversionConfig {
    #[inline]
    pub const fn mode(&self) -> ConversionMode {
        self.mode
    }

    #[inline]
    pub const fn verify_after_conversion(&self) -> bool {
        self.verify_after_conversion
    }

    #[inline]
    pub const fn reconcile_unrealized_casts(&self) -> bool {
        self.reconcile_unrealized_casts
    }

    #[inline]
    pub const fn max_iterations(&self) -> usize {
        self.max_iterations
    }

    pub fn with_mode(&mut self, mode: ConversionMode) -> &mut Self {
        self.mode = mode;
        self
    }

    pub fn with_verify_after_conversion(&mut self, yes: bool) -> &mut Self {
        self.verify_after_conversion = yes;
        self
    }

    pub fn with_reconcile_unrealized_casts(&mut self, yes: bool) -> &mut Self {
        self.reconcile_unrealized_casts = yes;
        self
    }

    pub fn with_max_iterations(&mut self, max_iterations: usize) -> &mut Self {
        self.max_iterations = max_iterations;
        self
    }
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            mode: ConversionMode::Full,
            reconcile_unrealized_casts: true,
            verify_after_conversion: true,
            max_iterations: 1024,
        }
    }
}

/// Result of a conversion driver run.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ConversionResult {
    changed: bool,
    converted_ops: usize,
}

impl ConversionResult {
    #[inline]
    pub const fn changed(&self) -> bool {
        self.changed
    }

    #[inline]
    pub const fn converted_ops(&self) -> usize {
        self.converted_ops
    }
}

/// Apply full dialect conversion to `root`.
///
/// Full conversion requires every operation under `root` to be legal for `target`, except for
/// operations nested under recursively legal operations.
pub fn apply_full_conversion(
    root: OperationRef,
    target: ConversionTarget,
    patterns: ConversionPatternSet,
    config: ConversionConfig,
) -> Result<ConversionResult, Report> {
    if config.mode() != ConversionMode::Full {
        return Err(Report::msg("apply_full_conversion requires ConversionMode::Full"));
    }

    let frozen_patterns = FrozenConversionPatternSet::new(patterns);
    let graph = LegalizationGraph::new(&target, &frozen_patterns);
    let mut driver = FullConversionDriver {
        root,
        target: &target,
        graph,
        context: target.context(),
        config,
        result: ConversionResult {
            changed: false,
            converted_ops: 0,
        },
    };

    driver.run()?;
    Ok(driver.result)
}

struct FullConversionDriver<'a> {
    root: OperationRef,
    target: &'a ConversionTarget,
    graph: LegalizationGraph<'a>,
    context: Rc<crate::Context>,
    config: ConversionConfig,
    result: ConversionResult,
}

impl FullConversionDriver<'_> {
    fn run(&mut self) -> Result<(), Report> {
        let ops = self.collect_conversion_order()?;
        for op in ops {
            if self.is_live(op) {
                self.legalize_operation(op)?;
            }
        }

        if self.config.reconcile_unrealized_casts() {
            self.result.changed |= self.reconcile_unrealized_conversion_casts()?;
        }
        self.verify_full_conversion_legality()?;

        if self.config.verify_after_conversion() {
            self.root.borrow().recursively_verify()?;
        }

        Ok(())
    }

    fn collect_conversion_order(&self) -> Result<Vec<OperationRef>, Report> {
        let mut ops = vec![];
        self.root
            .borrow()
            .prewalk(|op| {
                if self.target.is_recursively_legal(op) {
                    WalkResult::<Report>::Skip
                } else {
                    ops.push(op.as_operation_ref());
                    WalkResult::<Report>::Continue(())
                }
            })
            .into_result()?;
        Ok(ops)
    }

    fn legalize_operation(&mut self, op: OperationRef) -> Result<(), Report> {
        if self.config.reconcile_unrealized_casts() && is_unrealized_conversion_cast(op) {
            return Ok(());
        }
        if self.target.is_recursively_legal(&op.borrow()) || self.target.is_legal(&op.borrow()) {
            return Ok(());
        }

        let op_name = op.borrow().name();
        if !self.graph.is_legalizable(&op_name) {
            return Err(self.no_legalization_path_error(op));
        }

        let candidates = self.candidate_patterns(&op_name);
        if candidates.is_empty() {
            return Err(self.no_legalization_path_error(op));
        }

        let mut match_failures = vec![];
        for pattern in candidates {
            let type_converter = pattern.type_converter().cloned();
            let mut rewriter = ConversionPatternRewriter::new_with_type_converter(
                Rc::clone(&self.context),
                type_converter.clone(),
            );
            if op.parent().is_some() {
                rewriter.set_insertion_point_before(op);
            }

            let operand_groups =
                converted_operands_for(op, type_converter.as_ref(), &mut rewriter)?;
            let mutation_count_before_pattern = rewriter.mutation_count();
            let operands = ConvertedOperands::new(&operand_groups);
            let applied = pattern.match_and_rewrite(op, operands, &mut rewriter);
            let mutation_count = rewriter.mutation_count();

            match applied {
                Err(err) => {
                    if mutation_count > mutation_count_before_pattern {
                        return Err(pattern_mutated_then_failed_error(
                            pattern.as_ref(),
                            op,
                            mutation_count,
                            err,
                        ));
                    }
                    rewriter.erase_unused_materializations()?;
                    return Err(err);
                }
                Ok(false) => {
                    if mutation_count > mutation_count_before_pattern {
                        return Err(pattern_mutated_without_success_error(
                            pattern.as_ref(),
                            op,
                            mutation_count,
                        ));
                    }
                    rewriter.erase_unused_materializations()?;
                    collect_match_failures(&mut match_failures, &mut rewriter);
                }
                Ok(true) => {
                    if mutation_count == mutation_count_before_pattern {
                        return Err(pattern_succeeded_without_mutation_error(pattern.as_ref(), op));
                    }
                    let mutations = rewriter.take_tracked_mutations();
                    self.note_pattern_application(pattern.as_ref())?;
                    self.validate_generated_ops(pattern.as_ref(), &mutations)?;
                    self.result.changed = true;

                    self.legalize_tracked_ops(&mutations)?;
                    if self.is_live(op) {
                        self.legalize_operation(op)?;
                    }
                    return Ok(());
                }
            }
        }

        Err(no_matching_pattern_error(op, match_failures))
    }

    fn candidate_patterns(&self, op_name: &OperationName) -> Vec<Rc<dyn ConversionPattern>> {
        self.graph
            .legalizer_patterns(op_name)
            .iter()
            .chain(self.graph.any_op_patterns().iter())
            .cloned()
            .collect()
    }

    fn note_pattern_application(&mut self, pattern: &dyn ConversionPattern) -> Result<(), Report> {
        if self.result.converted_ops >= self.config.max_iterations() {
            return Err(Report::msg(format!(
                "dialect conversion exceeded the configured rewrite limit of {} while applying \
                 pattern '{}'",
                self.config.max_iterations(),
                pattern.name()
            )));
        }

        self.result.converted_ops += 1;
        Ok(())
    }

    fn validate_generated_ops(
        &self,
        pattern: &dyn ConversionPattern,
        mutations: &TrackedMutations,
    ) -> Result<(), Report> {
        for op in mutations.inserted_ops() {
            if mutations.materialized_ops().iter().any(|materialized| materialized == op) {
                continue;
            }
            let name = op.borrow().name();
            if !pattern.generated_ops().iter().any(|generated| generated == &name) {
                return Err(Report::msg(format!(
                    "conversion pattern '{}' generated undeclared operation '{}'; add it to the \
                     pattern's generated operation metadata",
                    pattern.name(),
                    name
                )));
            }
        }
        Ok(())
    }

    fn legalize_tracked_ops(&mut self, mutations: &TrackedMutations) -> Result<(), Report> {
        for op in mutations.inserted_ops().iter().chain(mutations.modified_ops().iter()).copied() {
            if self.is_live(op) {
                if self.config.reconcile_unrealized_casts() && is_unrealized_conversion_cast(op) {
                    continue;
                }
                self.legalize_operation(op)?;
            }
        }
        Ok(())
    }

    fn reconcile_unrealized_conversion_casts(&mut self) -> Result<bool, Report> {
        let mut reconciled = false;
        loop {
            let mut changed = false;
            let casts = self.collect_unrealized_conversion_casts()?;
            if casts.is_empty() {
                return Ok(reconciled);
            }

            let mut rewriter = ConversionPatternRewriter::new(Rc::clone(&self.context));
            for cast in casts {
                if !self.is_live(cast) {
                    continue;
                }
                if !cast.borrow().is_used() {
                    rewriter.erase_op(cast)?;
                    reconciled = true;
                    changed = true;
                    continue;
                }

                let Some((input, result)) = unrealized_conversion_cast_values(cast) else {
                    continue;
                };
                let input_ty = input.borrow().ty().clone();
                let result_ty = result.borrow().ty().clone();
                if input_ty == result_ty {
                    rewriter.replace_op(cast, &[input])?;
                    reconciled = true;
                    changed = true;
                    continue;
                }

                let Some(previous_cast) = input
                    .borrow()
                    .get_defining_op()
                    .filter(|op| is_unrealized_conversion_cast(*op))
                else {
                    continue;
                };
                let Some((previous_input, _)) = unrealized_conversion_cast_values(previous_cast)
                else {
                    continue;
                };
                if *previous_input.borrow().ty() == result_ty {
                    rewriter.replace_op(cast, &[previous_input])?;
                    if self.is_live(previous_cast) && !previous_cast.borrow().is_used() {
                        rewriter.erase_op(previous_cast)?;
                    }
                    reconciled = true;
                    changed = true;
                }
            }

            if !changed {
                return Ok(reconciled);
            }
        }
    }

    fn collect_unrealized_conversion_casts(&self) -> Result<Vec<OperationRef>, Report> {
        let mut casts = vec![];
        self.root
            .borrow()
            .prewalk(|op| {
                if is_unrealized_conversion_cast(op.as_operation_ref()) {
                    casts.push(op.as_operation_ref());
                }
                WalkResult::<Report>::Continue(())
            })
            .into_result()?;
        Ok(casts)
    }

    fn verify_full_conversion_legality(&self) -> Result<(), Report> {
        self.root
            .borrow()
            .prewalk(|op| {
                if self.target.is_recursively_legal(op) {
                    return WalkResult::<Report>::Skip;
                }

                if self.target.is_legal(op) {
                    WalkResult::<Report>::Continue(())
                } else {
                    let op_ref = op.as_operation_ref();
                    WalkResult::Break(self.no_legalization_path_error(op_ref))
                }
            })
            .into_result()
    }

    fn no_legalization_path_error(&self, op: OperationRef) -> Report {
        let op = op.borrow();
        let reason = legality_failure_reason(self.target, &op);
        if let Some(reason) = reason {
            Report::msg(format!(
                "failed to legalize operation '{}': no legalization path to target; {reason}",
                op.name()
            ))
        } else {
            Report::msg(format!(
                "failed to legalize operation '{}': no legalization path to target",
                op.name()
            ))
        }
    }

    fn is_live(&self, op: OperationRef) -> bool {
        op == self.root || op.parent().is_some()
    }
}

fn converted_operands_for(
    op: OperationRef,
    type_converter: Option<&TypeConverter>,
    rewriter: &mut ConversionPatternRewriter,
) -> Result<Vec<SmallVec<[ValueRef; 2]>>, Report> {
    let (span, operand_groups) = {
        let op = op.borrow();
        let span = op.span();
        let operand_groups = op
            .operands()
            .groups()
            .map(|group| {
                group
                    .iter()
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<SmallVec<[ValueRef; 2]>>()
            })
            .collect::<Vec<_>>();
        (span, operand_groups)
    };

    let mut converted = vec![];
    for group in operand_groups {
        let mut converted_group = SmallVec::<[ValueRef; 2]>::new();
        for value in group {
            let Some(type_converter) = type_converter else {
                converted_group.push(value);
                continue;
            };
            let converted_ty = type_converter.convert_value_1_to_1(value)?;
            converted_group.push(type_converter.materialize_target_conversion(
                rewriter,
                value,
                converted_ty,
                span,
            )?);
        }
        converted.push(converted_group);
    }
    Ok(converted)
}

fn collect_match_failures(failures: &mut Vec<String>, rewriter: &mut ConversionPatternRewriter) {
    failures.extend(
        rewriter
            .take_match_failures()
            .into_iter()
            .map(|failure| format!("{}: {}", failure.op().borrow().name(), failure.reason())),
    );
}

fn is_unrealized_conversion_cast(op: OperationRef) -> bool {
    op.try_downcast_op::<UnrealizedConversionCast>().is_ok()
}

fn unrealized_conversion_cast_values(op: OperationRef) -> Option<(ValueRef, ValueRef)> {
    let cast = op.try_downcast_op::<UnrealizedConversionCast>().ok()?;
    let cast = cast.borrow();
    Some((cast.operand().as_value_ref(), cast.result().as_value_ref()))
}

fn legality_failure_reason(target: &ConversionTarget, op: &crate::Operation) -> Option<String> {
    match target.legality(op) {
        Legality::DynamicIllegal {
            reason: Some(reason),
        } => Some(format!("{reason}")),
        Legality::DynamicIllegal { reason: None } => {
            Some(String::from("dynamic legality predicate returned illegal"))
        }
        Legality::Illegal => Some(String::from("operation is explicitly illegal")),
        Legality::Unknown => Some(String::from("operation is unknown to the conversion target")),
        Legality::Legal | Legality::DynamicLegal => None,
    }
}

fn pattern_mutated_then_failed_error(
    pattern: &dyn ConversionPattern,
    op: OperationRef,
    mutation_count: usize,
    err: Report,
) -> Report {
    Report::msg(format!(
        "conversion pattern '{}' failed after mutating operation '{}' ({} mutations observed): {}",
        pattern.name(),
        op.borrow().name(),
        mutation_count,
        err
    ))
}

fn pattern_mutated_without_success_error(
    pattern: &dyn ConversionPattern,
    op: OperationRef,
    mutation_count: usize,
) -> Report {
    Report::msg(format!(
        "conversion pattern '{}' mutated operation '{}' but returned no match ({} mutations \
         observed)",
        pattern.name(),
        op.borrow().name(),
        mutation_count
    ))
}

fn pattern_succeeded_without_mutation_error(
    pattern: &dyn ConversionPattern,
    op: OperationRef,
) -> Report {
    Report::msg(format!(
        "conversion pattern '{}' reported success for operation '{}' without mutating IR",
        pattern.name(),
        op.borrow().name()
    ))
}

fn no_matching_pattern_error(op: OperationRef, failures: Vec<String>) -> Report {
    if failures.is_empty() {
        return Report::msg(format!(
            "failed to legalize operation '{}': no conversion pattern matched",
            op.borrow().name()
        ));
    }

    Report::msg(format!(
        "failed to legalize operation '{}': no conversion pattern matched ({})",
        op.borrow().name(),
        failures.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, string::ToString};

    use pretty_assertions::assert_str_eq;

    use super::*;
    use crate::{
        Context, Immediate, Op, OperationRef, Overflow, Report, SourceSpan, Spanned, Type,
        ValueRef,
        conversion::{
            ConvertedOperands, DynamicLegalityResult, TypeConversion, TypeConverter,
            UnknownOpPolicy,
        },
        dialects::{
            builtin::{BuiltinOpBuilder, UnrealizedConversionCast},
            test::{Add, Constant, Mul, Shl, TestDialect, TestOpBuilder},
        },
        patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind},
        testing::Test,
    };

    struct AddToMul {
        info: PatternInfo,
    }

    impl AddToMul {
        fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            let mut info = PatternInfo::new(
                context,
                "add-to-mul",
                PatternKind::Operation(dialect.expect_registered_name::<Add>()),
                PatternBenefit::new(1),
            );
            info.with_generated_ops([dialect.expect_registered_name::<Mul>()]);
            Self { info }
        }
    }

    impl Pattern for AddToMul {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for AddToMul {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            let Some((span, lhs, rhs)) = add_parts(op, operands) else {
                rewriter.notify_match_failure(op, Report::msg("expected test.add"));
                return Ok(false);
            };

            rewriter.replace_op_with_new_op::<Mul, _>(op, span, (lhs, rhs, Overflow::Checked))?;
            Ok(true)
        }
    }

    struct AddToMulWithTypeConversion {
        info: PatternInfo,
        type_converter: TypeConverter,
    }

    impl AddToMulWithTypeConversion {
        fn new(context: Rc<Context>, type_converter: TypeConverter) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            let mut info = PatternInfo::new(
                context,
                "add-to-mul-with-type-conversion",
                PatternKind::Operation(dialect.expect_registered_name::<Add>()),
                PatternBenefit::new(1),
            );
            info.with_generated_ops([dialect.expect_registered_name::<Mul>()]);
            Self {
                info,
                type_converter,
            }
        }
    }

    impl Pattern for AddToMulWithTypeConversion {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for AddToMulWithTypeConversion {
        fn type_converter(&self) -> Option<&TypeConverter> {
            Some(&self.type_converter)
        }

        fn match_and_rewrite(
            &self,
            op: OperationRef,
            operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            let Some((span, lhs, rhs)) = add_parts(op, operands) else {
                return Ok(false);
            };

            rewriter.replace_op_with_new_op::<Mul, _>(op, span, (lhs, rhs, Overflow::Checked))?;
            Ok(true)
        }
    }

    struct MulToShl {
        info: PatternInfo,
    }

    impl MulToShl {
        fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            let mut info = PatternInfo::new(
                context,
                "mul-to-shl",
                PatternKind::Operation(dialect.expect_registered_name::<Mul>()),
                PatternBenefit::new(1),
            );
            info.with_generated_ops([dialect.expect_registered_name::<Shl>()]);
            Self { info }
        }
    }

    impl Pattern for MulToShl {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for MulToShl {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            if op.try_downcast_op::<Mul>().is_err() {
                rewriter.notify_match_failure(op, Report::msg("expected test.mul"));
                return Ok(false);
            }

            let Some(values) = operands.get(0) else {
                return Ok(false);
            };
            let Some(lhs) = values.first().copied() else {
                return Ok(false);
            };
            let Some(rhs) = values.get(1).copied() else {
                return Ok(false);
            };
            let span = op.borrow().span();

            rewriter.replace_op_with_new_op::<Shl, _>(op, span, (lhs, rhs))?;
            Ok(true)
        }
    }

    struct AddToMulByTwo {
        info: PatternInfo,
    }

    impl AddToMulByTwo {
        fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            let mut info = PatternInfo::new(
                context,
                "add-to-mul-by-two",
                PatternKind::Operation(dialect.expect_registered_name::<Add>()),
                PatternBenefit::new(1),
            );
            info.with_generated_ops([
                dialect.expect_registered_name::<Constant>(),
                dialect.expect_registered_name::<Mul>(),
            ]);
            Self { info }
        }
    }

    impl Pattern for AddToMulByTwo {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for AddToMulByTwo {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            if op.try_downcast_op::<Add>().is_err() {
                return Ok(false);
            }

            let span = op.borrow().span();
            let Some(values) = operands.get(0) else {
                return Ok(false);
            };
            let Some(lhs) = values.first().copied() else {
                return Ok(false);
            };
            let Some(_rhs) = values.get(1).copied() else {
                return Ok(false);
            };
            let constant = rewriter.create_op::<Constant, _>(span, (Immediate::U32(2),))?;
            let rhs = constant.borrow().result().as_value_ref();
            let mul = rewriter.create_op::<Mul, _>(span, (lhs, rhs, Overflow::Checked))?;
            let result = mul.borrow().result().as_value_ref();
            rewriter.replace_op(op, &[result])?;
            Ok(true)
        }
    }

    struct UndeclaredAddToMul {
        info: PatternInfo,
    }

    impl UndeclaredAddToMul {
        fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            Self {
                info: PatternInfo::new(
                    context,
                    "undeclared-add-to-mul",
                    PatternKind::Operation(dialect.expect_registered_name::<Add>()),
                    PatternBenefit::new(1),
                ),
            }
        }
    }

    impl Pattern for UndeclaredAddToMul {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for UndeclaredAddToMul {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            let Some((span, lhs, rhs)) = add_parts(op, operands) else {
                return Ok(false);
            };
            rewriter.replace_op_with_new_op::<Mul, _>(op, span, (lhs, rhs, Overflow::Checked))?;
            Ok(true)
        }
    }

    struct MutatesButFails {
        info: PatternInfo,
    }

    impl MutatesButFails {
        fn new(context: Rc<Context>) -> Self {
            let dialect = context.get_or_register_dialect::<TestDialect>();
            let mut info = PatternInfo::new(
                context,
                "mutates-but-fails",
                PatternKind::Operation(dialect.expect_registered_name::<Add>()),
                PatternBenefit::new(1),
            );
            info.with_generated_ops([dialect.expect_registered_name::<Constant>()]);
            Self { info }
        }
    }

    impl Pattern for MutatesButFails {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }

    impl ConversionPattern for MutatesButFails {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            _operands: ConvertedOperands<'_>,
            rewriter: &mut ConversionPatternRewriter,
        ) -> Result<bool, Report> {
            let span = op.borrow().span();
            let _ = rewriter.create_op::<Constant, _>(span, (Immediate::U32(1),))?;
            Ok(false)
        }
    }

    fn add_parts(
        op: OperationRef,
        operands: ConvertedOperands<'_>,
    ) -> Option<(SourceSpan, ValueRef, ValueRef)> {
        op.try_downcast_op::<Add>().ok()?;
        let values = operands.get(0)?;
        let lhs = values.first().copied()?;
        let rhs = values.get(1).copied()?;
        Some((op.borrow().span(), lhs, rhs))
    }

    fn add_function(name: &'static str) -> Test {
        let mut test = Test::new(name, &[Type::U32, Type::U32], &[Type::U32]);
        {
            let mut builder = test.function_builder();
            let block = builder.current_block();
            let lhs = block.borrow().arguments()[0] as ValueRef;
            let rhs = block.borrow().arguments()[1] as ValueRef;
            let result = builder.add(lhs, rhs, SourceSpan::default()).unwrap();
            builder.ret(Some(result), SourceSpan::default()).unwrap();
        }
        test
    }

    fn target_with_test_ops(
        context: Rc<Context>,
        legal_ops: impl FnOnce(&mut ConversionTarget),
    ) -> ConversionTarget {
        let mut target = ConversionTarget::new(context);
        target.set_unknown_op_policy(UnknownOpPolicy::Legal);
        target.add_illegal_op::<Add>();
        legal_ops(&mut target);
        target
    }

    fn u32_to_i32_converter() -> TypeConverter {
        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::One(Type::I32))
            } else {
                None
            }
        });
        converter
    }

    fn cast_chain_function(name: &'static str, middle: Type, result: Type) -> Test {
        let mut test = Test::new(name, &[Type::U32], core::slice::from_ref(&result));
        {
            let mut builder = test.function_builder();
            let block = builder.current_block();
            let arg = block.borrow().arguments()[0] as ValueRef;
            let first = builder
                .builder_mut()
                .unrealized_conversion_cast(arg, middle, SourceSpan::default())
                .unwrap();
            let second = builder
                .builder_mut()
                .unrealized_conversion_cast(first, result, SourceSpan::default())
                .unwrap();
            builder.ret(Some(second), SourceSpan::default()).unwrap();
        }
        test
    }

    #[test]
    fn full_conversion_rewrites_illegal_op_to_legal_op() {
        let test = add_function("full_conversion_rewrites_illegal_op_to_legal_op");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Mul>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMul::new(test.context_rc()));

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap();

        assert!(result.changed());
        assert_eq!(result.converted_ops(), 1);
        let output = test.function().borrow().as_operation().to_string();
        let expected = "\
builtin.function public extern(\"C\") @full_conversion_rewrites_illegal_op_to_legal_op(%0: u32, \
                        %1: u32) -> u32 {
    %3 = test.mul %0, %1 <{ overflow = #builtin.overflow<checked> }>;
    builtin.ret %3 : (u32);
};";
        assert_str_eq!(output.as_str(), expected);
    }

    #[test]
    fn full_conversion_reports_missing_legalization_path() {
        let test = add_function("full_conversion_reports_missing_legalization_path");
        let target = target_with_test_ops(test.context_rc(), |_| {});
        let patterns = ConversionPatternSet::new(test.context_rc());

        let err = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap_err();

        assert!(format!("{err}").contains("no legalization path"));
    }

    #[test]
    fn full_conversion_legalizes_transitive_rewrites() {
        let test = add_function("full_conversion_legalizes_transitive_rewrites");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_illegal_op::<Mul>().add_legal_op::<Shl>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMul::new(test.context_rc()));
        patterns.push(MulToShl::new(test.context_rc()));

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap();

        assert_eq!(result.converted_ops(), 2);
        let output = test.function().borrow().as_operation().to_string();
        assert!(output.contains("test.shl"));
        assert!(!output.contains("test.add"));
        assert!(!output.contains("test.mul"));
    }

    #[test]
    fn full_conversion_rewrites_dynamically_illegal_ops() {
        let test = add_function("full_conversion_rewrites_dynamically_illegal_ops");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target
                .add_dynamically_legal_op::<Add, _>(|_| DynamicLegalityResult::illegal())
                .add_legal_op::<Mul>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMul::new(test.context_rc()));

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap();

        assert_eq!(result.converted_ops(), 1);
        let output = test.function().borrow().as_operation().to_string();
        assert!(output.contains("test.mul"));
        assert!(!output.contains("test.add"));
    }

    #[test]
    fn full_conversion_materializes_1_to_1_type_conversions() {
        let test = add_function("full_conversion_materializes_1_to_1_type_conversions");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Mul>().add_legal_op::<UnrealizedConversionCast>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMulWithTypeConversion::new(test.context_rc(), u32_to_i32_converter()));

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap();

        assert_eq!(result.converted_ops(), 1);
        let output = test.function().borrow().as_operation().to_string();
        assert!(output.contains("builtin.unrealized_conversion_cast"));
        assert!(output.contains("#builtin.type<i32>"));
        assert!(output.contains("#builtin.type<u32>"));
        assert!(output.contains("test.mul"));
        assert!(!output.contains("test.add"));
    }

    #[test]
    fn full_conversion_rejects_non_1_to_1_operand_materialization() {
        let test = add_function("full_conversion_rejects_non_1_to_1_operand_materialization");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Mul>();
        });
        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::Many(smallvec::smallvec![Type::I32, Type::I32]))
            } else {
                None
            }
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMulWithTypeConversion::new(test.context_rc(), converter));

        let err = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap_err();

        assert!(format!("{err}").contains("1:1"));
    }

    #[test]
    fn full_conversion_reconciles_identity_unrealized_casts() {
        let test = cast_chain_function(
            "full_conversion_reconciles_identity_unrealized_casts",
            Type::U32,
            Type::U32,
        );
        let mut target = ConversionTarget::new(test.context_rc());
        target
            .set_unknown_op_policy(UnknownOpPolicy::Legal)
            .add_illegal_op::<UnrealizedConversionCast>();

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            ConversionPatternSet::new(test.context_rc()),
            ConversionConfig::default(),
        )
        .unwrap();

        assert!(result.changed());
        let output = test.function().borrow().as_operation().to_string();
        assert!(!output.contains("builtin.unrealized_conversion_cast"));
    }

    #[test]
    fn full_conversion_reconciles_reversible_unrealized_cast_chains() {
        let test = cast_chain_function(
            "full_conversion_reconciles_reversible_unrealized_cast_chains",
            Type::I32,
            Type::U32,
        );
        let mut target = ConversionTarget::new(test.context_rc());
        target
            .set_unknown_op_policy(UnknownOpPolicy::Legal)
            .add_illegal_op::<UnrealizedConversionCast>();

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            ConversionPatternSet::new(test.context_rc()),
            ConversionConfig::default(),
        )
        .unwrap();

        assert!(result.changed());
        let output = test.function().borrow().as_operation().to_string();
        assert!(!output.contains("builtin.unrealized_conversion_cast"));
    }

    #[test]
    fn full_conversion_supports_op_to_sequence_rewrites() {
        let test = add_function("full_conversion_supports_op_to_sequence_rewrites");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Constant>().add_legal_op::<Mul>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(AddToMulByTwo::new(test.context_rc()));

        let result = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap();

        assert_eq!(result.converted_ops(), 1);
        let output = test.function().borrow().as_operation().to_string();
        assert!(output.contains("test.constant 2"));
        assert!(output.contains("test.mul"));
        assert!(!output.contains("test.add"));
    }

    #[test]
    fn full_conversion_rejects_undeclared_generated_ops() {
        let test = add_function("full_conversion_rejects_undeclared_generated_ops");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Mul>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(UndeclaredAddToMul::new(test.context_rc()));

        let err = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap_err();

        assert!(format!("{err}").contains("generated undeclared operation"));
    }

    #[test]
    fn full_conversion_rejects_mutation_contract_violations() {
        let test = add_function("full_conversion_rejects_mutation_contract_violations");
        let target = target_with_test_ops(test.context_rc(), |target| {
            target.add_legal_op::<Constant>();
        });
        let mut patterns = ConversionPatternSet::new(test.context_rc());
        patterns.push(MutatesButFails::new(test.context_rc()));

        let err = apply_full_conversion(
            test.function().as_operation_ref(),
            target,
            patterns,
            ConversionConfig::default(),
        )
        .unwrap_err();

        assert!(format!("{err}").contains("mutated operation"));
    }
}
