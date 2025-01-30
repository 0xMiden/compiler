//! This module contains the bulk of the code performing the translation between
//! WebAssembly and Miden IR.
//!
//! The translation is done in one pass, opcode by opcode. Two main data structures are used during
//! code translations: the value stack and the control stack. The value stack mimics the execution
//! of the WebAssembly stack machine: each instruction result is pushed onto the stack and
//! instruction arguments are popped off the stack. Similarly, when encountering a control flow
//! block, it is pushed onto the control stack and popped off when encountering the corresponding
//! `End`.
//!
//! Another data structure, the translation state, records information concerning unreachable code
//! status and about if inserting a return at the end of the function is necessary.
//!
//! Based on Cranelift's Wasm -> CLIF translator v11.0.0

use midenc_dialect_hir::InstBuilder;
use midenc_hir::{
    cranelift_entity::packed_option::ReservedValue,
    diagnostics::{DiagnosticsHandler, IntoDiagnostic, Report, SourceSpan},
    Block, FieldElement, Inst, Type,
    Type::*,
};
use midenc_hir2::{BlockRef, Immediate, ValueRef};
use wasmparser::{MemArg, Operator};

use crate::{
    error::WasmResult,
    miden_abi::{is_miden_abi_module, transform::transform_miden_abi_call},
    module::{
        func_translation_state::{ControlStackFrame, ElseData, FuncTranslationState},
        function_builder_ext::FunctionBuilderExt,
        module_translation_state::ModuleTranslationState,
        types::{ir_type, BlockType, FuncIndex, GlobalIndex, ModuleTypes},
        Module,
    },
    ssa::Variable,
    unsupported_diag,
};

#[cfg(test)]
mod tests;

/// Translates wasm operators into Miden IR instructions.
#[allow(clippy::too_many_arguments)]
pub fn translate_operator(
    op: &Operator,
    builder: &mut FunctionBuilderExt,
    state: &mut FuncTranslationState,
    module_state: &mut ModuleTranslationState,
    module: &Module,
    mod_types: &ModuleTypes,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    if !state.reachable {
        translate_unreachable_operator(op, builder, state, mod_types, diagnostics, span)?;
        return Ok(());
    }

    // Given that we believe the current block is reachable, the FunctionBuilderExt ought to agree.
    debug_assert!(!builder.is_unreachable());

    match op {
        /********************************** Locals ****************************************
         *  `get_local` and `set_local` are treated as non-SSA variables and will completely
         *  disappear in the Miden IR
         ***********************************************************************************/
        Operator::LocalGet { local_index } => {
            let val = builder.use_var(Variable::from_u32(*local_index));
            state.push1(val);
        }
        Operator::LocalSet { local_index } => {
            let val = state.pop1();
            let var = Variable::from_u32(*local_index);
            let expected_ty = builder.variable_type(var).clone();
            let value_ty = val.borrow().ty().clone();
            let mut ins = builder.ins();
            let val = if expected_ty != value_ty {
                if expected_ty == I32 && value_ty == U32 {
                    ins.bitcast(val, I32, span)?
                } else if expected_ty == I64 && value_ty == U64 {
                    ins.bitcast(val, I64, span)?
                } else {
                    let expected_ty = expected_ty.clone();
                    ins.cast(val, expected_ty, span)?
                }
            } else {
                val
            };
            builder.def_var(var, val);
        }
        Operator::LocalTee { local_index } => {
            let val = state.peek1();
            builder.def_var(Variable::from_u32(*local_index), val);
        }
        /********************************** Globals ****************************************/
        Operator::GlobalGet { global_index } => {
            let global_index = GlobalIndex::from_u32(*global_index);
            let name = module.global_name(global_index);
            let ty = ir_type(module.globals[global_index].ty, diagnostics)?;
            state.push1(builder.ins().load_symbol(name.as_str(), ty, span));
        }
        Operator::GlobalSet { global_index } => {
            let global_index = GlobalIndex::from_u32(*global_index);
            let name = module.global_name(global_index);
            let ty = ir_type(module.globals[global_index].ty, diagnostics)?;
            let ptr = builder.ins().symbol_addr(name.as_str(), Ptr(ty.clone().into()), span);
            let val = state.pop1();
            builder.ins().store(ptr, val, span);
        }
        /********************************* Stack misc **************************************/
        Operator::Drop => _ = state.pop1(),
        Operator::Select => {
            let (arg1, arg2, cond) = state.pop3();
            // if cond is not 0, return arg1, else return arg2
            // https://www.w3.org/TR/wasm-core-1/#-hrefsyntax-instr-parametricmathsfselect%E2%91%A0
            // cond is expected to be an i32
            let cond_i1 = builder.ins().neq_imm(cond, 0, span)?;
            state.push1(builder.ins().select(cond_i1, arg1, arg2, span)?);
        }
        Operator::TypedSelect { ty } => {
            let (arg1, arg2, cond) = state.pop3();
            match ty {
                wasmparser::ValType::F32 => {
                    let cond =
                        builder.ins().gt_imm(cond, Immediate::Felt(midenc_hir::Felt::ZERO), span);
                    state.push1(builder.ins().select(cond, arg1, arg2, span)?);
                }
                wasmparser::ValType::I32 => {
                    let cond = builder.ins().neq_imm(cond, 0, span)?;
                    state.push1(builder.ins().select(cond, arg1, arg2, span)?);
                }
                wasmparser::ValType::I64 => {
                    todo!()
                    // let cond = builder.ins().neq_imm(cond, Immediate::I64(0), span);
                    // state.push1(builder.ins().select(cond, arg1, arg2, span)?);
                }
                ty => panic!("unsupported value type for 'select': {ty}"),
            };
        }
        Operator::Unreachable => {
            builder.ins().unreachable(span);
            state.reachable = false;
        }
        Operator::Nop => {}
        /***************************** Control flow blocks *********************************/
        Operator::Block { blockty } => {
            translate_block(blockty, builder, state, mod_types, diagnostics, span)?;
        }
        Operator::Loop { blockty } => {
            translate_loop(blockty, builder, state, mod_types, diagnostics, span)?;
        }
        Operator::If { blockty } => {
            translate_if(blockty, state, builder, mod_types, diagnostics, span)?;
        }
        Operator::Else => translate_else(state, builder, span)?,
        Operator::End => translate_end(state, builder, span),

        /**************************** Branch instructions *********************************/
        Operator::Br { relative_depth } => translate_br(state, relative_depth, builder, span),
        Operator::BrIf { relative_depth } => {
            translate_br_if(*relative_depth, builder, state, span)?
        }
        Operator::BrTable { targets } => translate_br_table(targets, state, builder, span)?,
        Operator::Return => translate_return(state, builder, diagnostics, span)?,
        /************************************ Calls ****************************************/
        Operator::Call { function_index } => {
            translate_call(
                state,
                module_state,
                builder,
                FuncIndex::from_u32(*function_index),
                span,
                diagnostics,
            )?;
        }
        Operator::CallIndirect {
            type_index: _,
            table_index: _,
        } => {
            // TODO:
        }
        /******************************* Memory management *********************************/
        Operator::MemoryGrow { .. } => {
            let arg = state.pop1_bitcasted(U32, builder, span);
            state.push1(builder.ins().mem_grow(arg, span)?);
        }
        Operator::MemorySize { .. } => {
            // Return total Miden memory size
            state.push1(builder.ins().mem_size(span)?);
        }
        /******************************* Bulk memory operations *********************************/
        Operator::MemoryCopy { dst_mem, src_mem } => {
            // See semantics at https://github.com/WebAssembly/bulk-memory-operations/blob/master/proposals/bulk-memory-operations/Overview.md#memorycopy-instruction
            if *src_mem == 0 && src_mem == dst_mem {
                let count_i32 = state.pop1();
                let src_i32 = state.pop1();
                let dst_i32 = state.pop1();
                let count = builder.ins().bitcast(count_i32, Type::U32, span)?;
                let dst = prepare_addr(dst_i32, &U8, None, builder, span)?;
                let src = prepare_addr(src_i32, &U8, None, builder, span)?;
                builder.ins().memcpy(src, dst, count, span);
            } else {
                unsupported_diag!(diagnostics, "MemoryCopy: only single memory is supported");
            }
        }
        Operator::MemoryFill { mem } => {
            // See semantics at https://webassembly.github.io/spec/core/exec/instructions.html#exec-memory-fill
            if *mem != 0 {
                unsupported_diag!(diagnostics, "MemoryFill: only single memory is supported");
            }
            let num_bytes = state.pop1();
            let value = state.pop1();
            let dst_i32 = state.pop1();
            let value = builder.ins().trunc(value, Type::U8, span)?;
            let num_bytes = builder.ins().bitcast(num_bytes, Type::U32, span)?;
            let dst = prepare_addr(dst_i32, &U8, None, builder, span)?;
            builder.ins().memset(dst, num_bytes, value, span);
        }
        /******************************* Load instructions ***********************************/
        Operator::I32Load8U { memarg } => {
            translate_load_zext(U8, I32, memarg, state, builder, span)?;
        }
        Operator::I32Load16U { memarg } => {
            translate_load_zext(U16, I32, memarg, state, builder, span)?;
        }
        Operator::I32Load8S { memarg } => {
            translate_load_sext(I8, I32, memarg, state, builder, span)?;
        }
        Operator::I32Load16S { memarg } => {
            translate_load_sext(I16, I32, memarg, state, builder, span)?;
        }
        Operator::I64Load8U { memarg } => {
            translate_load_zext(U8, I64, memarg, state, builder, span)?;
        }
        Operator::I64Load16U { memarg } => {
            translate_load_zext(U16, I64, memarg, state, builder, span)?;
        }
        Operator::I64Load8S { memarg } => {
            translate_load_sext(I8, I64, memarg, state, builder, span)?;
        }
        Operator::I64Load16S { memarg } => {
            translate_load_sext(I16, I64, memarg, state, builder, span)?;
        }
        Operator::I64Load32S { memarg } => {
            translate_load_sext(I32, I64, memarg, state, builder, span)?;
        }
        Operator::I64Load32U { memarg } => {
            translate_load_zext(U32, I64, memarg, state, builder, span)?;
        }
        Operator::I32Load { memarg } => translate_load(I32, memarg, state, builder, span)?,
        Operator::I64Load { memarg } => translate_load(I64, memarg, state, builder, span)?,
        Operator::F32Load { memarg } => translate_load(Felt, memarg, state, builder, span)?,
        /****************************** Store instructions ***********************************/
        Operator::I32Store { memarg } => translate_store(I32, memarg, state, builder, span)?,
        Operator::I64Store { memarg } => translate_store(I64, memarg, state, builder, span)?,
        Operator::F32Store { memarg } => translate_store(Felt, memarg, state, builder, span)?,
        Operator::I32Store8 { memarg } | Operator::I64Store8 { memarg } => {
            translate_store(U8, memarg, state, builder, span);
        }
        Operator::I32Store16 { memarg } | Operator::I64Store16 { memarg } => {
            translate_store(U16, memarg, state, builder, span)?;
        }
        Operator::I64Store32 { memarg } => translate_store(U32, memarg, state, builder, span)?,
        /****************************** Nullary Operators **********************************/
        Operator::I32Const { value } => state.push1(builder.ins().i32(*value, span)),
        Operator::I64Const { value } => state.push1(builder.ins().i64(*value, span)),

        /******************************* Unary Operators *************************************/
        Operator::I32Clz | Operator::I64Clz => {
            let val = state.pop1();
            let count = builder.ins().clz(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of clz as an i32
            state.push1(builder.ins().bitcast(count, Type::I32, span)?);
        }
        Operator::I32Ctz | Operator::I64Ctz => {
            let val = state.pop1();
            let count = builder.ins().ctz(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of ctz as an i32
            state.push1(builder.ins().bitcast(count, Type::I32, span)?);
        }
        Operator::I32Popcnt | Operator::I64Popcnt => {
            let val = state.pop1();
            let count = builder.ins().popcnt(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of popcnt as an i32
            state.push1(builder.ins().bitcast(count, Type::I32, span)?);
        }
        Operator::I32Extend8S | Operator::I32Extend16S => {
            let val = state.pop1();
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64ExtendI32S => {
            let val = state.pop1();
            state.push1(builder.ins().sext(val, I64, span)?);
        }
        Operator::I64ExtendI32U => {
            let val = state.pop1();
            let u32_val = builder.ins().bitcast(val, U32, span)?;
            let u64_val = builder.ins().zext(u32_val, U64, span)?;
            let i64_val = builder.ins().bitcast(u64_val, I64, span)?;
            state.push1(i64_val);
        }
        Operator::I32WrapI64 => {
            let val = state.pop1();
            state.push1(builder.ins().trunc(val, I32, span)?);
        }
        /****************************** Binary Operators ************************************/
        Operator::I32Add | Operator::I64Add => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-iadd

            let value_type = arg1.borrow().ty().clone();
            let arg2 = if &value_type != arg2.borrow().ty() {
                let value_type = value_type.clone();
                builder.ins().bitcast(arg2, value_type, span)?
            } else {
                arg2
            };
            state.push1(builder.ins().add_wrapping(arg1, arg2, span)?);
        }
        Operator::I32And | Operator::I64And => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().band(arg1, arg2, span)?);
        }
        Operator::I32Or | Operator::I64Or => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().bor(arg1, arg2, span)?);
        }
        Operator::I32Xor | Operator::I64Xor => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().bxor(arg1, arg2, span)?);
        }
        Operator::I32Shl => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.ins().bitcast(arg2, U32, span)?;
            state.push1(builder.ins().shl(arg1, arg2, span)?);
        }
        Operator::I64Shl => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.ins().cast(arg2, U32, span)?;
            state.push1(builder.ins().shl(arg1, arg2, span)?);
        }
        Operator::I32ShrU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let val = builder.ins().shr(arg1, arg2, span)?;
            state.push1(builder.ins().bitcast(val, I32, span)?);
        }
        Operator::I64ShrU => {
            let (arg1, arg2) = state.pop2();
            let arg1 = builder.ins().bitcast(arg1, U64, span)?;
            let arg2 = builder.ins().cast(arg2, U32, span)?;
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let val = builder.ins().shr(arg1, arg2, span)?;
            state.push1(builder.ins().bitcast(val, I64, span)?);
        }
        Operator::I32ShrS => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.ins().bitcast(arg2, Type::U32, span)?;
            state.push1(builder.ins().shr(arg1, arg2, span)?);
        }
        Operator::I64ShrS => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.ins().cast(arg2, Type::U32, span)?;
            state.push1(builder.ins().shr(arg1, arg2, span)?);
        }
        Operator::I32Rotl => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.ins().bitcast(arg2, Type::U32, span)?;
            state.push1(builder.ins().rotl(arg1, arg2, span)?);
        }
        Operator::I64Rotl => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.ins().cast(arg2, Type::U32, span)?;
            state.push1(builder.ins().rotl(arg1, arg2, span)?);
        }
        Operator::I32Rotr => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.ins().bitcast(arg2, Type::U32, span)?;
            state.push1(builder.ins().rotr(arg1, arg2, span)?);
        }
        Operator::I64Rotr => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.ins().cast(arg2, Type::U32, span)?;
            state.push1(builder.ins().rotr(arg1, arg2, span)?);
        }
        Operator::I32Sub | Operator::I64Sub => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-isub
            state.push1(builder.ins().sub_wrapping(arg1, arg2, span)?);
        }
        Operator::I32Mul | Operator::I64Mul => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-imul
            state.push1(builder.ins().mul_wrapping(arg1, arg2, span)?);
        }
        Operator::I32DivS | Operator::I64DivS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().div_unchecked(arg1, arg2, span));
        }
        Operator::I32DivU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().div_unchecked(arg1, arg2, span);
            state.push1(builder.ins().bitcast(val, I32, span)?);
        }
        Operator::I64DivU => {
            let (arg1, arg2) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().div_unchecked(arg1, arg2, span);
            state.push1(builder.ins().bitcast(val, I64, span)?);
        }
        Operator::I32RemU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().r#mod_checked(arg1, arg2, span);
            state.push1(builder.ins().bitcast(val, I32, span)?);
        }
        Operator::I64RemU => {
            let (arg1, arg2) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().r#mod_checked(arg1, arg2, span);
            state.push1(builder.ins().bitcast(val, I64, span)?);
        }
        Operator::I32RemS | Operator::I64RemS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.ins().r#mod_checked(arg1, arg2, span));
        }
        /**************************** Comparison Operators **********************************/
        Operator::I32LtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().lt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64LtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().lt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I32LtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().lt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64LtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().lt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I32LeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().lte(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64LeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().lte(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I32LeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().lte(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64LeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().lte(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I32GtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().gt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I64GtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().gt(arg0, arg1, span)?;
            state.push1(builder.ins().sext(val, I32, span)?);
        }
        Operator::I32GtS | Operator::I64GtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().gt(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I32GeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.ins().gte(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I64GeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.ins().gte(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I32GeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().gte(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I64GeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().gte(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I32Eqz => {
            let arg = state.pop1();
            let val = builder.ins().eq_imm(arg, Immediate::I32(0), span);
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I64Eqz => {
            let arg = state.pop1();
            let val = builder.ins().eq_imm(arg, Immediate::I64(0), span);
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I32Eq => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().eq(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I64Eq => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().eq(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I32Ne => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().neq(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        Operator::I64Ne => {
            let (arg0, arg1) = state.pop2();
            let val = builder.ins().neq(arg0, arg1, span)?;
            state.push1(builder.ins().zext(val, I32, span)?);
        }
        op => {
            unsupported_diag!(diagnostics, "Wasm op {:?} is not supported", op);
        }
    };
    Ok(())
}

fn translate_load(
    ptr_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<()> {
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    state.push1(builder.ins().load(addr, span));
    Ok(())
}

fn translate_load_sext(
    ptr_ty: Type,
    sext_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<()> {
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    let val = builder.ins().load(addr, span);
    let sext_val = builder.ins().sext(val, sext_ty, span)?;
    state.push1(sext_val);
    Ok(())
}

fn translate_load_zext(
    ptr_ty: Type,
    zext_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<()> {
    assert!(ptr_ty.is_unsigned_integer());
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    let val = builder.ins().load(addr, span);
    let sext_val = builder.ins().zext(val, zext_ty, span)?;
    state.push1(sext_val);
    Ok(())
}

fn translate_store(
    ptr_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<()> {
    let (addr_int, val) = state.pop2();
    let val_ty = val.borrow().ty().clone();
    let arg = if ptr_ty != val_ty {
        if ptr_ty.size_in_bits() == val_ty.size_in_bits() {
            builder.ins().bitcast(val, ptr_ty.clone(), span)?
        } else if ptr_ty.is_unsigned_integer() && val_ty.is_signed_integer() {
            let unsigned_val_ty = val_ty.as_unsigned();
            let uval = builder.ins().bitcast(val, unsigned_val_ty, span)?;
            builder.ins().trunc(uval, ptr_ty.clone(), span)?
        } else {
            builder.ins().trunc(val, ptr_ty.clone(), span)?
        }
    } else {
        val
    };
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    builder.ins().store(addr, arg, span);
    Ok(())
}

fn prepare_addr(
    addr_int: ValueRef,
    ptr_ty: &Type,
    memarg: Option<&MemArg>,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let addr_int_ty = addr_int.borrow().ty().clone();
    let addr_u32 = if addr_int_ty == U32 {
        addr_int
    } else if addr_int_ty == I32 {
        builder.ins().bitcast(addr_int, U32, span)?
    } else if matches!(addr_int_ty, Ptr(_)) {
        builder.ins().ptrtoint(addr_int, U32, span)?
    } else {
        panic!("unexpected type used as pointer value: {addr_int_ty}");
    };
    let mut full_addr_int = addr_u32;
    if let Some(memarg) = memarg {
        if memarg.offset != 0 {
            full_addr_int =
                builder
                    .ins()
                    .add_imm_checked(addr_u32, Immediate::U32(memarg.offset as u32), span);
        }
        // TODO(pauls): For now, asserting alignment helps us catch mistakes/bugs, but we should
        // probably make this something that can be disabled to avoid the overhead in release builds
        if memarg.align > 0 {
            // Generate alignment assertion - aligned addresses should always produce 0 here
            let align_offset = builder.ins().mod_imm_unchecked(
                full_addr_int,
                Immediate::U32(2u32.pow(memarg.align as u32)),
                span,
            );
            builder.ins().assertz_with_error(
                align_offset,
                midenc_hir::ASSERT_FAILED_ALIGNMENT,
                span,
            );
        }
    };
    builder.ins().inttoptr(full_addr_int, Type::Ptr(ptr_ty.clone().into()), span)
}

fn translate_call(
    func_state: &mut FuncTranslationState,
    module_state: &mut ModuleTranslationState,
    builder: &mut FunctionBuilderExt,
    function_index: FuncIndex,
    span: SourceSpan,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    todo!()
    // let func_id =
    //     module_state.get_direct_func(builder.data_flow_graph_mut(), function_index, diagnostics)?;
    // let wasm_sig = module_state.signature(function_index);
    // let num_wasm_args = wasm_sig.params().len();
    // let args = func_state.peekn(num_wasm_args);
    // if is_miden_intrinsics_module(func_id.module.as_symbol()) {
    //     let results = convert_intrinsics_call(func_id, args, builder, span);
    //     func_state.popn(num_wasm_args);
    //     func_state.pushn(&results);
    // } else if is_miden_abi_module(func_id.module.as_symbol()) {
    //     // Miden SDK function call, transform the call to the Miden ABI if needed
    //     let results = transform_miden_abi_call(func_id, args, builder, span, diagnostics);
    //     assert_eq!(
    //         wasm_sig.results().len(),
    //         results.len(),
    //         "Adapted function call results quantity are not the same as the original Wasm \
    //          function results quantity for function {}",
    //         func_id
    //     );
    //     assert_eq!(
    //         wasm_sig.results().iter().map(|p| p.ty).collect::<Vec<Type>>(),
    //         results.iter().map(|r| (*r).borrow().ty().clone()).collect::<Vec<Type>>(),
    //         "Adapted function call result types are not the same as the original Wasm function \
    //          result types for function {}",
    //         func_id
    //     );
    //     func_state.popn(num_wasm_args);
    //     func_state.pushn(&results);
    // } else {
    //     // no transformation needed
    //     let call = builder.ins().exec(func_id, args, span)?;
    //     let results = builder.inst_results(call);
    //     func_state.popn(num_wasm_args);
    //     func_state.pushn(results);
    // };
    // Ok(())
}

fn translate_return(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let return_count = {
        let frame = &mut state.control_stack[0];
        frame.num_return_values()
    };
    {
        let return_args = match return_count {
            0 => None,
            1 => Some(*state.peekn_mut(return_count).first().unwrap()),
            _ => {
                unsupported_diag!(diagnostics, "Multiple values are not supported");
            }
        };

        builder.ins().ret(return_args, span);
    }
    state.popn(return_count);
    state.reachable = false;
    Ok(())
}

fn translate_br(
    state: &mut FuncTranslationState,
    relative_depth: &u32,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) {
    let i = state.control_stack.len() - 1 - (*relative_depth as usize);
    let (return_count, br_destination) = {
        let frame = &mut state.control_stack[i];
        // We signal that all the code that follows until the next End is unreachable
        frame.set_branched_to_exit();
        let return_count = if frame.is_loop() {
            frame.num_param_values()
        } else {
            frame.num_return_values()
        };
        (return_count, frame.br_destination())
    };
    let destination_args = state.peekn_mut(return_count).to_vec();
    builder.ins().br(br_destination, destination_args, span);
    state.popn(return_count);
    state.reachable = false;
}

fn translate_br_if(
    relative_depth: u32,
    builder: &mut FunctionBuilderExt,
    state: &mut FuncTranslationState,
    span: SourceSpan,
) -> WasmResult<()> {
    let cond = state.pop1_bitcasted(Type::I32, builder, span);
    let (br_destination, inputs) = translate_br_if_args(relative_depth, state);
    let next_block = builder.create_block();
    let then_dest = br_destination;
    let then_args = inputs.to_vec();
    let else_dest = next_block;
    let else_args = vec![];
    // cond is expected to be a i32 value
    let cond_i1 = builder.ins().neq_imm(cond, 0, span)?;
    builder.ins().cond_br(cond_i1, then_dest, then_args, else_dest, else_args, span);
    builder.seal_block(next_block); // The only predecessor is the current block.
    builder.switch_to_block(next_block);
    Ok(())
}

fn translate_br_if_args(
    relative_depth: u32,
    state: &mut FuncTranslationState,
) -> (BlockRef, &mut [ValueRef]) {
    let i = state.control_stack.len() - 1 - (relative_depth as usize);
    let (return_count, br_destination) = {
        let frame = &mut state.control_stack[i];
        // The values returned by the branch are still available for the reachable
        // code that comes after it
        frame.set_branched_to_exit();
        let return_count = if frame.is_loop() {
            frame.num_param_values()
        } else {
            frame.num_return_values()
        };
        (return_count, frame.br_destination())
    };
    let inputs = state.peekn_mut(return_count);
    (br_destination, inputs)
}

fn translate_br_table(
    br_targets: &wasmparser::BrTable<'_>,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> Result<(), Report> {
    todo!()
    // let mut targets = Vec::default();
    // for depth in br_targets.targets() {
    //     let depth = depth.into_diagnostic()?;
    //
    //     targets.push(depth);
    // }
    // targets.sort();
    //
    // let default_depth = br_targets.default();
    // let min_depth =
    //     core::cmp::min(targets.iter().copied().min().unwrap_or(default_depth), default_depth);
    //
    // let argc = {
    //     let i = state.control_stack.len() - 1 - (min_depth as usize);
    //     let min_depth_frame = &state.control_stack[i];
    //     if min_depth_frame.is_loop() {
    //         min_depth_frame.num_param_values()
    //     } else {
    //         min_depth_frame.num_return_values()
    //     }
    // };
    //
    // let default_block = {
    //     let i = state.control_stack.len() - 1 - (default_depth as usize);
    //     let frame = &mut state.control_stack[i];
    //     frame.set_branched_to_exit();
    //     frame.br_destination()
    // };
    //
    // let val = state.pop1();
    // let val = if val.borrow().ty().clone() != &U32 {
    //     builder.ins().cast(val, U32, span)?
    // } else {
    //     val
    // };
    //
    // let switch_builder = builder.ins().switch(val, span);
    // let switch_builder =
    //     targets.into_iter().enumerate().fold(switch_builder, |acc, (label_idx, depth)| {
    //         let block = {
    //             let i = state.control_stack.len() - 1 - (depth as usize);
    //             let frame = &mut state.control_stack[i];
    //             frame.set_branched_to_exit();
    //             frame.br_destination()
    //         };
    //         let args = state.peekn_mut(argc);
    //         acc.case(label_idx as u32, block, args)
    //     });
    // switch_builder.or_else(default_block, state.peekn_mut(argc));
    //
    // state.popn(argc);
    // state.reachable = false;
    // Ok(())
}

fn translate_block(
    blockty: &wasmparser::BlockType,
    builder: &mut FunctionBuilderExt,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypes,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    let next = builder.create_block_with_params(blockty.results.clone(), span);
    state.push_block(next, blockty.params.len(), blockty.results.len());
    Ok(())
}

fn translate_end(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) {
    // The `End` instruction pops the last control frame from the control stack, seals
    // the destination block (since `br` instructions targeting it only appear inside the
    // block and have already been translated) and modify the value stack to use the
    // possible `Block`'s arguments values.
    let frame = state.control_stack.pop().unwrap();
    let next_block = frame.following_code();
    let return_count = frame.num_return_values();
    let return_args = state.peekn_mut(return_count);

    builder.ins().br(next_block, return_args.iter().cloned(), span);

    // You might expect that if we just finished an `if` block that
    // didn't have a corresponding `else` block, then we would clean
    // up our duplicate set of parameters that we pushed earlier
    // right here. However, we don't have to explicitly do that,
    // since we truncate the stack back to the original height
    // below.

    builder.switch_to_block(next_block);
    builder.seal_block(next_block);

    // If it is a loop we also have to seal the body loop block
    if let ControlStackFrame::Loop { header, .. } = frame {
        builder.seal_block(header)
    }

    frame.truncate_value_stack_to_original_size(&mut state.stack);
    let next_block_args: Vec<ValueRef> = next_block
        .borrow()
        .arguments()
        .iter()
        .map(|ba| ba.borrow().as_value_ref())
        .collect();
    state.stack.extend_from_slice(&next_block_args);
}

fn translate_else(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> WasmResult<()> {
    todo!()
    // let i = state.control_stack.len() - 1;
    // match state.control_stack[i] {
    //     ControlStackFrame::If {
    //         ref else_data,
    //         head_is_reachable,
    //         ref mut consequent_ends_reachable,
    //         num_return_values,
    //         ref blocktype,
    //         destination,
    //         ..
    //     } => {
    //         // We finished the consequent, so record its final
    //         // reachability state.
    //         debug_assert!(consequent_ends_reachable.is_none());
    //         *consequent_ends_reachable = Some(state.reachable);
    //
    //         if head_is_reachable {
    //             // We have a branch from the head of the `if` to the `else`.
    //             state.reachable = true;
    //
    //             // Ensure we have a block for the `else` block (it may have
    //             // already been pre-allocated, see `ElseData` for details).
    //             let else_block = match *else_data {
    //                 ElseData::NoElse {
    //                     branch_inst,
    //                     placeholder,
    //                 } => {
    //                     debug_assert_eq!(blocktype.params.len(), num_return_values);
    //                     let else_block =
    //                         builder.create_block_with_params(blocktype.params.clone(), span);
    //                     let params_len = blocktype.params.len();
    //                     builder.ins().br(destination, state.peekn(params_len), span);
    //                     state.popn(params_len);
    //
    //                     builder.change_jump_destination(branch_inst, placeholder, else_block);
    //                     builder.seal_block(else_block);
    //                     else_block
    //                 }
    //                 ElseData::WithElse { else_block } => {
    //                     builder.ins().br(destination, state.peekn(num_return_values), span);
    //                     state.popn(num_return_values);
    //                     else_block
    //                 }
    //             };
    //
    //             // You might be expecting that we push the parameters for this
    //             // `else` block here, something like this:
    //             //
    //             //     state.pushn(&control_stack_frame.params);
    //             //
    //             // We don't do that because they are already on the top of the stack
    //             // for us: we pushed the parameters twice when we saw the initial
    //             // `if` so that we wouldn't have to save the parameters in the
    //             // `ControlStackFrame` as another `Vec` allocation.
    //
    //             builder.switch_to_block(else_block);
    //
    //             // We don't bother updating the control frame's `ElseData`
    //             // to `WithElse` because nothing else will read it.
    //         }
    //     }
    //     _ => unreachable!(),
    // };
    // Ok(())
}

fn translate_if(
    blockty: &wasmparser::BlockType,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt,
    mod_types: &ModuleTypes,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    todo!()
    // let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    // let cond = state.pop1();
    // // cond is expected to be a i32 value
    // let cond_i1 = builder.ins().neq_imm(cond, Immediate::I32(0), span);
    // let next_block = builder.create_block();
    // let (destination, else_data) = if blockty.params.eq(&blockty.results) {
    //     // It is possible there is no `else` block, so we will only
    //     // allocate a block for it if/when we find the `else`. For now,
    //     // we if the condition isn't true, then we jump directly to the
    //     // destination block following the whole `if...end`. If we do end
    //     // up discovering an `else`, then we will allocate a block for it
    //     // and go back and patch the jump.
    //     let destination = builder.create_block_with_params(blockty.results.clone(), span);
    //     let branch_inst = builder.ins().cond_br(
    //         cond_i1,
    //         next_block,
    //         &[],
    //         destination,
    //         state.peekn(blockty.params.len()),
    //         span,
    //     );
    //     (
    //         destination,
    //         ElseData::NoElse {
    //             branch_inst,
    //             placeholder: destination,
    //         },
    //     )
    // } else {
    //     // The `if` type signature is not valid without an `else` block,
    //     // so we eagerly allocate the `else` block here.
    //     let destination = builder.create_block_with_params(blockty.results.clone(), span);
    //     let else_block = builder.create_block_with_params(blockty.params.clone(), span);
    //     builder.ins().cond_br(
    //         cond_i1,
    //         next_block,
    //         &[],
    //         else_block,
    //         state.peekn(blockty.params.len()),
    //         span,
    //     );
    //     builder.seal_block(else_block);
    //     (destination, ElseData::WithElse { else_block })
    // };
    // builder.seal_block(next_block);
    // builder.switch_to_block(next_block);
    // state.push_if(destination, else_data, blockty.params.len(), blockty.results.len(), blockty);
    // Ok(())
}

fn translate_loop(
    blockty: &wasmparser::BlockType,
    builder: &mut FunctionBuilderExt,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypes,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    let loop_body = builder.create_block_with_params(blockty.params.clone(), span);
    let next = builder.create_block_with_params(blockty.results.clone(), span);
    let args = state.peekn(blockty.params.len()).to_vec();
    builder.ins().br(loop_body, args, span);
    state.push_loop(loop_body, next, blockty.params.len(), blockty.results.len());
    state.popn(blockty.params.len());
    let loop_body_args: Vec<ValueRef> = loop_body
        .borrow()
        .arguments()
        .iter()
        .map(|ba| ba.borrow().as_value_ref())
        .collect();
    state.stack.extend_from_slice(&loop_body_args);
    builder.switch_to_block(loop_body);
    Ok(())
}

/// Deals with a Wasm instruction located in an unreachable portion of the code. Most of them
/// are dropped but special ones like `End` or `Else` signal the potential end of the unreachable
/// portion so the translation state must be updated accordingly.
fn translate_unreachable_operator(
    op: &Operator,
    builder: &mut FunctionBuilderExt,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypes,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    debug_assert!(!state.reachable);
    match *op {
        Operator::If { blockty } => {
            // Push a placeholder control stack entry. The if isn't reachable,
            // so we don't have any branches anywhere.
            let blockty = BlockType::from_wasm(&blockty, mod_types, diagnostics)?;
            let detached_block = builder.create_detached_block();
            state.push_if(
                detached_block,
                ElseData::NoElse {
                    branch_inst: builder
                        .ins()
                        .unreachable(span)
                        .unwrap()
                        .borrow()
                        .as_ref()
                        .as_operation_ref(),
                    placeholder: detached_block,
                },
                0,
                0,
                blockty,
            );
        }
        Operator::Loop { blockty: _ } | Operator::Block { blockty: _ } => {
            state.push_block(builder.create_detached_block(), 0, 0);
        }
        Operator::Else => {
            let i = state.control_stack.len() - 1;
            match state.control_stack[i] {
                ControlStackFrame::If {
                    ref else_data,
                    head_is_reachable,
                    ref mut consequent_ends_reachable,
                    ref blocktype,
                    ..
                } => {
                    debug_assert!(consequent_ends_reachable.is_none());
                    *consequent_ends_reachable = Some(state.reachable);

                    if head_is_reachable {
                        // We have a branch from the head of the `if` to the `else`.
                        state.reachable = true;

                        let else_block = match *else_data {
                            ElseData::NoElse {
                                branch_inst,
                                placeholder,
                            } => {
                                let else_block = builder
                                    .create_block_with_params(blocktype.params.clone(), span);
                                let frame = state.control_stack.last().unwrap();
                                frame.truncate_value_stack_to_else_params(&mut state.stack);

                                // We change the target of the branch instruction.
                                builder.change_jump_destination(
                                    branch_inst,
                                    placeholder,
                                    else_block,
                                );
                                builder.seal_block(else_block);
                                else_block
                            }
                            ElseData::WithElse { else_block } => {
                                let frame = state.control_stack.last().unwrap();
                                frame.truncate_value_stack_to_else_params(&mut state.stack);
                                else_block
                            }
                        };

                        builder.switch_to_block(else_block);

                        // Again, no need to push the parameters for the `else`,
                        // since we already did when we saw the original `if`. See
                        // the comment for translating `Operator::Else` in
                        // `translate_operator` for details.
                    }
                }
                _ => unreachable!(),
            }
        }
        Operator::End => {
            let stack = &mut state.stack;
            let control_stack = &mut state.control_stack;
            let frame = control_stack.pop().unwrap();

            // Pop unused parameters from stack.
            frame.truncate_value_stack_to_original_size(stack);

            let reachable_anyway = match frame {
                // If it is a loop we also have to seal the body loop block
                ControlStackFrame::Loop { header, .. } => {
                    builder.seal_block(header);
                    // And loops can't have branches to the end.
                    false
                }
                // If we never set `consequent_ends_reachable` then that means
                // we are finishing the consequent now, and there was no
                // `else`. Whether the following block is reachable depends only
                // on if the head was reachable.
                ControlStackFrame::If {
                    head_is_reachable,
                    consequent_ends_reachable: None,
                    ..
                } => head_is_reachable,
                // Since we are only in this function when in unreachable code,
                // we know that the alternative just ended unreachable. Whether
                // the following block is reachable depends on if the consequent
                // ended reachable or not.
                ControlStackFrame::If {
                    head_is_reachable,
                    consequent_ends_reachable: Some(consequent_ends_reachable),
                    ..
                } => head_is_reachable && consequent_ends_reachable,
                // All other control constructs are already handled.
                _ => false,
            };

            if frame.exit_is_branched_to() || reachable_anyway {
                builder.switch_to_block(frame.following_code());
                builder.seal_block(frame.following_code());

                // And add the return values of the block but only if the next block is reachable
                // (which corresponds to testing if the stack depth is 1)
                let next_block_args: Vec<ValueRef> = frame
                    .following_code()
                    .borrow()
                    .arguments()
                    .iter()
                    .map(|ba| ba.borrow().as_value_ref())
                    .collect();
                stack.extend_from_slice(&next_block_args);
                state.reachable = true;
            }
        }
        _ => {
            // We don't translate because this is unreachable code
        }
    }

    Ok(())
}
