use alloc::rc::Rc;

use midenc_hir2::{
    dialects::builtin,
    dominance::DominanceInfo,
    pass::{Pass, PassExecutionState},
    transforms::{self, CFGToSCFInterface},
    Builder, EntityMut, Op, Operation, OperationName, OperationRef, RawWalk, Report, SmallVec,
    Spanned, Type, ValueRef, WalkResult,
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

        let after_region = { op.after().as_region_ref() };
        let after_block = builder.create_block(after_region, None, &[]);
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
        Ok(op.as_operation_ref())
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
        let mut builder = FunctionBuilder::new(&mut func, &mut builder);

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

        let operation = func.as_operation_ref();
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

    #[test]
    fn cfg_to_scf_lift_simple_while_loop() -> Result<(), Report> {
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
        let mut builder = FunctionBuilder::new(&mut func, &mut builder);

        let loop_header = builder.create_block();
        let n = builder.append_block_param(loop_header, Type::U32, span);
        let counter = builder.append_block_param(loop_header, Type::U32, span);
        let if_is_zero = builder.create_block();
        let if_is_nonzero = builder.create_block();

        let block = builder.current_block();
        let input = block.borrow().arguments()[0].upcast();

        let zero = builder.ins().u32(0, span);
        let one = builder.ins().u32(1, span);
        builder.ins().br(loop_header, [input, zero], span)?;

        builder.switch_to_block(loop_header);
        let is_zero = builder.ins().eq(n, zero, span)?;
        builder.ins().cond_br(is_zero, if_is_zero, [], if_is_nonzero, [], span)?;

        builder.switch_to_block(if_is_zero);
        builder.ins().ret(Some(counter), span)?;

        builder.switch_to_block(if_is_nonzero);
        let n_prime = builder.ins().sub_unchecked(n, one, span)?;
        let counter_prime = builder.ins().incr(counter, span)?;
        builder.ins().br(loop_header, [n_prime, counter_prime], span)?;

        let operation = func.as_operation_ref();
        drop(func);

        // Run transformation on function body
        let expected_input = "\
builtin.function public @test(v0: u32) -> u32 {
^block0(v0: u32):
    v3 = hir.constant 0 : u32;
    v4 = hir.constant 1 : u32;
    hir.br block1(v0, v3);
^block1(v1: u32, v2: u32):
    v5 = hir.eq v1, v3 : i1;
    hir.cond_br v5 block2, block3;
^block2:
    hir.ret v2;
^block3:
    v6 = hir.sub v1, v4 : u32 #[overflow = unchecked];
    v7 = hir.incr v2 : u32;
    hir.br block1(v6, v7);
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
    v15 = hir.poison  : u32 #[ty = u32];
    v14 = hir.constant 1 : u32;
    v9 = hir.constant 0 : u32;
    v3 = hir.constant 0 : u32;
    v4 = hir.constant 1 : u32;
    v23, v24, v25 = hir.while v0, v3, v15 : u32, u32, u32 {
    ^block1(v1: u32, v2: u32, v19: u32):
        v5 = hir.eq v1, v3 : i1;
        v30, v31, v32, v33 = hir.if v5 : u32, u32, u32, u32 {
        ^block9:
            hir.yield v15, v15, v14, v9;
        } {
        ^block3:
            v6 = hir.sub v1, v4 : u32 #[overflow = unchecked];
            v7 = hir.incr v2 : u32;
            hir.yield v6, v7, v9, v14;
        };
        v26 = hir.trunc v33 : i1 #[ty = i1];
        hir.condition v26, v30, v31, v2;
    } {
    ^block8(v27: u32, v28: u32, v29: u32):
        hir.yield v27, v28, v29;
    };
    hir.ret v25;
};";
        let output = format!("{}", &operation.borrow());
        assert_str_eq!(&expected_output, &output);

        Ok(())
    }

    #[test]
    fn cfg_to_scf_lift_nested_while_loop() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let mut function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new(
                [
                    AbiParam::new(Type::Ptr(Box::new(Type::U32))),
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
        let mut func = function.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder);

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

        let zero = builder.ins().u32(0, span);
        builder.ins().br(outer_loop_header, [zero, zero], span)?;

        builder.switch_to_block(outer_loop_header);
        let end_of_rows = builder.ins().lt(row_offset, num_rows, span)?;
        builder
            .ins()
            .cond_br(end_of_rows, no_more_rows, [], has_more_rows, [row_sum], span)?;

        builder.switch_to_block(no_more_rows);
        builder.ins().ret(Some(row_sum), span)?;

        builder.switch_to_block(has_more_rows);
        let offset = builder.ins().mul_unchecked(row_offset, num_cols, span)?;
        builder.ins().br(inner_loop_header, [zero, row_sum], span)?;

        builder.switch_to_block(inner_loop_header);
        let end_of_cols = builder.ins().lt(col_offset, num_cols, span)?;
        builder.ins().cond_br(
            end_of_cols,
            no_more_columns,
            [],
            has_more_columns,
            [col_sum],
            span,
        )?;

        builder.switch_to_block(no_more_columns);
        let new_row_offset = builder.ins().incr(row_offset, span)?;
        builder.ins().br(outer_loop_header, [new_row_offset, col_sum], span)?;

        builder.switch_to_block(has_more_columns);
        let addr_offset = builder.ins().add_unchecked(offset, col_offset, span)?;
        let addr = builder.ins().ptrtoint(ptr, Type::U32, span)?;
        let cell_addr = builder.ins().add_unchecked(addr, addr_offset, span)?;
        let cell_ptr = builder.ins().inttoptr(cell_addr, Type::Ptr(Box::new(Type::U32)), span)?;
        let cell = builder.ins().load(cell_ptr, span)?;
        let new_col_offset = builder.ins().incr(col_offset, span)?;
        let new_sum = builder.ins().add_unchecked(col_sum, cell, span)?;
        builder.ins().br(inner_loop_header, [new_col_offset, new_sum], span)?;

        let operation = func.as_operation_ref();
        drop(func);

        // Run transformation on function body
        let expected_input = "\
builtin.function public @test(v0: (ptr u32), v1: u32, v2: u32) -> u32 {
^block0(v0: (ptr u32), v1: u32, v2: u32):
    v7 = hir.constant 0 : u32;
    hir.br block1(v7, v7);
^block1(v3: u32, v4: u32):
    v8 = hir.lt v3, v1 : i1;
    hir.cond_br v8 block4, block3(v4);
^block2(v5: u32, v6: u32):
    v10 = hir.lt v5, v2 : i1;
    hir.cond_br v10 block6, block5(v6);
^block3:
    v9 = hir.mul v3, v2 : u32 #[overflow = unchecked];
    hir.br block2(v7, v4);
^block4:
    hir.ret v4;
^block5:
    v12 = hir.add v9, v5 : u32 #[overflow = unchecked];
    v13 = hir.ptr_to_int v0 : u32 #[ty = u32];
    v14 = hir.add v13, v12 : u32 #[overflow = unchecked];
    v15 = hir.int_to_ptr v14 : (ptr u32) #[ty = (ptr u32)];
    v16 = hir.load v15 : u32;
    v17 = hir.incr v5 : u32;
    v18 = hir.add v6, v16 : u32 #[overflow = unchecked];
    hir.br block2(v17, v18);
^block6:
    v11 = hir.incr v3 : u32;
    hir.br block1(v11, v6);
};";
        let input = format!("{}", &operation.borrow());
        assert_str_eq!(&expected_input, &input);

        let mut pm = pass::PassManager::on::<builtin::Function>(context, pass::Nesting::Implicit);
        pm.add_pass(Box::new(LiftControlFlowToSCF));
        pm.run(operation)?;

        // Verify that the function body now consists of a single `hir.if` operation, followed by
        // an `hir.return`.
        let expected_output = "\
builtin.function public @test(v0: (ptr u32), v1: u32, v2: u32) -> u32 {
^block0(v0: (ptr u32), v1: u32, v2: u32):
    v26 = hir.poison  : u32 #[ty = u32];
    v25 = hir.constant 1 : u32;
    v20 = hir.constant 0 : u32;
    v7 = hir.constant 0 : u32;
    v35, v36, v37 = hir.while v7, v7, v26 : u32, u32, u32 {
    ^block1(v3: u32, v4: u32, v30: u32):
        v8 = hir.lt v3, v1 : i1;
        v60, v61, v62, v63, v64 = hir.if v8 : u32, u32, u32, u32, u32 {
        ^block16:
            hir.yield v26, v26, v25, v20, v4;
        } {
        ^block3:
            v9 = hir.mul v3, v2 : u32 #[overflow = unchecked];
            v53, v54, v55 = hir.while v7, v4, v26 : u32, u32, u32 {
            ^block2(v5: u32, v6: u32, v49: u32):
                v10 = hir.lt v5, v2 : i1;
                v65, v66, v67, v68 = hir.if v10 : u32, u32, u32, u32 {
                ^block17:
                    hir.yield v26, v26, v25, v20;
                } {
                ^block5:
                    v12 = hir.add v9, v5 : u32 #[overflow = unchecked];
                    v13 = hir.ptr_to_int v0 : u32 #[ty = u32];
                    v14 = hir.add v13, v12 : u32 #[overflow = unchecked];
                    v15 = hir.int_to_ptr v14 : (ptr u32) #[ty = (ptr u32)];
                    v16 = hir.load v15 : u32;
                    v17 = hir.incr v5 : u32;
                    v18 = hir.add v6, v16 : u32 #[overflow = unchecked];
                    hir.yield v17, v18, v20, v25;
                };
                v56 = hir.trunc v68 : i1 #[ty = i1];
                hir.condition v56, v65, v66, v6;
            } {
            ^block15(v57: u32, v58: u32, v59: u32):
                hir.yield v57, v58, v59;
            };
            v11 = hir.incr v3 : u32;
            hir.yield v11, v55, v20, v25, v26;
        };
        v38 = hir.trunc v63 : i1 #[ty = i1];
        hir.condition v38, v60, v61, v64;
    } {
    ^block11(v39: u32, v40: u32, v41: u32):
        hir.yield v39, v40, v41;
    };
    hir.ret v37;
};";
        let output = format!("{}", &operation.borrow());
        assert_str_eq!(&expected_output, &output);

        Ok(())
    }
}
