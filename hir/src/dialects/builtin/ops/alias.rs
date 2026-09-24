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
    /// hop, cycle, or excessive depth. Use [SymbolRef::resolve_canonical] to retain the
    /// error cause.
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
        use crate::formatter::const_text;

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

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, string::ToString};

    use super::*;
    use crate::{
        CallableOpInterface, Context, Ident, SymbolTable, Type,
        diagnostics::Uri,
        dialects::builtin::{Module, ModuleBuilder, ModuleRef, attributes::Signature},
        parse::{ParserConfig, parse},
        testing::Test,
    };

    fn module_with_alias() -> (Rc<Context>, ModuleRef, FunctionAliasRef) {
        let mut test = Test::default().in_module("test");
        let primary = test.define_function("foo", &[], &[Type::I32]);
        let module = test.module();
        let alias = {
            let mut mb = ModuleBuilder::new(module);
            mb.define_function_alias(
                Ident::with_empty_span(SymbolName::intern("bar")),
                Visibility::Public,
                primary,
            )
            .unwrap()
        };
        let ctx = module.borrow().as_operation().context_rc();
        (ctx, module, alias)
    }

    #[test]
    fn alias_resolves_to_primary_and_verifies() {
        let (_ctx, module, alias) = module_with_alias();
        module.borrow().as_operation().recursively_verify().unwrap();
        let mb = ModuleBuilder::new(module);
        // Primary and alias both resolve to the primary definition.
        let foo = mb.get_function("foo").expect("primary should resolve");
        let bar_resolved = mb.resolve_function("bar").expect("alias should resolve to primary");
        assert!(foo == bar_resolved);
        assert!(mb.resolve_function("foo").unwrap() == foo);
        assert!(mb.get_function("bar").is_none());
        assert!(mb.get_function("missing").is_none());
        assert!(mb.resolve_function("missing").is_none());
        let bar_alias = mb.get_function_alias("bar").expect("alias should exist");
        assert!(bar_alias == alias);
        assert!(mb.get_function_alias("foo").is_none());
    }

    #[test]
    fn alias_is_callable_symbol_but_not_callable_op_interface() {
        let (_ctx, module, _alias) = module_with_alias();
        let mb = ModuleBuilder::new(module);
        let alias = mb.get_function_alias("bar").unwrap();
        let alias_borrow = alias.borrow();
        let op = alias_borrow.as_operation();
        assert!(op.implements::<dyn CallableSymbol>());
        assert!(!op.implements::<dyn CallableOpInterface>());
    }

    #[test]
    fn alias_print_parse_roundtrip_with_forward_references() {
        let test = Test::default();
        let source = r#"
builtin.module public @test {
    builtin.function_alias public @bar -> @first;
    builtin.function_alias private @first -> @foo;
    builtin.function private extern("C") @foo(%arg: u32) -> u32 {
        builtin.ret %arg : (u32);
    };
};
"#;
        let module =
            parse::<Module>(ParserConfig::new(test.context_rc()), Uri::new("alias.hir"), source)
                .unwrap();
        let mb = ModuleBuilder::new(module);
        assert!(mb.resolve_function("bar").unwrap() == mb.get_function("foo").unwrap());

        let printed = module.borrow().as_operation().to_string();
        for line in printed.lines().filter(|line| line.contains("builtin.function_alias")) {
            assert!(!line.contains("extern"), "{line}");
            assert!(!line.contains(" : "), "{line}");
        }

        let reparse_test = Test::default();
        let reparsed = parse::<Module>(
            ParserConfig::new(reparse_test.context_rc()),
            Uri::new("alias.reparsed.hir"),
            &printed,
        )
        .unwrap();
        assert_eq!(printed, reparsed.borrow().as_operation().to_string());
    }

    #[test]
    fn alias_missing_target_fails_verification() {
        let mut test = Test::default().in_module("test");
        let primary = test.define_function("foo", &[], &[Type::I32]);
        let mut module = test.module();
        {
            let mut mb = ModuleBuilder::new(module);
            mb.define_function_alias(
                Ident::with_empty_span(SymbolName::intern("bar")),
                Visibility::Public,
                primary,
            )
            .unwrap();
        }

        // Delete the target
        {
            module.borrow_mut().remove(SymbolName::intern("foo"));
        }
        let err = module.borrow().as_operation().recursively_verify().unwrap_err();
        assert!(format!("{err}").contains("does not resolve"), "{err}");
    }

    #[test]
    fn alias_to_non_callable_fails_verification() {
        let mut test = Test::default().in_module("test");
        let primary = test.define_function("foo", &[], &[Type::I32]);
        let mut module = test.module();
        {
            let mut mb = ModuleBuilder::new(module);
            mb.define_function_alias(
                Ident::with_empty_span(SymbolName::intern("bar")),
                Visibility::Public,
                primary,
            )
            .unwrap();
        }
        // Replace the callable target with a with non-callable global with same name
        {
            module.borrow_mut().remove(SymbolName::intern("foo"));
            let mut mb = ModuleBuilder::new(module);
            mb.define_global_variable(
                Ident::with_empty_span(SymbolName::intern("foo")),
                Visibility::Public,
                Type::I32,
            )
            .unwrap();
            assert!(mb.get_function("foo").is_none());
            assert!(mb.resolve_function("foo").is_none());
            assert!(mb.resolve_function("bar").is_none());
        }
        let err = module.borrow().as_operation().recursively_verify().unwrap_err();
        assert!(format!("{err}").contains("not callable"), "{err}");
    }

    #[test]
    fn alias_chain_observes_target_signature_changes() {
        let (ctx, module, _alias) = module_with_alias();
        let mut mb = ModuleBuilder::new(module);
        let alias = mb.get_function_alias("bar").unwrap();
        mb.define_function_alias(Ident::from("chain"), Visibility::Public, alias)
            .unwrap();
        let mut target = mb.get_function("foo").unwrap();
        let updated_signature = Signature::new(&ctx, [Type::U32], [Type::I64]);
        *target.borrow_mut().get_signature_mut() = updated_signature.clone();

        module.borrow().as_operation().recursively_verify().unwrap();
        assert_eq!(
            mb.resolve_function("chain").unwrap().borrow().get_signature().clone(),
            updated_signature
        );
    }

    #[test]
    fn alias_cycles_fail_verification_after_parsing() {
        for aliases in [
            "builtin.function_alias public @bar -> @bar;",
            "builtin.function_alias public @bar -> @first; builtin.function_alias private @first \
             -> @bar;",
        ] {
            let test = Test::default();
            let source = format!("builtin.module public @test {{ {aliases} }};");
            let err = parse::<Module>(
                ParserConfig::new(test.context_rc()),
                Uri::new("alias_cycle.hir"),
                &source,
            )
            .err()
            .expect("alias cycles must fail verification");
            let message = format!("{err}");
            assert!(
                message.contains("cannot reference itself") || message.contains("cycle"),
                "{message}"
            );
        }
    }
}
