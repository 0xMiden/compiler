use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::{
        FunctionTable,
        attributes::{LocalVariableArrayAttr, SignatureAttr, U32Attr},
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
///
/// A private callee may be referenced only from its defining symbol table, which lowers to the
/// same MASM module. Cross-module users must target a callee declared `internal` or `public`;
/// MASM legalization rejects a private cross-module target rather than widening its visibility.
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
/// size, asserts that the slot's signature tag equals `type_tag`, computes the slot's memory
/// address, and executes the procedure whose MAST root is stored there via `dynexec`. Both
/// checks trap deterministically; the tag check also traps for null slots, whose tag is the
/// reserved 0.
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
    /// The signature tag the call site expects of the callee; dispatch traps if the slot's tag
    /// differs (see [midenc_hir::dialects::builtin::FunctionTableEntry])
    #[attr(hidden)]
    type_tag: U32Attr,
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
        *printer += const_text(" tag ") + display(*self.get_type_tag());
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
        use midenc_hir::parse::{ParserError, ParserExt, Token};

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

        parser.parse_custom_keyword("tag")?;
        let type_tag = parser.parse_decimal_integer::<u32>()?.into_inner();
        state.add_attribute(
            "type_tag",
            parser.context_rc().create_attribute::<U32Attr, _>(type_tag),
        );

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

    /// The signature is the contract the lowering emits against — the stack shape pushed before
    /// `dynexec` and popped after — so it is the call site's answer even though no single
    /// callee is known.
    fn callee_signature(&self) -> Option<midenc_hir::dialects::builtin::attributes::Signature> {
        Some(self.get_signature().clone())
    }

    /// The possible callees are the table entries whose signature tag matches the tag this call
    /// site expects: dispatch to any other slot traps on the runtime signature check before the
    /// callee runs, so no other entry can observe the arguments or produce results. Later
    /// entries overwrite earlier ones at the same slot, so only the last entry per slot is
    /// dispatchable. Returns `None` (unknown) if the table or any dispatchable entry does not
    /// resolve, so analyses cannot treat a partially-resolved set as complete.
    fn possible_callees(&self) -> Option<SmallVec<[SymbolRef; 2]>> {
        let symbol_table_op = self.as_operation().nearest_symbol_table()?;
        let symbol_table_op = symbol_table_op.borrow();
        let symbol_table = symbol_table_op.as_symbol_table()?;
        let table = symbol_table.resolve(self.table().path())?;
        let table = table.borrow();
        let table = table.as_symbol_operation().downcast_ref::<FunctionTable>()?;

        // A malformed body is not something this analysis can answer for; the
        // `hir.exec_indirect` verifier is where it is reported.
        let live_entries = table.live_entries().ok()?;

        let expected_tag = *self.get_type_tag();
        let mut callees = SmallVec::new();
        for entry in live_entries.into_values() {
            let entry = entry.borrow();
            // The tag filter comes before resolution on purpose: an entry the runtime check
            // would reject cannot contribute a callee, so resolving it would be pure cost — and
            // tables are large enough (up to `1 << 20` slots) for that cost to matter.
            if *entry.get_type_tag() != expected_tag {
                continue;
            }
            // One unresolved dispatchable entry makes the whole set unknown: a partially
            // resolved set would be indistinguishable from a complete one to callers, and
            // would understate the call's effects. Valid IR cannot reach this — the
            // `hir.exec_indirect` verifier rejects a call whose tag-matching entry does not
            // resolve — so this is the malformed-IR path only.
            let callee = entry.resolve_callee()?;
            if !callees.contains(&callee) {
                callees.push(callee);
            }
        }
        Some(callees)
    }
}

/// `hir.exec_indirect` carries its callee's signature as an attribute rather than deriving it
/// from a resolved callee, so nothing but this verifier ties the operands to it. The emitter
/// consumes one operand per parameter and asserts each type, and its lowering cannot recover
/// from a mismatch — an under-supplied call pops an empty stack.
impl Verify<dyn CallOpInterface> for ExecIndirect {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        let signature = self.get_signature();
        let arguments = self
            .arguments()
            .iter()
            .map(|operand| operand.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();

        if arguments.len() != signature.params.len() {
            return Err(Report::msg(format!(
                "invalid hir.exec_indirect: the call signature declares {} parameter(s), but {} \
                 argument(s) were given",
                signature.params.len(),
                arguments.len()
            )));
        }

        for (index, (argument, param)) in arguments.iter().zip(signature.params.iter()).enumerate()
        {
            let argument_ty = argument.borrow().ty().clone();
            if argument_ty != param.ty {
                return Err(Report::msg(format!(
                    "invalid hir.exec_indirect: parameter {index} has type '{}', but the argument \
                     given has type '{argument_ty}'",
                    &param.ty
                )));
            }
        }

        // The tag is what the runtime check compares, and it is a producer-supplied integer:
        // nothing else ties it to a signature. If a table entry claims this call's tag while
        // its callee expects a different stack contract, the runtime check passes and
        // `dynexec` transfers control anyway — the type confusion the check exists to prevent.
        let expected_tag = *self.get_type_tag();
        if expected_tag == 0 {
            return Err(Report::msg(
                "invalid hir.exec_indirect: signature tag 0 is reserved for null table slots",
            ));
        }

        let table_path = self.table().path().clone();
        let Some(symbol_table_op) = self.as_operation().nearest_symbol_table() else {
            return Err(Report::msg(format!(
                "invalid hir.exec_indirect: '{table_path}' cannot be resolved outside a symbol \
                 table"
            )));
        };
        let symbol_table_op = symbol_table_op.borrow();
        let Some(table) = symbol_table_op
            .as_symbol_table()
            .and_then(|symbols| symbols.resolve(&table_path))
        else {
            return Err(Report::msg(format!(
                "invalid hir.exec_indirect: unable to resolve function table '{table_path}'"
            )));
        };
        let table = table.borrow();
        let Some(table) = table.as_symbol_operation().downcast_ref::<FunctionTable>() else {
            return Err(Report::msg(format!(
                "invalid hir.exec_indirect: '{table_path}' is not a 'builtin.function_table'"
            )));
        };

        // Only the last entry per slot is dispatchable; see `builtin::FunctionTable::live_entries`.
        let live_entries = table.live_entries().map_err(|op_name| {
            Report::msg(format!(
                "invalid hir.exec_indirect: this call dispatches through '{table_path}', whose \
                 body holds a '{op_name}' — only 'builtin.function_table_entry' is supported in a \
                 function table body"
            ))
        })?;

        for (slot, entry) in live_entries {
            let entry = entry.borrow();
            // Everything below resolves a symbol or clones a path, and this verifier runs once
            // per pass per call site over a table that may hold `1 << 20` entries — so the tag
            // filter, which is a pair of integer comparisons, goes first. An entry whose tag
            // differs cannot reach this call site: the runtime check traps before control
            // transfers, which is precisely why it is not this verifier's business.
            if *entry.get_type_tag() != expected_tag {
                continue;
            }
            let callee_ref = entry.callee();
            let callee_path = callee_ref.path();
            let Some(callee) = entry.resolve_callee() else {
                return Err(Report::msg(format!(
                    "invalid hir.exec_indirect: slot {slot} of '{table_path}' matches tag \
                     {expected_tag}, but its callee '{callee_path}' does not resolve"
                )));
            };
            let callee = callee.borrow();
            let Some(callable) = callee.as_symbol_operation().as_trait::<dyn CallableOpInterface>()
            else {
                return Err(Report::msg(format!(
                    "invalid hir.exec_indirect: slot {slot} of '{table_path}' names \
                     '{callee_path}', which is not callable"
                )));
            };
            let callee_signature = callable.signature();
            if callee_signature != *signature {
                return Err(Report::msg(format!(
                    "invalid hir.exec_indirect: this call dispatches through '{table_path}' with \
                     tag {expected_tag} and signature '{signature}', but slot {slot} holds \
                     '{callee_path}' with signature '{callee_signature}' — the runtime tag check \
                     would pass and transfer control with a mismatched stack contract"
                )));
            }
        }

        Ok(())
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
    use alloc::{
        format,
        string::{String, ToString},
    };

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        CallOpInterface, Operation, SourceSpan, Symbol, SymbolTable, Type, Usable,
        conversion::{
            TypeConversion, TypeConverter, converted_resolved_call_signature_1_to_1,
            verify_call_signature_operands_and_results,
        },
        diagnostics::Uri,
        dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
        parse::{self, ParserConfig},
        testing::Test,
    };

    use super::ExecIndirect;
    use crate::HirOpBuilder;

    /// Build a module with a one-slot table and a `dispatch` function whose `hir.exec_indirect`
    /// declares `signature_params` but passes `argument_count` `u32` constants, then verify it.
    ///
    /// Every argument is a `u32` constant, so a `signature_params` entry of any other type is
    /// how the mistyped case is built — no other constant builder is needed.
    fn verify_exec_indirect_with(
        signature_params: &[Type],
        argument_count: usize,
    ) -> Result<(), midenc_hir::Report> {
        use midenc_hir::{
            Ident, Op, ValueRef, Visibility,
            dialects::builtin::{ModuleBuilder, attributes::Signature},
        };

        let mut test = Test::named("verify_exec_indirect").in_module("m");
        test.with_function("dispatch", &[Type::U32], &[]);
        let table = ModuleBuilder::new(test.module())
            .define_function_table(Ident::from("tbl"), Visibility::Private, 1)
            .unwrap();
        let signature = Signature::new(&test.context_rc(), signature_params.to_vec(), []);
        {
            let mut builder = test.function_builder();
            let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let args = (0..argument_count)
                .map(|_| builder.u32(0, SourceSpan::UNKNOWN))
                .collect::<alloc::vec::Vec<_>>();
            builder
                .exec_indirect(table, signature, 1, index, args, SourceSpan::UNKNOWN)
                .unwrap();
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
        }

        test.module().borrow().as_operation().recursively_verify()
    }

    /// A signature declaring a parameter the call does not pass would pop an empty operand
    /// stack in the emitter; verification must reject it first.
    #[test]
    fn exec_indirect_with_too_few_arguments_fails_verification() {
        let err = verify_exec_indirect_with(&[Type::U32], 0)
            .expect_err("a missing argument must fail verification");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("1 parameter"), "{message}");
    }

    /// An argument whose type differs from its parameter reaches an `assert_eq!` in the
    /// emitter; verification must reject it first.
    #[test]
    fn exec_indirect_with_mistyped_argument_fails_verification() {
        let err = verify_exec_indirect_with(&[Type::I32], 1)
            .expect_err("a mistyped argument must fail verification");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("parameter 0"), "{message}");
    }

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
        // Keep a caller-owned context handle alive through verification and parsed-op destruction.
        parse::parse_any(
            ParserConfig::new(reparse_context.clone()),
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

    /// Parse `source`, which must contain exactly one `hir.exec_indirect`, and return the names
    /// of its possible callees, or `None` if the set is unknown.
    ///
    /// The fixtures reaching this helper are all valid IR, so parsing verifies them: an analysis
    /// answering for IR the verifier would reject proves nothing about what analyses actually
    /// see. The one deliberately malformed fixture goes through
    /// [`possible_callee_names_unverified`] instead.
    fn possible_callee_names(name: &str, source: &str) -> Option<alloc::vec::Vec<String>> {
        let test = Test::default();
        let parsed = parse::parse_any(ParserConfig::new(test.context_rc()), Uri::new(name), source)
            .expect("test module should parse and verify");
        possible_callee_names_of(parsed)
    }

    /// Like [`possible_callee_names`], but parses without verifying.
    ///
    /// This exists for the single fixture whose table entry does not resolve: that is precisely
    /// what the `hir.exec_indirect` verifier rejects (see
    /// `exec_indirect_with_an_unresolvable_entry_fails_verification`), so verifying would reject
    /// it before `possible_callees` ever ran. Every other fixture must verify.
    fn possible_callee_names_unverified(
        name: &str,
        source: &str,
    ) -> Option<alloc::vec::Vec<String>> {
        let test = Test::default();
        let parsed = parse::parse_any(
            ParserConfig::new(test.context_rc()).verify_after_parse(false),
            Uri::new(name),
            source,
        )
        .expect("test module should parse");
        possible_callee_names_of(parsed)
    }

    fn possible_callee_names_of(
        parsed: midenc_hir::OperationRef,
    ) -> Option<alloc::vec::Vec<String>> {
        let mut sets = alloc::vec::Vec::new();
        parsed.borrow().prewalk_all(|op| {
            if let Some(call) = op.downcast_ref::<ExecIndirect>() {
                sets.push(call.possible_callees().map(|callees| {
                    callees
                        .iter()
                        .map(|callee| callee.borrow().name().to_string())
                        .collect::<alloc::vec::Vec<_>>()
                }));
            }
        });
        assert_eq!(sets.len(), 1, "expected exactly one hir.exec_indirect in the test module");
        sets.pop().unwrap()
    }

    /// Wrap `entries` (function-table entry lines) and a `call_tag` dispatch in a module with
    /// three single-parameter callees `@a`, `@b`, and `@c`.
    fn table_module(entries: &str, call_tag: u32) -> String {
        format!(
            r#"
builtin.module public @test {{
    builtin.function internal extern("C") @a(%x: i32) -> i32 {{
        builtin.ret %x : (i32);
    }};
    builtin.function internal extern("C") @b(%x: i32) -> i32 {{
        builtin.ret %x : (i32);
    }};
    builtin.function internal extern("C") @c(%x: i32) -> i32 {{
        builtin.ret %x : (i32);
    }};

    builtin.function public extern("C") @dispatch(%idx: u32, %x: i32) -> i32 {{
        %r = hir.exec_indirect @tbl[%idx](%x) : extern("C") (i32) -> (i32) tag {call_tag};
        builtin.ret %r : (i32);
    }};

    builtin.function_table private @tbl : 4 {{
{entries}
    }};
}};"#
        )
    }

    /// Entries whose tag matches the call site are dispatchable; others are filtered, since they
    /// can only trap on the runtime signature check.
    #[test]
    fn exec_indirect_possible_callees_filters_by_type_tag() {
        let source = table_module(
            "        builtin.function_table_entry 0 @a tag 1;\n        \
             builtin.function_table_entry 1 @b tag 2;\n        builtin.function_table_entry 2 @c \
             tag 1;",
            1,
        );
        let callees = possible_callee_names("possible_callees_filter.hir", &source);
        assert_eq!(callees, Some(alloc::vec!["a".to_string(), "c".to_string()]));
    }

    /// A later entry overwrites an earlier one at the same slot, so only the last entry is a
    /// possible callee.
    #[test]
    fn exec_indirect_possible_callees_keeps_only_last_entry_per_slot() {
        let source = table_module(
            "        builtin.function_table_entry 0 @a tag 1;\n        \
             builtin.function_table_entry 0 @b tag 1;",
            1,
        );
        let callees = possible_callee_names("possible_callees_overwrite.hir", &source);
        assert_eq!(callees, Some(alloc::vec!["b".to_string()]));
    }

    /// A callee referenced from several slots appears once in the possible-callee set.
    #[test]
    fn exec_indirect_possible_callees_dedups_repeated_callees() {
        let source = table_module(
            "        builtin.function_table_entry 0 @a tag 1;\n        \
             builtin.function_table_entry 1 @a tag 1;",
            1,
        );
        let callees = possible_callee_names("possible_callees_dedup.hir", &source);
        assert_eq!(callees, Some(alloc::vec!["a".to_string()]));
    }

    /// When no entry matches the call site's tag, the possible-callee set is known and empty:
    /// every dispatch traps.
    #[test]
    fn exec_indirect_possible_callees_empty_when_no_tag_matches() {
        let source = table_module("        builtin.function_table_entry 0 @a tag 2;", 1);
        let callees = possible_callee_names("possible_callees_empty.hir", &source);
        assert_eq!(callees, Some(alloc::vec![]));
    }

    /// An entry's callee path belongs to the entry, so a relative path must resolve from the
    /// module holding the *table*, not the module holding the call. Both modules here define a
    /// `@target`, and only the one beside the table is dispatchable.
    #[test]
    fn possible_callees_resolves_entries_from_their_own_symbol_table() {
        let test = Test::default();
        let source = r#"
builtin.world {
builtin.module public @a {
    builtin.function internal extern("C") @target(%x: i32) -> i32 {
        builtin.ret %x : (i32);
    };

    builtin.function public extern("C") @dispatch(%idx: u32, %x: i32) -> i32 {
        %r = hir.exec_indirect ::@b::@tbl[%idx](%x) : extern("C") (i32) -> (i32) tag 1;
        builtin.ret %r : (i32);
    };
};

builtin.module public @b {
    builtin.function internal extern("C") @target(%x: i32) -> i32 {
        builtin.ret %x : (i32);
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @target tag 1;
    };
};
};"#;

        // Parsing verifies, so this also asserts that a cross-module indirect call agreeing with
        // its table is valid IR.
        let world = parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("possible_callees_resolves_entries_from_their_own_symbol_table.hir"),
            source,
        )
        .expect("a cross-module indirect call agreeing with its table must parse and verify");

        let mut resolved = String::new();
        world.borrow().prewalk_all(|op: &Operation| {
            if let Some(call) = op.downcast_ref::<ExecIndirect>() {
                let callees = call.possible_callees().expect("targets should be known");
                assert_eq!(callees.len(), 1, "one entry matches the call's tag");
                resolved = callees[0].borrow().path().to_string();
            }
        });

        assert_eq!(
            resolved, "b/target",
            "the entry's relative path must resolve beside the table, not beside the call"
        );
    }

    /// An entry whose callee does not resolve makes the whole set unknown, so analyses cannot
    /// treat a partially-resolved set as complete.
    #[test]
    fn exec_indirect_possible_callees_unknown_when_entry_unresolvable() {
        let source = table_module("        builtin.function_table_entry 0 @missing tag 1;", 1);
        let callees =
            possible_callee_names_unverified("possible_callees_unresolvable.hir", &source);
        assert_eq!(callees, None);
    }

    /// Verify the parsed world in `source`, returning the verification result.
    fn verify_world(name: &str, source: &str) -> Result<(), midenc_hir::Report> {
        let test = Test::default();
        let world = parse::parse_any(ParserConfig::new(test.context_rc()), Uri::new(name), source)?;
        world.borrow().recursively_verify()
    }

    /// The runtime check compares integers only, so a tag that claims a signature the callee
    /// does not have would dispatch with a mismatched stack contract. The tag/signature mapping
    /// must therefore be verified, not trusted.
    #[test]
    fn exec_indirect_with_a_forged_signature_tag_fails_verification() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @nullary() {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @nullary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32, %arg: u32) {
        hir.exec_indirect @tbl[%index](%arg) : extern("C") (u32) -> () tag 1;
        builtin.ret;
    };
};
};"#;

        let err = verify_world("forged_signature_tag.hir", source)
            .expect_err("a tag claiming a signature its callee does not have must be rejected");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("tag 1"), "{message}");
        assert!(message.contains("nullary"), "{message}");
    }

    /// The agreeing case must keep verifying: this is the shape the Wasm frontend emits.
    #[test]
    fn exec_indirect_agreeing_with_its_table_verifies() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @unary(%a: u32) {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @unary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32, %arg: u32) {
        hir.exec_indirect @tbl[%index](%arg) : extern("C") (u32) -> () tag 1;
        builtin.ret;
    };
};
};"#;

        verify_world("agreeing_signature_tag.hir", source)
            .expect("a call whose tag matches its entry's callee signature must verify");
    }

    /// A tag-matching entry whose callee does not resolve makes the dispatchable set unknowable;
    /// `possible_callees` answers `None` for it, so analyses would silently lose the call.
    #[test]
    fn exec_indirect_with_an_unresolvable_entry_fails_verification() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @missing tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32) {
        hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 1;
        builtin.ret;
    };
};
};"#;

        let err = verify_world("unresolvable_entry.hir", source)
            .expect_err("an unresolvable tag-matching entry must be rejected");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("missing"), "{message}");
    }

    /// Absolute symbol paths resolve from the root, so a verifier that resolves one only sees the
    /// right answer once the parsed world is the root. `parse_anchored_source` parses into a
    /// synthetic anchor world and detaches the parsed world from it, and it used to verify while
    /// the parsed world was still nested — under which `::@m::@tbl` names a grandchild of the
    /// root rather than a child, and this verifier rejected it.
    ///
    /// This is the shape the Wasm frontend prints, so it is also the shape `midenc compile
    /// --emit hir | midenc compile` has to be able to read back.
    #[test]
    fn exec_indirect_naming_its_table_by_absolute_path_parses_and_verifies() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @unary(%a: u32) {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 ::@m::@unary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32, %arg: u32) {
        hir.exec_indirect ::@m::@tbl[%index](%arg) : extern("C") (u32) -> () tag 1;
        builtin.ret;
    };
};
};"#;

        let test = Test::default();
        parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("exec_indirect_absolute_table_path.hir"),
            source,
        )
        .expect("an indirect call naming its table by absolute path must parse and verify");
    }

    /// Only the last entry at a slot can be reached, so a dead entry disagreeing with the call
    /// must not be checked — otherwise valid IR is rejected for an entry no dispatch can hit.
    #[test]
    fn exec_indirect_ignores_an_overwritten_mismatched_entry() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @nullary() {
        builtin.ret;
    };

    builtin.function private extern("C") @unary(%a: u32) {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @nullary tag 1;
        builtin.function_table_entry 0 @unary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32, %arg: u32) {
        hir.exec_indirect @tbl[%index](%arg) : extern("C") (u32) -> () tag 1;
        builtin.ret;
    };
};
};"#;

        verify_world("overwritten_mismatched_entry.hir", source).expect(
            "an entry overwritten at the same slot is not dispatchable, so it must not be checked",
        );
    }

    /// The mirror of the above: overwriting an agreeing entry with a disagreeing one at the same
    /// slot makes the disagreeing one the dispatchable one, so the call must be rejected.
    #[test]
    fn exec_indirect_checks_the_entry_that_overwrote_a_matching_one() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @nullary() {
        builtin.ret;
    };

    builtin.function private extern("C") @unary(%a: u32) {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @unary tag 1;
        builtin.function_table_entry 0 @nullary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32, %arg: u32) {
        hir.exec_indirect @tbl[%index](%arg) : extern("C") (u32) -> () tag 1;
        builtin.ret;
    };
};
};"#;

        let err = verify_world("overwriting_mismatched_entry.hir", source)
            .expect_err("the entry that overwrote the matching one is the dispatchable one");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("nullary"), "{message}");
    }

    /// Tag 0 marks a null table slot, so a call claiming it would dispatch to an empty slot: the
    /// runtime check compares integers and cannot tell the difference.
    #[test]
    fn exec_indirect_with_the_reserved_tag_zero_fails_verification() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function private extern("C") @nullary() {
        builtin.ret;
    };

    builtin.function_table private @tbl : 2 {
        builtin.function_table_entry 0 @nullary tag 1;
    };

    builtin.function public extern("C") @dispatch(%index: u32) {
        hir.exec_indirect @tbl[%index] : extern("C") () -> () tag 0;
        builtin.ret;
    };
};
};"#;

        let err = verify_world("reserved_tag_zero.hir", source)
            .expect_err("tag 0 is reserved for null table slots and must be rejected");
        let message = format!("{err}");
        assert!(message.contains("hir.exec_indirect"), "{message}");
        assert!(message.contains("reserved for null table slots"), "{message}");
    }
}
