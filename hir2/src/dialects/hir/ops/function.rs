use crate::{
    derive::operation,
    dialects::hir::HirDialect,
    traits::{IsolatedFromAbove, RegionKind, RegionKindInterface, SingleRegion},
    BlockRef, CallableOpInterface, Ident, Operation, OperationRef, RegionRef, Report, Signature,
    Symbol, SymbolName, SymbolNameAttr, SymbolRef, SymbolUse, SymbolUseList, SymbolUseRef,
    SymbolUsesIter, Usable, Visibility,
};

trait UsableSymbol = Usable<Use = SymbolUse>;

#[operation(
    dialect = HirDialect,
    traits(SingleRegion, IsolatedFromAbove),
    implements(
        UsableSymbol,
        Symbol,
        CallableOpInterface,
        RegionKindInterface
    )
)]
pub struct Function {
    #[region]
    body: RegionRef,
    #[attr]
    name: Ident,
    #[attr]
    signature: Signature,
    /// The uses of this function as a symbol
    uses: SymbolUseList,
}

impl Function {
    #[inline]
    pub fn entry_block(&self) -> BlockRef {
        unsafe { BlockRef::from_raw(&*self.body().entry()) }
    }

    pub fn last_block(&self) -> BlockRef {
        self.body()
            .body()
            .back()
            .as_pointer()
            .expect("cannot access blocks of a function declaration")
    }
}

impl RegionKindInterface for Function {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::SSA
    }
}

impl Usable for Function {
    type Use = SymbolUse;

    #[inline(always)]
    fn uses(&self) -> &SymbolUseList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut SymbolUseList {
        &mut self.uses
    }
}

impl Symbol for Function {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        Self::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        let id = self.name_mut();
        id.name = name;
    }

    fn visibility(&self) -> Visibility {
        self.signature().visibility
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        self.signature_mut().visibility = visibility;
    }

    fn symbol_uses(&self, from: OperationRef) -> SymbolUsesIter {
        SymbolUsesIter::from_iter(self.uses.iter().filter_map(|user| {
            if OperationRef::ptr_eq(&from, &user.owner)
                || from.borrow().is_proper_ancestor_of(user.owner.clone())
            {
                Some(unsafe { SymbolUseRef::from_raw(&*user) })
            } else {
                None
            }
        }))
    }

    fn replace_all_uses(
        &mut self,
        replacement: SymbolRef,
        from: OperationRef,
    ) -> Result<(), Report> {
        for symbol_use in self.symbol_uses(from) {
            let (mut owner, attr_name) = {
                let user = symbol_use.borrow();
                (user.owner.clone(), user.symbol)
            };
            let mut owner = owner.borrow_mut();
            // Unlink previously used symbol
            {
                let current_symbol = owner
                    .get_typed_attribute_mut::<SymbolNameAttr, _>(&attr_name)
                    .expect("stale symbol user");
                unsafe {
                    self.uses.cursor_mut_from_ptr(current_symbol.user.clone()).remove();
                }
            }
            // Link replacement symbol
            owner.set_symbol_attribute(attr_name, replacement.clone());
        }

        Ok(())
    }

    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    #[inline]
    fn is_declaration(&self) -> bool {
        self.body().is_empty()
    }
}

impl CallableOpInterface for Function {
    fn get_callable_region(&self) -> Option<RegionRef> {
        if self.is_declaration() {
            None
        } else {
            self.op.regions().front().as_pointer()
        }
    }

    #[inline]
    fn signature(&self) -> &Signature {
        Function::signature(self)
    }
}
