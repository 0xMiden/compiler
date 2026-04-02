use alloc::format;

use midenc_hir::{
    derive::operation, dialects::builtin::attributes::SignatureAttr, print::AsmPrinter, traits::*,
    *,
};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    implements(CallOpInterface, InferTypeOpInterface, OpPrinter)
)]
pub struct Exec {
    #[symbol(callable)]
    callee: SymbolPath,
    #[attr(hidden)]
    signature: SignatureAttr,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for Exec {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let span = self.span();
        let sig = self.signature.borrow();
        let owner = self.as_operation_ref();
        for (i, result) in sig.results().iter().enumerate() {
            let value = context.make_result(span, result.ty.clone(), owner, i as u8);
            self.op.results.push(value);
        }
        Ok(())
    }
}

impl OpPrinter for Exec {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        let callee = self.callee();
        printer.print_space();
        printer.print_symbol_path(callee.path());
        printer.print_operand_list(self.arguments());
        let callee_sig = self.signature();
        *printer += const_text(" : ");
        callee_sig.print(printer);
        if self.op.has_attributes() {
            printer.print_space();
            *printer += const_text(" attributes ");
            printer.print_attribute_dictionary(
                self.op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }
    }
}

impl OpParser for Exec {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use midenc_hir::parse::ParserError;

        let callee = parser.parse_symbol_ref()?;

        state.attrs.push(NamedAttribute::new("callee", callee.into_inner()));

        let mut operands = SmallVec::default();
        parser.parse_operand_list(
            &mut operands,
            parse::Delimiter::OptionalParen,
            /*allow_result_number=*/ true,
            None,
        )?;

        parser.parse_colon()?;
        let sig_attr = <SignatureAttr as midenc_hir::attributes::AttrParser>::parse(parser)?;
        state.attrs.push(NamedAttribute::new("signature", sig_attr));

        let span = SourceSpan::new(
            state.span.source_id(),
            state.span.start()..parser.current_location().end(),
        );
        let sig_attribute = sig_attr.borrow();
        let Some(signature) = sig_attribute.downcast_ref::<SignatureAttr>() else {
            return Err(ParserError::InvalidAttributeValue {
                span,
                reason: format!(
                    "expected 'signature' property to be of type #builtin.signature, got '{}' \
                     instead",
                    sig_attribute.name()
                ),
            });
        };

        let span = SourceSpan::new(
            state.span.source_id(),
            state.span.start()..parser.current_location().end(),
        );
        if operands.len() != signature.arity() {
            return Err(ParserError::MismatchedValueAndTypeLists {
                span,
                num_values: operands.len(),
                num_types: signature.arity(),
            });
        }

        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;

        let type_params =
            signature.results.iter().map(|p| p.ty.clone()).collect::<SmallVec<[Type; 2]>>();
        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;

        state.operands.push(operand_values);

        Ok(())
    }
}

/*
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct ExecIndirect {
    #[attr]
    signature: Signature,
    /// TODO(pauls): Change this to FunctionType
    #[operand]
    callee: AnyType,
}
 */
impl CallOpInterface for Exec {
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        self.callee().path().into()
    }

    fn set_callee(&mut self, callable: Callable) {
        let callee = callable.unwrap_symbol_path();
        let symbol_table = self
            .as_operation()
            .nearest_symbol_table()
            .expect("cannot set callee outside of symbol table");
        let resolved = symbol_table
            .borrow()
            .as_symbol_table()
            .unwrap()
            .resolve(&callee)
            .expect("invalid callee: could not be resolved");
        // SAFETY: This is guaranteed to be safe because the original reference was an UnsafeIntrusiveEntityRef;
        let callable = unsafe {
            let resolved = resolved.borrow();
            let callable = resolved
                .as_symbol_operation()
                .as_trait::<dyn CallableSymbol>()
                .expect("invalid callee: not a callable symbol");
            CallableSymbolRef::from_raw(callable)
        };
        Exec::set_callee(self, callable).expect("invalid callee");
    }

    #[inline(always)]
    fn arguments(&self) -> OpOperandRange<'_> {
        self.operands().group(0)
    }

    #[inline(always)]
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().group_mut(0)
    }

    fn resolve(&self) -> Option<SymbolRef> {
        let callee = self.callee();
        let symbol_table = self.as_operation().nearest_symbol_table()?;
        let symbol_table = symbol_table.borrow();
        let symbol_table = symbol_table.as_symbol_table().unwrap();
        symbol_table.resolve(callee.path())
    }

    fn resolve_in_symbol_table(&self, symbols: &dyn SymbolTable) -> Option<SymbolRef> {
        let callee = self.callee();
        symbols.resolve(callee.path())
    }
}

// TODO(pauls): Validate that the arguments/results of the callee of this operation do not contain
// any types which are invalid for cross-context calls
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface, InferTypeOpInterface, OpPrinter)
)]
pub struct Call {
    #[symbol(callable)]
    callee: SymbolPath,
    #[attr]
    signature: SignatureAttr,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for Call {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let span = self.span();
        let signature = self.signature.borrow();
        let owner = self.as_operation_ref();
        for (i, result) in signature.results().iter().enumerate() {
            let value = context.make_result(span, result.ty.clone(), owner, i as u8);
            self.op.results.push(value);
        }
        Ok(())
    }
}

impl OpPrinter for Call {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        let callee = self.callee();
        printer.print_space();
        printer.print_symbol_path(callee.path());
        printer.print_operand_list(self.arguments());
        *printer += const_text(" <");
        printer.print_attribute_dictionary(self.op.properties().filter(|p| p.name == "signature"));
        *printer += const_text(" >");
        if self.op.has_attributes() {
            printer.print_space();
            *printer += const_text(" attributes ");
            printer.print_attribute_dictionary(
                self.op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }
    }
}

/*
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct CallIndirect {
    #[attr]
    signature: Signature,
    /// TODO(pauls): Change this to FunctionType
    #[operand]
    callee: AnyType,
}
 */
impl CallOpInterface for Call {
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        self.callee().path().into()
    }

    fn set_callee(&mut self, callable: Callable) {
        let callee = callable.unwrap_symbol_path();
        let symbol_table = self
            .as_operation()
            .nearest_symbol_table()
            .expect("cannot set callee outside of symbol table");
        let resolved = symbol_table
            .borrow()
            .as_symbol_table()
            .unwrap()
            .resolve(&callee)
            .expect("invalid callee: could not be resolved");
        // SAFETY: This is guaranteed to be safe because the original reference was an UnsafeIntrusiveEntityRef;
        let callable = unsafe {
            let resolved = resolved.borrow();
            let callable = resolved
                .as_symbol_operation()
                .as_trait::<dyn CallableSymbol>()
                .expect("invalid callee: not a callable symbol");
            CallableSymbolRef::from_raw(callable)
        };
        Call::set_callee(self, callable).expect("invalid callee");
    }

    #[inline(always)]
    fn arguments(&self) -> OpOperandRange<'_> {
        self.operands().group(0)
    }

    #[inline(always)]
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().group_mut(0)
    }

    fn resolve(&self) -> Option<SymbolRef> {
        let callee = self.callee();
        let symbol_table = self.as_operation().nearest_symbol_table()?;
        let symbol_table = symbol_table.borrow();
        let symbol_table = symbol_table.as_symbol_table().unwrap();
        symbol_table.resolve(callee.path())
    }

    fn resolve_in_symbol_table(&self, symbols: &dyn SymbolTable) -> Option<SymbolRef> {
        let callee = self.callee();
        symbols.resolve(callee.path())
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::{
        SourceSpan, Symbol, Type, Usable,
        dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
        testing::Test,
    };

    use crate::HirOpBuilder;

    #[test]
    fn call_set_callee_rebinds_property_backed_symbol_use() {
        let mut test =
            Test::named("call_set_callee_rebinds_property_backed_symbol_use").in_module("test");
        let original = test.define_function("original", &[], &[]);
        let replacement = test.define_function("replacement", &[], &[]);
        test.with_function("caller", &[], &[]);

        let signature = Signature::new(
            &test.context_rc(),
            core::iter::empty::<Type>(),
            core::iter::empty::<Type>(),
        );
        let mut call = {
            let mut builder = test.function_builder();
            let call = builder.call(original, signature, [], SourceSpan::default()).unwrap();
            builder.ret(None, SourceSpan::default()).unwrap();
            call
        };

        assert_eq!(original.borrow().iter_uses().count(), 1);
        assert_eq!(replacement.borrow().iter_uses().count(), 0);

        call.borrow_mut().set_callee(replacement).unwrap();

        let replacement_path = replacement.borrow().path();
        assert_eq!(call.borrow().callee().path(), &replacement_path);
        assert_eq!(original.borrow().iter_uses().count(), 0);
        assert_eq!(replacement.borrow().iter_uses().count(), 1);
    }
}
