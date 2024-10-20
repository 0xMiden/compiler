use crate::{
    derive::operation,
    dialects::hir::HirDialect,
    traits::{IsolatedFromAbove, SingleRegion},
    Block, BlockRef, CallableOpInterface, Ident, Op, Operation, OperationRef, RegionKind,
    RegionKindInterface, RegionRef, Report, Signature, Symbol, SymbolName, SymbolNameAttr,
    SymbolRef, SymbolUse, SymbolUseList, SymbolUseRef, SymbolUsesIter, Usable, Visibility,
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
    #[attr]
    name: Ident,
    #[attr]
    signature: Signature,
    #[region]
    body: RegionRef,
    /// The uses of this function as a symbol
    #[default]
    uses: SymbolUseList,
}

/// Builders
impl Function {
    /// Conver this function from a declaration (no body) to a definition (has a body) by creating
    /// the entry block based on the function signature.
    ///
    /// NOTE: The resulting function is _invalid_ until the block has a terminator inserted into it.
    ///
    /// This function will panic if an entry block has already been created
    pub fn create_entry_block(&mut self) -> BlockRef {
        use crate::EntityWithParent;

        assert!(self.body().is_empty(), "entry block already exists");
        let signature = self.signature();
        let block = self
            .as_operation()
            .context()
            .create_block_with_params(signature.params().iter().map(|p| p.ty.clone()));
        let mut body = self.body_mut();
        body.push_back(block.clone());
        Block::on_inserted_into_parent(block.clone(), body.as_region_ref());
        block
    }
}

/// Accessors
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
                || from.borrow().is_proper_ancestor_of(&user.owner)
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
                    .get_typed_attribute_mut::<SymbolNameAttr>(attr_name)
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
