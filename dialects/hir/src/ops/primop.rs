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
/// The source and destination ranges may overlap, the destination receives the values the source
/// range held before the copy.
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
/// take it from either pointer, so the two pointer types must agree.
impl Verify<dyn MemoryEffectOpInterface> for MemCpy {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        let source_ty = self.source().ty();
        let destination_ty = self.destination().ty();
        if source_ty != destination_ty {
            return Err(Report::msg(format!(
                "invalid hir.mem_cpy: the source has type '{source_ty}', but the destination has \
                 type '{destination_ty}'"
            )));
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use alloc::format;

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        Op, PointerType, SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, testing::Test,
    };

    use crate::HirOpBuilder;

    /// Build a function whose body is a `hir.mem_cpy` from a `src_pointee` pointer to a
    /// `dst_pointee` pointer, then verify the module.
    fn verify_mem_cpy_with(src_pointee: Type, dst_pointee: Type) -> Result<(), midenc_hir::Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("verify_mem_cpy").in_module("m");
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
            builder.memcpy(src, dst, count, span).unwrap();
            builder.ret(None, span).unwrap();
        }

        test.module().borrow().as_operation().recursively_verify()
    }

    /// Checks that a `hir.mem_cpy` whose source and destination pointer types differ fails
    /// verification.
    #[test]
    fn mem_cpy_with_mismatched_pointer_types_fails_verification() {
        let err = verify_mem_cpy_with(Type::U8, Type::U16)
            .expect_err("mismatched pointer types must fail verification");
        let message = format!("{err}");
        assert!(message.contains("invalid hir.mem_cpy"), "{message}");
    }

    /// Checks that a `hir.mem_cpy` whose source and destination pointer types agree passes
    /// verification.
    #[test]
    fn mem_cpy_with_matching_pointer_types_passes_verification() {
        verify_mem_cpy_with(Type::U8, Type::U8).expect("matching pointer types must verify");
    }
}
