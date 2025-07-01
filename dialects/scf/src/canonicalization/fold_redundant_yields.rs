use alloc::rc::Rc;
use core::{any::TypeId, ops::Index};

use midenc_hir::{
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Lift common yield results from their regions to their predecessors.
pub struct FoldRedundantYields {
    info: PatternInfo,
}

impl FoldRedundantYields {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "fold-redundant-yields",
                PatternKind::Trait(TypeId::of::<dyn RegionBranchOpInterface>()),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for FoldRedundantYields {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for FoldRedundantYields {
    fn match_and_rewrite(
        &self,
        mut op_ref: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let mut op = op_ref.borrow_mut();

        let Some(br_op) = op.as_trait::<dyn RegionBranchOpInterface>() else {
            return Ok(false);
        };

        // For each successor gather its terminator and the terminator operands.
        let mut term_ops: SmallVec<[_; 4]> = SmallVec::new();
        let mut region_yields: SmallVec<[_; 4]> = SmallVec::new();

        for succ_region in br_op.get_successor_regions(RegionBranchPoint::Parent) {
            let Some(region_ref) = succ_region.successor() else {
                return Ok(false);
            };

            // Several assertions follow: an SCF region always has a single block, that block must
            // have a terminator which then implements RegionBranchTerminatorOpInterface.
            let region = region_ref.borrow();
            assert!(region.has_one_block());

            let block = region.entry();
            let term_op_ref =
                block.terminator().expect("All region blocks must have a terminator.");

            // Got the terminator.
            let term_op = term_op_ref.borrow();
            let term_op = term_op.as_trait::<dyn RegionBranchTerminatorOpInterface>().expect(
                "All region block terminators must impl RegionBranchTerminatorOpInterface.",
            );

            // For now we only support regions with a simple `yield` terminator.  This may change
            // in the future if/when we support other RegionBranchOps (e.g., `while`).
            let term_op_name = term_op.as_operation().name();
            if term_op_name.dialect() != "scf" || term_op_name.name() != "yield" {
                return Ok(false);
            }

            // Save the terminator and each of its opands paired with their indices.
            term_ops.push(term_op_ref);
            region_yields.push(
                term_op
                    .get_successor_operands(RegionBranchPoint::Parent)
                    .forwarded()
                    .iter()
                    .map(|opand_ref| {
                        let opand = opand_ref.borrow();
                        (opand.index(), opand.as_value_ref())
                    })
                    .collect::<adt::SmallSet<_, 4>>(),
            );
        }

        if region_yields.len() < 2 {
            return Ok(false);
        }

        // Fold the yield opand sets down via intersection to a final set which will contain the
        // redundant values.
        let redundant_yield_vals = region_yields
            .into_iter()
            .reduce(|acc, region_yield_vals| acc.intersection(&region_yield_vals))
            .expect("Have already checked region_yields is non-empty.");

        if redundant_yield_vals.is_empty() {
            // No redundant values found.
            return Ok(false);
        }

        // Save a copy of the redundant result positions.
        let mut redundant_result_positions =
            redundant_yield_vals.iter().map(|(pos, _)| *pos).collect::<SmallVec<[_; 4]>>();

        let all_results_are_redundant = redundant_yield_vals.len() == op.num_results();

        if all_results_are_redundant && op.is_memory_effect_free() {
            // The entire operation is actually redundant; just remove it.  Make sure the yield
            // vals are sorted first.
            let mut sorted_vals = redundant_yield_vals.into_vec();
            sorted_vals.sort_unstable_by(|a, b| a.0.cmp(&b.0));

            // Wrap each of the values in Some.
            let some_vals =
                sorted_vals.into_iter().map(|(_, val)| Some(val)).collect::<SmallVec<[_; 4]>>();

            // Replace the operation.
            drop(op);
            rewriter.replace_op_with_values(op_ref, &some_vals);
        } else {
            // Replace all uses of the redundant results.
            for (redundant_opand_pos, redundant_yield_val) in redundant_yield_vals {
                let result_val_ref =
                    op.results().index(redundant_opand_pos).borrow().as_value_ref();
                if result_val_ref.borrow().is_used() {
                    rewriter.replace_all_uses_of_value_with(result_val_ref, redundant_yield_val);
                }
            }

            // Next remove the redundant results.  Iterate for each position in reverse to avoid
            // invalidating offsets as we go.
            redundant_result_positions.sort_unstable_by(|a, b| b.cmp(a));
            for idx in &redundant_result_positions {
                op.results_mut().group_mut(0).erase(*idx);
            }

            // And remove the redundant terminator operands.
            for mut term_op in term_ops {
                let mut new_opands = SmallVec::<[ValueRef; 4]>::default();

                // Make a copy of the old operands, except for the redundant value, except in the
                // case where they're all redundant where we need no operands.
                if !all_results_are_redundant {
                    for old_opand in term_op.borrow().operands().iter() {
                        if !redundant_result_positions.contains(&old_opand.index()) {
                            new_opands.push(old_opand.borrow().as_value_ref());
                        }
                    }
                }

                // Update the terminator with the new operands.
                let _guard = rewriter.modify_op_in_place(term_op);
                let mut term_op_mut = term_op.borrow_mut();
                term_op_mut.set_operands(new_opands);
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, format, rc::Rc, sync::Arc, vec::Vec};

    use builtin::{BuiltinOpBuilder, FunctionBuilder};
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_cf::{ControlFlowOpBuilder, SwitchCase};
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_expect_test::expect_file;
    use midenc_hir::{
        dialects::builtin,
        pass,
        pass::{Pass, PassExecutionState},
        patterns::{
            FrozenRewritePatternSet, GreedyRewriteConfig, RewritePattern, RewritePatternSet,
        },
        AbiParam, BuilderExt, Context, Ident, OpBuilder, Report, Signature, SourceSpan, Type,
    };

    use super::*;

    struct SingleCanonicalizerPass {
        rewrites: Rc<FrozenRewritePatternSet>,
        should_modify: bool,
    }

    impl SingleCanonicalizerPass {
        fn new(
            context: Rc<Context>,
            pattern: Box<dyn RewritePattern>,
            should_modify: bool,
        ) -> Self {
            let pattern_set = RewritePatternSet::from_iter(context, [pattern]);
            let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));

            Self {
                rewrites,
                should_modify,
            }
        }
    }

    impl Pass for SingleCanonicalizerPass {
        type Target = Operation;

        fn name(&self) -> &'static str {
            "test-single-rewriter"
        }

        fn can_schedule_on(&self, _name: &OperationName) -> bool {
            true
        }

        fn run_on_operation(
            &mut self,
            op: EntityMut<'_, Self::Target>,
            state: &mut PassExecutionState,
        ) -> Result<(), Report> {
            let op_ref = op.as_operation_ref();
            drop(op);

            let converged = patterns::apply_patterns_and_fold_greedily(
                op_ref,
                self.rewrites.clone(),
                GreedyRewriteConfig::default(),
            );

            let changed = match converged {
                Ok(b) => b,
                Err(e) => {
                    panic!("Pass returned error: {e}");
                }
            };

            match (changed, self.should_modify) {
                (true, false) => panic!("Pass modified input unexpectedly."),
                (false, true) => panic!("Pass did not modify input."),
                _ => {}
            }

            state.set_post_pass_status(changed.into());

            Ok(())
        }
    }

    fn run_single_canonicalizer(
        context: Rc<Context>,
        operation: OperationRef,
        name: &'static str,
        should_modify: bool,
    ) -> Result<(), Report> {
        // UNCOMMENT TO DUMP (SHOW DIFF OF) CF INPUT.
        // let input = format!("{}", &operation.borrow());
        // expect_file!["non-existentent"].assert_eq(&input);

        // Run the CF->SCF pass first, then the canonicalisation pass.  Need to register the SCF
        // dialect to make sure the patterns are registered.
        let _scf_dialect = context.get_or_register_dialect::<ScfDialect>();
        let mut pm =
            pass::PassManager::on::<builtin::Function>(context.clone(), pass::Nesting::Implicit);
        pm.add_pass(Box::new(transforms::LiftControlFlowToSCF));
        pm.run(operation)?;

        // Confirm the CF->SCF transformed IR is correct.
        let input = format!("{}", operation.borrow());
        let before_file_path = format!("expected/{name}_before.hir");
        expect_file![before_file_path.as_str()].assert_eq(&input);

        pm.add_pass(Box::new(SingleCanonicalizerPass::new(
            context.clone(),
            Box::new(FoldRedundantYields::new(context.clone())),
            should_modify,
        )));
        pm.run(operation)?;

        // Confirm the canonicalised IR is correct.
        let output = format!("{}", operation.borrow());
        let after_file_path = format!("expected/{name}_after.hir");
        expect_file![after_file_path.as_str()].assert_eq(&output);

        Ok(())
    }

    #[test]
    fn fold_redundant_yields_subset_if_switch() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let if_then = builder.create_block();
        let if_else = builder.create_block();
        let switch_on_one_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_default_block = builder.create_block();
        let if_final = builder.create_block();

        let if_sum_lhs = builder.append_block_param(if_final, Type::U32, span);
        let if_sum_rhs = builder.append_block_param(if_final, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();
        let redundant_val = builder.u32(11, span);

        let zero = builder.u32(0, span);
        let is_zero = builder.eq(input, zero, span)?;
        builder.cond_br(is_zero, if_then, [], if_else, [], span)?;

        builder.switch_to_block(if_then);
        let then_non_redundant_val = builder.u32(22, span);
        builder.br(if_final, [redundant_val, then_non_redundant_val], span)?;

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        builder.switch_to_block(if_else);
        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case],
            switch_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        let switch_on_one_non_redundant_val = builder.u32(33, span);
        builder.br(if_final, [redundant_val, switch_on_one_non_redundant_val], span)?;

        builder.switch_to_block(switch_on_two_block);
        let switch_on_two_non_redundant_val = builder.u32(44, span);
        builder.br(if_final, [redundant_val, switch_on_two_non_redundant_val], span)?;

        builder.switch_to_block(switch_default_block);
        let switch_default_non_redundant_val = builder.u32(55, span);
        builder.br(if_final, [redundant_val, switch_default_non_redundant_val], span)?;

        // Add all the results together along with the redundant value to give them all users.
        builder.switch_to_block(if_final);
        let if_sum0 = builder.add(if_sum_lhs, if_sum_rhs, span)?;
        let if_sum1 = builder.add(if_sum0, redundant_val, span)?;
        builder.ret(Some(if_sum1), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_subset_if_switch",
            true,
        )
    }

    #[test]
    fn fold_redundant_yields_all_if_switch() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let switch_on_one_block = builder.create_block();
        let if_then_block = builder.create_block();
        let if_else_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_default_block = builder.create_block();
        let switch_final_block = builder.create_block();

        let sum_lhs = builder.append_block_param(switch_final_block, Type::U32, span);
        let sum_rhs = builder.append_block_param(switch_final_block, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();
        let redundant_val0 = builder.u32(11, span);
        let redundant_val1 = builder.u32(22, span);

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case],
            switch_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        builder.br(switch_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(switch_on_two_block);
        let zero = builder.u32(0, span);
        let is_zero = builder.eq(input, zero, span)?;
        builder.cond_br(is_zero, if_then_block, [], if_else_block, [], span)?;

        builder.switch_to_block(if_then_block);
        builder.br(switch_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(if_else_block);
        builder.br(switch_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(switch_default_block);
        builder.br(switch_final_block, [redundant_val0, redundant_val1], span)?;

        // Add all the results together along with the redundant value to give them all users.
        builder.switch_to_block(switch_final_block);
        let sum = builder.add(sum_lhs, sum_rhs, span)?;
        builder.ret(Some(sum), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_all_if_switch",
            true,
        )
    }

    #[test]
    fn fold_redundant_yields_all_switch_if() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let if_then_block = builder.create_block();
        let switch_on_one_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_default_block = builder.create_block();
        let if_else_block = builder.create_block();
        let if_final_block = builder.create_block();

        let sum_lhs = builder.append_block_param(if_final_block, Type::U32, span);
        let sum_rhs = builder.append_block_param(if_final_block, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();
        let redundant_val0 = builder.u32(11, span);
        let redundant_val1 = builder.u32(22, span);

        let zero = builder.u32(0, span);
        let is_not_zero = builder.neq(input, zero, span)?;
        builder.cond_br(is_not_zero, if_then_block, [], if_else_block, [], span)?;

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        builder.switch_to_block(if_then_block);
        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case],
            switch_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        builder.br(if_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(switch_on_two_block);
        builder.br(if_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(switch_default_block);
        builder.br(if_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(if_else_block);
        builder.br(if_final_block, [redundant_val0, redundant_val1], span)?;

        builder.switch_to_block(if_final_block);
        let sum = builder.add(sum_lhs, sum_rhs, span)?;
        builder.ret(Some(sum), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_all_switch_if",
            true,
        )
    }

    #[test]
    fn fold_redundant_yields_many_switch() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let switch_on_one_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_on_three_block = builder.create_block();
        let switch_on_default_block = builder.create_block();
        let switch_final_block = builder.create_block();
        let exit_block = builder.create_block();

        let final_arg0 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg1 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg2 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg3 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg4 = builder.append_block_param(switch_final_block, Type::U32, span);

        let ret_val = builder.append_block_param(exit_block, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();

        let redundant_val11 = builder.u32(11, span);
        let redundant_val22 = builder.u32(22, span);
        let redundant_val33 = builder.u32(33, span);

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        let switch_on_three_case = SwitchCase {
            value: 3,
            successor: switch_on_three_block,
            arguments: Vec::default(),
        };

        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case, switch_on_three_case],
            switch_on_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        let switch_on_one_non_redundant_val0 = builder.u32(100, span);
        let switch_on_one_non_redundant_val1 = builder.u32(101, span);
        builder.br(
            switch_final_block,
            [
                redundant_val11,
                redundant_val22,
                switch_on_one_non_redundant_val0,
                redundant_val33,
                switch_on_one_non_redundant_val1,
            ],
            span,
        )?;

        builder.switch_to_block(switch_on_two_block);
        let switch_on_two_non_redundant_val0 = builder.u32(200, span);
        let switch_on_two_non_redundant_val1 = builder.u32(201, span);
        builder.br(
            switch_final_block,
            [
                redundant_val11,
                redundant_val22,
                switch_on_two_non_redundant_val0,
                redundant_val33,
                switch_on_two_non_redundant_val1,
            ],
            span,
        )?;

        builder.switch_to_block(switch_on_three_block);
        let switch_on_three_non_redundant_val0 = builder.u32(300, span);
        let switch_on_three_non_redundant_val1 = builder.u32(301, span);
        builder.br(
            switch_final_block,
            [
                redundant_val11,
                redundant_val22,
                switch_on_three_non_redundant_val0,
                redundant_val33,
                switch_on_three_non_redundant_val1,
            ],
            span,
        )?;

        builder.switch_to_block(switch_on_default_block);
        let switch_on_default_non_redundant_val0 = builder.u32(400, span);
        let switch_on_default_non_redundant_val1 = builder.u32(401, span);
        builder.br(
            switch_final_block,
            [
                redundant_val11,
                redundant_val22,
                switch_on_default_non_redundant_val0,
                redundant_val33,
                switch_on_default_non_redundant_val1,
            ],
            span,
        )?;

        // Add all the results together.
        builder.switch_to_block(switch_final_block);
        let sum0 = builder.add(final_arg0, final_arg1, span)?;
        let sum1 = builder.add(sum0, final_arg2, span)?;
        let sum2 = builder.add(sum1, final_arg3, span)?;
        let sum3 = builder.add(sum2, final_arg4, span)?;
        builder.br(exit_block, [sum3], span)?;

        builder.switch_to_block(exit_block);
        builder.ret(Some(ret_val), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_many_switch",
            true,
        )
    }

    #[test]
    fn fold_redundant_yields_different_pos_switch() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let switch_on_one_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_on_default_block = builder.create_block();
        let switch_final_block = builder.create_block();

        let final_arg0 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg1 = builder.append_block_param(switch_final_block, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();

        let redundant_val11 = builder.u32(11, span);
        let redundant_val22 = builder.u32(22, span);

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case],
            switch_on_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        builder.br(switch_final_block, [redundant_val11, redundant_val22], span)?;

        // 'yielding' args in reverse order.
        builder.switch_to_block(switch_on_two_block);
        builder.br(switch_final_block, [redundant_val22, redundant_val11], span)?;

        builder.switch_to_block(switch_on_default_block);
        builder.br(switch_final_block, [redundant_val11, redundant_val22], span)?;

        // Add all the results together.
        builder.switch_to_block(switch_final_block);
        let sum = builder.add(final_arg0, final_arg1, span)?;
        builder.ret(Some(sum), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_different_pos_switch",
            false, // Should not modify input.
        )
    }

    #[test]
    fn fold_redundant_yields_all_but_one_switch() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let switch_on_one_block = builder.create_block();
        let switch_on_two_block = builder.create_block();
        let switch_on_three_block = builder.create_block();
        let switch_on_default_block = builder.create_block();
        let switch_final_block = builder.create_block();
        let exit_block = builder.create_block();

        let final_arg0 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg1 = builder.append_block_param(switch_final_block, Type::U32, span);
        let final_arg2 = builder.append_block_param(switch_final_block, Type::U32, span);

        let ret_val = builder.append_block_param(exit_block, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();

        let redundant_val11 = builder.u32(11, span);
        let redundant_val22 = builder.u32(22, span);
        let redundant_val33 = builder.u32(33, span);

        let switch_on_one_case = SwitchCase {
            value: 1,
            successor: switch_on_one_block,
            arguments: Vec::default(),
        };

        let switch_on_two_case = SwitchCase {
            value: 2,
            successor: switch_on_two_block,
            arguments: Vec::default(),
        };

        let switch_on_three_case = SwitchCase {
            value: 3,
            successor: switch_on_three_block,
            arguments: Vec::default(),
        };

        builder.switch(
            input,
            [switch_on_one_case, switch_on_two_case, switch_on_three_case],
            switch_on_default_block,
            Vec::default(),
            span,
        )?;

        builder.switch_to_block(switch_on_one_block);
        builder.br(
            switch_final_block,
            [redundant_val11, redundant_val22, redundant_val33],
            span,
        )?;

        builder.switch_to_block(switch_on_two_block);
        builder.br(
            switch_final_block,
            [redundant_val11, redundant_val22, redundant_val33],
            span,
        )?;

        builder.switch_to_block(switch_on_three_block);
        builder.br(
            switch_final_block,
            [redundant_val11, redundant_val22, redundant_val33],
            span,
        )?;

        builder.switch_to_block(switch_on_default_block);
        let non_redundant_val44 = builder.u32(44, span);
        builder.br(
            switch_final_block,
            [redundant_val11, non_redundant_val44, redundant_val33],
            span,
        )?;

        // Add all the results together.
        builder.switch_to_block(switch_final_block);
        let sum0 = builder.add(final_arg0, final_arg1, span)?;
        let sum1 = builder.add(sum0, final_arg2, span)?;
        builder.br(exit_block, [sum1], span)?;

        builder.switch_to_block(exit_block);
        builder.ret(Some(ret_val), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_all_but_one_switch",
            true,
        )
    }

    #[test]
    fn fold_redundant_yields_effects_if() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new(
                [
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::Ptr(Arc::new(PointerType::new(Type::U32)))),
                ],
                [AbiParam::new(Type::U32)],
            );
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let if_then = builder.create_block();
        let if_else = builder.create_block();
        let if_final = builder.create_block();

        let ret_val = builder.append_block_param(if_final, Type::U32, span);

        let input = builder.current_block().borrow().arguments()[0].upcast();
        let input_ptr = builder.current_block().borrow().arguments()[1].upcast();
        let redundant_val = builder.u32(11, span);

        let zero = builder.u32(0, span);
        let is_zero = builder.eq(input, zero, span)?;
        builder.cond_br(is_zero, if_then, [], if_else, [], span)?;

        builder.switch_to_block(if_then);
        builder.br(if_final, [redundant_val], span)?;

        builder.switch_to_block(if_else);
        builder.store(input_ptr, redundant_val, span)?;
        builder.br(if_final, [redundant_val], span)?;

        // Add all the results together along with the redundant value to give them all users.
        builder.switch_to_block(if_final);
        builder.ret(Some(ret_val), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_effects_if",
            true,
        )
    }

    /* A `while` test which initially showed that this rewriter probably isn't suited for them. {{{

    #[test]
    fn fold_redundant_yields_subset_while() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());

        let span = SourceSpan::default();
        let function = {
            let builder = builder.create::<builtin::Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(name, signature).unwrap()
        };

        let mut builder = FunctionBuilder::new(function, &mut builder);

        let input = builder.current_block().borrow().arguments()[0].upcast();
        let redundant_val = builder.u32(11, span);

        // Create a while op, pass zero.
        let zero = builder.u32(0, span);
        let while_op = builder.r#while([zero], &[Type::U32], span)?;
        let while_before_block = while_op.borrow().before().entry().as_block_ref();
        let while_after_block = while_op.borrow().after().entry().as_block_ref();

        // Check the passed arg against the function arg.
        builder.switch_to_block(while_before_block);
        let while_cond_arg = while_before_block.borrow().arguments()[0].upcast();
        let finished = builder.eq(while_cond_arg, input, span)?;
        let next_iter = builder.incr(while_cond_arg, span)?;
        builder.condition(finished, [next_iter], span)?;

        // Just yield the final results from the while op.
        builder.switch_to_block(while_after_block);
        let yield_values = while_after_block
            .borrow()
            .arguments()
            .iter()
            .map(|a| a.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        builder.r#yield(yield_values, span)?;

        // Add all the results together along with the redundant value to give them all users.
        builder.switch_to_block(builder.entry_block());
        let summed_while_results =
            while_op.borrow().results().iter().try_fold(zero, |acc, res| {
                let res_val_ref = res.borrow().as_value_ref();
                builder.add(acc, res_val_ref, span)
            })?;
        let final_sum = builder.add(summed_while_results, redundant_val, span)?;
        builder.ret(Some(final_sum), span)?;

        run_single_canonicalizer(
            context,
            function.as_operation_ref(),
            "fold_redundant_yields_subset_while",
        )
    }
    }}} */
}
