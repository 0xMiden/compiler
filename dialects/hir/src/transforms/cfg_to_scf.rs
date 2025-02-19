use alloc::rc::Rc;

use midenc_hir2::{
    dialects::builtin,
    dominance::DominanceInfo,
    pass::{Pass, PassExecutionState},
    transforms::{self, CFGToSCFInterface},
    Builder, EntityMut, Op, Operation, OperationName, OperationRef, RawWalk, Report, SmallVec,
    Spanned, Type, WalkResult,
};
use midenc_session::diagnostics::Severity;

use crate::{builders::DefaultInstBuilder, InstBuilder};

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

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let mut transformation = ControlFlowToSCFTransformation;
        let mut changed = false;

        let root = op.as_operation_ref();
        drop(op);

        let result = root.raw_postwalk(|operation: OperationRef| -> WalkResult {
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
                    let mut next_region = inner.borrow().regions().front().as_pointer();
                    while let Some(region) = next_region.take() {
                        next_region = region.next();

                        let result =
                            transforms::transform_cfg_to_scf(region, &mut transformation, dominfo);
                        match result {
                            Ok(did_change) => {
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

                operation.raw_postwalk(visitor)?;
            }

            WalkResult::Continue(())
        });

        if result.was_interrupted() {
            return result.into_result();
        }

        if !changed {
            state.preserved_analyses_mut().preserve_all();
        }

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
        builder: &mut midenc_hir2::OpBuilder,
        control_flow_cond_op: midenc_hir2::OperationRef,
        result_types: &[midenc_hir2::Type],
        regions: &mut midenc_hir2::SmallVec<[midenc_hir2::RegionRef; 2]>,
    ) -> Result<midenc_hir2::OperationRef, midenc_hir2::Report> {
        let ins = DefaultInstBuilder::new(builder);

        let cf_op = control_flow_cond_op.borrow();
        if let Some(cond_br) = cf_op.downcast_ref::<crate::ops::CondBr>() {
            assert_eq!(regions.len(), 2);

            let span = cond_br.span();
            let mut if_op = ins.r#if(cond_br.condition().as_value_ref(), span)?;
            let mut op = if_op.borrow_mut();
            let operation = op.as_operation().as_operation_ref();
            for (i, result) in result_types.iter().enumerate() {
                let result =
                    builder.context().make_result(span, result.clone(), operation, i as u8);
                op.results_mut().push(result);
            }

            op.then_body_mut().take_body(regions[0]);
            op.else_body_mut().take_body(regions[1]);

            return Ok(operation);
        }

        if let Some(_switch) = cf_op.downcast_ref::<crate::ops::Switch>() {
            // `get_cfg_switch_value` returns a u32 that we need to convert to index first.
            /*
                auto cast = builder.create<arith::IndexCastUIOp>(
                    controlFlowCondOp->getLoc(), builder.getIndexType(),
                    switchOp.getFlag());
                SmallVector<int64_t> cases;
                if (auto caseValues = switchOp.getCaseValues())
                    llvm::append_range(
                        cases, llvm::map_range(*caseValues, [](const llvm::APInt &apInt) {
                        return apInt.getZExtValue();
                        }));

                assert(regions.size() == cases.size() + 1);

                auto indexSwitchOp = builder.create<scf::IndexSwitchOp>(
                    controlFlowCondOp->getLoc(), resultTypes, cast, cases, cases.size());

                indexSwitchOp.getDefaultRegion().takeBody(regions[0]);
                for (auto &&[targetRegion, sourceRegion] :
                        llvm::zip(indexSwitchOp.getCaseRegions(), llvm::drop_begin(regions)))
                    targetRegion.takeBody(sourceRegion);

                return indexSwitchOp.getOperation();
            */
            unimplemented!("scf.index_switch has not yet been implemented")
        }

        Err(builder
            .context()
            .session
            .diagnostics
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
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        _branch_region_op: midenc_hir2::OperationRef,
        _replaced_control_flow_op: Option<midenc_hir2::OperationRef>,
        results: &[midenc_hir2::ValueRef],
    ) -> Result<(), midenc_hir2::Report> {
        let ins = DefaultInstBuilder::new(builder);
        ins.r#yield(results.iter().copied(), span)?;

        Ok(())
    }

    /// Creates an `scf.while` op. The loop body is made the before-region of the
    /// while op and terminated with an `scf.condition` op. The after-region does
    /// nothing but forward the iteration variables.
    fn create_structured_do_while_loop_op(
        &self,
        builder: &mut midenc_hir2::OpBuilder,
        replaced_op: midenc_hir2::OperationRef,
        loop_values_init: &[midenc_hir2::ValueRef],
        condition: midenc_hir2::ValueRef,
        loop_values_next_iter: &[midenc_hir2::ValueRef],
        loop_body: midenc_hir2::RegionRef,
    ) -> Result<midenc_hir2::OperationRef, midenc_hir2::Report> {
        let span = replaced_op.span();

        let ins = DefaultInstBuilder::new(builder);
        let mut while_op = ins.r#while(loop_values_init.iter().copied(), span)?;
        let mut op = while_op.borrow_mut();
        let operation = op.as_operation().as_operation_ref();

        // Results are derived from the forwarded values given to `hir.condition`
        for (i, forwarded) in loop_values_next_iter.iter().enumerate() {
            let fwd = forwarded.borrow();
            let ty = fwd.ty().clone();
            let span = fwd.span();
            let result = builder.context().make_result(span, ty, operation, i as u8);
            op.results_mut().push(result);
        }

        op.before_mut().take_body(loop_body);

        builder.set_insertion_point_to_end(op.before().body().back().as_pointer().unwrap());

        // `get_cfg_switch_value` returns a u32. We therefore need to truncate the condition to i1
        // first. It is guaranteed to be either 0 or 1 already.
        let ins = DefaultInstBuilder::new(builder);
        let cond = ins.trunc(condition, Type::I1, span)?;
        let ins = DefaultInstBuilder::new(builder);
        ins.condition(cond, loop_values_next_iter.iter().copied(), span)?;

        let after_block = builder.create_block(op.after().as_region_ref(), None, &[]);
        let context = builder.context_rc();
        let yielded = loop_values_init.iter().map(|loop_var| {
            context.append_block_argument(after_block, loop_var.borrow().ty().clone(), span)
        });

        let ins = DefaultInstBuilder::new(builder);
        ins.r#yield(yielded, span)?;

        Ok(operation)
    }

    /// Creates an `arith.constant` with an i32 attribute of the given value.
    fn get_cfg_switch_value(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        value: u32,
    ) -> midenc_hir2::ValueRef {
        let ins = DefaultInstBuilder::new(builder);
        ins.u32(value, span)
    }

    /// Creates a `cf.switch` op with the given cases and flag.
    fn create_cfg_switch_op(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        flag: midenc_hir2::ValueRef,
        case_values: &[u32],
        case_destinations: &[midenc_hir2::BlockRef],
        case_arguments: &[&[midenc_hir2::ValueRef]],
        default_dest: midenc_hir2::BlockRef,
        default_args: &[midenc_hir2::ValueRef],
    ) -> Result<(), Report> {
        let cases = case_values
            .iter()
            .copied()
            .zip(case_destinations.iter().copied().zip(case_arguments.iter().copied()))
            .map(|(value, (successor, args))| crate::SwitchCase {
                value,
                successor,
                arguments: args.to_vec(),
            })
            .collect::<SmallVec<[_; 4]>>();

        let ins = DefaultInstBuilder::new(builder);
        ins.switch(flag, cases, default_dest, default_args.iter().copied(), span)?;

        Ok(())
    }

    fn create_single_destination_branch(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        _dummy_flag: midenc_hir2::ValueRef,
        destination: midenc_hir2::BlockRef,
        arguments: &[midenc_hir2::ValueRef],
    ) -> Result<(), Report> {
        let ins = DefaultInstBuilder::new(builder);
        ins.br(destination, arguments.iter().copied(), span)?;

        Ok(())
    }

    fn create_conditional_branch(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        condition: midenc_hir2::ValueRef,
        true_dest: midenc_hir2::BlockRef,
        true_args: &[midenc_hir2::ValueRef],
        false_dest: midenc_hir2::BlockRef,
        false_args: &[midenc_hir2::ValueRef],
    ) -> Result<(), Report> {
        let ins = DefaultInstBuilder::new(builder);
        ins.cond_br(
            condition,
            true_dest,
            true_args.iter().copied(),
            false_dest,
            false_args.iter().copied(),
            span,
        )?;

        Ok(())
    }

    /// Creates a `ub.poison` op of the given type.
    fn get_undef_value(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        ty: midenc_hir2::Type,
    ) -> midenc_hir2::ValueRef {
        let ins = DefaultInstBuilder::new(builder);
        ins.poison(ty, span)
    }

    fn create_unreachable_terminator(
        &self,
        span: midenc_hir2::SourceSpan,
        builder: &mut midenc_hir2::OpBuilder,
        _region: midenc_hir2::RegionRef,
    ) -> Result<midenc_hir2::OperationRef, midenc_hir2::Report> {
        let ins = DefaultInstBuilder::new(builder);
        let op = ins.unreachable(span)?;
        Ok(op.borrow().as_operation().as_operation_ref())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc};

    use midenc_hir2::{
        dialects::builtin, pass, AbiParam, BuilderExt, Context, Ident, OpBuilder, Report,
        Signature, SourceSpan, Type,
    };
    use pretty_assertions::assert_str_eq;

    use super::*;
    use crate::builders::FunctionBuilder;

    #[test]
    fn cfg_to_scf_lift_simple_conditional() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let mut function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        // Define function body
        let mut func = function.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut func, builder);

        let if_is_zero = builder.create_block();
        let if_is_nonzero = builder.create_block();
        let exit_block = builder.create_block();
        let return_val = builder.append_block_param(exit_block, Type::U32, span);

        let block = builder.current_block();
        let input = block.borrow().arguments()[0].upcast();

        let zero = builder.ins().u32(0, span);
        let is_zero = builder.ins().eq(input, zero, span)?;
        builder.ins().cond_br(is_zero, if_is_zero, [], if_is_nonzero, [], span)?;

        builder.switch_to_block(if_is_zero);
        let a = builder.ins().incr(input, span)?;
        builder.ins().br(exit_block, [a], span)?;

        builder.switch_to_block(if_is_nonzero);
        let b = builder.ins().mul(input, input, span)?;
        builder.ins().br(exit_block, [b], span)?;

        builder.switch_to_block(exit_block);
        builder.ins().ret(Some(return_val), span)?;

        drop(builder);

        let operation = func.as_operation().as_operation_ref();
        drop(func);

        // Run transformation on function body
        let expected_input = "\
builtin.function public @test(v0: u32) -> u32 {
^block0(v0: u32):
    v2 = hir.constant 0 : u32;
    v3 = hir.eq v0, v2 : i1;
    hir.cond_br v3 block1, block2;
^block1:
    v4 = hir.incr v0 : u32;
    hir.br block3(v4);
^block2:
    v5 = hir.mul v0, v0 : u32 #[overflow = checked];
    hir.br block3(v5);
^block3(v1: u32):
    hir.ret v1;
};";
        let input = format!("{}", &operation.borrow());
        assert_str_eq!(&expected_input, &input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `hir.if` operation, followed by
        // an `hir.return`.
        let expected_output = "\
builtin.function public @test(v0: u32) -> u32 {
^block0(v0: u32):
    v2 = hir.constant 0 : u32;
    v3 = hir.eq v0, v2 : i1;
    v8 = hir.if v3 : u32 {
    ^block1:
        v4 = hir.incr v0 : u32;
        hir.yield v4;
    } {
    ^block2:
        v5 = hir.mul v0, v0 : u32 #[overflow = checked];
        hir.yield v5;
    };
    hir.ret v8;
};";
        let output = format!("{}", &operation.borrow());
        assert_str_eq!(&expected_output, &output);

        Ok(())
    }
}
