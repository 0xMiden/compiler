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

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::{ControlFlowOpBuilder, SwitchCase};
use midenc_dialect_hir::{assertions, HirOpBuilder};
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    dialects::builtin::BuiltinOpBuilder,
    BlockRef, Builder, Felt, FieldElement, Immediate, PointerType,
    Type::{self, *},
    ValueRef,
};
use midenc_session::diagnostics::{DiagnosticsHandler, IntoDiagnostic, Report, SourceSpan};
use wasmparser::{MemArg, Operator};

use crate::{
    callable::CallableFunction,
    error::WasmResult,
    intrinsics::convert_intrinsics_call,
    module::{
        func_translation_state::{ControlStackFrame, ElseData, FuncTranslationState},
        function_builder_ext::FunctionBuilderExt,
        module_translation_state::ModuleTranslationState,
        types::{BlockType, FuncIndex, GlobalIndex, ModuleTypesBuilder},
        Module,
    },
    ssa::Variable,
    unsupported_diag,
};

#[cfg(test)]
mod tests;

/// Translates wasm operators into Miden IR instructions.
#[allow(clippy::too_many_arguments)]
pub fn translate_operator<B: ?Sized + Builder>(
    op: &Operator,
    builder: &mut FunctionBuilderExt<'_, B>,
    state: &mut FuncTranslationState,
    module_state: &mut ModuleTranslationState,
    module: &Module,
    mod_types: &ModuleTypesBuilder,
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
            let val = if expected_ty != value_ty {
                if expected_ty == I32 && value_ty == U32 {
                    builder.bitcast(val, I32, span)?
                } else if expected_ty == I64 && value_ty == U64 {
                    builder.bitcast(val, I64, span)?
                } else {
                    let expected_ty = expected_ty.clone();
                    builder.cast(val, expected_ty, span)?
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
            let gv = module_state.module_builder.get_global_var(name).unwrap_or_else(|| {
                panic!("global var not found: index={}, name={}", global_index.as_u32(), name)
            });
            let val = builder.load_global(gv, span)?;
            state.push1(val);
        }
        Operator::GlobalSet { global_index } => {
            let global_index = GlobalIndex::from_u32(*global_index);
            let name = module.global_name(global_index);
            let gv = module_state.module_builder.get_global_var(name).unwrap_or_else(|| {
                panic!("global var not found: index={}, name={}", global_index.as_u32(), name)
            });
            let arg = state.pop1();
            builder.store_global(gv, arg, span)?;
        }
        /********************************* Stack misc **************************************/
        Operator::Drop => _ = state.pop1(),
        Operator::Select => {
            let (arg1, arg2, cond) = state.pop3();
            // if cond is not 0, return arg1, else return arg2
            // https://www.w3.org/TR/wasm-core-1/#-hrefsyntax-instr-parametricmathsfselect%E2%91%A0
            // cond is expected to be an i32
            let imm = builder.imm(Immediate::I32(0), span);
            let cond_i1 = builder.neq(cond, imm, span)?;
            state.push1(builder.select(cond_i1, arg1, arg2, span)?);
        }
        Operator::TypedSelect { ty } => {
            let (arg1, arg2, cond) = state.pop3();
            match ty {
                wasmparser::ValType::F32 => {
                    let imm = builder.felt(Felt::ZERO, span);
                    let cond = builder.gt(cond, imm, span)?;
                    state.push1(builder.select(cond, arg1, arg2, span)?);
                }
                wasmparser::ValType::I32 => {
                    let imm = builder.imm(Immediate::I32(0), span);
                    let cond = builder.neq(cond, imm, span)?;
                    state.push1(builder.select(cond, arg1, arg2, span)?);
                }
                wasmparser::ValType::I64 => {
                    let imm = builder.imm(Immediate::I64(0), span);
                    let cond = builder.neq(cond, imm, span)?;
                    state.push1(builder.select(cond, arg1, arg2, span)?);
                }
                ty => panic!("unsupported value type for 'select': {ty}"),
            };
        }
        Operator::Unreachable => {
            builder.unreachable(span);
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
        Operator::End => translate_end(state, builder, span)?,

        /**************************** Branch instructions *********************************/
        Operator::Br { relative_depth } => translate_br(state, relative_depth, builder, span)?,
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
            todo!("CallIndirect is not supported yet");
        }
        /******************************* Memory management *********************************/
        Operator::MemoryGrow { .. } => {
            let arg = state.pop1_bitcasted(U32, builder, span);
            let result = builder.mem_grow(arg, span)?;
            // WASM memory.grow returns i32, so bitcast from U32 to I32
            state.push1(builder.bitcast(result, I32, span)?);
        }
        Operator::MemorySize { .. } => {
            // Return total Miden memory size
            let result = builder.mem_size(span)?;
            // WASM memory.size returns i32, so bitcast from U32 to I32
            state.push1(builder.bitcast(result, I32, span)?);
        }
        /******************************* Bulk memory operations *********************************/
        Operator::MemoryCopy { dst_mem, src_mem } => {
            // See semantics at https://github.com/WebAssembly/bulk-memory-operations/blob/master/proposals/bulk-memory-operations/Overview.md#memorycopy-instruction
            if *src_mem == 0 && src_mem == dst_mem {
                let count_i32 = state.pop1();
                let src_i32 = state.pop1();
                let dst_i32 = state.pop1();
                let count = builder.bitcast(count_i32, Type::U32, span)?;
                let dst = prepare_addr(dst_i32, &U8, None, builder, span)?;
                let src = prepare_addr(src_i32, &U8, None, builder, span)?;
                builder.memcpy(src, dst, count, span)?;
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
            let value = builder.trunc(value, Type::U8, span)?;
            let num_bytes = builder.bitcast(num_bytes, Type::U32, span)?;
            let dst = prepare_addr(dst_i32, &U8, None, builder, span)?;
            builder.memset(dst, num_bytes, value, span)?;
        }
        /******************************* Load instructions ***********************************/
        Operator::I32Load8U { memarg } => {
            translate_load_zext(U8, U32, memarg, state, builder, span)?;
        }
        Operator::I32Load16U { memarg } => {
            translate_load_zext(U16, U32, memarg, state, builder, span)?;
        }
        Operator::I32Load8S { memarg } => {
            translate_load_sext(I8, I32, memarg, state, builder, span)?;
        }
        Operator::I32Load16S { memarg } => {
            translate_load_sext(I16, I32, memarg, state, builder, span)?;
        }
        Operator::I64Load8U { memarg } => {
            translate_load_zext(U8, U64, memarg, state, builder, span)?;
        }
        Operator::I64Load16U { memarg } => {
            translate_load_zext(U16, U64, memarg, state, builder, span)?;
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
            translate_load_zext(U32, U64, memarg, state, builder, span)?;
        }
        Operator::I32Load { memarg } => translate_load(I32, memarg, state, builder, span)?,
        Operator::I64Load { memarg } => translate_load(I64, memarg, state, builder, span)?,
        Operator::F32Load { memarg } => translate_load(Felt, memarg, state, builder, span)?,
        /****************************** Store instructions ***********************************/
        Operator::I32Store { memarg } => translate_store(I32, memarg, state, builder, span)?,
        Operator::I64Store { memarg } => translate_store(I64, memarg, state, builder, span)?,
        Operator::F32Store { memarg } => translate_store(Felt, memarg, state, builder, span)?,
        Operator::I32Store8 { memarg } | Operator::I64Store8 { memarg } => {
            translate_store(U8, memarg, state, builder, span)?;
        }
        Operator::I32Store16 { memarg } | Operator::I64Store16 { memarg } => {
            translate_store(U16, memarg, state, builder, span)?;
        }
        Operator::I64Store32 { memarg } => translate_store(U32, memarg, state, builder, span)?,
        /****************************** Nullary Operators **********************************/
        Operator::I32Const { value } => state.push1(builder.i32(*value, span)),
        Operator::I64Const { value } => state.push1(builder.i64(*value, span)),

        /******************************* Unary Operators *************************************/
        Operator::I32Clz | Operator::I64Clz => {
            let val = state.pop1();
            let count = builder.clz(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of clz as an i32
            state.push1(builder.bitcast(count, Type::I32, span)?);
        }
        Operator::I32Ctz | Operator::I64Ctz => {
            let val = state.pop1();
            let count = builder.ctz(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of ctz as an i32
            state.push1(builder.bitcast(count, Type::I32, span)?);
        }
        Operator::I32Popcnt | Operator::I64Popcnt => {
            let val = state.pop1();
            let count = builder.popcnt(val, span)?;
            // To ensure we match the Wasm semantics, treat the output of popcnt as an i32
            state.push1(builder.bitcast(count, Type::I32, span)?);
        }
        Operator::I32Extend8S | Operator::I32Extend16S => {
            let val = state.pop1();
            state.push1(builder.sext(val, I32, span)?);
        }
        Operator::I64ExtendI32S => {
            let val = state.pop1();
            state.push1(builder.sext(val, I64, span)?);
        }
        Operator::I64ExtendI32U => {
            let val = state.pop1();
            let u32_val = builder.bitcast(val, U32, span)?;
            let u64_val = builder.zext(u32_val, U64, span)?;
            let i64_val = builder.bitcast(u64_val, I64, span)?;
            state.push1(i64_val);
        }
        Operator::I32WrapI64 => {
            let val = state.pop1();
            state.push1(builder.trunc(val, I32, span)?);
        }
        Operator::F32ReinterpretI32 => {
            let val = state.pop1_bitcasted(Felt, builder, span);
            state.push1(val);
        }
        /****************************** Binary Operators ************************************/
        Operator::I32Add | Operator::I64Add => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-iadd

            let value_type = arg1.borrow().ty().clone();
            let arg2 = if &value_type != arg2.borrow().ty() {
                let value_type = value_type.clone();
                builder.bitcast(arg2, value_type, span)?
            } else {
                arg2
            };
            state.push1(builder.add_wrapping(arg1, arg2, span)?);
        }
        Operator::I64Add128 => {
            let (rhs_hi, rhs_lo) = state.pop2();
            let (lhs_hi, lhs_lo) = state.pop2();

            let lhs = builder.join(lhs_hi, lhs_lo, span)?;
            let rhs = builder.join(rhs_hi, rhs_lo, span)?;

            let res = builder.add_wrapping(lhs, rhs, span)?;

            // Ensure the high limb is left on the top of the value stack.
            let (res_hi, res_lo) = builder.split(res, span)?;
            state.pushn(&[res_lo, res_hi]);
        }
        Operator::I32And | Operator::I64And => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.band(arg1, arg2, span)?);
        }
        Operator::I32Or | Operator::I64Or => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.bor(arg1, arg2, span)?);
        }
        Operator::I32Xor | Operator::I64Xor => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.bxor(arg1, arg2, span)?);
        }
        Operator::I32Shl => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.bitcast(arg2, U32, span)?;
            state.push1(builder.shl(arg1, arg2, span)?);
        }
        Operator::I64Shl => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.cast(arg2, U32, span)?;
            state.push1(builder.shl(arg1, arg2, span)?);
        }
        Operator::I32ShrU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let val = builder.shr(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I32, span)?);
        }
        Operator::I64ShrU => {
            let (arg1, arg2) = state.pop2();
            let arg1 = builder.bitcast(arg1, U64, span)?;
            let arg2 = builder.cast(arg2, U32, span)?;
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let val = builder.shr(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I64, span)?);
        }
        Operator::I32ShrS => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.bitcast(arg2, Type::U32, span)?;
            state.push1(builder.shr(arg1, arg2, span)?);
        }
        Operator::I64ShrS => {
            let (arg1, arg2) = state.pop2();
            // wrapping shift semantics drop any bits that would cause
            // the shift to exceed the bitwidth of the type
            let arg2 = builder.cast(arg2, Type::U32, span)?;
            state.push1(builder.shr(arg1, arg2, span)?);
        }
        Operator::I32Rotl => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.bitcast(arg2, Type::U32, span)?;
            state.push1(builder.rotl(arg1, arg2, span)?);
        }
        Operator::I64Rotl => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.cast(arg2, Type::U32, span)?;
            state.push1(builder.rotl(arg1, arg2, span)?);
        }
        Operator::I32Rotr => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.bitcast(arg2, Type::U32, span)?;
            state.push1(builder.rotr(arg1, arg2, span)?);
        }
        Operator::I64Rotr => {
            let (arg1, arg2) = state.pop2();
            let arg2 = builder.cast(arg2, Type::U32, span)?;
            state.push1(builder.rotr(arg1, arg2, span)?);
        }
        Operator::I32Sub | Operator::I64Sub => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-isub
            state.push1(builder.sub_wrapping(arg1, arg2, span)?);
        }
        Operator::I64Sub128 => {
            let (rhs_hi, rhs_lo) = state.pop2();
            let (lhs_hi, lhs_lo) = state.pop2();

            let lhs = builder.join(lhs_hi, lhs_lo, span)?;
            let rhs = builder.join(rhs_hi, rhs_lo, span)?;

            let res = builder.sub_wrapping(lhs, rhs, span)?;

            // Ensure the high limb is left on the top of the value stack.
            let (res_hi, res_lo) = builder.split(res, span)?;
            state.pushn(&[res_lo, res_hi]);
        }
        Operator::I32Mul | Operator::I64Mul => {
            let (arg1, arg2) = state.pop2();
            // wrapping because the result is mod 2^N
            // https://www.w3.org/TR/wasm-core-1/#op-imul
            state.push1(builder.mul_wrapping(arg1, arg2, span)?);
        }
        Operator::I64MulWideU => {
            let (arg1, arg2) = state.pop2();

            let lhs_u = builder.bitcast(arg1, Type::U64, span)?;
            let lhs = builder.zext(lhs_u, Type::U128, span)?;

            let rhs_u = builder.bitcast(arg2, Type::U64, span)?;
            let rhs = builder.zext(rhs_u, Type::U128, span)?;

            let res = builder.mul_wrapping(lhs, rhs, span)?;

            // Ensure the high limb is left on the top of the value stack.
            let (res_hi, res_lo) = builder.split(res, span)?;
            state.pushn(&[res_lo, res_hi]);
        }
        Operator::I64MulWideS => {
            let (arg1, arg2) = state.pop2();

            let lhs = builder.sext(arg1, Type::I128, span)?;
            let rhs = builder.sext(arg2, Type::I128, span)?;

            let res = builder.mul_wrapping(lhs, rhs, span)?;

            // Ensure the high limb is left on the top of the value stack.
            let (res_hi, res_lo) = builder.split(res, span)?;
            state.pushn(&[res_lo, res_hi]);
        }
        Operator::I32DivS | Operator::I64DivS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.div(arg1, arg2, span)?);
        }
        Operator::I32DivU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.div(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I32, span)?);
        }
        Operator::I64DivU => {
            let (arg1, arg2) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.div(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I64, span)?);
        }
        Operator::I32RemU => {
            let (arg1, arg2) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.r#mod(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I32, span)?);
        }
        Operator::I64RemU => {
            let (arg1, arg2) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.r#mod(arg1, arg2, span)?;
            state.push1(builder.bitcast(val, I64, span)?);
        }
        Operator::I32RemS | Operator::I64RemS => {
            let (arg1, arg2) = state.pop2();
            state.push1(builder.r#mod(arg1, arg2, span)?);
        }
        /**************************** Comparison Operators **********************************/
        Operator::I32LtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.lt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64LtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.lt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32LtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.lt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64LtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.lt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32LeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.lte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64LeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.lte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32LeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.lte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64LeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.lte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32GtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.gt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64GtU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.gt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32GtS | Operator::I64GtS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.gt(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32GeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U32, builder, span)?;
            let val = builder.gte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64GeU => {
            let (arg0, arg1) = state.pop2_bitcasted(U64, builder, span)?;
            let val = builder.gte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32GeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.gte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64GeS => {
            let (arg0, arg1) = state.pop2();
            let val = builder.gte(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32Eqz => {
            let arg = state.pop1();
            let imm = builder.imm(Immediate::I32(0), span);
            let val = builder.eq(arg, imm, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64Eqz => {
            let arg = state.pop1();
            let imm = builder.imm(Immediate::I64(0), span);
            let val = builder.eq(arg, imm, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32Eq => {
            let (arg0, arg1) = state.pop2();
            let val = builder.eq(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64Eq => {
            let (arg0, arg1) = state.pop2();
            let val = builder.eq(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I32Ne => {
            let (arg0, arg1) = state.pop2();
            let val = builder.neq(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        Operator::I64Ne => {
            let (arg0, arg1) = state.pop2();
            let val = builder.neq(arg0, arg1, span)?;
            let extended = builder.zext(val, U32, span)?;
            state.push1(builder.bitcast(extended, I32, span)?);
        }
        op => {
            unsupported_diag!(diagnostics, "Wasm op {:?} is not supported", op);
        }
    };
    Ok(())
}

fn translate_load<B: ?Sized + Builder>(
    ptr_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    state.push1(builder.load(addr, span)?);
    Ok(())
}

fn translate_load_sext<B: ?Sized + Builder>(
    ptr_ty: Type,
    sext_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    let val = builder.load(addr, span)?;
    let sext_val = builder.sext(val, sext_ty, span)?;
    state.push1(sext_val);
    Ok(())
}

fn translate_load_zext<B: ?Sized + Builder>(
    ptr_ty: Type,
    zext_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    assert!(ptr_ty.is_unsigned_integer());
    let addr_int = state.pop1();
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    let val = builder.load(addr, span)?;
    let zext_val = builder.zext(val, zext_ty, span)?;
    state.push1(zext_val);
    Ok(())
}

fn translate_store<B: ?Sized + Builder>(
    ptr_ty: Type,
    memarg: &MemArg,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    let (addr_int, val) = state.pop2();
    let val_ty = val.borrow().ty().clone();
    let arg = if ptr_ty != val_ty {
        if ptr_ty.size_in_bits() == val_ty.size_in_bits() {
            builder.bitcast(val, ptr_ty.clone(), span)?
        } else if ptr_ty.is_unsigned_integer() && val_ty.is_signed_integer() {
            let unsigned_val_ty = val_ty.as_unsigned();
            let uval = builder.bitcast(val, unsigned_val_ty, span)?;
            builder.trunc(uval, ptr_ty.clone(), span)?
        } else {
            builder.trunc(val, ptr_ty.clone(), span)?
        }
    } else {
        val
    };
    let addr = prepare_addr(addr_int, &ptr_ty, Some(memarg), builder, span)?;
    builder.store(addr, arg, span)?;
    Ok(())
}

fn prepare_addr<B: ?Sized + Builder>(
    addr_int: ValueRef,
    ptr_ty: &Type,
    memarg: Option<&MemArg>,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let addr_int_ty = addr_int.borrow().ty().clone();
    let addr_u32 = if addr_int_ty == U32 {
        addr_int
    } else if addr_int_ty == I32 {
        builder.bitcast(addr_int, U32, span)?
    } else if matches!(addr_int_ty, Ptr(_)) {
        builder.ptrtoint(addr_int, U32, span)?
    } else {
        panic!("unexpected type used as pointer value: {addr_int_ty}");
    };
    let mut full_addr_int = addr_u32;
    if let Some(memarg) = memarg {
        if memarg.offset != 0 {
            let imm = builder.imm(Immediate::U32(memarg.offset as u32), span);
            full_addr_int = builder.add(addr_u32, imm, span)?;
        }
        // TODO(pauls): For now, asserting alignment helps us catch mistakes/bugs, but we should
        // probably make this something that can be disabled to avoid the overhead in release builds
        if memarg.align > 0 {
            // Generate alignment assertion - aligned addresses should always produce 0 here
            let imm = builder.imm(Immediate::U32(2u32.pow(memarg.align as u32)), span);
            let align_offset = builder.r#mod(full_addr_int, imm, span)?;
            builder.assertz_with_error(align_offset, assertions::ASSERT_FAILED_ALIGNMENT, span)?;
        }
    };
    builder.inttoptr(full_addr_int, Type::from(PointerType::new(ptr_ty.clone())), span)
}

fn translate_call<B: ?Sized + Builder>(
    func_state: &mut FuncTranslationState,
    module_state: &mut ModuleTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    function_index: FuncIndex,
    span: SourceSpan,
    _diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    match module_state.get_direct_func(function_index)? {
        CallableFunction::Instruction {
            intrinsic,
            signature,
        } => {
            let arity = signature.arity();
            let args = func_state.peekn(arity);
            let results = convert_intrinsics_call(intrinsic, None, args, builder, span)?;
            func_state.popn(arity);
            func_state.pushn(&results);
        }
        CallableFunction::Intrinsic {
            intrinsic,
            function_ref,
            signature,
        } => {
            let arity = signature.arity();
            let args = func_state.peekn(arity);
            let results =
                convert_intrinsics_call(intrinsic, Some(function_ref), args, builder, span)?;
            func_state.popn(arity);
            func_state.pushn(&results);
        }
        CallableFunction::Function {
            function_ref,
            signature,
            ..
        } => {
            let arity = signature.arity();
            let args = func_state.peekn(arity);
            let exec = builder.exec(function_ref, signature, args.iter().copied(), span)?;
            let borrow = exec.borrow();
            let results = borrow.as_ref().results();
            func_state.popn(arity);
            let result_vals: Vec<ValueRef> =
                results.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
            func_state.pushn(&result_vals);
        }
    }
    Ok(())
}

fn translate_return<B: ?Sized + Builder>(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
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

        builder.ret(return_args, span)?;
    }
    state.popn(return_count);
    state.reachable = false;
    Ok(())
}

fn translate_br<B: ?Sized + Builder>(
    state: &mut FuncTranslationState,
    relative_depth: &u32,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
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
    builder.br(br_destination, destination_args, span)?;
    state.popn(return_count);
    state.reachable = false;
    Ok(())
}

fn translate_br_if<B: ?Sized + Builder>(
    relative_depth: u32,
    builder: &mut FunctionBuilderExt<'_, B>,
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
    let imm = builder.imm(Immediate::I32(0), span);
    let cond_i1 = builder.neq(cond, imm, span)?;
    builder.cond_br(cond_i1, then_dest, then_args, else_dest, else_args, span)?;
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

fn translate_br_table<B: ?Sized + Builder>(
    br_targets: &wasmparser::BrTable<'_>,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> Result<(), Report> {
    let mut targets = Vec::default();
    for depth in br_targets.targets() {
        let depth = depth.into_diagnostic()?;

        targets.push(depth);
    }
    targets.sort();

    let default_depth = br_targets.default();
    let min_depth =
        core::cmp::min(targets.iter().copied().min().unwrap_or(default_depth), default_depth);

    let argc = {
        let i = state.control_stack.len() - 1 - (min_depth as usize);
        let min_depth_frame = &state.control_stack[i];
        if min_depth_frame.is_loop() {
            min_depth_frame.num_param_values()
        } else {
            min_depth_frame.num_return_values()
        }
    };

    let default_block = {
        let i = state.control_stack.len() - 1 - (default_depth as usize);
        let frame = &mut state.control_stack[i];
        frame.set_branched_to_exit();
        frame.br_destination()
    };

    let selector = state.pop1();
    let selector = if selector.borrow().ty().clone() != U32 {
        builder.cast(selector, U32, span)?
    } else {
        selector
    };

    let mut cases = Vec::new();
    for (label_idx, depth) in targets.into_iter().enumerate() {
        let block = {
            let i = state.control_stack.len() - 1 - (depth as usize);
            let frame = &mut state.control_stack[i];
            frame.set_branched_to_exit();
            frame.br_destination()
        };
        let args = state.peekn_mut(argc).to_vec();
        let case = SwitchCase {
            value: label_idx as u32,
            successor: block,
            arguments: args,
        };
        cases.push(case);
    }

    let default_args = state.peekn_mut(argc).to_vec();
    state.popn(argc);
    builder.switch(selector, cases, default_block, default_args, span)?;
    state.reachable = false;
    Ok(())
}

fn translate_block<B: ?Sized + Builder>(
    blockty: &wasmparser::BlockType,
    builder: &mut FunctionBuilderExt<'_, B>,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypesBuilder,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    let next = builder.create_block_with_params(blockty.results.clone(), span);
    state.push_block(next, blockty.params.len(), blockty.results.len());
    Ok(())
}

fn translate_end<B: ?Sized + Builder>(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    // The `End` instruction pops the last control frame from the control stack, seals
    // the destination block (since `br` instructions targeting it only appear inside the
    // block and have already been translated) and modify the value stack to use the
    // possible `Block`'s arguments values.
    let frame = state.control_stack.pop().unwrap();
    let next_block = frame.following_code();
    let return_count = frame.num_return_values();
    let return_args = state.peekn_mut(return_count);

    builder.br(next_block, return_args.iter().cloned(), span)?;

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
    Ok(())
}

fn translate_else<B: ?Sized + Builder>(
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<()> {
    let i = state.control_stack.len() - 1;
    match state.control_stack[i] {
        ControlStackFrame::If {
            ref else_data,
            head_is_reachable,
            ref mut consequent_ends_reachable,
            num_return_values,
            ref blocktype,
            destination,
            ..
        } => {
            // We finished the consequent, so record its final
            // reachability state.
            debug_assert!(consequent_ends_reachable.is_none());
            *consequent_ends_reachable = Some(state.reachable);

            if head_is_reachable {
                // We have a branch from the head of the `if` to the `else`.
                state.reachable = true;

                // Ensure we have a block for the `else` block (it may have
                // already been pre-allocated, see `ElseData` for details).
                let else_block = match *else_data {
                    ElseData::NoElse {
                        branch_inst,
                        placeholder,
                    } => {
                        debug_assert_eq!(blocktype.params.len(), num_return_values);
                        let else_block =
                            builder.create_block_with_params(blocktype.params.clone(), span);
                        let params_len = blocktype.params.len();
                        builder.br(destination, state.peekn(params_len).iter().copied(), span)?;
                        state.popn(params_len);

                        builder.change_jump_destination(branch_inst, placeholder, else_block);
                        builder.seal_block(else_block);
                        else_block
                    }
                    ElseData::WithElse { else_block } => {
                        builder.br(
                            destination,
                            state.peekn(num_return_values).iter().copied(),
                            span,
                        )?;
                        state.popn(num_return_values);
                        else_block
                    }
                };

                // You might be expecting that we push the parameters for this
                // `else` block here, something like this:
                //
                //     state.pushn(&control_stack_frame.params);
                //
                // We don't do that because they are already on the top of the stack
                // for us: we pushed the parameters twice when we saw the initial
                // `if` so that we wouldn't have to save the parameters in the
                // `ControlStackFrame` as another `Vec` allocation.

                builder.switch_to_block(else_block);

                // We don't bother updating the control frame's `ElseData`
                // to `WithElse` because nothing else will read it.
            }
        }
        _ => unreachable!(),
    };
    Ok(())
}

fn translate_if<B: ?Sized + Builder>(
    blockty: &wasmparser::BlockType,
    state: &mut FuncTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    mod_types: &ModuleTypesBuilder,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    let cond = state.pop1();
    // cond is expected to be a i32 value
    let imm = builder.imm(Immediate::I32(0), span);
    let cond_i1 = builder.neq(cond, imm, span)?;
    let next_block = builder.create_block();
    let (destination, else_data) = if blockty.params.eq(&blockty.results) {
        // It is possible there is no `else` block, so we will only
        // allocate a block for it if/when we find the `else`. For now,
        // we if the condition isn't true, then we jump directly to the
        // destination block following the whole `if...end`. If we do end
        // up discovering an `else`, then we will allocate a block for it
        // and go back and patch the jump.
        let destination = builder.create_block_with_params(blockty.results.clone(), span);
        let branch_inst = builder
            .cond_br(
                cond_i1,
                next_block,
                [],
                destination,
                state.peekn(blockty.params.len()).iter().copied(),
                span,
            )?
            .as_operation_ref();
        (
            destination,
            ElseData::NoElse {
                branch_inst,
                placeholder: destination,
            },
        )
    } else {
        // The `if` type signature is not valid without an `else` block,
        // so we eagerly allocate the `else` block here.
        let destination = builder.create_block_with_params(blockty.results.clone(), span);
        let else_block = builder.create_block_with_params(blockty.params.clone(), span);
        builder.cond_br(
            cond_i1,
            next_block,
            [],
            else_block,
            state.peekn(blockty.params.len()).iter().copied(),
            span,
        )?;
        builder.seal_block(else_block);
        (destination, ElseData::WithElse { else_block })
    };
    builder.seal_block(next_block);
    builder.switch_to_block(next_block);
    state.push_if(destination, else_data, blockty.params.len(), blockty.results.len(), blockty);
    Ok(())
}

fn translate_loop<B: ?Sized + Builder>(
    blockty: &wasmparser::BlockType,
    builder: &mut FunctionBuilderExt<'_, B>,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypesBuilder,
    diagnostics: &DiagnosticsHandler,
    span: SourceSpan,
) -> WasmResult<()> {
    let blockty = BlockType::from_wasm(blockty, mod_types, diagnostics)?;
    let loop_body = builder.create_block_with_params(blockty.params.clone(), span);
    let next = builder.create_block_with_params(blockty.results.clone(), span);
    let args = state.peekn(blockty.params.len()).to_vec();
    builder.br(loop_body, args, span)?;
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
fn translate_unreachable_operator<B: ?Sized + Builder>(
    op: &Operator,
    builder: &mut FunctionBuilderExt<'_, B>,
    state: &mut FuncTranslationState,
    mod_types: &ModuleTypesBuilder,
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
                    branch_inst: builder.unreachable(span).as_operation_ref(),
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
