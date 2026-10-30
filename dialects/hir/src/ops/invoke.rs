use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, operation},
    dialects::builtin::{
        FunctionTable,
        attributes::{Signature, SignatureAttr, U32Attr},
    },
    effects::*,
    interner::symbols,
    print::AsmPrinter,
    traits::*,
    *,
};

use crate::HirDialect;

/// Declare one result per result of `signature` on `op`, in signature order.
///
/// Call ops carry the callee signature as an attribute, so this is what inferring their result
/// types amounts to.
pub(super) fn infer_results_from_signature(
    op: &mut Operation,
    signature: &Signature,
    context: &Context,
) -> Result<(), Report> {
    let span = op.span;
    let owner = op.as_operation_ref();
    for (i, result) in signature.results().iter().enumerate() {
        let value = context.make_result(span, result.ty.clone(), owner, i as u16);
        op.results.push(value);
    }
    Ok(())
}

/// Verify that `arguments` agrees with the parameters of `signature` in count and type, naming
/// the op as `op_name` in the diagnostics.
///
/// The indirect call ops carry their callee's signature as an attribute rather than deriving it
/// from a resolved callee, so nothing but this ties their operands to it.
pub(super) fn verify_arguments_against_signature(
    op_name: &str,
    arguments: OpOperandRange<'_>,
    signature: &Signature,
) -> Result<(), Report> {
    let arguments = arguments
        .iter()
        .map(|operand| operand.borrow().as_value_ref())
        .collect::<SmallVec<[_; 4]>>();

    if arguments.len() != signature.params.len() {
        return Err(Report::msg(format!(
            "invalid {op_name}: the call signature declares {} parameter(s), but {} argument(s) \
             were given",
            signature.params.len(),
            arguments.len()
        )));
    }

    for (index, (argument, param)) in arguments.iter().zip(signature.params.iter()).enumerate() {
        let argument_ty = argument.borrow().ty().clone();
        if argument_ty != param.ty {
            return Err(Report::msg(format!(
                "invalid {op_name}: parameter {index} has type '{}', but the argument given has \
                 type '{argument_ty}'",
                param.ty
            )));
        }
    }

    Ok(())
}

/// The `(arguments) : signature` tail as [parse_arguments_and_signature] returns it: the
/// unresolved arguments, and the parameter types to resolve them against.
type ParsedArgumentsAndSignature = (SmallVec<[parse::UnresolvedOperand; 2]>, SmallVec<[Type; 2]>);

/// Parse the `(arguments) : signature` tail shared by the call ops, recording the parsed
/// signature as the `signature` attribute of `state`.
///
/// Returns the unresolved arguments along with the parameter types to resolve them against,
/// having checked that the two agree in count. The caller resolves them, so that ops with
/// further operand groups can push those groups in their declared order.
pub(super) fn parse_arguments_and_signature(
    state: &mut OperationState,
    parser: &mut dyn OpAsmParser<'_>,
) -> ParseResult<ParsedArgumentsAndSignature> {
    use midenc_hir::parse::ParserError;

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
                "expected 'signature' property to be of type #builtin.signature, got '{}' instead",
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

    let type_params =
        signature.params().iter().map(|p| p.ty.clone()).collect::<SmallVec<[Type; 2]>>();
    Ok((operands, type_params))
}

/// Print the `(arguments) : signature` suffix shared by the call ops.
pub(super) fn print_arguments_and_signature(
    printer: &mut AsmPrinter<'_>,
    arguments: OpOperandRange<'_>,
    signature: &SignatureAttr,
) {
    printer.print_operand_list(arguments);
    *printer += formatter::const_text(" : ");
    signature.print(printer);
}

/// Print the trailing `attributes { .. }` dictionary of `op`, if it has any attributes.
pub(super) fn print_optional_attributes(printer: &mut AsmPrinter<'_>, op: &Operation) {
    if op.has_attributes() {
        printer.print_space();
        *printer += formatter::const_text(" attributes ");
        printer.print_attribute_dictionary(
            op.attributes().iter().map(|attr| *attr.as_named_attribute()),
        );
    }
}

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
        let signature = self.signature.borrow();
        infer_results_from_signature(&mut self.op, &signature, context)
    }
}

impl OperandRangeRequirementOpInterface for Exec {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
    }
}

impl OpPrinter for Exec {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        let callee = self.callee();
        printer.print_space();
        printer.print_symbol_path(callee.path());
        print_arguments_and_signature(printer, self.arguments(), &self.signature());
        print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for Exec {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        let callee = parser.parse_symbol_ref()?;

        state.attrs.push(NamedAttribute::new("callee", callee.into_inner()));

        let (operands, type_params) = parse_arguments_and_signature(state, parser)?;

        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;

        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;

        state.operands.push(operand_values);

        Ok(())
    }
}

/// Invoke a foreign account procedure via the transaction kernel FPI executor.
///
/// Its six `prefix` operands are felts in executor order: account id suffix, account id
/// prefix, and four procedure-root elements. `inputs` contains at most sixteen flattened
/// procedure input felts. Every operand is a semantic use; storage and stack preparation are
/// assigned by the backend. The executor returns sixteen felts.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct ExecFpi {
    #[operands]
    prefix: IntFelt,
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
    /// Number of semantic executor prefix operands.
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

impl OpPrinter for ExecFpi {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        printer.print_space();
        *printer += const_text("[");
        printer.print_value_uses(self.prefix().as_value_range());
        *printer += const_text("](");
        printer.print_value_uses(self.inputs().as_value_range());
        *printer += const_text(")");
        print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for ExecFpi {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        // Explicit delimiters retain both semantic groups, including an empty payload.
        for delimiter in [parse::Delimiter::Bracket, parse::Delimiter::Paren] {
            let mut operands = SmallVec::default();
            parser.parse_operand_list(&mut operands, delimiter, true, None)?;
            let types = alloc::vec![Type::Felt; operands.len()];
            let mut values = SmallVec::default();
            parser.resolve_operands(state.span, &operands, &types, &mut values)?;
            state.operands.push(values);
        }
        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;
        Ok(())
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

        if self.prefix().len() != Self::PREFIX_FELTS {
            return Err(Report::msg(format!(
                "invalid hir.exec_fpi: expected {} prefix operand(s), but got {}",
                Self::PREFIX_FELTS,
                self.prefix().len(),
            )));
        }
        for (index, operand) in self.prefix().iter().chain(self.inputs().iter()).enumerate() {
            let ty = operand.borrow().as_value_ref().borrow().ty().clone();
            if ty != Type::Felt {
                return Err(Report::msg(format!(
                    "invalid hir.exec_fpi: operand {index} must be felt, but got {ty}",
                )));
            }
        }
        if !self.op.results.is_empty() && self.op.results.len() != Self::EXECUTOR_RESULT_FELTS {
            return Err(Report::msg(format!(
                "invalid hir.exec_fpi: expected {} result(s), but got {}",
                Self::EXECUTOR_RESULT_FELTS,
                self.op.results.len(),
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..Self::EXECUTOR_RESULT_FELTS {
                let value = context.make_result(span, Type::Felt, owner, i as u16);
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
///
/// If `callee` names a `FunctionAlias`, the alias's own visibility applies.
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
                let value = context.make_result(span, Type::Felt, owner, i as u16);
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
        printer.print_space();
        let callee = self.callee();
        printer.print_symbol_path(callee.path());
        print_optional_attributes(printer, &self.op);
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

/// Materialize a callable's MAST root as one first-class four-felt word.
///
/// Linkability, aliases, and note-script intent follow [`ProcedureRoot`]. The single result
/// retains its word identity while occupying the same four operand-stack lanes.
#[derive(EffectOpInterface)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Procref {
    #[symbol(callable)]
    callee: SymbolPath,
    #[result]
    result: AnyArray,
}

impl Procref {
    pub fn word_type() -> Type {
        Type::from(ArrayType::new(Type::Felt, ProcedureRoot::DIGEST_FELTS))
    }
}

impl InferTypeOpInterface for Procref {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        if self.op.results.is_empty() {
            let value =
                context.make_result(self.span(), Self::word_type(), self.as_operation_ref(), 0);
            self.op.results.push(value);
        } else if self.op.results.len() != 1 {
            return Err(Report::msg("invalid hir.procref: expected one four-felt word result"));
        } else {
            self.result_mut().set_type(Self::word_type());
        }
        Ok(())
    }
}

impl Verify<dyn MemoryEffectOpInterface> for Procref {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        if self.num_results() != 1 || self.result().ty() != &Self::word_type() {
            return Err(Report::msg("invalid hir.procref: expected one four-felt word result"));
        }
        Ok(())
    }
}

impl OpPrinter for Procref {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_space();
        printer.print_symbol_path(self.callee().path());
        print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for Procref {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        ProcedureRoot::parse(state, parser)
    }
}

/// Execute a root read from source memory in the caller's context.
///
/// `callee` is an element pointer to one four-felt word. The signature describes the unknown
/// procedure's arguments and results; it does not establish that the runtime root matches it.
#[operation(
    name = "dynexec",
    dialect = HirDialect,
    implements(CallOpInterface, InferTypeOpInterface, OperandRangeRequirementOpInterface, OpPrinter)
)]
pub struct DynExec {
    #[attr(hidden)]
    signature: SignatureAttr,
    #[operand]
    callee: AnyPointer,
    #[operands]
    arguments: AnyType,
}

impl DynExec {
    /// The implicit source-memory read consumes exactly one aligned digest word.
    pub fn root_pointer_type() -> Type {
        Type::from(PointerType::new_with_address_space(Procref::word_type(), AddressSpace::Element))
    }
}

/// Invoke a root read from the caller's source memory in a fresh context.
///
/// The root pointer is separate from the sixteen-lane argument window. Only a component-model
/// signature can describe the new context's operand-stack boundary.
#[operation(
    name = "dyncall_local",
    dialect = HirDialect,
    implements(CallOpInterface, InferTypeOpInterface, OperandRangeRequirementOpInterface, OpPrinter)
)]
pub struct DynCallLocal {
    #[attr(hidden)]
    signature: SignatureAttr,
    #[operand]
    callee: AnyPointer,
    #[operands]
    arguments: AnyType,
}

// Both forms share the same source contract; only their permitted execution context differs.
macro_rules! pointer_call_interfaces {
    ($ty:ident, $context:expr) => {
        impl InferTypeOpInterface for $ty {
            fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
                let signature = self.signature.borrow();
                infer_results_from_signature(&mut self.op, &signature, context)
            }
        }

        impl OperandRangeRequirementOpInterface for $ty {
            fn operand_range_requirement(&self, _index: usize) -> OperandRangeRequirement {
                OperandRangeRequirement::None
            }
        }

        impl OpPrinter for $ty {
            fn print(&self, printer: &mut AsmPrinter<'_>) {
                printer.print_space();
                *printer += formatter::display(self.callee().as_value_ref().borrow().id());
                print_arguments_and_signature(printer, self.arguments(), &self.signature());
                print_optional_attributes(printer, &self.op);
            }
        }

        impl OpParser for $ty {
            fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
                let callee = parser.parse_operand(true)?;
                let (arguments, types) = parse_arguments_and_signature(state, parser)?;
                parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;
                let mut values = SmallVec::default();
                parser.resolve_operands(
                    state.span,
                    &[callee],
                    &[DynExec::root_pointer_type()],
                    &mut values,
                )?;
                state.operands.push(values);
                let mut values = SmallVec::default();
                parser.resolve_operands(state.span, &arguments, &types, &mut values)?;
                state.operands.push(values);
                Ok(())
            }
        }

        impl CallOpInterface for $ty {
            fn callable_for_callee(&self) -> Callable {
                Callable::Value(self.callee().as_value_ref())
            }

            fn set_callee(&mut self, callable: Callable) {
                let Callable::Value(value) = callable else {
                    panic!("a pointer call requires an SSA callee");
                };
                self.callee_mut().set(value);
            }

            fn arguments(&self) -> OpOperandRange<'_> {
                self.operands().group(1)
            }

            fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
                self.operands_mut().group_mut(1)
            }

            fn resolve(&self) -> Option<CanonicalCallableRef> {
                None
            }

            fn resolve_in_symbol_table(
                &self,
                _symbols: &dyn SymbolTable,
            ) -> Option<CanonicalCallableRef> {
                None
            }

            fn callee_signature(&self) -> Option<Signature> {
                Some(self.get_signature().clone())
            }

            fn possible_callees(&self) -> Option<SmallVec<[CanonicalCallableRef; 2]>> {
                None
            }
        }

        impl Verify<dyn CallOpInterface> for $ty {
            fn verify(&self, _context: &Context) -> Result<(), Report> {
                verify_pointer_call(
                    self.as_operation(),
                    self.callee().ty(),
                    self.arguments(),
                    &self.get_signature(),
                    $context,
                )
            }
        }
    };
}

fn verify_pointer_call(
    op: &Operation,
    pointer: Type,
    arguments: OpOperandRange<'_>,
    signature: &Signature,
    context: bool,
) -> Result<(), Report> {
    let name = format!("{}", op.name());
    if pointer != DynExec::root_pointer_type() {
        return Err(Report::msg(format!(
            "invalid {name}: callee must be an element pointer to a four-felt word"
        )));
    }
    if (signature.cc == CallConv::ComponentModel) != context {
        return Err(Report::msg(format!("invalid {name}: incompatible calling convention")));
    }
    verify_arguments_against_signature(&name, arguments, signature)?;
    if op.num_results() != signature.results().len()
        || op
            .results()
            .iter()
            .zip(signature.results())
            .any(|(result, abi)| result.borrow().ty() != &abi.ty)
    {
        return Err(Report::msg(format!("invalid {name}: results disagree with call signature")));
    }
    Ok(())
}

pointer_call_interfaces!(DynExec, false);
pointer_call_interfaces!(DynCallLocal, true);

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
        let signature = self.signature.borrow();
        infer_results_from_signature(&mut self.op, &signature, context)
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
        print_arguments_and_signature(printer, self.arguments(), &self.signature());
        *printer += const_text(" tag ") + display(*self.get_type_tag());
        print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for ExecIndirect {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use midenc_hir::parse::{ParserExt, Token};

        let table = parser.parse_symbol_ref()?;
        state.attrs.push(NamedAttribute::new("table", table.into_inner()));

        // The bracketed table-index operand
        parser.token_stream_mut().expect(Token::Lbracket)?;
        let index = parser.parse_operand(/*allow_result_number=*/ true)?;
        parser.token_stream_mut().expect(Token::Rbracket)?;

        let (operands, type_params) = parse_arguments_and_signature(state, parser)?;

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
        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;
        state.operands.push(operand_values);

        Ok(())
    }
}

impl CallOpInterface for ExecIndirect {
    fn resolve(&self) -> Option<CanonicalCallableRef> {
        None
    }

    fn resolve_in_symbol_table(&self, _symbols: &dyn SymbolTable) -> Option<CanonicalCallableRef> {
        None
    }

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
    fn possible_callees(&self) -> Option<SmallVec<[CanonicalCallableRef; 2]>> {
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
            let callee = entry.resolve_callable().ok()?.target();
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
        verify_arguments_against_signature("hir.exec_indirect", self.arguments(), &signature)?;

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
            let callee = entry.resolve_callable().map_err(|err| {
                Report::msg(format!(
                    "invalid hir.exec_indirect: slot {slot} of '{table_path}' matches tag \
                     {expected_tag}, but its callee '{callee_path}' does not resolve to a \
                     callable: {err}"
                ))
            })?;
            let callee_signature = callee.signature();
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
        let signature = self.signature.borrow();
        infer_results_from_signature(&mut self.op, &signature, context)
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
        print_optional_attributes(printer, &self.op);
    }
}

// The cross-context indirect twin of this op is `Dyncall` below: the callee is a runtime MAST
// root rather than a symbol, lowered via `dyncall`.
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
}

/// Invoke the procedure whose MAST root is `root` in a new execution context (`dyncall`).
///
/// This is the twin of [`Call`] whose callee is a runtime value rather than a symbol: `root` holds
/// the callee's MAST root as four felts, element 0 first, typically read from an account storage
/// slot populated off-chain with a sibling component's procedure root. As with `hir.call`, the
/// callee runs in a fresh context under `signature`, which must use the component-model calling
/// convention: arguments are passed and results returned on the operand stack only. The MASM
/// lowering stages the root in an aligned scratch word before preparing arguments. The root
/// does not count against the sixteen-felt argument window; both argument and result tuples
/// are bounded at sixteen felts by the context-call ABI.
///
/// The signature is the contract the lowering emits against, exactly as for [`ExecIndirect`];
/// nothing ties it to the procedure the root names. A root that names no procedure of the
/// assembled program, or one with a different stack contract, traps in the VM or yields wrong
/// results, but cannot corrupt the caller's context. The callee set is unknown to analyses, which
/// treat the op as an external call.
#[operation(
    dialect = HirDialect,
    implements(
        CallOpInterface,
        InferTypeOpInterface,
        OperandRangeRequirementOpInterface,
        OpPrinter
    )
)]
pub struct Dyncall {
    /// The signature the call site expects of the callee
    #[attr(hidden)]
    signature: SignatureAttr,
    /// The callee's MAST root, element 0 first
    #[operands]
    root: IntFelt,
    /// The arguments to pass to the callee, one per parameter of `signature`
    #[operands]
    arguments: AnyType,
}

impl Dyncall {
    /// Number of felt operands forming the callee's MAST root: one digest word.
    pub const ROOT_FELTS: usize = ProcedureRoot::DIGEST_FELTS;
}

impl InferTypeOpInterface for Dyncall {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let signature = self.signature.borrow();
        infer_results_from_signature(&mut self.op, &signature, context)
    }
}

impl OperandRangeRequirementOpInterface for Dyncall {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        OperandRangeRequirement::None
    }
}

impl OpPrinter for Dyncall {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use formatter::*;

        printer.print_space();
        *printer += const_text("[");
        printer.print_value_uses(self.root().as_value_range());
        *printer += const_text("]");
        print_arguments_and_signature(printer, self.arguments(), &self.signature());
        print_optional_attributes(printer, &self.op);
    }
}

impl OpParser for Dyncall {
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult {
        use midenc_hir::parse::ParserError;

        // The bracketed root word
        let mut root = SmallVec::default();
        parser.parse_operand_list(
            &mut root,
            parse::Delimiter::Bracket,
            /*allow_result_number=*/ true,
            None,
        )?;

        let (operands, type_params) = parse_arguments_and_signature(state, parser)?;

        // The root word is fixed-size; like the argument arity checked above, a mismatch is
        // reported against the op parsed so far
        if root.len() != Self::ROOT_FELTS {
            let span = SourceSpan::new(
                state.span.source_id(),
                state.span.start()..parser.current_location().end(),
            );
            return Err(ParserError::MismatchedValueAndTypeLists {
                span,
                num_values: root.len(),
                num_types: Self::ROOT_FELTS,
            });
        }

        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;

        // Operand group 0: the root word
        let mut root_values = SmallVec::default();
        let root_types = [const { Type::Felt }; Self::ROOT_FELTS];
        parser.resolve_operands(state.span, &root, &root_types, &mut root_values)?;
        state.operands.push(root_values);

        // Operand group 1: the callee arguments, typed per the signature
        let mut operand_values = SmallVec::default();
        parser.resolve_operands(state.span, &operands, &type_params, &mut operand_values)?;
        state.operands.push(operand_values);

        Ok(())
    }
}

impl CallOpInterface for Dyncall {
    /// The callee is the root word: the procedure it names is only known at runtime. The first
    /// root felt stands in for the word, as a callable can name a single value.
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        Callable::Value(self.root()[0].borrow().as_value_ref())
    }

    /// The callee of a dynamic call is its root operand; rewriting it to a resolved symbol
    /// requires replacing the op (e.g. with `hir.call`), which no pass does today.
    fn set_callee(&mut self, _callable: Callable) {
        unimplemented!("hir.dyncall does not support replacing its callee")
    }

    #[inline(always)]
    fn arguments(&self) -> OpOperandRange<'_> {
        self.operands().group(1)
    }

    #[inline(always)]
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().group_mut(1)
    }

    fn resolve(&self) -> Option<CanonicalCallableRef> {
        None
    }

    fn resolve_in_symbol_table(&self, _symbols: &dyn SymbolTable) -> Option<CanonicalCallableRef> {
        None
    }

    /// The signature is the contract the lowering emits against — the stack shape pushed before
    /// `dyncall` and popped after — so it is the call site's answer even though no callee is
    /// known.
    fn callee_signature(&self) -> Option<midenc_hir::dialects::builtin::attributes::Signature> {
        Some(self.get_signature().clone())
    }

    /// The root is runtime data (it need not even name a procedure of this program), so the
    /// callee set is unknown: analyses must treat this op as an external call.
    fn possible_callees(&self) -> Option<SmallVec<[CanonicalCallableRef; 2]>> {
        None
    }
}

/// `hir.dyncall` carries its callee's signature as an attribute rather than deriving it from a
/// resolved callee, so nothing but this verifier ties the operands to it. The emitter consumes one
/// operand per parameter and asserts each type, and its lowering cannot recover from a mismatch.
impl Verify<dyn CallOpInterface> for Dyncall {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        let num_root_felts = self.root().len();
        if num_root_felts != Self::ROOT_FELTS {
            return Err(Report::msg(format!(
                "invalid hir.dyncall: expected {} root felt operand(s), but got {num_root_felts}",
                Self::ROOT_FELTS
            )));
        }

        let signature = self.get_signature();
        // The callee runs in a new context, where only operand-stack arguments and results can
        // cross; the component-model convention is the one that guarantees that shape.
        if signature.cc != CallConv::ComponentModel {
            return Err(Report::msg(format!(
                "invalid hir.dyncall: the call signature must use the component-model calling \
                 convention, but uses '{}'",
                signature.cc
            )));
        }
        verify_arguments_against_signature("hir.dyncall", self.arguments(), &signature)
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
        let signature = self.signature.borrow();
        infer_results_from_signature(&mut self.op, &signature, context)
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
        print_optional_attributes(printer, &self.op);
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
}

#[cfg(test)]
mod tests {
    use alloc::{
        format,
        string::{String, ToString},
    };

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        CallConv, CallOpInterface, Ident, Op, Operation, SourceSpan, Symbol, SymbolTable, Type,
        UnsafeIntrusiveEntityRef, Usable, ValueRef, Visibility,
        conversion::{
            TypeConversion, TypeConverter, converted_resolved_call_signature_1_to_1,
            verify_call_signature_operands_and_results,
        },
        diagnostics::Uri,
        dialects::builtin::{BuiltinOpBuilder, ModuleBuilder, attributes::Signature},
        parse::{self, ParserConfig},
        testing::Test,
    };

    use super::{Dyncall, Exec, ExecIndirect};
    use crate::HirOpBuilder;

    #[test]
    fn semantic_exec_fpi_roundtrips_full_width_and_empty_payloads() {
        for payload_width in [0, super::ExecFpi::MAX_INPUT_FELTS] {
            let mut test = Test::new("fpi_roundtrip", &alloc::vec![Type::Felt; 6], &[]);
            {
                let mut builder = test.function_builder();
                let args = builder
                    .entry_block()
                    .borrow()
                    .arguments()
                    .iter()
                    .map(|arg| *arg as ValueRef)
                    .collect::<alloc::vec::Vec<_>>();
                let prefix: [ValueRef; 6] = args.as_slice().try_into().unwrap();
                let payload =
                    (0..payload_width).map(|i| args[i % 6]).collect::<alloc::vec::Vec<_>>();
                builder.exec_fpi(prefix, payload, SourceSpan::UNKNOWN).unwrap();
                builder.ret(None, SourceSpan::UNKNOWN).unwrap();
            }
            let text = format!(
                "{}",
                <midenc_hir::dialects::builtin::Function as Op>::print(
                    &test.function().borrow(),
                    &Default::default()
                )
            );
            let parsed = parse::parse_any(
                ParserConfig {
                    context: test.context_rc(),
                    verify: true,
                },
                Uri::new("fpi_roundtrip.hir"),
                text.as_str(),
            )
            .expect("semantic FPI syntax roundtrips");
            let mut calls = 0;
            parsed.borrow().postwalk_all(|op| {
                if let Some(exec) = op.downcast_ref::<super::ExecFpi>() {
                    calls += 1;
                    assert_eq!(exec.prefix().len(), 6);
                    assert_eq!(exec.inputs().len(), payload_width);
                    assert_eq!(exec.results().len(), 16);
                }
            });
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn exec_fpi_requires_six_felt_prefix_operands_and_felt_payload() {
        use midenc_hir::BuilderExt;

        for (prefix_len, payload_len, bad_operand, expected) in [
            (5, 0, None, "expected 6 prefix operand(s)"),
            (7, 0, None, "expected 6 prefix operand(s)"),
            (6, 17, None, "expected at most 16 procedure input operand(s)"),
            (6, 1, Some(0), "expected a field element, got u32"),
            (6, 1, Some(6), "expected a field element, got u32"),
        ] {
            let mut test = Test::new("invalid_fpi", &[], &[]);
            let mut builder = test.function_builder();
            let operands = (0..prefix_len + payload_len)
                .map(|index| {
                    if bad_operand == Some(index) {
                        builder.u32(index as u32, SourceSpan::UNKNOWN)
                    } else {
                        builder
                            .felt(midenc_hir::Felt::new(index as u64).unwrap(), SourceSpan::UNKNOWN)
                    }
                })
                .collect::<alloc::vec::Vec<_>>();
            let result = builder
                .builder_mut()
                .create::<super::ExecFpi, (alloc::vec::Vec<ValueRef>, alloc::vec::Vec<ValueRef>)>(
                    SourceSpan::UNKNOWN,
                )(
                operands[..prefix_len].to_vec(), operands[prefix_len..].to_vec()
            );
            let message = match result {
                Err(error) => error.to_string(),
                Ok(_) => panic!("malformed FPI must fail inference"),
            };
            assert!(message.contains(expected), "{message}");
        }
    }

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

    /// Build a `dispatch` function whose `hir.dyncall` declares `signature_params` under calling
    /// convention `cc` but passes `argument_count` `u32` constants, hand the built op and the
    /// context to `mutate`, then verify the module.
    ///
    /// Every argument is a `u32` constant, so a `signature_params` entry of any other type is how
    /// the mistyped case is built — no other constant builder is needed. `mutate` is how the cases
    /// the builder's fixed-size root array makes unreachable are built.
    fn verify_dyncall_with(
        cc: CallConv,
        signature_params: &[Type],
        argument_count: usize,
        mutate: impl FnOnce(&alloc::rc::Rc<midenc_hir::Context>, UnsafeIntrusiveEntityRef<Dyncall>),
    ) -> Result<(), midenc_hir::Report> {
        use midenc_hir::{Felt, Op, ValueRef};

        let mut test = Test::named("verify_dyncall").in_module("m");
        test.with_function("dispatch", &[], &[]);
        let context = test.context_rc();
        let signature = Signature::with_convention(
            &context,
            cc,
            signature_params.to_vec(),
            core::iter::empty(),
        );
        let dyncall = {
            let mut builder = test.function_builder();
            let root: [ValueRef; Dyncall::ROOT_FELTS] = core::array::from_fn(|element| {
                builder
                    .felt(Felt::new(element as u64 + 1).expect("a small felt"), SourceSpan::UNKNOWN)
            });
            let args = (0..argument_count)
                .map(|_| builder.u32(0, SourceSpan::UNKNOWN))
                .collect::<alloc::vec::Vec<_>>();
            let dyncall = builder.dyncall(root, signature, args, SourceSpan::UNKNOWN).unwrap();
            builder.ret(None, SourceSpan::UNKNOWN).unwrap();
            dyncall
        };
        mutate(&context, dyncall);

        test.module().borrow().as_operation().recursively_verify()
    }

    /// A signature declaring a parameter the call does not pass would pop an empty operand stack
    /// in the emitter; verification must reject it first.
    #[test]
    fn dyncall_with_too_few_arguments_fails_verification() {
        let err = verify_dyncall_with(CallConv::ComponentModel, &[Type::U32], 0, |_, _| {})
            .expect_err("a missing argument must fail verification");
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("1 parameter"), "{message}");
    }

    /// An argument whose type differs from its parameter reaches an `assert_eq!` in the emitter;
    /// verification must reject it first.
    #[test]
    fn dyncall_with_mistyped_argument_fails_verification() {
        let err = verify_dyncall_with(CallConv::ComponentModel, &[Type::I32], 1, |_, _| {})
            .expect_err("a mistyped argument must fail verification");
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("parameter 0"), "{message}");
    }

    /// The callee runs in a fresh context, which only operand-stack arguments and results can
    /// cross; a signature under any other convention promises a transfer through caller memory or
    /// a return pointer that the callee cannot reach.
    #[test]
    fn dyncall_under_another_calling_convention_fails_verification() {
        let err = verify_dyncall_with(CallConv::C, &[Type::U32], 1, |_, _| {})
            .expect_err("only the component-model convention crosses a context switch");
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("component-model calling convention"), "{message}");
    }

    /// A MAST root is a whole word, and the lowering spills exactly one word to the address the
    /// VM reads the `dyncall` target from. The builder's fixed-size root array and the parser's
    /// arity guard both rule a partial word out, which leaves the verifier answering for IR a
    /// transform mangled — so it is rebuilt here by rewriting the operand group directly.
    #[test]
    fn dyncall_with_a_partial_root_word_fails_verification() {
        let err = verify_dyncall_with(
            CallConv::ComponentModel,
            &[Type::U32],
            1,
            |context, mut dyncall| {
                use midenc_hir::Op;

                let root = dyncall
                    .borrow()
                    .root()
                    .iter()
                    .take(Dyncall::ROOT_FELTS - 1)
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<alloc::vec::Vec<_>>();
                let mut dyncall = dyncall.borrow_mut();
                let owner = dyncall.as_operation_ref();
                dyncall
                    .as_operation_mut()
                    .operands_mut()
                    .group_mut(0)
                    .set_operands(root, owner, context);
            },
        )
        .expect_err("a root operand group narrower than a word must be rejected");
        let message = format!("{err}");
        assert!(message.contains("hir.dyncall"), "{message}");
        assert!(message.contains("expected 4 root felt operand(s), but got 3"), "{message}");
    }

    /// The bracketed root word and the `(arguments) : signature` tail have to survive a print and
    /// re-parse, as `midenc compile --emit=hir | midenc compile` reads back what the printer
    /// wrote.
    #[test]
    fn dyncall_prints_and_reparses() {
        let source = r#"
builtin.world {
builtin.module public @m {
    builtin.function public extern("C") @dispatch(%r0: felt, %r1: felt, %r2: felt, %r3: felt, %a: felt, %b: u32) -> u32 {
        %result = hir.dyncall [%r0, %r1, %r2, %r3](%a, %b) : extern("component-model") (felt, u32) -> (u32);
        builtin.ret %result : (u32);
    };
};
};"#;

        let test = Test::default();
        let world = parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("dyncall_prints_and_reparses.hir"),
            source,
        )
        .expect("a hir.dyncall must parse and verify");
        let printed = format!("{}", world.borrow());
        assert!(printed.contains("hir.dyncall ["), "expected the root word to print: {printed}");

        // Re-parse in a fresh context: the printing context already owns the `@m` symbols. The
        // handle is kept here so the context outlives the parsed operation's destruction.
        let reparse_context = Test::default().context_rc();
        let reparsed = parse::parse_any(
            ParserConfig::new(reparse_context.clone()),
            Uri::new("dyncall_prints_and_reparses.reparsed.hir"),
            printed.as_str(),
        )
        .expect("the printed hir.dyncall must re-parse");
        assert_eq!(
            printed,
            format!("{}", reparsed.borrow()),
            "printing a hir.dyncall is not a parse/print fixpoint"
        );
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
    fn pointer_call_builders_verify_root_and_arguments_without_inference_repair() {
        use midenc_hir::{AddressSpace, ArrayType, PointerType};
        for context_call in [false, true] {
            for (pointer, params, arguments, valid) in [
                (super::DynExec::root_pointer_type(), alloc::vec![Type::U32], 1, true),
                (
                    Type::from(PointerType::new(super::Procref::word_type())),
                    alloc::vec![Type::U32],
                    1,
                    false,
                ),
                (
                    Type::from(PointerType::new_with_address_space(
                        Type::from(ArrayType::new(Type::Felt, 3)),
                        AddressSpace::Element,
                    )),
                    alloc::vec![Type::U32],
                    1,
                    false,
                ),
                (super::DynExec::root_pointer_type(), alloc::vec![Type::U32], 0, false),
                (super::DynExec::root_pointer_type(), alloc::vec![Type::Felt], 1, false),
            ] {
                let mut test = Test::new("pointer_builder", &[pointer], &[]);
                let signature = Signature::with_convention(
                    &test.context_rc(),
                    if context_call {
                        CallConv::ComponentModel
                    } else {
                        CallConv::C
                    },
                    params,
                    [],
                );
                {
                    let mut builder = test.function_builder();
                    let root = builder.entry_block().borrow().arguments()[0] as ValueRef;
                    let args = (0..arguments)
                        .map(|_| builder.u32(42, SourceSpan::UNKNOWN))
                        .collect::<alloc::vec::Vec<_>>();
                    if context_call {
                        builder.dyncall_local(root, signature, args, SourceSpan::UNKNOWN).unwrap();
                    } else {
                        builder.dynexec(root, signature, args, SourceSpan::UNKNOWN).unwrap();
                    }
                    builder.ret([], SourceSpan::UNKNOWN).unwrap();
                }
                assert_eq!(
                    test.function().borrow().as_operation().recursively_verify().is_ok(),
                    valid
                );
            }
        }
    }

    #[test]
    fn pointer_calls_roundtrip_and_verify_the_complete_contract() {
        use midenc_hir::Value;
        for (operation, convention) in [("dynexec", "sigma"), ("dyncall_local", "component-model")]
        {
            let context = Test::default().context_rc();
            let source = format!(
                r#"builtin.function public extern("C") @dispatch(%p: ptr<[felt; 4], element>, %x: felt) -> felt {{
                %r = hir.{operation} %p(%x) : extern("{convention}") (felt) -> (felt);
                builtin.ret %r : (felt);
            }};"#
            );
            let parsed = parse::parse_any(
                ParserConfig::new(context.clone()),
                Uri::new("pointer-call.hir"),
                &source,
            )
            .unwrap();
            let mut found = false;
            parsed.borrow().postwalk_all(|op| {
                if let Some(call) = op.as_trait::<dyn midenc_hir::CallOpInterface>() {
                    found = true;
                    assert!(call.possible_callees().is_none());
                    assert!(call.resolve().is_none());
                    assert_eq!(call.arguments().len(), 1);
                    assert_eq!(call.arguments()[0].borrow().ty(), Type::Felt);
                }
            });
            assert!(found);
            let printed = format!("{}", parsed.borrow());
            let fresh = Test::default().context_rc();
            parse::parse_any(
                ParserConfig::new(fresh.clone()),
                Uri::new("pointer-call-roundtrip.hir"),
                &printed,
            )
            .unwrap();
            // Verification must detect transforms that mutate an inferred result without reinference.
            let mut call = None;
            parsed.borrow().postwalk_all(|op| {
                if op.as_trait::<dyn midenc_hir::CallOpInterface>().is_some() {
                    call = Some(op.as_operation_ref());
                }
            });
            call.unwrap().borrow_mut().results_mut().group_mut(0)[0]
                .borrow_mut()
                .set_type(Type::U32);
            assert!(parsed.borrow().recursively_verify().is_err());
            for invalid in [
                source.replace("ptr<[felt; 4], element>", "ptr<[felt; 4], byte>"),
                source.replace("ptr<[felt; 4], element>", "ptr<[felt; 3], element>"),
                source.replace("(felt) -> (felt)", "(u32) -> (felt)"),
                source.replace(
                    &format!("extern(\"{convention}\") (felt)"),
                    if operation == "dynexec" {
                        "extern(\"component-model\") (felt)"
                    } else {
                        "extern(\"C\") (felt)"
                    },
                ),
            ] {
                let fresh = Test::default().context_rc();
                assert!(
                    parse::parse_any(
                        ParserConfig::new(fresh.clone()),
                        Uri::new("invalid-pointer-call.hir"),
                        &invalid
                    )
                    .is_err(),
                    "{invalid}"
                );
            }
        }
    }

    #[test]
    fn procref_infers_one_word_and_roundtrips() {
        use midenc_hir::Value;
        let context = Test::default().context_rc();
        let source = r#"builtin.module public @m {
            builtin.function private extern("C") @callee() { builtin.ret; };
            builtin.function public extern("C") @caller() {
                %root = hir.procref @callee;
                builtin.ret;
            };
        };"#;
        let parsed =
            parse::parse_any(ParserConfig::new(context.clone()), Uri::new("procref.hir"), source)
                .expect("procedure word reference parses");
        let mut roots = 0;
        parsed.borrow().postwalk_all(|op| {
            if op.name().name().as_str() == "procref" {
                roots += 1;
                assert_eq!(op.num_results(), 1);
                assert_eq!(
                    op.results()[0].borrow().ty(),
                    &Type::from(midenc_hir::ArrayType::new(Type::Felt, 4))
                );
            }
        });
        assert_eq!(roots, 1);
        let printed = format!("{}", parsed.borrow());
        let fresh = Test::default().context_rc();
        parse::parse_any(
            ParserConfig::new(fresh.clone()),
            Uri::new("procref-roundtrip.hir"),
            &printed,
        )
        .expect("inferred word does not require aggregate result syntax");
        let mut reference = None;
        parsed.borrow().postwalk_all(|op| {
            if op.is::<super::Procref>() {
                reference = Some(op.as_operation_ref());
            }
        });
        reference.unwrap().borrow_mut().results_mut().group_mut(0)[0]
            .borrow_mut()
            .set_type(Type::from(midenc_hir::ArrayType::new(Type::Felt, 3)));
        assert!(parsed.borrow().recursively_verify().is_err());
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
    fn typed_inline_assembly_parses_and_roundtrips() {
        use midenc_hir::{diagnostics::Uri, parse, parse::ParserConfig};
        let source = r#"builtin.function public extern("C") @inline(%0: felt) -> felt {
            %1 = hir.inline_asm "push.1 add" (%0) : extern("C") (felt) -> felt;
            builtin.ret %1 : (felt);
        };"#;
        let context = Test::default().context_rc();
        let parsed =
            parse::parse_any(ParserConfig::new(context.clone()), Uri::new("inline.hir"), source)
                .expect("typed inline assembly parses");
        let printed = parsed.borrow().to_string();
        let context2 = Test::default().context_rc();
        parse::parse_any(
            ParserConfig::new(context2.clone()),
            Uri::new("inline-roundtrip.hir"),
            &printed,
        )
        .expect("typed inline assembly roundtrips");
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
        assert_eq!(resolved.as_symbol_ref().borrow().path(), replacement_path);
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
    fn direct_calls_resolve_aliases_without_rewriting_symbol_uses() {
        for kind in ["exec", "call", "syscall"] {
            let mut test = Test::default().in_module("test");
            let target = test.define_function("target", &[], &[]);
            // `alias` uses `target`
            let alias = ModuleBuilder::new(test.module())
                .define_function_alias(Ident::from("alias"), Visibility::Public, target)
                .unwrap();

            // `op` uses `alias`
            test.with_function("caller", &[], &[]);
            let signature = target.borrow().get_signature().clone();
            let op = {
                let mut builder = test.function_builder();
                let op = match kind {
                    "exec" => builder
                        .exec(alias, signature, [], SourceSpan::UNKNOWN)
                        .unwrap()
                        .as_operation_ref(),
                    "call" => builder
                        .call(alias, signature, [], SourceSpan::UNKNOWN)
                        .unwrap()
                        .as_operation_ref(),
                    "syscall" => builder
                        .syscall(alias, signature, [], SourceSpan::UNKNOWN)
                        .unwrap()
                        .as_operation_ref(),
                    _ => unreachable!(),
                };
                builder.ret(None, SourceSpan::UNKNOWN).unwrap();
                op
            };
            let op = op.borrow();
            let call = op.as_trait::<dyn CallOpInterface>().unwrap();
            let module = test.module();
            let module = module.borrow();

            let expected = target
                .borrow()
                .as_operation()
                .as_symbol_ref()
                .unwrap()
                .resolve_callable()
                .unwrap()
                .target();
            assert_eq!(call.resolve(), Some(expected), "{kind}");
            assert_eq!(call.resolve_in_symbol_table(&*module), Some(expected), "{kind}");
            assert_eq!(call.possible_callees().unwrap().as_slice(), &[expected]);
            // the use chain `op -> alias -> target` stays intact
            assert_eq!(call.callable_for_callee().unwrap_symbol_path(), alias.borrow().path());
            assert_eq!(alias.borrow().iter_uses().count(), 1);
            assert_eq!(target.borrow().iter_uses().count(), 1);
        }
    }

    #[test]
    fn calls_resolve_alias_chains_in_each_alias_symbol_table() {
        // Assert resolution ends up in correct module when same names used in both modules.
        let test = Test::default();
        let source = r#"
builtin.world {
    builtin.module public @a {
        builtin.function private extern("C") @target() { builtin.ret; };
        builtin.function_alias private @callee -> @target;
        builtin.function public extern("C") @caller() {
            hir.exec ::@b::@callee() : extern("C") () -> ();
            builtin.ret;
        };
    };
    builtin.module public @b {
        builtin.function public extern("C") @target() { builtin.ret; };
        builtin.function_alias private @target_alias -> @target;
        builtin.function_alias public @callee -> @target_alias;
    };
};
"#;
        let world = parse::parse_any(
            ParserConfig::new(test.context_rc()),
            Uri::new("alias_scopes.hir"),
            source,
        )
        .unwrap();
        let mut checked = false;
        world.borrow().prewalk_all(|op| {
            if let Some(call) = op.downcast_ref::<Exec>() {
                let table = op.nearest_symbol_table().unwrap();
                let table = table.borrow();
                for target in
                    [call.resolve(), call.resolve_in_symbol_table(table.as_symbol_table().unwrap())]
                {
                    assert_eq!(
                        target.unwrap().as_symbol_ref().borrow().path().to_string(),
                        "::b::target"
                    );
                }
                assert_eq!(
                    call.callee().resolve().unwrap().borrow().path().to_string(),
                    "::b::callee"
                );
                checked = true;
            }
        });
        assert!(checked);
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
                        .map(|callee| callee.as_symbol_ref().borrow().name().to_string())
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
                resolved = callees[0].as_symbol_ref().borrow().path().to_string();
            }
        });

        assert_eq!(
            resolved, "::b::target",
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
