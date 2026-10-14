use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Return the caller procedure hash as a word.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read))]
pub struct Caller {
    #[result]
    result: AnyArray,
}

impl InferTypeOpInterface for Caller {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::from(ArrayType::new(Type::Felt, 4)));
        Ok(())
    }
}

/// View a first-class word as four field elements in operand-stack order.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct UnpackWord {
    #[operand]
    word: AnyArrayOf<IntFelt>,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
}

impl InferTypeOpInterface for UnpackWord {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        if self.word().ty() != Type::from(ArrayType::new(Type::Felt, 4)) {
            return Err(Report::msg(format!(
                "hir.unpack_word requires a four-felt word, got {}",
                self.word().ty()
            )));
        }
        self.result0_mut().set_type(Type::Felt);
        self.result1_mut().set_type(Type::Felt);
        self.result2_mut().set_type(Type::Felt);
        self.result3_mut().set_type(Type::Felt);
        Ok(())
    }
}

/// Return the current VM clock cycle.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Clk {
    #[result]
    result: IntFelt,
}

impl InferTypeOpInterface for Clk {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::Felt);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MemGrow {
    #[operand]
    pages: UInt32,
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemGrow {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read))]
pub struct MemSize {
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemSize {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemSet {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    addr: AnyPointer,
    #[operand]
    count: UInt32,
    #[operand]
    value: AnyType,
}

/// Copies `count` values from the memory at address `source`, to the memory at address
/// `destination`.
///
/// The unit of `count` is the pointee type of `source`, i.e. `count * size_of(pointee)` bytes are
/// copied. A `count` of zero leaves the memory unchanged. The byte length and the end addresses
/// `source + len` and `destination + len` must fit in a `u32`.
///
/// The source and destination ranges must not overlap, a copy between overlapping ranges of a
/// non-zero length traps. Use [MemMove] when the ranges may overlap.
///
/// The MASM lowering supports pointers in the byte address space only, copies values of 16 bytes
/// or a multiple of 16 a word at a time, which requires 16-byte aligned addresses (also when
/// `count` is zero), and cannot copy other aggregate values.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemCpy {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    source: AnyPointer,
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}

/// The pointee type of the pointers is the unit of `count`, and the lowering and the evaluator
/// take it from the source pointer, so the destination pointer type must agree with it.
impl Verify<dyn MemoryEffectOpInterface> for MemCpy {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        verify_copy_pointer_types("hir.mem_cpy", &self.source().ty(), &self.destination().ty())
    }
}

/// Copies `count` values from the memory at address `source`, to the memory at address
/// `destination`, where the two ranges may overlap.
///
/// The unit of `count` is the pointee type of `source`, i.e. `count * size_of(pointee)` bytes are
/// copied. A `count` of zero leaves the memory unchanged. The byte length and the end addresses
/// `source + len` and `destination + len` must fit in a `u32`.
///
/// The destination receives the values the source range held before the copy. Use [MemCpy] when
/// the ranges are known to be disjoint.
///
/// The MASM lowering supports pointers in the byte address space only, copies values of 16 bytes
/// or a multiple of 16 a word at a time, which requires 16-byte aligned addresses (also when
/// `count` is zero), and cannot copy other aggregate values.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemMove {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    source: AnyPointer,
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}

/// The pointee type of the pointers is the unit of `count`, and the lowering and the evaluator
/// take it from the source pointer, so the destination pointer type must agree with it.
impl Verify<dyn MemoryEffectOpInterface> for MemMove {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        verify_copy_pointer_types("hir.mem_move", &self.source().ty(), &self.destination().ty())
    }
}

/// Checks that the source and destination pointers of the copy operation `op` have the same type.
fn verify_copy_pointer_types(
    op: &str,
    source_ty: &Type,
    destination_ty: &Type,
) -> Result<(), Report> {
    if source_ty != destination_ty {
        return Err(Report::msg(format!(
            "invalid {op}: the source has type '{source_ty}', but the destination has type \
             '{destination_ty}'"
        )));
    }
    Ok(())
}

/// Prints a string to the debug output.
///
/// The string bytes are read from memory at the given pointer address and length.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(OpPrinter, MemoryEffectOpInterface)
)]
pub struct PrintLn {
    // TODO(pauls): The Write effect here is added to prevent DCE from removing this op, but
    // we should model this with a specific I/O effect
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
    ptr: PointerOf<UInt8>,
    #[operand]
    len: UInt32,
}

/// Verifier tests of the memory primitives.
#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString};

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        Op, PointerType, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder,
        testing::Test,
    };

    use crate::HirOpBuilder;

    #[test]
    fn unpack_word_rejects_an_array_with_the_wrong_length() {
        let ty = Type::from(midenc_hir::ArrayType::new(Type::Felt, 3));
        let mut test = Test::new("invalid_word", &[ty], &[]);
        let mut builder = test.function_builder();
        let word = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let error = builder.unpack_word(word, SourceSpan::UNKNOWN).unwrap_err();
        assert_eq!(error.to_string(), "hir.unpack_word requires a four-felt word, got [felt; 3]");
    }

    /// The copy operations under test.
    #[derive(Copy, Clone)]
    enum CopyOp {
        MemCpy,
        MemMove,
    }

    /// Build a function whose body is the copy operation `op` from a `src_pointee` pointer to a
    /// `dst_pointee` pointer, then verify the module.
    fn verify_copy_with(
        op: CopyOp,
        src_pointee: Type,
        dst_pointee: Type,
    ) -> Result<(), midenc_hir::Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("verify_copy").in_module("m");
        test.with_function("copy", &[], &[]);
        {
            let mut builder = test.function_builder();
            let src_addr = builder.u32(0, span);
            let src = builder
                .inttoptr(src_addr, Type::from(PointerType::new(src_pointee)), span)
                .unwrap();
            let dst_addr = builder.u32(16, span);
            let dst = builder
                .inttoptr(dst_addr, Type::from(PointerType::new(dst_pointee)), span)
                .unwrap();
            let count = builder.u32(1, span);
            match op {
                CopyOp::MemCpy => {
                    builder.memcpy(src, dst, count, span).unwrap();
                }
                CopyOp::MemMove => {
                    builder.memmove(src, dst, count, span).unwrap();
                }
            }
            builder.ret(None, span).unwrap();
        }

        test.module().borrow().as_operation().recursively_verify()
    }

    /// Checks that a `hir.mem_cpy` whose source and destination pointer types differ fails
    /// verification.
    #[test]
    fn mem_cpy_with_mismatched_pointer_types_fails_verification() {
        let err = verify_copy_with(CopyOp::MemCpy, Type::U8, Type::U16)
            .expect_err("mismatched pointer types must fail verification");
        let message = format!("{err}");
        assert!(message.contains("invalid hir.mem_cpy"), "{message}");
    }

    /// Checks that a `hir.mem_cpy` whose source and destination pointer types agree passes
    /// verification.
    #[test]
    fn mem_cpy_with_matching_pointer_types_passes_verification() {
        verify_copy_with(CopyOp::MemCpy, Type::U8, Type::U8)
            .expect("matching pointer types must verify");
    }

    /// Checks that a `hir.mem_move` whose source and destination pointer types differ fails
    /// verification.
    #[test]
    fn mem_move_with_mismatched_pointer_types_fails_verification() {
        let err = verify_copy_with(CopyOp::MemMove, Type::U8, Type::U16)
            .expect_err("mismatched pointer types must fail verification");
        let message = format!("{err}");
        assert!(message.contains("invalid hir.mem_move"), "{message}");
    }

    /// Checks that a `hir.mem_move` whose source and destination pointer types agree passes
    /// verification.
    #[test]
    fn mem_move_with_matching_pointer_types_passes_verification() {
        verify_copy_with(CopyOp::MemMove, Type::U8, Type::U8)
            .expect("matching pointer types must verify");
    }
}
