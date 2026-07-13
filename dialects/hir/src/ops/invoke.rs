use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::{
        FunctionTable,
        attributes::{LocalVariableArrayAttr, SignatureAttr},
    },
    effects::*,
    interner::symbols,
    print::AsmPrinter,
    traits::*,
    *,
};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    implements(
        CallOpInterface,
        InferTypeOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
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

impl OperandRangeRequirementOpInterface for Exec {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
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
            signature.params().iter().map(|p| p.ty.clone()).collect::<SmallVec<[Type; 2]>>();
        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;

        state.operands.push(operand_values);

        Ok(())
    }
}

/// Invoke a foreign account procedure via the transaction kernel FPI executor.
///
/// This op is the canonical HIR form of a foreign procedure invocation, targeting
/// `miden::protocol::tx::execute_foreign_procedure`. Its operands are the flattened procedure
/// input felts (at most [`ExecFpi::MAX_INPUT_FELTS`]), while `prefix_locals` references the six
/// function locals holding the executor prefix in protocol order: account id suffix, account id
/// prefix, and the four procedure root felts. The locals must be stored before this op executes.
///
/// Keeping the prefix in locals means lowering only ever schedules the procedure inputs on the
/// operand stack: it pads them with zeroes to [`ExecFpi::MAX_INPUT_FELTS`], then loads the six
/// locals on top to form the full [`ExecFpi::EXECUTOR_INPUT_FELTS`]-felt executor ABI without any
/// stack shuffling beyond the addressable 16-element window.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct ExecFpi {
    #[attr]
    prefix_locals: LocalVariableArrayAttr,
    #[operands]
    inputs: IntFelt,
    #[results]
    outputs: IntFelt,
}

impl ExecFpi {
    /// Total number of felt operands expected by the executor.
    pub const EXECUTOR_INPUT_FELTS: usize = Self::PREFIX_FELTS + Self::MAX_INPUT_FELTS;
    /// Number of felts returned by the executor, one per procedure input slot.
    pub const EXECUTOR_RESULT_FELTS: usize = 16;
    /// Maximum number of flattened procedure input felts accepted by the executor.
    pub const MAX_INPUT_FELTS: usize = 16;
    /// Number of executor prefix felts referenced by `prefix_locals`.
    pub const PREFIX_FELTS: usize = 6;

    /// Returns the symbol path of the transaction kernel FPI executor.
    pub fn executor_symbol_path() -> SymbolPath {
        SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(symbols::Miden),
            SymbolNameComponent::Component(symbols::Protocol),
            SymbolNameComponent::Component(symbols::Tx),
            SymbolNameComponent::Leaf(symbols::ExecuteForeignProcedure),
        ])
    }
}

impl InferTypeOpInterface for ExecFpi {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        if self.inputs().len() > Self::MAX_INPUT_FELTS {
            return Err(Report::msg(format!(
                "invalid hir.exec_fpi: expected at most {} procedure input operand(s), but got {}",
                Self::MAX_INPUT_FELTS,
                self.inputs().len()
            )));
        }

        let num_prefix_locals = self.get_prefix_locals().len();
        if num_prefix_locals != Self::PREFIX_FELTS {
            return Err(Report::msg(format!(
                "invalid hir.exec_fpi: expected {} prefix local(s), but got {num_prefix_locals}",
                Self::PREFIX_FELTS,
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..Self::EXECUTOR_RESULT_FELTS {
                let value = context.make_result(span, Type::Felt, owner, i as u8);
                self.op.results.push(value);
            }
        } else {
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(Type::Felt);
            }
        }

        Ok(())
    }
}

/// Materializes the MAST root digest of the referenced function as four felt values (one word).
///
/// This op is the HIR form of the MASM `procref` instruction: the digest of `callee` is computed
/// by the assembler when the containing component is assembled, and pushed on the operand stack
/// as one word with `root[0]` on top, i.e. result `i` holds digest element `i`.
///
/// The callee is referenced, not invoked: no arguments are consumed and control never transfers
/// to it, so this op deliberately does not implement `CallOpInterface`. The symbol property still
/// records a use of the callee, keeping it linked into the program.
///
/// The op is effect-free: it only materializes an assembly-time constant.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct ProcedureRoot {
    /// The function whose MAST root digest is materialized
    #[symbol(callable)]
    callee: SymbolPath,
    #[results]
    digest: IntFelt,
}

impl ProcedureRoot {
    /// Number of felts in a MAST root digest word.
    pub const DIGEST_FELTS: usize = 4;
    /// Marker attribute recording that this op must yield the note script root of the enclosing
    /// component.
    ///
    /// The op is initially built against a placeholder callee (the note-script export wrapper
    /// does not exist until component exports are lifted); export lifting repoints marked ops at
    /// the lifted note-script export, and codegen refuses to lower a marked op whose callee does
    /// not carry the `note_script` attribute.
    pub const NOTE_SCRIPT_ROOT_ATTR: &'static str = "note_script_root";
}

impl InferTypeOpInterface for ProcedureRoot {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..Self::DIGEST_FELTS {
                let value = context.make_result(span, Type::Felt, owner, i as u8);
                self.op.results.push(value);
            }
        } else {
            // Lowering models exactly one digest word on the operand stack, so IR declaring any
            // other result count (e.g. parsed from source) is malformed.
            if self.op.results.len() != Self::DIGEST_FELTS {
                return Err(Report::msg(format!(
                    "invalid hir.procedure_root: expected {} result(s), but got {}",
                    Self::DIGEST_FELTS,
                    self.op.results.len()
                )));
            }
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(Type::Felt);
            }
        }
        Ok(())
    }
}

impl OpPrinter for ProcedureRoot {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        printer.print_space();
        let callee = self.callee();
        printer.print_symbol_path(callee.path());
        if self.op.has_attributes() {
            printer.print_space();
            *printer += const_text(" attributes ");
            printer.print_attribute_dictionary(
                self.op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }
    }
}

impl OpParser for ProcedureRoot {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        let callee = parser.parse_symbol_ref()?;
        state.attrs.push(NamedAttribute::new("callee", callee.into_inner()));
        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;
        Ok(())
    }
}

/// Indirect same-context invocation through a slot of a
/// [midenc_hir::dialects::builtin::FunctionTable]; this is the op Wasm `call_indirect` lowers
/// to.
///
/// `index` is the table slot to dispatch through; lowering bounds-checks it against the table
/// size, computes the slot's memory address, and executes the procedure whose MAST root is
/// stored there via `dynexec`. No runtime check that the callee matches `signature` is
/// performed; only the bounds check traps deterministically.
#[operation(
    dialect = HirDialect,
    implements(
        CallOpInterface,
        InferTypeOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct ExecIndirect {
    /// The function table being indexed
    #[symbol]
    table: FunctionTable,
    /// The signature the call site expects of the callee
    #[attr(hidden)]
    signature: SignatureAttr,
    /// The table slot holding the callee's MAST root
    #[operand]
    index: UInt32,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for ExecIndirect {
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

impl OperandRangeRequirementOpInterface for ExecIndirect {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
    }
}

impl OpPrinter for ExecIndirect {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        printer.print_space();
        printer.print_symbol_path(self.get_table().path());
        {
            let index = self.index().as_value_ref();
            let index = index.borrow();
            *printer += const_text("[") + display(index.id()) + const_text("]");
        }
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

impl OpParser for ExecIndirect {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use midenc_hir::parse::{ParserError, Token};

        let table = parser.parse_symbol_ref()?;
        state.attrs.push(NamedAttribute::new("table", table.into_inner()));

        // The bracketed table-index operand
        parser.token_stream_mut().expect(Token::Lbracket)?;
        let index = parser.parse_operand(/*allow_result_number=*/ true)?;
        parser.token_stream_mut().expect(Token::Rbracket)?;

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
        if operands.len() != signature.arity() {
            return Err(ParserError::MismatchedValueAndTypeLists {
                span,
                num_values: operands.len(),
                num_types: signature.arity(),
            });
        }

        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;

        // Operand group 0: the u32 table index
        let mut index_values = SmallVec::default();
        parser.resolve_operands(
            state.span,
            core::slice::from_ref(&index),
            &[Type::U32],
            &mut index_values,
        )?;
        state.operands.push(index_values);

        // Operand group 1: the callee arguments, typed per the signature
        let type_params =
            signature.params().iter().map(|p| p.ty.clone()).collect::<SmallVec<[Type; 2]>>();
        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;
        state.operands.push(operand_values);

        Ok(())
    }
}

impl CallOpInterface for ExecIndirect {
    /// The callee is the table-index value: the function it names is only known at runtime.
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        Callable::Value(self.index().as_value_ref())
    }

    /// The callee of an indirect call is its table-index operand; rewriting it to a resolved
    /// symbol requires replacing the op (e.g. with `hir.exec`), which is left to a future
    /// devirtualization pass.
    fn set_callee(&mut self, _callable: Callable) {
        unimplemented!("hir.exec_indirect does not support replacing its callee")
    }

    #[inline(always)]
    fn arguments(&self) -> OpOperandRange<'_> {
        self.operands().group(1)
    }

    #[inline(always)]
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().group_mut(1)
    }

    fn resolve(&self) -> Option<SymbolRef> {
        None
    }

    fn resolve_in_symbol_table(&self, _symbols: &dyn SymbolTable) -> Option<SymbolRef> {
        None
    }
}

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
        let callable = resolved
            .as_trait_ref::<dyn CallableSymbol>()
            .expect("invalid callee: not a callable symbol");
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
    implements(
        CallOpInterface,
        InferTypeOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
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

impl OperandRangeRequirementOpInterface for Call {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
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

// NOTE: should a cross-context indirect call ever be needed, model it as a `CallIndirect` twin
// of `ExecIndirect` (table symbol + signature + u32 index operand), lowered via `dyncall`.
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
        let callable = resolved
            .as_trait_ref::<dyn CallableSymbol>()
            .expect("invalid callee: not a callable symbol");
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

// TODO(pauls): Validate that the arguments/results of the callee of this operation do not contain
// any types which are invalid for syscalls
#[operation(
    dialect = HirDialect,
    implements(
        CallOpInterface,
        InferTypeOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct Syscall {
    #[symbol(callable)]
    callee: SymbolPath,
    #[attr]
    signature: SignatureAttr,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for Syscall {
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

impl OperandRangeRequirementOpInterface for Syscall {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
    }
}

impl OpPrinter for Syscall {
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

impl CallOpInterface for Syscall {
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
        let callable = resolved
            .as_trait_ref::<dyn CallableSymbol>()
            .expect("invalid callee: not a callable symbol");
        Syscall::set_callee(self, callable).expect("invalid callee");
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
        CallOpInterface, SourceSpan, Symbol, SymbolTable, Type, Usable,
        conversion::{
            TypeConversion, TypeConverter, converted_resolved_call_signature_1_to_1,
            verify_call_signature_operands_and_results,
        },
        diagnostics::Uri,
        dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
        parse::{self, ParserConfig},
        testing::Test,
    };

    use crate::HirOpBuilder;

    #[test]
    fn exec_parser_resolves_operand_types_from_signature_params() {
        let test = Test::default();
        let source = r#"
builtin.module public @test {
    builtin.function private extern("C") @callee(%arg: i32) -> u64 {
        %result = builtin.unrealized_conversion_cast %arg <{ ty = #builtin.type<u64> }>;
        builtin.ret %result : (u64);
    };

    builtin.function public extern("C") @entrypoint(%arg: i32) -> u64 {
        %result = hir.exec @callee(%arg) : extern("C") (i32) -> u64;
        builtin.ret %result : (u64);
    };
};"#;

        parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("exec_parser_resolves_operand_types_from_signature_params.hir"),
            source,
        )
        .expect("hir.exec parser should type operands from signature params");
    }

    #[test]
    fn procedure_root_prints_and_reparses_with_intent_attribute() {
        use alloc::{format, vec::Vec};

        use midenc_hir::{Op, dialects::builtin::attributes::UnitAttr};

        let mut test = Test::named("procedure_root_prints_and_reparses_with_intent_attribute")
            .in_module("test");
        let callee = test.define_function("callee", &[], &[]);
        test.with_function("caller", &[], &[Type::Felt, Type::Felt, Type::Felt, Type::Felt]);
        // Give the callee a body: declaration-only functions do not survive a print/parse
        // round trip, and this test exercises exactly that round trip.
        {
            let mut callee_builder =
                midenc_hir::dialects::builtin::FunctionBuilder::new(callee, test.builder_mut());
            callee_builder.ret(None, SourceSpan::default()).unwrap();
        }

        let context = test.context_rc();
        {
            let mut builder = test.function_builder();
            let op = builder.procedure_root(callee, SourceSpan::default()).unwrap();
            {
                let mut op = op;
                let attr = context.create_attribute::<UnitAttr, _>(());
                op.borrow_mut()
                    .as_operation_mut()
                    .set_attribute(crate::ops::ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR, attr);
            }
            let results: Vec<_> = {
                let op = op.borrow();
                op.results().iter().map(|result| result.borrow().as_value_ref()).collect()
            };
            assert_eq!(results.len(), crate::ops::ProcedureRoot::DIGEST_FELTS);
            builder.ret(results, SourceSpan::default()).unwrap();
        }

        let printed = format!("{}", test.module().borrow().as_operation());
        assert!(
            printed.contains("hir.procedure_root"),
            "expected the printed module to contain the op: {printed}"
        );
        assert!(
            printed.contains(crate::ops::ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR),
            "expected the printed op to carry the intent attribute: {printed}"
        );

        // Re-parse in a fresh context: the printing context already owns the `@test` symbols.
        let reparse_context = Test::default().context_rc();
        parse::parse_any(
            ParserConfig::new(reparse_context),
            Uri::new("procedure_root_prints_and_reparses_with_intent_attribute.hir"),
            &printed,
        )
        .expect("printed hir.procedure_root should re-parse");
    }

    #[test]
    fn procedure_root_rejects_malformed_result_arity() {
        use alloc::format;

        use midenc_hir::{Op, traits::InferTypeOpInterface};

        let mut test =
            Test::named("procedure_root_rejects_malformed_result_arity").in_module("test");
        let callee = test.define_function("callee", &[], &[]);
        test.with_function("caller", &[], &[]);
        let context = test.context_rc();
        let mut op = {
            let mut builder = test.function_builder();
            let op = builder.procedure_root(callee, SourceSpan::default()).unwrap();
            builder.ret(None, SourceSpan::default()).unwrap();
            op
        };

        // Re-running inference retypes the existing results; an op left with the wrong result
        // count (by a transform or a parser) must be rejected instead of silently retyped, as
        // lowering models exactly one digest word on the operand stack.
        let mut op_mut = op.borrow_mut();
        let owner = op_mut.as_operation_ref();
        op_mut.op.results.clear();
        let lone_result = context.make_result(SourceSpan::default(), Type::Felt, owner, 0);
        op_mut.op.results.push(lone_result);
        let err = op_mut
            .infer_return_types(&context)
            .expect_err("inference must reject a result count differing from the digest width");
        assert!(format!("{err:?}").contains("expected 4 result"), "unexpected error: {err:?}");
    }

    #[test]
    fn conversion_helpers_resolve_and_convert_call_signatures() {
        let mut test =
            Test::named("conversion_helpers_resolve_and_convert_call_signatures").in_module("test");
        let callee = test.define_function("callee", &[Type::U32], &[Type::U32]);
        test.with_function("caller", &[Type::U32], &[]);

        let signature = Signature::new(&test.context_rc(), [Type::U32], [Type::U32]);
        let call = {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let arg = entry.borrow().arguments()[0].borrow().as_value_ref();
            builder.call(callee, signature, [arg], SourceSpan::default()).unwrap()
        };

        verify_call_signature_operands_and_results(call.as_operation_ref()).unwrap();

        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::One(Type::I32))
            } else {
                None
            }
        });
        let converted =
            converted_resolved_call_signature_1_to_1(call.as_operation_ref(), &converter)
                .unwrap()
                .expect("call should resolve to a callable signature");

        assert_eq!(converted.params()[0].ty, Type::I32);
        assert_eq!(converted.results()[0].ty, Type::I32);
    }

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

    #[test]
    fn call_op_interface_set_callee_resolves_callable_symbol_refs() {
        let mut test = Test::named("call_op_interface_set_callee_resolves_callable_symbol_refs")
            .in_module("test");
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

        let replacement_path = replacement.borrow().path();
        {
            let mut call_mut = call.borrow_mut();
            <crate::Call as CallOpInterface>::set_callee(
                &mut call_mut,
                replacement_path.clone().into(),
            );
        }

        let resolved = call.borrow().resolve().unwrap();
        assert_eq!(call.borrow().callee().path(), &replacement_path);
        assert_eq!(resolved.borrow().path(), replacement_path);
        assert_eq!(original.borrow().iter_uses().count(), 0);
        assert_eq!(replacement.borrow().iter_uses().count(), 1);
    }

    #[test]
    fn call_set_callee_relinks_symbol_use_after_old_symbol_is_removed_from_table() {
        let mut test = Test::named(
            "call_set_callee_relinks_symbol_use_after_old_symbol_is_removed_from_table",
        )
        .in_module("test");
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

        {
            let mut module = test.module().borrow_mut();
            let removed = module.remove("original".into());
            assert!(removed.is_some(), "expected the original symbol to be removed");
            assert!(module.get("original".into()).is_none());
        }

        assert_eq!(original.borrow().iter_uses().count(), 0);
        assert!(call.borrow().resolve().is_none());

        call.borrow_mut().set_callee(replacement).unwrap();

        let replacement_path = replacement.borrow().path();
        assert_eq!(call.borrow().callee().path(), &replacement_path);
        assert_eq!(original.borrow().iter_uses().count(), 0);
        assert_eq!(replacement.borrow().iter_uses().count(), 1);
    }

    #[test]
    fn syscall_set_callee_rebinds_property_backed_symbol_use() {
        let mut test =
            Test::named("syscall_set_callee_rebinds_property_backed_symbol_use").in_module("test");
        let original = test.define_function("original", &[], &[]);
        let replacement = test.define_function("replacement", &[], &[]);
        test.with_function("caller", &[], &[]);

        let signature = Signature::new(
            &test.context_rc(),
            core::iter::empty::<Type>(),
            core::iter::empty::<Type>(),
        );
        let mut syscall = {
            let mut builder = test.function_builder();
            let syscall = builder.syscall(original, signature, [], SourceSpan::default()).unwrap();
            builder.ret(None, SourceSpan::default()).unwrap();
            syscall
        };

        assert_eq!(original.borrow().iter_uses().count(), 1);
        assert_eq!(replacement.borrow().iter_uses().count(), 0);

        syscall.borrow_mut().set_callee(replacement).unwrap();

        let replacement_path = replacement.borrow().path();
        assert_eq!(syscall.borrow().callee().path(), &replacement_path);
        assert_eq!(original.borrow().iter_uses().count(), 0);
        assert_eq!(replacement.borrow().iter_uses().count(), 1);
    }
}
