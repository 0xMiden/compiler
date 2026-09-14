use alloc::format;

use crate::{
    CallableSymbol, Op, OpParser, OpPrinter, Operation, Symbol, SymbolName, SymbolRef,
    SymbolUseList, Usable, Visibility,
    derive::operation,
    dialects::builtin::{
        BuiltinDialect, FunctionRef,
        attributes::{IdentAttr, VisibilityAttr},
    },
    print::AsmPrinter,
};

pub type FunctionAliasRef = crate::UnsafeIntrusiveEntityRef<FunctionAlias>;

/// References a function under another name.
///
/// The signature and body belong to the resolved (canonical) target, as an alias has no region of
/// its own. An alias introduces a new name even if its target is only a declaration.
///
/// Alias chains are supported. Each hop is resolved in the symbol table of the alias being followed
/// (see [Self::resolve_target]), meaning an alias can only reference symbols resolvable from
/// its own table. The canonical target must be callable (see the `Verify<dyn CallableSymbol>`
/// implementation), but it does not need to live in the same symbol table as the aliases pointing
/// to it.
///
/// An alias's visibility is independent of its target, so a public alias can expose a private
/// target under the alias name.
#[operation(
    dialect = BuiltinDialect,
    implements(Symbol, CallableSymbol, OpPrinter)
)]
pub struct FunctionAlias {
    #[attr]
    name: IdentAttr,
    #[attr]
    linkage: VisibilityAttr,
    #[symbol(callable)]
    target: crate::SymbolPath,
    #[default]
    uses: SymbolUseList,
}

impl FunctionAlias {
    #[inline(always)]
    pub fn as_function_alias_ref(&self) -> FunctionAliasRef {
        unsafe { FunctionAliasRef::from_raw(self) }
    }

    /// Resolve the target in the alias's own table.
    pub fn resolve_target(&self) -> Option<SymbolRef> {
        let table = self.as_operation().nearest_symbol_table()?;
        let table = table.borrow();
        table.as_symbol_table()?.resolve(self.target().path())
    }

    /// Canonical target of `symbol`, following `FunctionAlias` hops in their own tables.
    ///
    /// Returns `symbol` itself when it is not an alias. Returns `None` on unresolvable
    /// hop or cycle. Use [SymbolRef::resolve_canonical] to retain the error cause.
    pub fn canonicalize(symbol: SymbolRef) -> Option<SymbolRef> {
        symbol.resolve_canonical().ok()
    }

    /// Like [`Self::canonicalize`], but downcasts to `Function`.
    pub fn canonicalize_function(symbol: SymbolRef) -> Option<FunctionRef> {
        symbol.resolve_function().ok()
    }
}

impl Usable for FunctionAlias {
    type Use = crate::SymbolUse;

    #[inline(always)]
    fn uses(&self) -> &SymbolUseList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut SymbolUseList {
        &mut self.uses
    }
}

impl Symbol for FunctionAlias {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        self.get_name().as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        self.get_name_mut().name = name;
    }

    fn visibility(&self) -> Visibility {
        *self.get_linkage()
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.get_linkage_mut() = visibility;
    }

    #[inline]
    fn is_declaration(&self) -> bool {
        false
    }
}

impl CallableSymbol for FunctionAlias {}

impl OpParser for FunctionAlias {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::parse::Token;

        let visibility = parser
            .parse_keyword_from(&[
                Token::BareIdent("public"),
                Token::BareIdent("private"),
                Token::BareIdent("internal"),
            ])?
            .into_inner()
            .parse::<Visibility>()
            .expect("visibilities above are exhaustive");
        state.add_attribute(
            "linkage",
            parser.context_rc().create_attribute::<VisibilityAttr, _>(visibility),
        );

        let name = parser.parse_symbol_name()?;
        state.add_attribute("name", parser.context_rc().create_attribute::<IdentAttr, _>(name));

        parser.parse_arrow()?;
        let target = parser.parse_symbol_ref()?;
        state.attrs.push(crate::NamedAttribute::new("target", target.into_inner()));

        parser.parse_optional_attribute_dict_with_keyword(&mut state.attrs)?;
        Ok(())
    }
}

impl OpPrinter for FunctionAlias {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        // TODO only use what's needed
        use crate::formatter::*;

        printer.print_space();
        printer.print_keyword(self.get_linkage().as_str());
        printer.print_space();
        printer.print_symbol_name(self.get_name().as_symbol());
        printer.print_space();
        *printer += const_text("->");
        printer.print_space();
        printer.print_symbol_path(self.target().path());
        if self.op.has_attributes() {
            printer.print_space();
            *printer += const_text("attributes");
            printer.print_space();
            printer.print_attribute_dictionary(
                self.op.attributes().iter().map(|attr| *attr.as_named_attribute()),
            );
        }
    }
}

impl crate::Verify<dyn CallableSymbol> for FunctionAlias {
    fn verify(&self, context: &crate::Context) -> Result<(), crate::Report> {
        use midenc_session::diagnostics::Severity;

        use crate::Spanned;

        let span = self.as_operation().span();
        self.as_operation()
            .as_symbol_ref()
            .expect("function aliases are symbols")
            .resolve_callable()
            .map(|_| ())
            .map_err(|err| {
                context
                    .diagnostics()
                    .diagnostic(Severity::Error)
                    .with_message(format!(
                        "invalid builtin.function_alias '{}': {err}",
                        self.get_name().as_str()
                    ))
                    .with_primary_label(span, "cannot resolve this alias to a callable")
                    .into_report()
            })
    }
}

        }

        let Some(target) = Self::canonicalize(resolved) else {
            // TODO when max chain length has become a CONST, mention that this might be the case in the error message
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message(format!(
                    "invalid builtin.function_alias '{}': alias chain does not resolve",
                    self.name().as_str()
                ))
                .with_primary_label(span, "unresolvable alias chain")
                .into_report());
        };

        let target = target.borrow();
        let target_op = target.as_symbol_operation();
        if !target_op.implements::<dyn CallableOpInterface>() {
            let got = target_op.name();
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message(format!(
                    "invalid builtin.function_alias '{}': target '{target_path}' is not callable \
                     (got '{got}')",
                    self.name().as_str()
                ))
                .with_primary_label(span, "expected a callable symbol")
                .into_report());
        }
        Ok(())
    }
}
