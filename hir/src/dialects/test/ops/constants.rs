use crate::{
    derive::{EffectOpInterface, operation},
    dialects::test::TestDialect,
    effects::*,
    traits::*,
    *,
};

/// An operation for expressing constant immediate values.
///
/// This is used to materialize folded constants for the HIR dialect.
#[derive(EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, Foldable, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Constant {
    #[attr(hidden)]
    value: ImmediateAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Constant {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.value().ty().clone();
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for Constant {
    #[inline]
    fn fold(&self, results: &mut smallvec::SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut smallvec::SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl OpPrinter for Constant {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        printer.print_decimal_integer(*self.get_value());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}

impl OpParser for Constant {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use alloc::format;

        use crate::{attributes::AttrParser, parse::ParserError};

        let start = parser.current_location();
        let imm = ImmediateAttr::parse(parser)?;
        let mut imm = imm.try_downcast::<ImmediateAttr>().unwrap();

        let (ty_span, ty) = parser.parse_colon_type()?.into_parts();
        let span = SourceSpan::new(start.source_id(), start.start()..ty_span.end());

        let mut imm_mut = imm.borrow_mut();
        let new_value = match ty {
            Type::I1 => imm_mut.as_bool().map(Immediate::I1),
            Type::I8 => imm_mut.as_i8().map(Immediate::I8),
            Type::U8 => imm_mut.as_u8().map(Immediate::U8),
            Type::I16 => imm_mut.as_i16().map(Immediate::I16),
            Type::U16 => imm_mut.as_u16().map(Immediate::U16),
            Type::I32 => imm_mut.as_i32().map(Immediate::I32),
            Type::U32 => imm_mut.as_u32().map(Immediate::U32),
            Type::I64 => imm_mut.as_i64().map(Immediate::I64),
            Type::U64 => imm_mut.as_u64().map(Immediate::U64),
            Type::I128 => imm_mut.as_i128().map(Immediate::I128),
            Type::U128 => imm_mut.as_u128().map(Immediate::U128),
            invalid => {
                return Err(ParserError::InvalidOperationType {
                    span,
                    ty_span,
                    reason: format!("expected integer type, got '{invalid}'"),
                });
            }
        };

        if let Some(new_value) = new_value {
            *imm_mut.as_value_mut() = new_value;
        } else {
            return Err(ParserError::InvalidAttributeValue {
                span,
                reason: format!("could not convert value to type {ty}: value is out of range"),
            });
        }

        state.add_attribute("value", imm);
        state.results.push(ty);

        Ok(())
    }
}
