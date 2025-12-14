use core::assert_matches::assert_matches;

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
                    masm::Instruction::EqImm(Felt::new(imm.as_i32().unwrap() as u32 as u64).into()),
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
                        Felt::new(imm.as_i32().unwrap() as u32 as u64).into(),
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
            Type::I32 => self.raw_exec("intrinsics::i32::is_gt", span),
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
                self.raw_exec("intrinsics::i32::is_gt", span);
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
            Type::I32 => self.raw_exec("intrinsics::i32::is_gte", span),
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
                self.raw_exec("intrinsics::i32::is_gte", span);
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
            Type::I32 => self.raw_exec("intrinsics::i32::is_lt", span),
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
                self.raw_exec("intrinsics::i32::is_lt", span);
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
            Type::I32 => self.raw_exec("intrinsics::i32::is_lte", span),
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
                self.raw_exec("intrinsics::i32::is_lte", span);
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
            Type::Felt => {
                self.emit(masm::Instruction::Add, span);
            }
            Type::I128 => {
                self.add_i128(overflow, span);
            }
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn add_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected add operands to be the same type");
        match &ty {
            Type::Felt if imm == 1 => self.emit(masm::Instruction::Incr, span),
            Type::Felt => {
                self.emit(masm::Instruction::AddImm(imm.as_felt().unwrap().into()), span);
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
    }

    pub fn sub(&mut self, overflow: Overflow, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected sub operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::Sub, span);
            }
            Type::I128 => {
                self.sub_i128(overflow, span);
            }
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn sub_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected sub operands to be the same type");
        match &ty {
            Type::Felt => {
                self.emit(masm::Instruction::SubImm(imm.as_felt().unwrap().into()), span);
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
    }

    pub fn mul(&mut self, overflow: Overflow, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected mul operands to be the same type");
        match &ty {
            Type::Felt => {
                assert_matches!(
                    overflow,
                    Overflow::Unchecked | Overflow::Wrapping,
                    "only unchecked or wrapping semantics are supported for felt"
                );
                self.emit(masm::Instruction::Mul, span);
            }
            Type::I128 | Type::U128 => self.mul_i128(span),
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
    }

    #[allow(unused)]
    pub fn mul_imm(&mut self, imm: Immediate, overflow: Overflow, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, imm.ty(), "expected mul operands to be the same type");
        match &ty {
            Type::Felt => {
                assert_matches!(
                    overflow,
                    Overflow::Unchecked | Overflow::Wrapping,
                    "only unchecked or wrapping semantics are supported for felt"
                );
                self.emit(masm::Instruction::MulImm(imm.as_felt().unwrap().into()), span);
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
        if overflow.is_overflowing() {
            self.push(Type::I1);
        }
        self.push(ty);
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
            Type::U32 => self.checked_mod_u32(span),
            ty @ (Type::U16 | Type::U8) => {
                self.checked_mod_uint(ty.size_in_bits() as u32, span);
            }
            ty if !ty.is_integer() => {
                panic!("invalid binary operand: mod expects integer operands, got {ty}")
            }
            ty => unimplemented!("mod for {ty} is not supported"),
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
                self.raw_exec("intrinsics::i32::ipow", span);
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
                self.emit(masm::Instruction::ExpImm(Felt::new(exp as u64).into()), span);
            }
            Type::U32 => {
                self.emit_all(
                    [
                        masm::Instruction::ExpImm(Felt::new(exp as u64).into()),
                        masm::Instruction::U32Assert,
                    ],
                    span,
                );
            }
            Type::I32 => {
                self.emit_push(exp, span);
                self.raw_exec("intrinsics::i32::ipow", span);
            }
            ty @ (Type::U16 | Type::U8) => {
                self.emit_all(
                    [
                        masm::Instruction::ExpImm(Felt::new(exp as u64).into()),
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
                // AND the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.band_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // AND the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.band_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
                // AND the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.band_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // AND the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.band_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
                // OR the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bor_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // OR the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bor_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
                // OR the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bor_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // OR the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bor_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
                // XOR the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // XOR the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
                // XOR the high bits
                //
                // [b_hi_hi, b_hi_lo, b_lo_hi, b_lo_lo, a_hi_hi, ..]
                self.emit_all(
                    [
                        // [a_hi_hi, a_hi_lo, b_hi_hi, b_hi_lo, ..]
                        masm::Instruction::MovUp5,
                        masm::Instruction::MovUp5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [band_hi_hi, band_hi_lo, b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo]
                // XOR the low bits
                self.emit_all(
                    [
                        // [b_lo_hi, b_lo_lo, a_lo_hi, a_lo_lo, band_hi_hi, band_hi_lo]
                        masm::Instruction::MovDn5,
                        masm::Instruction::MovDn5,
                    ],
                    span,
                );
                self.bxor_int64(span); // [band_lo_hi, band_lo_lo, band_hi_hi, band_hi_lo]
                self.emit_all(
                    [
                        // [band_hi_hi, band_hi_lo, band_lo_hi, band_lo_lo]
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
