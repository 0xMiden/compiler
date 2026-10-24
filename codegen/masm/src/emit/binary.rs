use miden_core::Felt;
use midenc_hir::{Immediate, Overflow, SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    pub fn eq(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected eq operands to be the same type");
        match &ty {
            Type::I128 | Type::U128 => {
                self.eq_i128(span);
            }
            Type::I64 | Type::U64 => {
                self.eq_int64(span);
            }
            Type::Felt
            | Type::Ptr(_)
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::I8
            | Type::U8
            | Type::I1 => {
                self.emit(masm::Instruction::Eq, span);
            }
            ty => unimplemented!("eq is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn eq_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected eq operands to be the same type");
        match &ty {
            Type::I128 | Type::U128 => {
                self.push_immediate(imm, span);
                self.eq_i128(span);
            }
            Type::I64 | Type::U64 => {
                self.push_immediate(imm, span);
                self.eq_int64(span);
            }
            Type::Felt | Type::Ptr(_) | Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::EqImm(imm.as_felt().unwrap().into()), span);
            }
            Type::I32 | Type::I16 | Type::I8 => {
                self.emit(
                    masm::Instruction::EqImm(
                        Felt::new_unchecked(imm.as_i32().unwrap() as u32 as u64).into(),
                    ),
                    span,
                );
            }
            ty => unimplemented!("eq is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn neq(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected neq operands to be the same type");
        match &ty {
            Type::I128 | Type::U128 => {
                self.neq_i128(span);
            }
            Type::I64 | Type::U64 => self.neq_int64(span),
            Type::Felt
            | Type::Ptr(_)
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::I8
            | Type::U8
            | Type::I1 => {
                self.emit(masm::Instruction::Neq, span);
            }
            ty => unimplemented!("neq is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn neq_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected neq operands to be the same type");
        match &ty {
            Type::I128 | Type::U128 => {
                self.push_immediate(imm, span);
                self.neq_i128(span);
            }
            Type::I64 | Type::U64 => {
                self.push_immediate(imm, span);
                self.neq_int64(span)
            }
            Type::Felt | Type::Ptr(_) | Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::NeqImm(imm.as_felt().unwrap().into()), span);
            }
            Type::I32 | Type::I16 | Type::I8 => {
                self.emit(
                    masm::Instruction::NeqImm(
                        Felt::new_unchecked(imm.as_i32().unwrap() as u32 as u64).into(),
                    ),
                    span,
                );
            }
            ty => unimplemented!("neq is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn gt(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected gt operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Gt, span);
            }
            Type::U64 => {
                self.gt_u64(span);
            }
            Type::I64 => {
                self.gt_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::U32Gt, span);
            }
            Type::I32 => self.raw_exec("::intrinsics::i32::is_gt", span),
            ty => unimplemented!("gt is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn gt_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected gt operands to be the same type");
        match &ty {
            Type::Felt => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::Gt, span);
            }
            Type::U64 => {
                self.push_u64(imm.as_u64().unwrap(), span);
                self.gt_u64(span);
            }
            Type::I64 => {
                self.push_i64(imm.as_i64().unwrap(), span);
                self.gt_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::U32Gt, span);
            }
            Type::I32 => {
                self.push_immediate(imm, span);
                self.raw_exec("::intrinsics::i32::is_gt", span);
            }
            ty => unimplemented!("gt is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn gte(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected gte operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Gte, span);
            }
            Type::U64 => {
                self.gte_u64(span);
            }
            Type::I64 => {
                self.gte_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::U32Gte, span);
            }
            Type::I32 => self.raw_exec("::intrinsics::i32::is_gte", span),
            ty => unimplemented!("gte is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn gte_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected gte operands to be the same type");
        match &ty {
            Type::Felt => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::Gte, span);
            }
            Type::U64 => {
                self.push_u64(imm.as_u64().unwrap(), span);
                self.gte_u64(span);
            }
            Type::I64 => {
                self.push_i64(imm.as_i64().unwrap(), span);
                self.gte_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::U32Gte, span);
            }
            Type::I32 => {
                self.push_immediate(imm, span);
                self.raw_exec("::intrinsics::i32::is_gte", span);
            }
            ty => unimplemented!("gte is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn lt(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected lt operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Lt, span);
            }
            Type::U64 => {
                self.lt_u64(span);
            }
            Type::I64 => {
                self.lt_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::U32Lt, span);
            }
            Type::I32 => self.raw_exec("::intrinsics::i32::is_lt", span),
            ty => unimplemented!("lt is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn lt_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected lt operands to be the same type");
        match &ty {
            Type::Felt => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::Lt, span);
            }
            Type::U64 => {
                self.push_u64(imm.as_u64().unwrap(), span);
                self.lt_u64(span);
            }
            Type::I64 => {
                self.push_i64(imm.as_i64().unwrap(), span);
                self.lt_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::U32Lt, span);
            }
            Type::I32 => {
                self.push_immediate(imm, span);
                self.raw_exec("::intrinsics::i32::is_lt", span);
            }
            ty => unimplemented!("lt is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn lte(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected lte operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Lte, span);
            }
            Type::U64 => {
                self.lte_u64(span);
            }
            Type::I64 => {
                self.lte_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.emit(masm::Instruction::U32Lte, span);
            }
            Type::I32 => self.raw_exec("::intrinsics::i32::is_lte", span),
            ty => unimplemented!("lte is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    #[allow(unused)]
    pub fn lte_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected lte operands to be the same type");
        match &ty {
            Type::Felt => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::Lte, span);
            }
            Type::U64 => {
                self.push_u64(imm.as_u64().unwrap(), span);
                self.lte_u64(span);
            }
            Type::I64 => {
                self.push_i64(imm.as_i64().unwrap(), span);
                self.lte_i64(span);
            }
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::U32Lte, span);
            }
            Type::I32 => {
                self.push_immediate(imm, span);
                self.raw_exec("::intrinsics::i32::is_lte", span);
            }
            ty => unimplemented!("lte is not yet implemented for {ty}"),
        }
        self.push(Type::I1);
    }

    pub fn add(&mut self, overflow: Overflow, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected add operands to be the same type");
        match &ty {
            Type::Felt => self.add_felt(overflow, span),
            Type::I128 => self.add_i128(overflow, span),
            Type::U128 => self.add_u128(overflow, span),
            Type::U64 => {
                self.add_u64(overflow, span);
            }
            Type::I64 => {
                self.add_i64(overflow, span);
            }
            Type::U32 => {
                self.add_u32(overflow, span);
            }
            Type::I32 => {
                self.add_i32(overflow, span);
            }
            ty @ (Type::U16 | Type::U8 | Type::I1) => {
                self.add_uint(ty.size_in_bits() as u32, overflow, span);
            }
            ty => unimplemented!("add is not yet implemented for {ty}"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    #[allow(unused)]
    pub fn add_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected add operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit_push(imm.as_felt().unwrap(), span);
                self.add_felt(overflow, span);
            }
            Type::U64 => {
                self.push_immediate(imm, span);
                self.add_u64(overflow, span);
            }
            Type::I64 => {
                self.add_imm_i64(imm.as_i64().unwrap(), overflow, span);
            }
            Type::U32 => {
                self.add_imm_u32(imm.as_u32().unwrap(), overflow, span);
            }
            Type::I32 => {
                self.add_imm_i32(imm.as_i32().unwrap(), overflow, span);
            }
            ty @ (Type::U16 | Type::U8 | Type::I1) => {
                self.add_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, overflow, span);
            }
            ty => unimplemented!("add is not yet implemented for {ty}"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    pub fn sub(&mut self, overflow: Overflow, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected sub operands to be the same type");
        match &ty {
            Type::Felt => self.sub_felt(overflow, span),
            Type::I128 => self.sub_i128(overflow, span),
            Type::U128 => self.sub_u128(overflow, span),
            Type::U64 => {
                self.sub_u64(overflow, span);
            }
            Type::I64 => {
                self.sub_i64(overflow, span);
            }
            Type::U32 => {
                self.sub_u32(overflow, span);
            }
            Type::I32 => {
                self.sub_i32(overflow, span);
            }
            ty @ (Type::U16 | Type::U8 | Type::I1) => {
                self.sub_uint(ty.size_in_bits() as u32, overflow, span);
            }
            ty => unimplemented!("sub is not yet implemented for {ty}"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    #[allow(unused)]
    pub fn sub_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected sub operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit_push(imm.as_felt().unwrap(), span);
                self.sub_felt(overflow, span);
            }
            Type::U64 => {
                self.push_immediate(imm, span);
                self.sub_u64(overflow, span);
            }
            Type::I64 => {
                self.sub_imm_i64(imm.as_i64().unwrap(), overflow, span);
            }
            Type::U32 => {
                self.sub_imm_u32(imm.as_u32().unwrap(), overflow, span);
            }
            Type::I32 => {
                self.sub_imm_i32(imm.as_i32().unwrap(), overflow, span);
            }
            ty @ (Type::U16 | Type::U8 | Type::I1) => {
                self.sub_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, overflow, span);
            }
            ty => unimplemented!("sub is not yet implemented for {ty}"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    pub fn mul(&mut self, overflow: Overflow, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected mul operands to be the same type");
        match &ty {
            Type::Felt => self.mul_felt(overflow, span),
            Type::I128 => self.mul_i128(overflow, span),
            Type::U128 => self.mul_u128(overflow, span),
            Type::U64 => self.mul_u64(overflow, span),
            Type::I64 => self.mul_i64(overflow, span),
            Type::U32 => self.mul_u32(overflow, span),
            Type::I32 => self.mul_i32(overflow, span),
            ty @ (Type::U16 | Type::U8) => {
                self.mul_uint(ty.size_in_bits() as u32, overflow, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mul expects integer operands, got {ty}")
            }
            ty => unimplemented!("mul for {ty} is not supported"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    #[allow(unused)]
    pub fn mul_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected mul operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit_push(imm.as_felt().unwrap(), span);
                self.mul_felt(overflow, span);
            }
            Type::U64 => {
                self.push_immediate(imm, span);
                self.mul_u64(overflow, span);
            }
            Type::I64 => self.mul_imm_i64(imm.as_i64().unwrap(), overflow, span),
            Type::U32 => self.mul_imm_u32(imm.as_u32().unwrap(), overflow, span),
            Type::I32 => self.mul_imm_i32(imm.as_i32().unwrap(), overflow, span),
            ty @ (Type::U16 | Type::U8) => {
                self.mul_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, overflow, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mul expects integer operands, got {ty}")
            }
            ty => unimplemented!("mul for {ty} is not supported"),
        }
        self.push(ty);
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
    }

    pub fn checked_div(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected div operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Div, span);
            }
            Type::U64 => self.checked_div_u64(span),
            Type::I64 => self.checked_div_i64(span),
            Type::U32 => self.checked_div_u32(span),
            Type::I32 => self.checked_div_i32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_div_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: div expects integer operands, got {ty}")
            }
            ty => unimplemented!("div for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn checked_div_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected div operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Div, span);
            }
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.checked_div_u64(span);
            }
            Type::I64 => self.checked_div_imm_i64(imm.as_i64().unwrap(), span),
            Type::U32 => self.checked_div_imm_u32(imm.as_u32().unwrap(), span),
            Type::I32 => self.checked_div_imm_i32(imm.as_i32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_div_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: div expects integer operands, got {ty}")
            }
            ty => unimplemented!("div for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_div(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected div operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Div, span);
            }
            Type::U64 => self.unchecked_div_u64(span),
            Type::I64 => self.checked_div_i64(span),
            Type::U32 | Type::U16 | Type::U8 => self.unchecked_div_u32(span),
            Type::I32 => self.checked_div_i32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: div expects integer operands, got {ty}")
            }
            ty => unimplemented!("div for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_div_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected div operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Div, span);
            }
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.unchecked_div_u64(span);
            }
            Type::I64 => self.checked_div_imm_i64(imm.as_i64().unwrap(), span),
            Type::U32 => self.unchecked_div_imm_u32(imm.as_u32().unwrap(), span),
            Type::I32 => self.checked_div_imm_i32(imm.as_i32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.unchecked_div_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: div expects integer operands, got {ty}")
            }
            ty => unimplemented!("div for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn checked_mod(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected mod operands to be the same type");
        match &ty {
            Type::U64 => self.checked_mod_u64(span),
            Type::I32 => self.checked_mod_i32(span),
            Type::U32 => self.checked_mod_u32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_mod_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("checked_mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn checked_mod_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected mod operands to be the same type");
        match &ty {
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.checked_mod_u64(span);
            }
            Type::I32 => self.checked_mod_imm_i32(imm.as_i32().unwrap(), span),
            Type::U32 => self.checked_mod_imm_u32(imm.as_u32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_mod_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn wrapping_mod(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected wrapping_mod operands to be the same type");
        match &ty {
            Type::I32 => self.wrapping_mod_i32(span),
            Type::I64 => self.wrapping_mod_i64(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: wrapping_mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("wrapping_mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_mod(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected mod operands to be the same type");
        match &ty {
            Type::U64 => self.unchecked_mod_u64(span),
            Type::U32 => self.unchecked_mod_u32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.unchecked_mod_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_mod_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected mod operands to be the same type");
        match &ty {
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.unchecked_mod_u64(span);
            }
            Type::U32 => self.unchecked_mod_imm_u32(imm.as_u32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.unchecked_mod_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn checked_divmod(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected divmod operands to be the same type");
        match &ty {
            Type::U64 => self.checked_divmod_u64(span),
            Type::U32 => self.checked_divmod_u32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_divmod_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: divmod expects integer operands, got {ty}")
            }
            ty => unimplemented!("divmod for {ty} is not supported"),
        }
        self.push(ty.clone());
        self.push(ty);
    }

    #[allow(unused)]
    pub fn checked_divmod_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected divmod operands to be the same type");
        match &ty {
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.checked_divmod_u64(span);
            }
            Type::U32 => self.checked_divmod_imm_u32(imm.as_u32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_divmod_imm_uint(imm.as_u32().unwrap(), ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: divmod expects integer operands, got {ty}")
            }
            ty => unimplemented!("divmod for {ty} is not supported"),
        }
        self.push(ty.clone());
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_divmod(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected divmod operands to be the same type");
        match &ty {
            Type::U64 => self.unchecked_divmod_u64(span),
            Type::U32 => self.unchecked_divmod_u32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.unchecked_divmod_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: divmod expects integer operands, got {ty}")
            }
            ty => unimplemented!("divmod for {ty} is not supported"),
        }
        self.push(ty.clone());
        self.push(ty);
    }

    #[allow(unused)]
    pub fn unchecked_divmod_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected divmod operands to be the same type");
        match &ty {
            Type::U64 => {
                assert_ne!(imm.as_u64().unwrap(), 0, "invalid division by zero");
                self.push_immediate(imm, span);
                self.unchecked_divmod_u64(span);
            }
            Type::U32 => self.unchecked_divmod_imm_u32(imm.as_u32().unwrap(), span),
            ty @ (Type::U16 | Type::U8) => {
                self.unchecked_divmod_imm_uint(
                    imm.as_u32().unwrap(),
                    ty.size_in_bits() as u32,
                    span,
                );
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: divmod expects integer operands, got {ty}")
            }
            ty => unimplemented!("divmod for {ty} is not supported"),
        }
        self.push(ty.clone());
        self.push(ty);
    }

    pub fn exp(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected exp operands to be the same type");
        match &ty {
            Type::U64 => todo!("exponentiation by squaring"),
            Type::Felt => {
                self.emit(masm::Instruction::Exp, span);
            }
            Type::U32 => {
                self.emit_all([masm::Instruction::Exp, masm::Instruction::U32Assert], span);
            }
            Type::I32 => {
                self.raw_exec("::intrinsics::i32::ipow", span);
            }
            ty @ (Type::U16 | Type::U8) => {
                self.emit_all([masm::Instruction::Exp, masm::Instruction::U32Assert], span);
                self.int32_to_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: exp expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn exp_u32_exponent(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, Type::Felt, "expected exp.u32 base to be felt");
        assert_eq!(rhs.ty(), Type::Felt, "expected exp.u32 exponent to be felt");
        self.emit(masm::Instruction::ExpBitLength(32), span);
        self.push(ty);
    }

    #[allow(unused)]
    pub fn exp_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected exp operands to be the same type");
        let exp: u8 =
            imm.as_u64().unwrap().try_into().expect("invalid exponent: must be value < 64");
        match &ty {
            Type::U64 => todo!("exponentiation by squaring"),
            Type::Felt => {
                self.emit(masm::Instruction::ExpImm(Felt::new_unchecked(exp as u64).into()), span);
            }
            Type::U32 => {
                self.emit_all(
                    [
                        masm::Instruction::ExpImm(Felt::new_unchecked(exp as u64).into()),
                        masm::Instruction::U32Assert,
                    ],
                    span,
                );
            }
            Type::I32 => {
                self.emit_push(exp, span);
                self.raw_exec("::intrinsics::i32::ipow", span);
            }
            ty @ (Type::U16 | Type::U8) => {
                self.emit_all(
                    [
                        masm::Instruction::ExpImm(Felt::new_unchecked(exp as u64).into()),
                        masm::Instruction::U32Assert,
                    ],
                    span,
                );
                self.int32_to_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: exp expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn and(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected and operands to be the same type");
        assert_eq!(ty, Type::I1, "expected and operands to be of boolean type");
        self.emit(masm::Instruction::And, span);
        self.push(ty);
    }

    #[allow(unused)]
    pub fn and_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected and operands to be the same type");
        assert_eq!(ty, Type::I1, "expected and operands to be of boolean type");
        self.push_immediate(imm, span);
        self.emit(masm::Instruction::And, span);
        self.push(ty);
    }

    pub fn or(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected or operands to be the same type");
        assert_eq!(ty, Type::I1, "expected or operands to be of boolean type");
        self.emit(masm::Instruction::Or, span);
        self.push(ty);
    }

    #[allow(unused)]
    pub fn or_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected or operands to be the same type");
        assert_eq!(ty, Type::I1, "expected or operands to be of boolean type");
        self.push_immediate(imm, span);
        self.emit(masm::Instruction::Or, span);
        self.push(ty);
    }

    pub fn xor(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected xor operands to be the same type");
        assert_eq!(ty, Type::I1, "expected xor operands to be of boolean type");
        self.emit(masm::Instruction::Xor, span);
        self.push(ty);
    }

    #[allow(unused)]
    pub fn xor_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected xor operands to be the same type");
        assert_eq!(ty, Type::I1, "expected xor operands to be of boolean type");
        self.push_immediate(imm, span);
        self.emit(masm::Instruction::Xor, span);
        self.push(ty);
    }

    pub fn band(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected band operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                // AND the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.band_int64(span); // [c0, c1, b2, b3, a2, a3]
                // AND the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.band_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => self.band_int64(span),
            Type::U32 | Type::I32 | Type::U16 | Type::I16 | Type::U8 | Type::I8 => {
                self.band_u32(span)
            }
            Type::I1 => self.emit(masm::Instruction::And, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: band expects integer operands, got {ty}")
            }
            ty => unimplemented!("band for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn band_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected band operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                self.push_immediate(imm, span);
                // AND the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.band_int64(span); // [c0, c1, b2, b3, a2, a3]
                // AND the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.band_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.push_immediate(imm, span);
                self.band_int64(span);
            }
            Type::U32 | Type::U16 | Type::U8 => self.band_imm_u32(imm.as_u32().unwrap(), span),
            Type::I32 | Type::I16 | Type::I8 => {
                self.band_imm_u32(imm.as_i64().unwrap() as u64 as u32, span)
            }
            Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::And, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: band expects integer operands, got {ty}")
            }
            ty => unimplemented!("band for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn bor(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected bor operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                // OR the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bor_int64(span); // [c0, c1, b2, b3, a2, a3]
                // OR the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bor_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => self.bor_int64(span),
            Type::U32 | Type::I32 | Type::U16 | Type::I16 | Type::U8 | Type::I8 => {
                self.bor_u32(span)
            }
            Type::I1 => self.emit(masm::Instruction::Or, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: bor expects integer operands, got {ty}")
            }
            ty => unimplemented!("bor for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn bor_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected bor operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                self.push_immediate(imm, span);
                // OR the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bor_int64(span); // [c0, c1, b2, b3, a2, a3]
                // OR the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bor_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.push_immediate(imm, span);
                self.bor_int64(span);
            }
            Type::U32 | Type::U16 | Type::U8 => self.bor_imm_u32(imm.as_u32().unwrap(), span),
            Type::I32 | Type::I16 | Type::I8 => {
                self.bor_imm_u32(imm.as_i64().unwrap() as u64 as u32, span)
            }
            Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::And, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: bor expects integer operands, got {ty}")
            }
            ty => unimplemented!("bor for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn bxor(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected bxor operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                // XOR the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [c0, c1, b2, b3, a2, a3]
                // XOR the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => self.bxor_int64(span),
            Type::U32 | Type::I32 => self.bxor_u32(span),
            ty @ (Type::U16 | Type::I16 | Type::U8 | Type::I8) => {
                self.bxor_u32(span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            Type::I1 => self.emit(masm::Instruction::Xor, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: bxor expects integer operands, got {ty}")
            }
            ty => unimplemented!("bxor for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn bxor_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected bxor operands to be the same type");
        match &ty {
            Type::U128 | Type::I128 => {
                self.push_immediate(imm, span);
                // XOR the low halves
                //
                // [b0, b1, b2, b3, a0, a1, a2, a3]
                self.emit_all(
                    [
                        // [a0, a1, b0, b1, b2, b3, a2, a3]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [c0, c1, b2, b3, a2, a3]
                // XOR the high halves
                self.emit_all(
                    [
                        // [b2, b3, a2, a3, c0, c1]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [c2, c3, c0, c1]
                self.emit_all(
                    [
                        // [c0, c1, c2, c3]
                        masm::Instruction::MovUp3,
                        masm::Instruction::MovUp3,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.push_immediate(imm, span);
                self.bxor_int64(span);
            }
            Type::U32 => self.bxor_imm_u32(imm.as_u32().unwrap(), span),
            Type::I32 => self.bxor_imm_u32(imm.as_i64().unwrap() as u64 as u32, span),
            ty @ (Type::U16 | Type::U8) => {
                self.bxor_imm_u32(imm.as_u32().unwrap(), span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            ty @ (Type::I16 | Type::I8) => {
                self.bxor_imm_u32(imm.as_i64().unwrap() as u64 as u32, span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            Type::I1 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::Xor, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: bxor expects integer operands, got {ty}")
            }
            ty => unimplemented!("bxor for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn shl(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(rhs.ty(), Type::U32, "expected shift operand to be u32");
        match &ty {
            Type::U64 | Type::I64 => self.shl_u64(span),
            Type::U32 | Type::I32 => self.shl_u32(span),
            ty @ (Type::U16 | Type::I16 | Type::U8 | Type::I8) => {
                self.shl_u32(span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: shl expects integer operands, got {ty}")
            }
            ty => unimplemented!("shl for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn shl_imm(&mut self, imm: u32, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        match &ty {
            Type::U64 | Type::I64 => {
                assert!(imm < 64, "invalid shift value: must be < 64");
                self.push_u32(imm, span);
                self.shl_u64(span);
            }
            Type::U32 => self.shl_imm_u32(imm, span),
            Type::I32 => self.shl_imm_u32(imm, span),
            ty @ (Type::U16 | Type::I16 | Type::U8 | Type::I8) => {
                self.shl_imm_u32(imm, span);
                self.trunc_int32(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: shl expects integer operands, got {ty}")
            }
            ty => unimplemented!("shl for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn shr(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(rhs.ty(), Type::U32, "expected shift operand to be u32");
        match &ty {
            Type::U64 => self.shr_u64(span),
            Type::I64 => self.shr_i64(span),
            Type::U32 | Type::U16 | Type::U8 => self.shr_u32(span),
            Type::I32 => self.shr_i32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: shr expects integer operands, got {ty}")
            }
            ty => unimplemented!("shr for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn shr_imm(&mut self, imm: u32, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        match &ty {
            Type::U64 => {
                assert!(imm < 64, "invalid shift value: must be < 64, got {imm}");
                self.push_u32(imm, span);
                self.shr_u64(span);
            }
            Type::I64 => self.shr_imm_i64(imm, span),
            Type::U32 | Type::U16 | Type::U8 => self.shr_imm_u32(imm, span),
            Type::I32 => self.shr_imm_i32(imm, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: shr expects integer operands, got {ty}")
            }
            ty => unimplemented!("shr for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn rotl(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(rhs.ty(), Type::U32, "expected shift operand to be u32");
        match &ty {
            Type::U64 | Type::I64 => self.rotl_u64(span),
            Type::U32 | Type::I32 => self.rotl_u32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: rotl expects integer operands, got {ty}")
            }
            ty => unimplemented!("rotl for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn rotl_imm(&mut self, imm: u32, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        match &ty {
            Type::U64 | Type::I64 => {
                self.push_u32(imm, span);
                self.rotl_u64(span);
            }
            Type::U32 | Type::I32 => self.rotl_imm_u32(imm, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: rotl expects integer operands, got {ty}")
            }
            ty => unimplemented!("rotl for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn rotr(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(rhs.ty(), Type::U32, "expected shift operand to be u32");
        match &ty {
            Type::U64 | Type::I64 => self.rotr_u64(span),
            Type::U32 | Type::I32 => self.rotr_u32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: rotr expects integer operands, got {ty}")
            }
            ty => unimplemented!("rotr for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn rotr_imm(&mut self, imm: u32, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        match &ty {
            Type::U64 | Type::I64 => {
                self.push_u32(imm, span);
                self.rotr_u64(span);
            }
            Type::U32 | Type::I32 => self.rotr_imm_u32(imm, span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: rotr expects integer operands, got {ty}")
            }
            ty => unimplemented!("rotr for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn min(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected min operands to be the same type");
        match &ty {
            Type::U64 => self.min_u64(span),
            Type::I64 => self.min_i64(span),
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => self.min_u32(span),
            Type::I32 => self.min_i32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: min expects integer operands, got {ty}")
            }
            ty => unimplemented!("min for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn min_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected min operands to be the same type");
        match &ty {
            Type::U64 => {
                self.push_immediate(imm, span);
                self.min_u64(span);
            }
            Type::I64 => self.min_imm_i64(imm.as_i64().unwrap(), span),
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.min_imm_u32(imm.as_u32().unwrap(), span)
            }
            Type::I32 => self.min_imm_i32(imm.as_i32().unwrap(), span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: min expects integer operands, got {ty}")
            }
            ty => unimplemented!("min for {ty} is not supported"),
        }
        self.push(ty);
    }

    pub fn max(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected max operands to be the same type");
        match &ty {
            Type::U64 => self.max_u64(span),
            Type::I64 => self.max_i64(span),
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => self.max_u32(span),
            Type::I32 => self.max_i32(span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: max expects integer operands, got {ty}")
            }
            ty => unimplemented!("max for {ty} is not supported"),
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn max_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected max operands to be the same type");
        match &ty {
            Type::U64 => {
                self.push_immediate(imm, span);
                self.max_u64(span);
            }
            Type::I64 => self.max_imm_i64(imm.as_i64().unwrap(), span),
            Type::U32 | Type::U16 | Type::U8 | Type::I1 => {
                self.max_imm_u32(imm.as_u32().unwrap(), span)
            }
            Type::I32 => self.max_imm_i32(imm.as_i32().unwrap(), span),
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: max expects integer operands, got {ty}")
            }
            ty => unimplemented!("max for {ty} is not supported"),
        }
        self.push(ty);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use miden_processor::{DefaultHost, FastProcessor, StackInputs};
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{Builder, Context, OpBuilder, Report, ValueRef, formatter::PrettyPrint};

    use super::*;
    use crate::{OperandStack, emit::OpEmitter};

    fn assert_overflowing_result_order(
        build: impl Fn(&mut OpBuilder, ValueRef, ValueRef, SourceSpan) -> (ValueRef, ValueRef),
        emit: impl Fn(&mut OpEmitter<'_>, SourceSpan),
        emit_imm: impl Fn(&mut OpEmitter<'_>, Immediate, SourceSpan),
    ) {
        for (ty, imm) in [
            (Type::U32, Immediate::U32(2)),
            (Type::I32, Immediate::I32(2)),
            (Type::U64, Immediate::U64(2)),
            (Type::I64, Immediate::I64(2)),
        ] {
            for immediate in [false, true] {
                let context = Rc::new(Context::default());
                let block = context.create_block_with_params([ty.clone(), ty.clone()]);
                let [lhs, rhs] = [0, 1].map(|index| block.borrow().arguments()[index] as ValueRef);
                let mut builder = OpBuilder::new(context.clone());
                builder.set_insertion_point_to_end(block);
                let span = SourceSpan::UNKNOWN;
                let _ = build(&mut builder, lhs, rhs, span);

                let mut stack = OperandStack::new(context);
                if !immediate {
                    stack.push(rhs);
                }
                stack.push(lhs);
                let mut invoked = BTreeSet::new();
                let mut instructions = Vec::new();
                {
                    let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
                    if immediate {
                        emit_imm(&mut emitter, imm, span);
                    } else {
                        emit(&mut emitter, span);
                    }
                    // Raw instruction algorithms preserve the declared result lane order.
                    assert_eq!(emitter.stack()[0].ty(), Type::I1);
                    assert_eq!(emitter.stack()[1].ty(), ty);
                }

                assert_eq!(stack.len(), 2, "{ty}, immediate = {immediate}");
                assert_eq!(stack[0].ty(), Type::I1);
                assert_eq!(stack[1].ty(), ty);
            }
        }
    }

    #[test]
    fn overflowing_add_results_follow_vm_stack_order() {
        assert_overflowing_result_order(
            |builder, lhs, rhs, span| builder.add_overflowing(lhs, rhs, span).unwrap(),
            |emitter, span| emitter.add(Overflow::Overflowing, span),
            |emitter, imm, span| emitter.add_imm(imm, Overflow::Overflowing, span),
        );
    }

    #[test]
    fn overflowing_sub_results_follow_vm_stack_order() {
        assert_overflowing_result_order(
            |builder, lhs, rhs, span| builder.sub_overflowing(lhs, rhs, span).unwrap(),
            |emitter, span| emitter.sub(Overflow::Overflowing, span),
            |emitter, imm, span| emitter.sub_imm(imm, Overflow::Overflowing, span),
        );
    }

    #[test]
    fn overflowing_mul_results_follow_vm_stack_order() {
        assert_overflowing_result_order(
            |builder, lhs, rhs, span| builder.mul_overflowing(lhs, rhs, span).unwrap(),
            |emitter, span| emitter.mul(Overflow::Overflowing, span),
            |emitter, imm, span| emitter.mul_imm(imm, Overflow::Overflowing, span),
        );
    }

    fn assert_immediate_shortcut_in_vm(
        emit: impl Fn(&mut OpEmitter<'_>, Immediate, Overflow, SourceSpan),
        immediate: u32,
        zero_result: bool,
    ) -> Result<(), Report> {
        // These are the input's VM limbs, top first, independent of the operand model.
        let cases: &[(Immediate, &[u64])] = &[
            (Immediate::U8(0), &[0]),
            (Immediate::U8(u8::MAX), &[255]),
            (Immediate::U16(u16::MAX), &[65_535]),
            (Immediate::U32(0), &[0]),
            (Immediate::U32(u32::MAX), &[4_294_967_295]),
            (Immediate::I32(i32::MIN), &[2_147_483_648]),
            (Immediate::I32(-1), &[4_294_967_295]),
            (Immediate::I32(i32::MAX), &[2_147_483_647]),
            (Immediate::I64(0), &[0, 0]),
            (Immediate::I64(i64::MIN), &[0, 2_147_483_648]),
            (Immediate::I64(-1), &[4_294_967_295, 4_294_967_295]),
            (Immediate::I64(i64::MAX), &[4_294_967_295, 2_147_483_647]),
            (Immediate::I64(0x1234_5678_9abc_def0), &[2_596_069_104, 305_419_896]),
        ];
        for &(input, limbs) in cases {
            let imm = match input {
                Immediate::U8(_) => Immediate::U8(immediate as u8),
                Immediate::U16(_) => Immediate::U16(immediate as u16),
                Immediate::U32(_) => Immediate::U32(immediate),
                Immediate::I32(_) => Immediate::I32(immediate as i32),
                Immediate::I64(_) => Immediate::I64(immediate as i64),
                _ => unreachable!(),
            };
            for overflow in [
                Overflow::Unchecked,
                Overflow::Checked,
                Overflow::Wrapping,
                Overflow::Overflowing,
            ] {
                let context = Rc::new(Context::default());
                let mut stack = OperandStack::new(context);
                let mut invoked = BTreeSet::new();
                let mut instructions = Vec::new();
                let span = SourceSpan::UNKNOWN;
                {
                    let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
                    // A sentinel catches a missing flag even when all result limbs are zero.
                    emitter.literal(17u32, span);
                    emitter.literal(input, span);
                    emit(&mut emitter, imm, overflow, span);
                }
                let body = masm::Block::new(span, instructions).to_pretty_string();
                // Remove padding beneath the at-most-four outputs to clear the VM overflow stack.
                let package = miden_assembly::Assembler::default().assemble_program(
                    "immediate_shortcut",
                    format!("begin{body}\nrepeat.4 movup.15 drop end\nend"),
                )?;
                let program = package.try_into_program()?;
                let output = FastProcessor::new(StackInputs::default())
                    .execute_sync(&program, &mut DefaultHost::default())
                    .map_err(|error| Report::msg(error.to_string()))?;

                let mut expected = Vec::new();
                if overflow.is_overflowing() {
                    expected.push(0);
                }
                expected.extend(limbs.iter().map(|&limb| if zero_result { 0 } else { limb }));
                expected.push(17);
                for (index, expected) in expected.into_iter().enumerate() {
                    assert_eq!(
                        output.stack.get_element(index),
                        Some(Felt::new_unchecked(expected)),
                        "{input:?}, immediate = {immediate}, overflow = {overflow:?}, index = \
                         {index}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn immediate_add_zero_preserves_results_in_vm() -> Result<(), Report> {
        assert_immediate_shortcut_in_vm(
            |emitter, imm, overflow, span| emitter.add_imm(imm, overflow, span),
            0,
            false,
        )
    }

    #[test]
    fn immediate_sub_zero_preserves_results_in_vm() -> Result<(), Report> {
        assert_immediate_shortcut_in_vm(
            |emitter, imm, overflow, span| emitter.sub_imm(imm, overflow, span),
            0,
            false,
        )
    }

    #[test]
    fn immediate_mul_zero_preserves_results_in_vm() -> Result<(), Report> {
        assert_immediate_shortcut_in_vm(
            |emitter, imm, overflow, span| emitter.mul_imm(imm, overflow, span),
            0,
            true,
        )
    }

    #[test]
    fn immediate_mul_one_preserves_results_in_vm() -> Result<(), Report> {
        assert_immediate_shortcut_in_vm(
            |emitter, imm, overflow, span| emitter.mul_imm(imm, overflow, span),
            1,
            false,
        )
    }

    // Hand-checked (lhs, rhs, wrapped result, overflow flag) cases for each width.
    const U8_SUB_CASES: &[(u32, u32, u32, u32)] = &[
        (2, 1, 1, 0),
        (0, 1, 255, 1),
        (1, 2, 255, 1),
        (0, 255, 1, 1),
        (255, 1, 254, 0),
        (255, 0, 255, 0),
        (255, 255, 0, 0),
        (0, 0, 0, 0),
    ];
    const U16_SUB_CASES: &[(u32, u32, u32, u32)] = &[
        (2, 1, 1, 0),
        (0, 1, 65_535, 1),
        (1, 2, 65_535, 1),
        (0, 65_535, 1, 1),
        (65_535, 1, 65_534, 0),
        (65_535, 0, 65_535, 0),
        (65_535, 65_535, 0, 0),
        (0, 0, 0, 0),
    ];

    fn assert_narrow_overflowing_sub_in_vm(
        cases: &[(u32, u32, u32, u32)],
        into_immediate: impl Fn(u32) -> Immediate,
        immediate: bool,
    ) -> Result<(), Report> {
        for &(lhs, rhs, result, flag) in cases {
            let context = Rc::new(Context::default());
            let mut stack = OperandStack::new(context);
            let mut invoked = BTreeSet::new();
            let mut instructions = Vec::new();
            let span = SourceSpan::UNKNOWN;
            {
                let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
                // Check that exactly the flag and result are produced above the existing stack.
                emitter.literal(17u32, span);
                emitter.literal(into_immediate(lhs), span);
                if immediate {
                    emitter.sub_imm(into_immediate(rhs), Overflow::Overflowing, span);
                } else {
                    emitter.literal(into_immediate(rhs), span);
                    emitter.sub(Overflow::Overflowing, span);
                }
                assert_eq!(emitter.stack().len(), 3);
                assert_eq!(emitter.stack()[0].ty(), Type::I1);
                assert_eq!(emitter.stack()[1].ty(), into_immediate(lhs).ty());
            }
            let body = masm::Block::new(span, instructions).to_pretty_string();
            // Clear padding beneath the outputs so the VM's overflow stack is empty.
            let package = miden_assembly::Assembler::default().assemble_program(
                "narrow_overflowing_sub",
                format!("begin{body}\nrepeat.4 movup.15 drop end\nend"),
            )?;
            let program = package.try_into_program()?;
            let output = FastProcessor::new(StackInputs::default())
                .execute_sync(&program, &mut DefaultHost::default())
                .map_err(|error| Report::msg(error.to_string()))?;
            for (index, expected) in [flag, result, 17].into_iter().enumerate() {
                assert_eq!(
                    output.stack.get_element(index),
                    Some(Felt::new_unchecked(expected as u64)),
                    "{:?} - {rhs}, immediate = {immediate}, index = {index}",
                    into_immediate(lhs),
                );
            }
        }
        Ok(())
    }

    #[test]
    fn overflowing_sub_u8_preserves_results_in_vm() -> Result<(), Report> {
        assert_narrow_overflowing_sub_in_vm(U8_SUB_CASES, |n| Immediate::U8(n as u8), false)
    }

    #[test]
    fn overflowing_sub_imm_u8_preserves_results_in_vm() -> Result<(), Report> {
        assert_narrow_overflowing_sub_in_vm(U8_SUB_CASES, |n| Immediate::U8(n as u8), true)
    }

    #[test]
    fn overflowing_sub_u16_preserves_results_in_vm() -> Result<(), Report> {
        assert_narrow_overflowing_sub_in_vm(U16_SUB_CASES, |n| Immediate::U16(n as u16), false)
    }

    #[test]
    fn overflowing_sub_imm_u16_preserves_results_in_vm() -> Result<(), Report> {
        assert_narrow_overflowing_sub_in_vm(U16_SUB_CASES, |n| Immediate::U16(n as u16), true)
    }

    fn assert_overflowing_mul_u32_in_vm(immediate: bool) -> Result<(), Report> {
        // Hand-checked (lhs, rhs, wrapped result, overflow flag) cases. A high product limb
        // greater than one must still produce a boolean flag, independent of the low limb.
        for (lhs, rhs, result, flag) in [
            (65_536u32, 65_536u32, 0u32, 1u32),
            (3, 5, 15, 0),
            (65_535, 65_535, 4_294_836_225, 0),
            (u32::MAX, 2, 4_294_967_294, 1),
            (u32::MAX, u32::MAX, 1, 1),
            (0, u32::MAX, 0, 0),
            (u32::MAX, 0, 0, 0),
            (u32::MAX, 1, u32::MAX, 0),
            (1, u32::MAX, u32::MAX, 0),
            (0, 0, 0, 0),
        ] {
            let mut stack = OperandStack::new(Rc::new(Context::default()));
            let mut invoked = BTreeSet::new();
            let mut instructions = Vec::new();
            let span = SourceSpan::UNKNOWN;
            {
                let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
                emitter.literal(17u32, span);
                emitter.literal(lhs, span);
                if immediate {
                    emitter.mul_imm(Immediate::U32(rhs), Overflow::Overflowing, span);
                } else {
                    emitter.literal(rhs, span);
                    emitter.mul(Overflow::Overflowing, span);
                }
                assert_eq!(emitter.stack().len(), 3);
                assert_eq!(emitter.stack()[0].ty(), Type::I1);
                assert_eq!(emitter.stack()[1].ty(), Type::U32);
            }
            let body = masm::Block::new(span, instructions).to_pretty_string();
            // Clear padding beneath the outputs so the VM's overflow stack is empty.
            let package = miden_assembly::Assembler::default().assemble_program(
                "overflowing_mul_u32",
                format!("begin{body}\nrepeat.4 movup.15 drop end\nend"),
            )?;
            let program = package.try_into_program()?;
            let output = FastProcessor::new(StackInputs::default())
                .execute_sync(&program, &mut DefaultHost::default())
                .map_err(|error| Report::msg(error.to_string()))?;
            for (index, expected) in [flag, result, 17].into_iter().enumerate() {
                assert_eq!(
                    output.stack.get_element(index),
                    Some(Felt::new_unchecked(expected as u64)),
                    "{lhs} * {rhs}, immediate = {immediate}, index = {index}",
                );
            }
        }
        Ok(())
    }

    #[test]
    fn overflowing_mul_u32_preserves_results_in_vm() -> Result<(), Report> {
        assert_overflowing_mul_u32_in_vm(false)
    }

    #[test]
    fn overflowing_mul_imm_u32_preserves_results_in_vm() -> Result<(), Report> {
        assert_overflowing_mul_u32_in_vm(true)
    }

    /// Exercise the actual shared emitter, linked intrinsics and core, checking every raw lane
    /// before root cleanup. The suffix exceeds the indexed window to catch accidental tail use.
    fn check_arithmetic_vm(
        operands: (Immediate, Immediate),
        op: &str,
        mode: Overflow,
        immediate: bool,
        expected: Option<&[u64]>,
        suffix_len: usize,
    ) -> Result<(), Report> {
        let (lhs, rhs) = operands;
        let context = Rc::new(Context::default());
        let core = midenc_session::LinkLibrary::core().load(&context.session().options)?;
        let intrinsics = crate::intrinsics::load(context.session())?;
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::new();
        let mut instructions = Vec::new();
        let span = SourceSpan::UNKNOWN;
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut instructions, &mut stack);
            for value in (0..suffix_len).rev() {
                emitter.literal(1000 + value as u32, span);
            }
            emitter.literal(lhs, span);
            if !immediate {
                emitter.literal(rhs, span);
            }
            match (op, immediate) {
                ("add", false) => emitter.add(mode, span),
                ("sub", false) => emitter.sub(mode, span),
                ("mul", false) => emitter.mul(mode, span),
                ("add", true) => emitter.add_imm(rhs, mode, span),
                ("sub", true) => emitter.sub_imm(rhs, mode, span),
                ("mul", true) => emitter.mul_imm(rhs, mode, span),
                _ => unreachable!(),
            }
            assert_eq!(emitter.stack().len(), suffix_len + 1 + usize::from(mode.is_overflowing()));
            assert_eq!(
                emitter.stack()[0].ty(),
                if mode.is_overflowing() {
                    Type::I1
                } else {
                    lhs.ty()
                }
            );
        }
        let body = masm::Block::new(span, instructions).to_pretty_string();
        let assertions = expected
            .into_iter()
            .flatten()
            .copied()
            .chain((0..suffix_len).filter(|_| expected.is_some()).map(|i| 1000 + i as u64))
            .map(|lane| format!("push.{lane} assert_eq\n"))
            .collect::<String>();
        let mut assembler = miden_assembly::Assembler::default();
        assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic)?;
        assembler.link_package(intrinsics.clone(), miden_assembly::Linkage::Dynamic)?;
        let package = assembler.assemble_program(
            "arithmetic_modes",
            format!("begin{body}\n{assertions}exec.::miden::core::sys::truncate_stack end"),
        )?;
        let program = package.try_into_program()?;
        let mut host = crate::plan::tests::lowering_units::host();
        host.load_library(miden_processor::HostLibrary::from(core))?;
        host.load_library(miden_processor::HostLibrary::from(intrinsics))?;
        let output = FastProcessor::new(StackInputs::default()).execute_sync(&program, &mut host);
        if expected.is_none() {
            let error = output.expect_err("checked arithmetic must trap on overflow");
            match error {
                miden_processor::ExecutionError::OperationError {
                    err:
                        miden_processor::operation::OperationError::FailedAssertion { err_code, .. },
                    ..
                } => assert_eq!(
                    err_code,
                    miden_core::mast::error_code_from_msg(format!("{} {op} overflow", lhs.ty())),
                    "{lhs:?} {op} {rhs:?}: wrong arithmetic assertion",
                ),
                other => panic!("{lhs:?} {op} {rhs:?}: unexpected trap: {other}"),
            }
        } else {
            output.map_err(|error| {
                Report::msg(format!("{lhs:?} {op} {rhs:?}, {mode:?}, suffix {suffix_len}: {error}"))
            })?;
        }
        Ok(())
    }

    fn wide_limbs(value: u128) -> Vec<u64> {
        (0..4).map(|i| ((value >> (32 * i)) as u32) as u64).collect()
    }

    #[test]
    fn overflowing_unsigned_128_modes_match_vm() -> Result<(), Report> {
        for op in ["add", "sub", "mul"] {
            for (a, b) in [
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
                (u128::MAX, 1),
                (u128::MAX, 2),
                (u128::MAX, u128::MAX),
                (1 << 32, 1),
                ((1 << 32) - 1, 1),
                (1 << 64, 1),
                ((1 << 64) - 1, 1),
                (1 << 96, 1),
                ((1 << 96) - 1, 1),
                (0x4444_4444_3333_3333_2222_2222_1111_1111, 0x1234_5678),
            ] {
                let (result, flag) = match op {
                    "add" => a.overflowing_add(b),
                    "sub" => a.overflowing_sub(b),
                    "mul" => a.overflowing_mul(b),
                    _ => unreachable!(),
                };
                for mode in [
                    Overflow::Wrapping,
                    Overflow::Checked,
                    Overflow::Overflowing,
                    Overflow::Unchecked,
                ] {
                    if flag && mode == Overflow::Unchecked {
                        continue;
                    }
                    let mut expected = wide_limbs(result);
                    if mode.is_overflowing() {
                        expected.insert(0, u64::from(flag));
                    }
                    for suffix_len in [0, 20] {
                        check_arithmetic_vm(
                            (Immediate::U128(a), Immediate::U128(b)),
                            op,
                            mode,
                            false,
                            if flag && mode == Overflow::Checked {
                                None
                            } else {
                                Some(&expected)
                            },
                            suffix_len,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn overflowing_signed_128_modes_match_vm() -> Result<(), Report> {
        for op in ["add", "sub", "mul"] {
            for (a, b) in [
                (0, 0),
                (0, -1),
                (-1, 0),
                (-1, -1),
                (-1, 1),
                (i128::MAX, 1),
                (i128::MAX, -1),
                (i128::MIN, 1),
                (i128::MIN, -1),
                (i128::MIN, 0),
                (i128::MIN, i128::MIN),
                (i128::MAX, 2),
                (i128::MAX, i128::MAX),
                (i128::MIN / 2, 2),
                (i128::MIN / 2 - 1, 2),
                (i128::MAX / 3, 3),
                (i128::MAX / 3 + 1, 3),
                (0x4444_4444_3333_3333_2222_2222_1111_1111, -0x1234_5678),
            ] {
                let (result, flag) = match op {
                    "add" => a.overflowing_add(b),
                    "sub" => a.overflowing_sub(b),
                    "mul" => a.overflowing_mul(b),
                    _ => unreachable!(),
                };
                for mode in [
                    Overflow::Wrapping,
                    Overflow::Checked,
                    Overflow::Overflowing,
                    Overflow::Unchecked,
                ] {
                    if flag && mode == Overflow::Unchecked {
                        continue;
                    }
                    let mut expected = wide_limbs(result as u128);
                    if mode.is_overflowing() {
                        expected.insert(0, u64::from(flag));
                    }
                    for suffix_len in [0, 20] {
                        check_arithmetic_vm(
                            (Immediate::I128(a), Immediate::I128(b)),
                            op,
                            mode,
                            false,
                            if flag && mode == Overflow::Checked {
                                None
                            } else {
                                Some(&expected)
                            },
                            suffix_len,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn overflowing_felt_canonical_modes_match_vm() -> Result<(), Report> {
        let p = 0xffff_ffff_0000_0001u128;
        for op in ["add", "sub", "mul"] {
            for (a, b) in [
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
                (p - 1, 1),
                (p - 1, p - 1),
                ((1 << 32) - 1, (1 << 32) + 1),
                ((1 << 32) - 1, 1 << 32),
                (3, 5),
            ] {
                let (result, flag) = match op {
                    "add" => ((a + b) % p, a + b >= p),
                    "sub" => ((a + p - b) % p, a < b),
                    "mul" => ((a * b) % p, a * b >= p),
                    _ => unreachable!(),
                };
                for mode in [
                    Overflow::Wrapping,
                    Overflow::Checked,
                    Overflow::Overflowing,
                    Overflow::Unchecked,
                ] {
                    if flag && mode == Overflow::Unchecked {
                        continue;
                    }
                    let mut expected = vec![result as u64];
                    if mode.is_overflowing() {
                        expected.insert(0, u64::from(flag));
                    }
                    for immediate in [false, true] {
                        for suffix_len in [0, 20] {
                            check_arithmetic_vm(
                                (
                                    Immediate::Felt(Felt::new_unchecked(a as u64)),
                                    Immediate::Felt(Felt::new_unchecked(b as u64)),
                                ),
                                op,
                                mode,
                                immediate,
                                if flag && mode == Overflow::Checked {
                                    None
                                } else {
                                    Some(&expected)
                                },
                                suffix_len,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
