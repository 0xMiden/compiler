use alloc::{format, string::ToString, sync::Arc};

use midenc_hir::{
    constants::ConstantData,
    derive::{EffectOpInterface, operation},
    dialects::builtin::attributes::BytesAttr,
    effects::MemoryEffectOpInterface,
    parse::ParserExt,
    traits::*,
    *,
};

use crate::{HirDialect, PointerAttr};

/// An operation for expressing constant pointer values.
///
/// This is used to materialize folded constants for the HIR dialect.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct ConstantPointer {
    #[attr(hidden)]
    value: PointerAttr,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for ConstantPointer {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = Type::from(self.value().pointer_type().clone());
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for ConstantPointer {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl OpPrinter for ConstantPointer {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        let ptr = self.get_value();
        printer.print_decimal_integer(ptr.addr());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}

impl OpParser for ConstantPointer {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        let addr = parser.parse_decimal_integer::<u32>()?;
        let (ty_span, result_ty) = parser.parse_colon_type()?.into_parts();
        let Type::Ptr(pointer_ty) = &result_ty else {
            return Err(parse::ParserError::InvalidOperationType {
                span: ty_span,
                ty_span,
                reason: "expected a pointer type".into(),
            });
        };

        let attr = parser.context_rc().create_attribute::<PointerAttr, _>(crate::Pointer::new(
            addr.into_inner(),
            pointer_ty.as_ref().clone(),
        ));
        state.attrs.push(NamedAttribute::new("value", attr));
        state.results.push(result_ty);

        Ok(())
    }
}

/// A constant operation used to define an array of arbitrary bytes.
///
/// This is intended for use in [midenc_hir::dialects::builtin::GlobalVariable] initializer regions
/// only. For non-global uses, the maximum size of immediate values is limited to a single word.
/// This restriction does not apply to global variable initializers, which are used to express the
/// data that should be placed in memory at the address allocated for the variable, without explicit
/// load/store ops.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    name = "bytes",
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct ConstantBytes {
    #[attr(hidden)]
    bytes: BytesAttr,
    #[result]
    result: AnyArrayOf<UInt8>,
}

impl InferTypeOpInterface for ConstantBytes {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let len = self.size_in_bytes();
        self.result_mut().set_type(Type::from(ArrayType::new(Type::U8, len)));

        Ok(())
    }
}

impl Foldable for ConstantBytes {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.bytes));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl ConstantBytes {
    pub fn size_in_bytes(&self) -> usize {
        self.get_bytes().len()
    }

    pub fn value(&self) -> Arc<ConstantData> {
        self.get_bytes().clone()
    }
}

impl OpPrinter for ConstantBytes {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        let bytes = self.get_bytes();
        printer.print_string(bytes.to_string());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}

impl OpParser for ConstantBytes {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use midenc_hir::parse::ParserError;

        let (span, bytes_as_string) = parser.parse_string()?.into_parts();
        match ConstantData::from_str_be(bytes_as_string.as_str()) {
            Ok(bytes) => {
                let constant_id = parser.context().create_constant(bytes);
                let bytes = parser.context().get_constant(constant_id);
                let attr = parser.context_rc().create_attribute::<BytesAttr, _>(bytes);
                state.results.push(attr.borrow().ty().clone());
                state.add_attribute("bytes", attr);
                Ok(())
            }
            Err(err) => Err(ParserError::InvalidAttributeValue {
                span,
                reason: format!("unable to parse big-endian bytes from string: {err}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{attributes::InferAttributeValueType, diagnostics::Uri};

    use super::*;

    #[test]
    fn pointer_constant_text_roundtrip_preserves_complete_type() {
        for space in ["byte", "element"] {
            for pointee in ["u32", "ptr<u8, byte>", "ptr<u8, element>"] {
                let ty = format!("ptr<{pointee}, {space}>");
                let source = format!(
                    "builtin.function public extern(\"C\") @f() -> {ty} {{ %p = \
                     hir.constant_pointer 4294967295 : {ty}; builtin.ret %p : ({ty}); }};"
                );
                let context = Rc::new(Context::default());
                let (function, _) =
                    midenc_hir::testing::parse_function_fixpoint(&context, "pointer.hir", &source)
                        .unwrap();
                let op = function
                    .borrow()
                    .entry_block()
                    .borrow()
                    .body()
                    .front()
                    .get()
                    .unwrap()
                    .as_operation_ref();
                let op = op.borrow();
                let pointer = op.downcast_ref::<ConstantPointer>().unwrap();
                assert_eq!(pointer.get_value().addr(), u32::MAX);
                assert_eq!(
                    pointer.get_value().infer_type_from_value(),
                    pointer.result().ty().clone()
                );
            }
        }
    }

    #[test]
    fn pointer_constant_builder_roundtrip_preserves_complete_type() {
        use midenc_hir::{
            dialects::builtin::BuiltinOpBuilder, formatter::PrettyPrint, testing::Test,
        };
        for space in [AddressSpace::Byte, AddressSpace::Element] {
            for pointee in [
                Type::U32,
                PointerType::new(Type::U8).into(),
                PointerType::new_with_address_space(Type::U8, AddressSpace::Element).into(),
            ] {
                let pointer_type = PointerType::new_with_address_space(pointee, space);
                let ty = Type::from(pointer_type.clone());
                let mut test = Test::new("pointer_builder", &[], core::slice::from_ref(&ty));
                {
                    let mut builder = test.function_builder();
                    let constant =
                        builder.builder_mut().create::<ConstantPointer, _>(SourceSpan::UNKNOWN)(
                            crate::Pointer::new(u32::MAX, pointer_type),
                        )
                        .unwrap();
                    let result = constant.borrow().result().as_value_ref();
                    assert_eq!(result.borrow().ty(), &ty);
                    builder.ret(Some(result), SourceSpan::UNKNOWN).unwrap();
                }
                let printed = test.function().borrow().as_operation().to_pretty_string();
                let context = Rc::new(Context::default());
                let (function, reprinted) = midenc_hir::testing::parse_function_fixpoint(
                    &context,
                    "pointer_builder.hir",
                    &printed,
                )
                .unwrap();
                assert_eq!(printed, reprinted);
                let op = function
                    .borrow()
                    .entry_block()
                    .borrow()
                    .body()
                    .front()
                    .get()
                    .unwrap()
                    .as_operation_ref();
                let op = op.borrow();
                let constant = op.downcast_ref::<ConstantPointer>().unwrap();
                assert_eq!(constant.get_value().addr(), u32::MAX);
                assert_eq!(constant.get_value().infer_type_from_value(), ty);
                assert_eq!(constant.result().ty(), &ty);
            }
        }
    }

    #[test]
    fn pointer_constant_nonpointer_text_is_a_parse_diagnostic() {
        let result = midenc_hir::parse::parse_any(
            midenc_hir::parse::ParserConfig {
                context: Rc::new(Context::default()),
                verify: false,
            },
            Uri::new("nonpointer.hir"),
            "builtin.function public extern(\"C\") @f() { %p = hir.constant_pointer 42 : u32; \
             builtin.ret; };",
        );
        let error = result.expect_err("nonpointer text must fail during parsing");
        assert!(format!("{error:?}").contains("expected a pointer type"));
    }
}
