use alloc::collections::{BTreeSet, VecDeque};

use cranelift_entity::packed_option::ReservedValue;
use either::Either::{Left, Right};
use intrusive_collections::UnsafeRef;
use midenc_session::Session;

use super::*;
use crate::{
    pass::{AnalysisManager, ConversionPass, ConversionResult},
    Immediate, Opcode, PassInfo, Signature, Type,
};

/// This pass converts the syntax tree of an HIR module to HIR
#[derive(PassInfo)]
pub struct ConvertAstToHir;
impl ConversionPass for ConvertAstToHir {
    type From = Box<Module>;
    type To = crate::Module;

    fn convert(
        &mut self,
        mut ast: Self::From,
        _analyses: &mut AnalysisManager,
        session: &Session,
    ) -> ConversionResult<Self::To> {
        use alloc::collections::btree_map::Entry;

        let mut module = if ast.is_kernel {
            crate::Module::new_kernel(ast.name)
        } else {
            crate::Module::new(ast.name)
        };

        let mut is_valid = true;

        // Validate constants
        let (constants_by_id, is_constants_valid) =
            ast.take_and_validate_constants(&session.diagnostics);
        is_valid &= is_constants_valid;

        let mut remapped_constants = RemappedConstants::default();
        for (constant_id, constant_data) in constants_by_id.into_iter() {
            if let Entry::Vacant(entry) = remapped_constants.entry(constant_id) {
                let new_constant_id = module.globals.insert_constant(constant_data.into_inner());
                entry.insert(new_constant_id);
            }
        }

        // Validate globals
        let (globals_by_id, is_global_vars_valid) =
            ast.take_and_validate_globals(&remapped_constants, &session.diagnostics);
        is_valid &= is_global_vars_valid;

        for (_, gv_data) in globals_by_id.into_iter() {
            unsafe {
                module.globals.insert(gv_data.into_inner());
            }
        }

        // Validate data segments
        for segment_ast in ast.data_segments.drain(..) {
            let span = segment_ast.span();
            if let Err(err) = module.declare_data_segment(
                segment_ast.offset,
                segment_ast.size,
                segment_ast.data,
                segment_ast.readonly,
            ) {
                is_valid = false;
                session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid data segment")
                    .with_primary_label(span, err)
                    .emit();
            }
        }

        // Validate functions
        let mut functions_by_id = BTreeMap::<Ident, Span<Signature>>::default();
        let mut worklist = Vec::with_capacity(ast.functions.len());
        for function in core::mem::take(&mut ast.functions).into_iter() {
            match functions_by_id.entry(function.name) {
                Entry::Vacant(entry) => {
                    entry.insert(Span::new(function.name.span(), function.signature.clone()));
                    worklist.push(function);
                }
                Entry::Occupied(entry) => {
                    let prev = entry.get().span();
                    session
                        .diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid function declaration")
                        .with_primary_label(
                            function.span(),
                            "a function with the same name has already been declared",
                        )
                        .with_secondary_label(prev, "previously declared here")
                        .emit();
                    is_valid = false;
                }
            }
        }

        // Validate imports
        let (imports_by_id, is_externals_valid) =
            ast.take_and_validate_imports(&session.diagnostics);
        is_valid &= is_externals_valid;

        let mut functions = crate::FunctionList::default();
        let mut values_by_id = ValuesById::default();
        let mut used_imports = BTreeSet::<FunctionIdent>::default();
        let mut inst_results = InstResults::default();
        for mut function in worklist.into_iter() {
            values_by_id.clear();
            inst_results.clear();

            let id = FunctionIdent {
                module: module.name,
                function: function.name,
            };

            is_valid &= function.is_declaration_valid(&session.diagnostics);
            let entry = function.blocks[0].id;
            let mut blocks_by_id = match function.populate_block_map(&session.diagnostics) {
                Ok(blocks) => blocks,
                Err(blocks) => {
                    is_valid = false;
                    blocks
                }
            };
            let mut blockq = VecDeque::from([entry]);

            // Build the HIR function
            let mut f = Box::new(crate::Function::new_uninit(id, function.signature));
            // Move attributes from the AST to the DFG
            f.dfg.attrs = function.attrs;
            // The entry block is always the first in the layout
            f.dfg.entry = entry;
            // Visit each block and build it, but do not yet write to the DataFlowGraph
            let mut visited = BTreeSet::<crate::Block>::default();
            while let Some(block_id) = blockq.pop_front() {
                // Do not visit the same block twice
                if !visited.insert(block_id) {
                    continue;
                }

                let mut block_data = crate::BlockData::new(block_id);
                let block = blocks_by_id.remove(&block_id).unwrap();

                // Ensure block parameters are not yet defined
                for (num, param) in block.params.into_iter().enumerate() {
                    match try_insert_param_value(
                        param.id,
                        param.span(),
                        block.id,
                        num as u16,
                        param.ty,
                        &mut values_by_id,
                        &session.diagnostics,
                    ) {
                        Some(id) => {
                            block_data.params.push(id, &mut f.dfg.value_lists);
                        }
                        None => {
                            is_valid = false;
                        }
                    }
                }

                // Populate the block with instructions from the AST
                for (num, inst) in block.body.into_iter().enumerate() {
                    is_valid &= try_insert_inst(
                        inst,
                        num as u16,
                        &mut block_data,
                        &mut blockq,
                        &blocks_by_id,
                        &visited,
                        &mut values_by_id,
                        &mut inst_results,
                        &mut used_imports,
                        &imports_by_id,
                        &functions_by_id,
                        &mut f,
                        &session.diagnostics,
                    );
                }
                f.dfg.blocks.append(block_id, block_data);
            }

            // Now that all of the blocks have been visited,
            // we can record the value data in the DataFlowGraph
            for (id, data) in core::mem::take(&mut values_by_id).into_iter() {
                let next_id = f.dfg.values.next_key();
                let next_raw_id = next_id.as_u32();
                let raw_id = id.as_u32();
                assert!(
                    raw_id >= next_raw_id,
                    "expected that {id} is always ahead of, or equal to {next_id}"
                );
                if raw_id == next_raw_id {
                    f.dfg.values.push(data.into_inner());
                    continue;
                }

                // If we reach here, we need to insert dummy data until the next
                // key is the one that we have data for
                while raw_id > f.dfg.values.next_key().as_u32() {
                    f.dfg.values.push(crate::ValueData::Inst {
                        ty: Type::Unknown,
                        num: 0,
                        inst: crate::Inst::reserved_value(),
                    });
                }
                assert_eq!(f.dfg.values.push(data.into_inner()), id);
            }

            // Also record all of the instruction results
            for (inst, results) in core::mem::take(&mut inst_results).into_iter() {
                f.dfg.results[inst].extend(results, &mut f.dfg.value_lists);
            }

            functions.push_back(f);
        }

        // If any of the imports are unused, add them to all functions in the module
        if imports_by_id.keys().any(|id| !used_imports.contains(id)) {
            while let Some(mut function) = functions.pop_front() {
                function.dfg.imports.extend(imports_by_id.iter().filter_map(|(id, ext)| {
                    if used_imports.contains(id) {
                        None
                    } else {
                        Some((*id, ext.inner().clone()))
                    }
                }));
                module.functions.push_back(function);
            }
        } else {
            module.functions = functions;
        }

        if is_valid {
            Ok(module)
        } else {
            Err(session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message(format!("failed to validate '{}'", module.name))
                .with_help("One or more diagnostics have been emitted, see them for details")
                .into_report())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn try_insert_inst(
    mut inst: Inst,
    num: u16,
    block_data: &mut crate::BlockData,
    blockq: &mut VecDeque<crate::Block>,
    blocks_by_id: &BlocksById,
    visited_blocks: &BTreeSet<crate::Block>,
    values_by_id: &mut ValuesById,
    inst_results: &mut InstResults,
    used_imports: &mut BTreeSet<FunctionIdent>,
    imports_by_id: &ImportsById,
    functions_by_id: &BTreeMap<Ident, Span<Signature>>,
    function: &mut crate::Function,
    diagnostics: &DiagnosticsHandler,
) -> bool {
    use crate::{BinaryOp, BinaryOpImm, Instruction, PrimOp, PrimOpImm, UnaryOp, UnaryOpImm};

    let id = function.dfg.insts.alloc_key();
    let span = inst.span;
    let results = core::mem::take(&mut inst.outputs);

    // Translate instruction data from AST representation to HIR
    let data = match inst.ty {
        InstType::BinaryOp {
            opcode: op,
            overflow,
            operands: [Operand::Value(lhs), Operand::Value(rhs)],
        } => Some(Instruction::BinaryOp(BinaryOp {
            op,
            overflow,
            args: [rhs.into_inner(), lhs.into_inner()],
        })),
        InstType::BinaryOp {
            opcode: op,
            overflow,
            operands: [imm, Operand::Value(rhs)],
        } => {
            let imm_ty = values_by_id.get(&rhs).map(|v| v.ty().clone()).unwrap_or(Type::Unknown);
            operand_to_immediate(imm, &imm_ty, diagnostics).map(|imm| {
                Instruction::BinaryOpImm(BinaryOpImm {
                    op,
                    overflow,
                    arg: rhs.into_inner(),
                    imm,
                })
            })
        }
        InstType::BinaryOp {
            operands: [_, rhs], ..
        } => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid instruction")
                .with_primary_label(rhs.span(), "unexpected immediate operand")
                .with_secondary_label(
                    span,
                    "only the left-hand operand of a binary operator may be immediate",
                )
                .emit();
            None
        }
        InstType::UnaryOp {
            opcode: op,
            overflow,
            operand: Operand::Value(operand),
        } => Some(Instruction::UnaryOp(UnaryOp {
            op,
            overflow,
            arg: operand.into_inner(),
        })),
        InstType::UnaryOp {
            opcode: op,
            overflow,
            operand,
        } => {
            let imm = match op {
                Opcode::ImmI1 => operand_to_immediate(operand, &Type::I1, diagnostics),
                Opcode::ImmI8 => operand_to_immediate(operand, &Type::I8, diagnostics),
                Opcode::ImmU8 => operand_to_immediate(operand, &Type::U8, diagnostics),
                Opcode::ImmI16 => operand_to_immediate(operand, &Type::I16, diagnostics),
                Opcode::ImmU16 => operand_to_immediate(operand, &Type::U16, diagnostics),
                Opcode::ImmI32 => operand_to_immediate(operand, &Type::I32, diagnostics),
                Opcode::ImmU32 => operand_to_immediate(operand, &Type::U32, diagnostics),
                Opcode::ImmI64 => operand_to_immediate(operand, &Type::I64, diagnostics),
                Opcode::ImmU64 => operand_to_immediate(operand, &Type::U64, diagnostics),
                Opcode::ImmF64 => {
                    diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid instruction")
                        .with_primary_label(
                            span,
                            "f64 support is not yet implemented in the parser",
                        )
                        .emit();
                    None
                }
                Opcode::ImmFelt => operand_to_immediate(operand, &Type::Felt, diagnostics),
                _ => {
                    diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid instruction")
                        .with_primary_label(
                            span,
                            "this operation does not support immediate arguments",
                        )
                        .with_secondary_label(operand.span(), "this operand cannot be immediate")
                        .emit();
                    None
                }
            };
            imm.map(|imm| Instruction::UnaryOpImm(UnaryOpImm { op, overflow, imm }))
        }
        InstType::Br {
            opcode: op,
            successor,
        } => {
            match is_valid_successor(
                &successor,
                span,
                blocks_by_id,
                visited_blocks,
                values_by_id,
                diagnostics,
            ) {
                Ok(next) => {
                    if let Some(next) = next {
                        blockq.push_back(next);
                    }
                    let args = crate::ValueList::from_iter(
                        successor.args.iter().map(|arg| arg.into_inner()),
                        &mut function.dfg.value_lists,
                    );
                    Some(Instruction::Br(crate::Br {
                        op,
                        successor: crate::Successor {
                            destination: successor.id,
                            args,
                        },
                    }))
                }
                Err(_) => None,
            }
        }
        InstType::CondBr {
            opcode: op,
            cond,
            then_dest,
            else_dest,
        } => {
            let mut is_valid = is_valid_value_reference(&cond, span, values_by_id, diagnostics);

            match is_valid_successor(
                &then_dest,
                span,
                blocks_by_id,
                visited_blocks,
                values_by_id,
                diagnostics,
            ) {
                Ok(next) => {
                    if let Some(next) = next {
                        blockq.push_back(next);
                    }
                }
                Err(_) => {
                    is_valid = false;
                }
            }

            match is_valid_successor(
                &else_dest,
                span,
                blocks_by_id,
                visited_blocks,
                values_by_id,
                diagnostics,
            ) {
                Ok(next) => {
                    if let Some(next) = next {
                        blockq.push_back(next);
                    }
                }
                Err(_) => {
                    is_valid = false;
                }
            }

            if is_valid {
                let then_args = crate::ValueList::from_iter(
                    then_dest.args.iter().map(|arg| arg.into_inner()),
                    &mut function.dfg.value_lists,
                );
                let else_args = crate::ValueList::from_iter(
                    else_dest.args.iter().map(|arg| arg.into_inner()),
                    &mut function.dfg.value_lists,
                );
                Some(Instruction::CondBr(crate::CondBr {
                    op,
                    cond: cond.into_inner(),
                    then_dest: crate::Successor {
                        destination: then_dest.id,
                        args: then_args,
                    },
                    else_dest: crate::Successor {
                        destination: else_dest.id,
                        args: else_args,
                    },
                }))
            } else {
                None
            }
        }
        InstType::Switch {
            opcode: op,
            selector,
            successors,
            fallback,
        } => {
            let mut is_valid = is_valid_value_reference(&selector, span, values_by_id, diagnostics);

            let mut used_discriminants = BTreeSet::<Span<u32>>::default();
            let mut arms = Vec::with_capacity(successors.len());
            for arm in successors.into_iter() {
                let arm_span = arm.span();
                let discriminant = arm.inner().0;
                if !used_discriminants.insert(Span::new(arm_span, discriminant)) {
                    let prev = used_discriminants
                        .get(&Span::new(SourceSpan::UNKNOWN, discriminant))
                        .unwrap()
                        .span();
                    diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid instruction")
                        .with_primary_label(
                            arm_span,
                            "the discriminant for this switch case has already been used",
                        )
                        .with_secondary_label(prev, "previously used here")
                        .emit();
                    is_valid = false;
                }
                let successor = arm.into_inner().1;
                let successor_args = crate::ValueList::from_iter(
                    successor.args.iter().map(|arg| arg.into_inner()),
                    &mut function.dfg.value_lists,
                );
                arms.push(crate::SwitchArm {
                    value: discriminant,
                    successor: crate::Successor {
                        destination: successor.id,
                        args: successor_args,
                    },
                });
                match is_valid_successor(
                    &successor,
                    span,
                    blocks_by_id,
                    visited_blocks,
                    values_by_id,
                    diagnostics,
                ) {
                    Ok(next) => {
                        if let Some(next) = next {
                            blockq.push_back(next);
                        }
                    }
                    Err(_) => {
                        is_valid = false;
                    }
                }
            }

            let fallback_args = crate::ValueList::from_iter(
                fallback.args.iter().map(|arg| arg.into_inner()),
                &mut function.dfg.value_lists,
            );
            match is_valid_successor(
                &fallback,
                span,
                blocks_by_id,
                visited_blocks,
                values_by_id,
                diagnostics,
            ) {
                Ok(next) => {
                    if let Some(next) = next {
                        blockq.push_back(next);
                    }
                }
                Err(_) => {
                    is_valid = false;
                }
            }

            if is_valid {
                Some(Instruction::Switch(crate::Switch {
                    op,
                    arg: selector.into_inner(),
                    arms,
                    default: crate::Successor {
                        destination: fallback.id,
                        args: fallback_args,
                    },
                }))
            } else {
                None
            }
        }
        InstType::Ret {
            opcode: op,
            mut operands,
        } => {
            let expected_returns = function.signature.results.len();
            let actual_returns = operands.len();
            if actual_returns != expected_returns {
                diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid instruction")
                    .with_primary_label(
                        span,
                        format!(
                            "the current function expects {expected_returns} values to be \
                             returned, but {actual_returns} are returned here"
                        ),
                    )
                    .emit();
                None
            } else {
                match actual_returns {
                    0 => Some(Instruction::Ret(crate::Ret {
                        op,
                        args: Default::default(),
                    })),
                    1 => match operands.pop().unwrap() {
                        Operand::Value(v) => {
                            if is_valid_value_reference(&v, span, values_by_id, diagnostics) {
                                let args = crate::ValueList::from_slice(
                                    &[v.into_inner()],
                                    &mut function.dfg.value_lists,
                                );
                                Some(Instruction::Ret(crate::Ret { op, args }))
                            } else {
                                None
                            }
                        }
                        operand @ (Operand::Int(_) | Operand::BigInt(_)) => operand_to_immediate(
                            operand,
                            &function.signature.results[0].ty,
                            diagnostics,
                        )
                        .map(|arg| Instruction::RetImm(crate::RetImm { op, arg })),
                    },
                    _ => {
                        let mut is_valid = true;
                        let mut args = crate::ValueList::new();
                        for operand in operands.iter() {
                            if let Operand::Value(ref v) = operand {
                                args.push(v.into_inner(), &mut function.dfg.value_lists);
                                is_valid &=
                                    is_valid_value_reference(v, span, values_by_id, diagnostics);
                            } else {
                                diagnostics
                                    .diagnostic(Severity::Error)
                                    .with_message("invalid operand")
                                    .with_primary_label(
                                        operand.span(),
                                        "expected an ssa value here, but got an immediate",
                                    )
                                    .with_secondary_label(span, "occurs here")
                                    .emit();
                                is_valid = false;
                            }
                        }
                        if is_valid {
                            Some(Instruction::Ret(crate::Ret { op, args }))
                        } else {
                            None
                        }
                    }
                }
            }
        }
        InstType::Call {
            opcode: op,
            callee,
            operands,
        } => {
            let mut is_valid = true;
            let callee = match callee {
                Left(local) => {
                    let callee = FunctionIdent {
                        function: local,
                        module: function.id.module,
                    };
                    if let Some(sig) = functions_by_id.get(&local) {
                        use hashbrown::hash_map::Entry;
                        if let Entry::Vacant(entry) = function.dfg.imports.entry(callee) {
                            entry.insert(ExternalFunction {
                                id: callee,
                                signature: sig.inner().clone(),
                            });
                        }
                    } else {
                        diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("reference to undefined local function")
                            .with_primary_label(
                                local.span(),
                                "this function is not defined in the current module, are you \
                                 missing an import?",
                            )
                            .emit();
                        is_valid = false;
                    }
                    callee
                }
                Right(external) => {
                    use hashbrown::hash_map::Entry;
                    used_imports.insert(external);
                    if let Entry::Vacant(entry) = function.dfg.imports.entry(external) {
                        if let Some(ef) = imports_by_id.get(&external) {
                            entry.insert(ef.inner().clone());
                        } else {
                            diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid call instruction")
                                .with_primary_label(
                                    external.span(),
                                    "this function is not imported in the current module, you \
                                     must do so in order to reference it",
                                )
                                .with_secondary_label(span, "invalid callee for this instruction")
                                .emit();
                            is_valid = false;
                        }
                    }
                    external
                }
            };

            is_valid &=
                is_valid_value_references(operands.as_slice(), span, values_by_id, diagnostics);
            if is_valid {
                let args = crate::ValueList::from_iter(
                    operands.iter().map(|arg| arg.into_inner()),
                    &mut function.dfg.value_lists,
                );
                Some(Instruction::Call(crate::Call { op, callee, args }))
            } else {
                None
            }
        }
        InstType::CallIndirect { .. } => {
            unimplemented!("indirect calls are not implemented in the IR yet")
        }
        InstType::PrimOp {
            opcode: op,
            operands,
        } => {
            if operands.is_empty() {
                Some(Instruction::PrimOp(PrimOp {
                    op,
                    args: Default::default(),
                }))
            } else {
                let mut is_valid = true;
                let mut imm = None;
                let mut args = crate::ValueList::new();
                for (i, operand) in operands.iter().cloned().enumerate() {
                    let is_first = i == 0;
                    match operand {
                        Operand::Value(v) => {
                            is_valid &=
                                is_valid_value_reference(&v, span, values_by_id, diagnostics);
                            args.push(v.into_inner(), &mut function.dfg.value_lists);
                        }
                        operand @ (Operand::Int(_) | Operand::BigInt(_)) if is_first => {
                            imm = match op {
                                Opcode::AssertEq => {
                                    if let Some(value) = operands[i + 1].as_value() {
                                        match values_by_id.get(value.inner()).map(|vd| vd.ty()) {
                                            Some(ty) => {
                                                operand_to_immediate(operand, ty, diagnostics)
                                            }
                                            None => {
                                                diagnostics
                                                    .diagnostic(Severity::Error)
                                                    .with_message("undefined value")
                                                    .with_primary_label(
                                                        operand.span(),
                                                        "this value is not defined yet",
                                                    )
                                                    .with_secondary_label(
                                                        span,
                                                        "ensure the value is defined in a \
                                                         dominating block of this instruction",
                                                    )
                                                    .emit();
                                                None
                                            }
                                        }
                                    } else {
                                        diagnostics
                                            .diagnostic(Severity::Error)
                                            .with_message("invalid operand")
                                            .with_primary_label(
                                                operand.span(),
                                                "expected an ssa value here, but got an immediate",
                                            )
                                            .with_secondary_label(
                                                span,
                                                "only the first argument of this instruction may \
                                                 be an immediate",
                                            )
                                            .emit();
                                        None
                                    }
                                }
                                Opcode::Store => {
                                    operand_to_immediate(operand, &Type::U32, diagnostics)
                                }
                                opcode => {
                                    unimplemented!("unsupported primop with immediate {opcode}")
                                }
                            };
                            if imm.is_none() {
                                is_valid = false;
                            }
                        }
                        operand @ (Operand::Int(_) | Operand::BigInt(_)) => {
                            diagnostics
                                .diagnostic(Severity::Error)
                                .with_message("invalid immediate operand")
                                .with_primary_label(
                                    operand.span(),
                                    "expected an ssa value here, but got an immediate",
                                )
                                .with_secondary_label(
                                    span,
                                    "only the first argument of this instruction may be an \
                                     immediate",
                                )
                                .emit();
                            is_valid = false;
                        }
                    }
                }
                if is_valid {
                    if let Some(imm) = imm {
                        Some(Instruction::PrimOpImm(PrimOpImm { op, imm, args }))
                    } else {
                        Some(Instruction::PrimOp(crate::PrimOp { op, args }))
                    }
                } else {
                    None
                }
            }
        }
        InstType::GlobalValue {
            opcode: _op,
            expr: _,
        } => {
            todo!()
        }
    };

    // If the instruction data is invalid, we still need to handle the instruction results
    // for subsequent instructions which may reference its results
    if let Some(data) = data {
        // Add instruction to DataFlowGraph
        let node = crate::InstNode::new(id, block_data.id, Span::new(span, data));
        function.dfg.insts.append(id, node);

        // Add results to values_by_id map
        let mut is_valid = true;
        let results_vec = inst_results.entry(id).or_default();
        for tv in results.into_iter() {
            if let Some(value) =
                try_insert_result_value(tv.id, tv.span(), id, num, tv.ty, values_by_id, diagnostics)
            {
                results_vec.push(value);
            } else {
                is_valid = false;
            }
        }

        // Append instruction to block
        let unsafe_ref =
            unsafe { UnsafeRef::from_raw(function.dfg.insts.get_raw(id).unwrap().as_ptr()) };
        block_data.append(unsafe_ref);

        is_valid
    } else {
        for tv in results.into_iter() {
            try_insert_result_value(tv.id, tv.span(), id, num, tv.ty, values_by_id, diagnostics);
        }

        false
    }
}

fn is_valid_successor(
    successor: &Successor,
    parent_span: SourceSpan,
    blocks_by_id: &BlocksById,
    visited: &BTreeSet<crate::Block>,
    values_by_id: &ValuesById,
    diagnostics: &DiagnosticsHandler,
) -> Result<Option<crate::Block>, crate::Block> {
    let is_visited = visited.contains(&successor.id);
    let is_valid = is_valid_value_references(
        successor.args.as_slice(),
        parent_span,
        values_by_id,
        diagnostics,
    );
    if blocks_by_id.contains_key(&successor.id) || is_visited {
        if is_valid {
            if is_visited {
                Ok(None)
            } else {
                Ok(Some(successor.id))
            }
        } else {
            Err(successor.id)
        }
    } else {
        diagnostics
            .diagnostic(Severity::Error)
            .with_message("invalid instruction")
            .with_primary_label(successor.span, "invalid successor: the named block does not exist")
            .with_secondary_label(parent_span, "found in this instruction")
            .emit();
        Err(successor.id)
    }
}

fn is_valid_value_references<'a, I: IntoIterator<Item = &'a Span<crate::Value>>>(
    values: I,
    parent_span: SourceSpan,
    values_by_id: &ValuesById,
    diagnostics: &DiagnosticsHandler,
) -> bool {
    let mut is_valid = true;
    let mut is_empty = true;
    for value in values.into_iter() {
        is_empty = false;
        is_valid &= is_valid_value_reference(value, parent_span, values_by_id, diagnostics);
    }
    is_empty || is_valid
}

fn is_valid_value_reference(
    value: &Span<crate::Value>,
    parent_span: SourceSpan,
    values_by_id: &ValuesById,
    diagnostics: &DiagnosticsHandler,
) -> bool {
    let is_valid = values_by_id.contains_key(value.inner());
    if !is_valid {
        diagnostics
            .diagnostic(Severity::Error)
            .with_message("invalid value operand")
            .with_primary_label(
                value.span(),
                "this value is either undefined, or its definition does not dominate this use",
            )
            .with_secondary_label(parent_span, "invalid use occurs in this instruction")
            .emit();
    }
    is_valid
}

fn try_insert_param_value(
    id: crate::Value,
    span: SourceSpan,
    block: crate::Block,
    num: u16,
    ty: Type,
    values: &mut BTreeMap<crate::Value, Span<crate::ValueData>>,
    diagnostics: &DiagnosticsHandler,
) -> Option<crate::Value> {
    use alloc::collections::btree_map::Entry;

    match values.entry(id) {
        Entry::Vacant(entry) => {
            let data = crate::ValueData::Param {
                ty,
                num,
                block,
                span,
            };
            entry.insert(Span::new(span, data));
            Some(id)
        }
        Entry::Occupied(entry) => {
            let prev = entry.get().span();
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid block")
                .with_primary_label(span, "the name of this block parameter is already in use")
                .with_secondary_label(prev, "previously declared here")
                .emit();
            None
        }
    }
}

fn try_insert_result_value(
    id: crate::Value,
    span: SourceSpan,
    inst: crate::Inst,
    num: u16,
    ty: Type,
    values: &mut BTreeMap<crate::Value, Span<crate::ValueData>>,
    diagnostics: &DiagnosticsHandler,
) -> Option<crate::Value> {
    use alloc::collections::btree_map::Entry;

    match values.entry(id) {
        Entry::Vacant(entry) => {
            let data = crate::ValueData::Inst { ty, num, inst };
            entry.insert(Span::new(span, data));
            Some(id)
        }
        Entry::Occupied(entry) => {
            let prev = entry.get().span();
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid instruction")
                .with_primary_label(span, "a value may only be declared a single time")
                .with_secondary_label(prev, "previously declared here")
                .emit();
            None
        }
    }
}

fn operand_to_immediate(
    operand: Operand,
    ty: &Type,
    diagnostics: &DiagnosticsHandler,
) -> Option<Immediate> {
    match operand {
        Operand::Int(i) => smallint_to_immediate(i.span(), i.into_inner(), ty, diagnostics),
        Operand::BigInt(i) => bigint_to_immediate(i.span(), i.into_inner(), ty, diagnostics),
        Operand::Value(_) => panic!("cannot convert ssa values to immediate"),
    }
}

fn smallint_to_immediate(
    span: SourceSpan,
    i: isize,
    ty: &Type,
    diagnostics: &DiagnosticsHandler,
) -> Option<Immediate> {
    match ty {
        Type::I1 => Some(Immediate::I1(i != 0)),
        Type::I8 => try_convert_imm(i, span, ty, diagnostics).map(Immediate::I8),
        Type::I16 => try_convert_imm(i, span, ty, diagnostics).map(Immediate::I16),
        Type::I32 => try_convert_imm(i, span, ty, diagnostics).map(Immediate::I32),
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::U256 if i < 0 => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(span, format!("expected a non-negative integer of type {ty}"))
                .emit();
            None
        }
        Type::U8 => try_convert_imm(i as usize, span, ty, diagnostics).map(Immediate::U8),
        Type::U16 => try_convert_imm(i as usize, span, ty, diagnostics).map(Immediate::U16),
        Type::U32 => try_convert_imm(i as usize, span, ty, diagnostics).map(Immediate::U32),
        Type::I64 => Some(Immediate::I64(i as i64)),
        Type::U64 => Some(Immediate::U64(i as u64)),
        Type::I128 => Some(Immediate::I128(i as i128)),
        Type::U128 | Type::U256 | Type::F64 => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(span, format!("immediates of type {ty} are not yet supported"))
                .emit();
            None
        }
        ty => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(
                    span,
                    format!("expected an immediate of type {ty}, but got an integer"),
                )
                .emit();
            None
        }
    }
}

fn bigint_to_immediate(
    span: SourceSpan,
    i: num_bigint::BigInt,
    ty: &Type,
    diagnostics: &DiagnosticsHandler,
) -> Option<Immediate> {
    use num_traits::cast::ToPrimitive;

    let is_negative = i.sign() == num_bigint::Sign::Minus;
    // NOTE: If we are calling this function, then `i` was too large to fit in an `isize`, so it
    // must be a large integer type
    let imm = match ty {
        Type::U32 if !is_negative => i.to_u32().map(Immediate::U32),
        Type::I64 => i.to_i64().map(Immediate::I64),
        Type::U64 if !is_negative => i.to_u64().map(Immediate::U64),
        Type::I128 => i.to_i128().map(Immediate::I128),
        Type::U128 | Type::U256 | Type::F64 => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(span, format!("immediates of type {ty} are not yet supported"))
                .emit();
            return None;
        }
        ty => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(
                    span,
                    format!("expected an immediate of type {ty}, but got an integer"),
                )
                .emit();
            return None;
        }
    };
    if imm.is_none() {
        diagnostics
            .diagnostic(Severity::Error)
            .with_message("invalid immediate operand")
            .with_primary_label(
                span,
                format!(
                    "expected an immediate of type {ty}, but got {i}, which is out of range for \
                     that type"
                ),
            )
            .emit();
    }
    imm
}

fn try_convert_imm<T, U>(
    i: T,
    span: SourceSpan,
    ty: &Type,
    diagnostics: &DiagnosticsHandler,
) -> Option<U>
where
    U: TryFrom<T>,
    <U as TryFrom<T>>::Error: fmt::Display,
{
    match U::try_from(i) {
        Ok(value) => Some(value),
        Err(err) => {
            diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid immediate operand")
                .with_primary_label(
                    span,
                    format!("cannot interpret this as a value of type {ty}: {err}"),
                )
                .emit();
            None
        }
    }
}
