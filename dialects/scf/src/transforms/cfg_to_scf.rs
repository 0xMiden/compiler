use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::{self as cf, ControlFlowOpBuilder};
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    Builder, EntityMut, Forward, Op, Operation, OperationName, OperationRef, RawWalk, Report,
    SmallVec, Spanned, Type, ValueRange, ValueRef, WalkResult,
    diagnostics::Severity,
    dialects::builtin,
    dominance::DominanceInfo,
    pass::{Pass, PassExecutionState, PostPassStatus},
};
use midenc_hir_transform::{self as transforms, CFGToSCFInterface};

use crate::*;

/// Lifts unstructured control flow operations to structured operations in the HIR dialect.
///
/// This pass is not always guaranteed to replace all unstructured control flow operations. If a
/// region contains only a single kind of return-like operation, all unstructured control flow ops
/// will be replaced successfully. Otherwise a single unstructured switch branching to one block per
/// return-like operation kind remains.
///
/// This pass may need to create unreachable terminators in case of infinite loops, which is only
/// supported for 'builtin.func' for now. If you potentially have infinite loops inside CFG regions
/// not belonging to 'builtin.func', consider using the `transform_cfg_to_scf` function directly
/// with a corresponding [CFGToSCFInterface::create_unreachable_terminator] implementation.
pub struct LiftControlFlowToSCF;

impl Pass for LiftControlFlowToSCF {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "lift-control-flow"
    }

    fn argument(&self) -> &'static str {
        "lift-control-flow"
    }

    fn description(&self) -> &'static str {
        "Lifts unstructured control flow to structured control flow"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<midenc_hir::Context>) -> Result<(), Report> {
        // Ensure that when this pass is initialized, that the SCF dialect is registered
        context.get_or_register_dialect::<crate::ScfDialect>();

        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let mut transformation = ControlFlowToSCFTransformation;
        let mut changed = false;

        let root = op.as_operation_ref();
        drop(op);

        log::debug!(target: "cfg-to-scf", "applying control flow lifting transformation pass starting from {}", root.borrow());

        let result = root.raw_prewalk::<Forward, _, _>(|operation: OperationRef| -> WalkResult {
            let op = operation.borrow();
            if op.is::<builtin::Function>() {
                if op.regions().is_empty() {
                    return WalkResult::Skip;
                }

                let dominfo = if OperationRef::ptr_eq(&operation, &root) {
                    state.analysis_manager().get_analysis::<DominanceInfo>()
                } else {
                    state.analysis_manager().get_child_analysis::<DominanceInfo>(operation)
                };

                let mut dominfo = match dominfo {
                    Ok(di) => di,
                    Err(err) => return WalkResult::Break(err),
                };
                let dominfo = Rc::make_mut(&mut dominfo);

                let visitor = |inner: OperationRef| -> WalkResult {
                    log::debug!(target: "cfg-to-scf", "applying control flow lifting to {}", inner.borrow());
                    let mut next_region = inner.borrow().regions().front().as_pointer();
                    while let Some(region) = next_region.take() {
                        next_region = region.next();

                        let result =
                            transforms::transform_cfg_to_scf(region, &mut transformation, dominfo);
                        match result {
                            Ok(did_change) => {
                                log::trace!(
                                    target: "cfg-to-scf",
                                    "control flow lifting completed for region \
                                     (did_change={did_change})"
                                );
                                changed |= did_change;
                            }
                            Err(err) => {
                                return WalkResult::Break(err);
                            }
                        }
                    }

                    WalkResult::Continue(())
                };

                drop(op);

                operation.raw_postwalk::<Forward, _, _>(visitor)?;

                // Do not enter the function body in the outer walk
                WalkResult::Skip
            } else if op.is::<builtin::World>()
                || op.is::<builtin::Component>()
                || op.is::<builtin::Module>()
            {
                // We only care to recurse into ops that can contain functions
                log::trace!(
                    target: "cfg-to-scf",
                    "looking for functions to apply control flow lifting to in '{}'",
                    op.name()
                );
                WalkResult::Continue(())
            } else {
                // Skip all other ops
                log::trace!("skipping control flow lifting for '{}'", op.name());
                WalkResult::Skip
            }
        });

        if result.was_interrupted() {
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return result.into_result();
        }

        log::debug!(
            target: "cfg-to-scf",
            "control flow lifting transformation pass completed successfully (changed = {changed}"
        );
        if !changed {
            state.preserved_analyses_mut().preserve_all();
        }

        state.set_post_pass_status(changed.into());

        Ok(())
    }
}

/// Implementation of [CFGToSCFInterface] used to lift unstructured control flow operations into
/// HIR's structured control flow operations.
struct ControlFlowToSCFTransformation;

impl CFGToSCFInterface for ControlFlowToSCFTransformation {
    /// Creates an `scf.if` op if `control_flow_cond_op` is a `cf.cond_br` op, or an
    /// `scf.index_switch` if it is a `cf.switch`. Otherwise, returns an error.
    fn create_structured_branch_region_op(
        &self,
        builder: &mut midenc_hir::OpBuilder,
        control_flow_cond_op: midenc_hir::OperationRef,
        result_types: &[midenc_hir::Type],
        regions: &mut midenc_hir::SmallVec<[midenc_hir::RegionRef; 2]>,
    ) -> Result<midenc_hir::OperationRef, midenc_hir::Report> {
        let cf_op = control_flow_cond_op.borrow();
        if let Some(cond_br) = cf_op.downcast_ref::<cf::CondBr>() {
            assert_eq!(regions.len(), 2);

            let span = cond_br.span();
            let mut if_op = builder.r#if(cond_br.condition().as_value_ref(), result_types, span)?;
            let mut op = if_op.borrow_mut();
            let operation = op.as_operation_ref();

            op.then_body_mut().take_body(regions[0]);
            op.else_body_mut().take_body(regions[1]);

            return Ok(operation);
        }

        if let Some(switch) = cf_op.downcast_ref::<cf::Switch>() {
            let span = switch.span();
            let cases = switch.cases();
            assert_eq!(regions.len(), cases.len() + 1);
            let cases = cases.iter().map(|case| *case.key().unwrap());
            let mut switch_op = builder.index_switch(
                switch.selector().as_value_ref(),
                cases,
                result_types,
                span,
            )?;
            let mut op = switch_op.borrow_mut();
            let operation = op.as_operation_ref();

            // If any of the case targets are duplicated, we have to duplicate the regions or
            // we will fail to properly lower the input

            // The order of the regions match the original 'cf.switch', hence the fallback region
            // coming first.
            op.default_region_mut().take_body(regions[0]);
            for (index, source_region) in regions.iter().copied().skip(1).enumerate() {
                let mut case_region = op.get_case_region(index);
                case_region.borrow_mut().take_body(source_region);
            }

            return Ok(operation);
        }

        Err(builder
            .context()
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message("control flow transformation failed")
            .with_primary_label(
                cf_op.span(),
                "unknown control flow operation cannot be lifted to structured control flow",
            )
            .into_report())
    }

    /// Creates an `scf.yield` op returning the given results.
    fn create_structured_branch_region_terminator_op(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        _branch_region_op: midenc_hir::OperationRef,
        _replaced_control_flow_op: Option<midenc_hir::OperationRef>,
        results: ValueRange<'_, 2>,
    ) -> Result<(), midenc_hir::Report> {
        builder.r#yield(results, span)?;

        Ok(())
    }

    /// Creates an `scf.while` op. The loop body is made the before-region of the
    /// while op and terminated with an `scf.condition` op. The after-region does
    /// nothing but forward the iteration variables.
    fn create_structured_do_while_loop_op(
        &self,
        builder: &mut midenc_hir::OpBuilder,
        replaced_op: midenc_hir::OperationRef,
        loop_values_init: ValueRange<'_, 2>,
        condition: midenc_hir::ValueRef,
        loop_values_next_iter: ValueRange<'_, 2>,
        loop_body: midenc_hir::RegionRef,
    ) -> Result<midenc_hir::OperationRef, midenc_hir::Report> {
        let span = replaced_op.span();

        // Results are derived from the forwarded values given to `scf.condition`
        let result_types = loop_values_next_iter
            .iter()
            .map(|v| v.borrow().ty().clone())
            .collect::<SmallVec<[_; 2]>>();
        let mut while_op = builder.r#while(loop_values_init, &result_types, span)?;
        let mut op = while_op.borrow_mut();
        let operation = op.as_operation_ref();

        op.before_mut().take_body(loop_body);

        builder.set_insertion_point_to_end(op.before().body().back().as_pointer().unwrap());

        // `get_cfg_switch_value` returns a u32. We therefore need to truncate the condition to i1
        // first. It is guaranteed to be either 0 or 1 already.
        let cond = builder.trunc(condition, Type::I1, span)?;
        builder.condition(cond, loop_values_next_iter, span)?;

        let yielded = op
            .after()
            .entry()
            .arguments()
            .iter()
            .map(|arg| arg.upcast())
            .collect::<SmallVec<[ValueRef; 4]>>();

        builder.set_insertion_point_to_end(op.after().entry().as_block_ref());

        builder.r#yield(yielded, span)?;

        Ok(operation)
    }

    /// Creates an `arith.constant` with an i32 attribute of the given value.
    fn get_cfg_switch_value(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        value: u32,
    ) -> midenc_hir::ValueRef {
        builder.u32(value, span)
    }

    /// Creates a `cf.switch` op with the given cases and flag.
    fn create_cfg_switch_op(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        flag: midenc_hir::ValueRef,
        case_values: &[u32],
        case_destinations: &[midenc_hir::BlockRef],
        case_arguments: &[ValueRange<'_, 2>],
        default_dest: midenc_hir::BlockRef,
        default_args: ValueRange<'_, 2>,
    ) -> Result<(), Report> {
        let cases = case_values
            .iter()
            .copied()
            .zip(case_destinations.iter().copied().zip(case_arguments))
            .map(|(value, (successor, args))| cf::SwitchCase {
                value,
                successor,
                arguments: args.to_vec(),
            })
            .collect::<SmallVec<[_; 4]>>();

        builder.switch(flag, cases, default_dest, default_args, span)?;

        Ok(())
    }

    fn create_single_destination_branch(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        _dummy_flag: midenc_hir::ValueRef,
        destination: midenc_hir::BlockRef,
        arguments: ValueRange<'_, 2>,
    ) -> Result<(), Report> {
        builder.br(destination, arguments, span)?;
        Ok(())
    }

    fn create_conditional_branch(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        condition: midenc_hir::ValueRef,
        true_dest: midenc_hir::BlockRef,
        true_args: ValueRange<'_, 2>,
        false_dest: midenc_hir::BlockRef,
        false_args: ValueRange<'_, 2>,
    ) -> Result<(), Report> {
        builder.cond_br(condition, true_dest, true_args, false_dest, false_args, span)?;

        Ok(())
    }

    /// Creates a `ub.poison` op of the given type.
    fn get_undef_value(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        ty: midenc_hir::Type,
    ) -> midenc_hir::ValueRef {
        builder.poison(ty, span)
    }

    fn create_unreachable_terminator(
        &self,
        span: midenc_hir::SourceSpan,
        builder: &mut midenc_hir::OpBuilder,
        _region: midenc_hir::RegionRef,
    ) -> Result<midenc_hir::OperationRef, midenc_hir::Report> {
        log::trace!(target: "cfg-to-scf", "creating unreachable terminator at {}", builder.insertion_point());
        let op = builder.unreachable(span);
        Ok(op.as_operation_ref())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc};

    use builtin::{BuiltinOpBuilder, FunctionBuilder};
    use midenc_expect_test::expect_file;
    use midenc_hir::{
        AbiParam, BuilderExt, Context, Ident, OpBuilder, PointerType, Report, Signature,
        SourceSpan, Type, dialects::builtin, pass,
    };

    use super::*;

    #[test]
    fn cfg_to_scf_lift_simple_conditional() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        // Define function body
        let mut builder = FunctionBuilder::new(function, &mut builder);

        let if_is_zero = builder.create_block();
        let if_is_nonzero = builder.create_block();
        let exit_block = builder.create_block();
        let return_val = builder.append_block_param(exit_block, Type::U32, span);

        let block = builder.current_block();
        let input = block.borrow().arguments()[0].upcast();

        let zero = builder.u32(0, span);
        let is_zero = builder.eq(input, zero, span)?;
        builder.cond_br(is_zero, if_is_zero, [], if_is_nonzero, [], span)?;

        builder.switch_to_block(if_is_zero);
        let a = builder.incr(input, span)?;
        builder.br(exit_block, [a], span)?;

        builder.switch_to_block(if_is_nonzero);
        let b = builder.mul(input, input, span)?;
        builder.br(exit_block, [b], span)?;

        builder.switch_to_block(exit_block);
        builder.ret(Some(return_val), span)?;

        let operation = function.as_operation_ref();
        // Run transformation on function body
        let input = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_simple_conditional_before.hir"].assert_eq(&input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `scf.if` operation, followed by
        // an `builtin.return`.
        let output = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_simple_conditional_after.hir"].assert_eq(&output);

        Ok(())
    }

    /// This test ensures that the CF->SCF transformation is correctly applied to unstructured
    /// conditional control flow, where one branch leads to an early exit from the function, while
    /// the other branch performs additional computation before exiting.
    #[test]
    fn cfg_to_scf_lift_conditional_early_exit() -> Result<(), Report> {
        let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
            .is_test(true)
            .format_timestamp(None)
            .try_init();

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new(
                [
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                ],
                [AbiParam::new(Type::U32)],
            );
            builder(name, signature).unwrap()
        };

        // Define function body
        let mut builder = FunctionBuilder::new(function, &mut builder);

        // This is the HIR we derived this test case from originally, as reported in issue #510
        //
        // public builtin.function @cabi_realloc_wit_bindgen_0_28_0(v325: i32, v326: i32, v327: i32, v328: i32) -> i32 {
        //     ^block32(v325: i32, v326: i32, v327: i32, v328: i32):
        //         v330 = arith.constant 0 : i32;
        //         v331 = arith.neq v326, v330 : i1;
        //         cf.cond_br v331 ^block35, ^block36;
        //     ^block34(v343: i32):
        //         v345 = arith.eq v343, v330 : i1;
        //         v346 = arith.zext v345 : u32;
        //         v347 = hir.bitcast v346 : i32;
        //         v349 = arith.neq v347, v330 : i1;
        //         cf.cond_br v349 ^block39, ^block40;
        //     ^block35:
        //         v342 = hir.exec @miden:test-proj-underscore/test-proj-underscore@0.1.0/test_proj_underscore/_RNvCs95KLPZDDxvS_7___rustc14___rust_realloc(v325, v326, v327, v328) : i32
        //         cf.br ^block34(v342);
        //     ^block36:
        //         v333 = arith.neq v328, v330 : i1;
        //         cf.cond_br v333 ^block37, ^block38;
        //     ^block37:
        //         v341 = hir.exec @miden:test-proj-underscore/test-proj-underscore@0.1.0/test_proj_underscore/_RNvCs95KLPZDDxvS_7___rustc12___rust_alloc(v328, v327) : i32
        //         cf.br ^block34(v341);
        //     ^block38:
        //         builtin.ret v327;
        //     ^block39:
        //         ub.unreachable ;
        //     ^block40:
        //         builtin.ret v343;
        //     };

        let block32 = builder.current_block();
        let block34 = builder.create_block();
        let v343 = builder.append_block_param(block34, Type::U32, span);
        let block35 = builder.create_block();
        let block36 = builder.create_block();
        let block37 = builder.create_block();
        let block38 = builder.create_block();
        let block39 = builder.create_block();
        let block40 = builder.create_block();

        let (v325, v326, v327, v328) = {
            let block32 = block32.borrow();
            let args = block32.arguments();
            let arg0: midenc_hir::ValueRef = args[0].upcast();
            let arg2: midenc_hir::ValueRef = args[2].upcast();
            let arg3: midenc_hir::ValueRef = args[3].upcast();
            (arg0, args[1].upcast(), arg2, arg3)
        };

        let v330 = builder.u32(0, span);
        let v331 = builder.neq(v326, v330, span)?;
        builder.cond_br(v331, block35, [], block36, [], span)?;

        builder.switch_to_block(block34);
        let v345 = builder.eq(v343, v330, span)?;
        let v346 = builder.zext(v345, Type::U32, span)?;
        let v349 = builder.neq(v346, v330, span)?;
        builder.cond_br(v349, block39, [], block40, [], span)?;

        builder.switch_to_block(block35);
        let v342 = builder.incr(v325, span)?;
        builder.br(block34, [v342], span)?;

        builder.switch_to_block(block36);
        let v333 = builder.neq(v328, v330, span)?;
        builder.cond_br(v333, block37, [], block38, [], span)?;

        builder.switch_to_block(block37);
        let v341 = builder.incr(v328, span)?;
        builder.br(block34, [v341], span)?;

        builder.switch_to_block(block38);
        builder.ret(Some(v327), span)?;

        builder.switch_to_block(block39);
        builder.unreachable(span);

        builder.switch_to_block(block40);
        builder.ret(Some(v343), span)?;

        let operation = function.as_operation_ref();

        // Run transformation on function body
        let input = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_conditional_early_exit_before.hir"]
            .assert_eq(&input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `scf.if` operation, followed by
        // a `cf.switch`, which branches to either a return, or an unreachable.
        let output = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_conditional_early_exit_after.hir"]
            .assert_eq(&output);

        Ok(())
    }

    #[test]
    fn cfg_to_scf_lift_simple_while_loop() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        // Define function body
        let mut builder = FunctionBuilder::new(function, &mut builder);

        let loop_header = builder.create_block();
        let n = builder.append_block_param(loop_header, Type::U32, span);
        let counter = builder.append_block_param(loop_header, Type::U32, span);
        let if_is_zero = builder.create_block();
        let if_is_nonzero = builder.create_block();

        let block = builder.current_block();
        let input = block.borrow().arguments()[0].upcast();

        let zero = builder.u32(0, span);
        let one = builder.u32(1, span);
        builder.br(loop_header, [input, zero], span)?;

        builder.switch_to_block(loop_header);
        let is_zero = builder.eq(n, zero, span)?;
        builder.cond_br(is_zero, if_is_zero, [], if_is_nonzero, [], span)?;

        builder.switch_to_block(if_is_zero);
        builder.ret(Some(counter), span)?;

        builder.switch_to_block(if_is_nonzero);
        let n_prime = builder.sub_unchecked(n, one, span)?;
        let counter_prime = builder.incr(counter, span)?;
        builder.br(loop_header, [n_prime, counter_prime], span)?;

        let operation = function.as_operation_ref();

        // Run transformation on function body
        let input = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_simple_while_loop_before.hir"].assert_eq(&input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `scf.if` operation, followed by
        // an `builtin.return`.
        let output = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_simple_while_loop_after.hir"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn cfg_to_scf_lift_nested_while_loop() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new(
                [
                    AbiParam::new(Type::from(PointerType::new(Type::U32))),
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                ],
                [AbiParam::new(Type::U32)],
            );
            builder(name, signature).unwrap()
        };

        // Define function body for the following pseudocode:
        //
        // function test(v0: *mut u32, rows: u32, cols: u32) -> u32 {
        //     let row_offset = 0;
        //     let sum = 0;
        //     while row_offset < rows {
        //         let offset = row_offset * cols;
        //         let col_offset = 0;
        //         while col_offset < cols {
        //             let cell = *(v0 + offset + col_offset);
        //             col_offset += 1;
        //             sum += cell;
        //         }
        //         row_offset += 1;
        //     }
        //
        //     return sum;
        // }
        //
        let mut builder = FunctionBuilder::new(function, &mut builder);

        let outer_loop_header = builder.create_block();
        let inner_loop_header = builder.create_block();
        let row_offset = builder.append_block_param(outer_loop_header, Type::U32, span);
        let row_sum = builder.append_block_param(outer_loop_header, Type::U32, span);
        let col_offset = builder.append_block_param(inner_loop_header, Type::U32, span);
        let col_sum = builder.append_block_param(inner_loop_header, Type::U32, span);
        let has_more_rows = builder.create_block();
        let no_more_rows = builder.create_block();
        let has_more_columns = builder.create_block();
        let no_more_columns = builder.create_block();

        let block = builder.current_block();
        let ptr = block.borrow().arguments()[0].upcast();
        let num_rows = block.borrow().arguments()[1].upcast();
        let num_cols = block.borrow().arguments()[2].upcast();

        let zero = builder.u32(0, span);
        builder.br(outer_loop_header, [zero, zero], span)?;

        builder.switch_to_block(outer_loop_header);
        let end_of_rows = builder.lt(row_offset, num_rows, span)?;
        builder.cond_br(end_of_rows, no_more_rows, [], has_more_rows, [row_sum], span)?;

        builder.switch_to_block(no_more_rows);
        builder.ret(Some(row_sum), span)?;

        builder.switch_to_block(has_more_rows);
        let offset = builder.mul_unchecked(row_offset, num_cols, span)?;
        builder.br(inner_loop_header, [zero, row_sum], span)?;

        builder.switch_to_block(inner_loop_header);
        let end_of_cols = builder.lt(col_offset, num_cols, span)?;
        builder.cond_br(end_of_cols, no_more_columns, [], has_more_columns, [col_sum], span)?;

        builder.switch_to_block(no_more_columns);
        let new_row_offset = builder.incr(row_offset, span)?;
        builder.br(outer_loop_header, [new_row_offset, col_sum], span)?;

        builder.switch_to_block(has_more_columns);
        let addr_offset = builder.add_unchecked(offset, col_offset, span)?;
        let addr = builder.unrealized_conversion_cast(ptr, Type::U32, span)?;
        let cell_addr = builder.add_unchecked(addr, addr_offset, span)?;
        // This represents a bitcast
        let cell_ptr = builder.unrealized_conversion_cast(
            cell_addr,
            Type::from(PointerType::new(Type::U32)),
            span,
        )?;
        // This represents a load
        let cell = builder.unrealized_conversion_cast(cell_ptr, Type::U32, span)?;
        let new_col_offset = builder.incr(col_offset, span)?;
        let new_sum = builder.add_unchecked(col_sum, cell, span)?;
        builder.br(inner_loop_header, [new_col_offset, new_sum], span)?;

        let operation = function.as_operation_ref();

        // Run transformation on function body
        let input = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_nested_while_loop_before.hir"].assert_eq(&input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `scf.if` operation, followed by
        // an `builtin.return`.
        let output = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_nested_while_loop_after.hir"].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn cfg_to_scf_lift_multiple_exit_nested_while_loop() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new(
                [
                    AbiParam::new(Type::from(PointerType::new(Type::U32))),
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                ],
                [AbiParam::new(Type::U32)],
            );
            builder(name, signature).unwrap()
        };

        // Define function body for the following pseudocode:
        //
        // function test(v0: *mut u32, rows: u32, cols: u32) -> u32 {
        //     let row_offset = 0;
        //     let sum = 0;
        //     while row_offset < rows {
        //         let offset = row_offset * cols;
        //         let col_offset = 0;
        //         while col_offset < cols {
        //             let cell = *(v0 + offset + col_offset);
        //             col_offset += 1;
        //             let (sum_p, overflowed) = sum.add_overflowing(cell);
        //             if overflowed {
        //                 return u32::MAX;
        //             }
        //             sum += cell;
        //         }
        //         row_offset += 1;
        //     }
        //
        //     return sum;
        // }
        //
        let mut builder = FunctionBuilder::new(function, &mut builder);

        let outer_loop_header = builder.create_block();
        let inner_loop_header = builder.create_block();
        let row_offset = builder.append_block_param(outer_loop_header, Type::U32, span);
        let row_sum = builder.append_block_param(outer_loop_header, Type::U32, span);
        let col_offset = builder.append_block_param(inner_loop_header, Type::U32, span);
        let col_sum = builder.append_block_param(inner_loop_header, Type::U32, span);
        let has_more_rows = builder.create_block();
        let no_more_rows = builder.create_block();
        let has_more_columns = builder.create_block();
        let no_more_columns = builder.create_block();
        let has_overflowed = builder.create_block();

        let block = builder.current_block();
        let ptr = block.borrow().arguments()[0].upcast();
        let num_rows = block.borrow().arguments()[1].upcast();
        let num_cols = block.borrow().arguments()[2].upcast();

        let zero = builder.u32(0, span);
        builder.br(outer_loop_header, [zero, zero], span)?;

        builder.switch_to_block(outer_loop_header);
        let more_rows = builder.lt(row_offset, num_rows, span)?;
        builder.cond_br(more_rows, has_more_rows, [row_sum], no_more_rows, [], span)?;

        builder.switch_to_block(no_more_rows);
        builder.ret(Some(row_sum), span)?;

        builder.switch_to_block(has_more_rows);
        let offset = builder.mul_unchecked(row_offset, num_cols, span)?;
        builder.br(inner_loop_header, [zero, row_sum], span)?;

        builder.switch_to_block(inner_loop_header);
        let more_cols = builder.lt(col_offset, num_cols, span)?;
        builder.cond_br(more_cols, has_more_columns, [col_sum], no_more_columns, [], span)?;

        builder.switch_to_block(no_more_columns);
        let new_row_offset = builder.incr(row_offset, span)?;
        builder.br(outer_loop_header, [new_row_offset, col_sum], span)?;

        builder.switch_to_block(has_more_columns);
        let addr_offset = builder.add_unchecked(offset, col_offset, span)?;
        let addr = builder.unrealized_conversion_cast(ptr, Type::U32, span)?;
        let cell_addr = builder.add_unchecked(addr, addr_offset, span)?;
        // This represents a bitcast
        let cell_ptr = builder.unrealized_conversion_cast(
            cell_addr,
            Type::from(PointerType::new(Type::U32)),
            span,
        )?;
        // This represents a load
        let cell = builder.unrealized_conversion_cast(cell_ptr, Type::U32, span)?;
        let new_col_offset = builder.incr(col_offset, span)?;
        let (overflowed, new_sum) = builder.add_overflowing(col_sum, cell, span)?;
        builder.cond_br(
            overflowed,
            has_overflowed,
            [],
            inner_loop_header,
            [new_col_offset, new_sum],
            span,
        )?;

        builder.switch_to_block(has_overflowed);
        builder.ret_imm(midenc_hir::Immediate::U32(u32::MAX), span)?;

        let operation = function.as_operation_ref();

        // Run transformation on function body
        let input = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_multiple_exit_nested_while_loop_before.hir"]
            .assert_eq(&input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.add_pass(transforms::Canonicalizer::create());
        pm.run(operation)?;

        // Verify that the function body now consists of a single `scf.if` operation, followed by
        // an `builtin.return`.
        let output = format!("{}", &operation.borrow());
        expect_file!["expected/cfg_to_scf_lift_multiple_exit_nested_while_loop_after.hir"]
            .assert_eq(&output);

        Ok(())
    }
}
