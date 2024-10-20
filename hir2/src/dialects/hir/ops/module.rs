use alloc::collections::BTreeMap;

use crate::{
    derive::operation,
    dialects::hir::HirDialect,
    symbol_table::SymbolUsesIter,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
    Ident, InsertionPoint, Operation, OperationRef, RegionKind, RegionKindInterface, Report,
    Symbol, SymbolName, SymbolNameAttr, SymbolRef, SymbolTable, SymbolUseList, SymbolUseRef,
    Usable, Visibility,
};

#[operation(
    dialect = HirDialect,
    traits(
        SingleRegion,
        SingleBlock,
        NoRegionArguments,
        NoTerminator,
        HasOnlyGraphRegion,
        GraphRegionNoTerminator,
        IsolatedFromAbove,
    ),
    implements(RegionKindInterface, SymbolTable, Symbol)
)]
pub struct Module {
    #[attr]
    name: Ident,
    #[attr]
    #[default]
    visibility: Visibility,
    #[region]
    body: RegionRef,
    #[default]
    registry: BTreeMap<SymbolName, SymbolRef>,
    #[default]
    uses: SymbolUseList,
}

impl RegionKindInterface for Module {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for Module {
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

impl Symbol for Module {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        Module::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        let id = self.name_mut();
        id.name = name;
    }

    fn visibility(&self) -> Visibility {
        *Module::visibility(self)
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.visibility_mut() = visibility;
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
}

impl SymbolTable for Module {
    #[inline(always)]
    fn as_symbol_table_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_table_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.registry.get(&name).cloned()
    }

    fn insert_new(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> bool {
        use crate::{BlockRef, Builder, OpBuilder};
        let op = {
            let symbol = entry.borrow();
            let name = symbol.name();
            if self.registry.contains_key(&name) {
                return false;
            }
            let op = symbol.as_operation_ref();
            drop(symbol);
            self.registry.insert(name, entry.clone());
            op
        };
        let mut builder = OpBuilder::new(self.op.context_rc());
        if let Some(ip) = ip {
            builder.set_insertion_point(ip);
        } else {
            builder.set_insertion_point_to_end(unsafe { BlockRef::from_raw(&*self.body().entry()) })
        }
        builder.insert(op);
        true
    }

    fn insert(&mut self, mut entry: SymbolRef, ip: Option<InsertionPoint>) -> SymbolName {
        let name = {
            let mut symbol = entry.borrow_mut();
            let mut name = symbol.name();
            if self.registry.contains_key(&name) {
                // Unique the symbol name
                let mut counter = 0;
                name = crate::symbol_table::generate_symbol_name(name, &mut counter, |name| {
                    self.registry.contains_key(name)
                });
                symbol.set_name(name);
            }
            name
        };
        self.insert_new(entry, ip);
        name
    }

    fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        if let Some(ptr) = self.registry.remove(&name) {
            let op = ptr.borrow().as_operation_ref();
            let mut body = self.body_mut();
            let mut entry = body.entry_mut();
            let mut cursor = unsafe { entry.body_mut().cursor_mut_from_ptr(op) };
            cursor.remove().map(|_| ptr)
        } else {
            None
        }
    }

    fn rename(&mut self, from: SymbolName, to: SymbolName) -> Result<(), Report> {
        if let Some(symbol) = self.registry.get_mut(&from) {
            let mut sym = symbol.borrow_mut();
            sym.set_name(to);
            let uses = sym.uses_mut();
            let mut cursor = uses.front_mut();
            while let Some(mut next_use) = cursor.as_pointer() {
                {
                    let mut next_use = next_use.borrow_mut();
                    let mut op = next_use.owner.borrow_mut();
                    let symbol_name = op
                        .get_typed_attribute_mut::<crate::SymbolNameAttr>(next_use.symbol)
                        .expect("stale symbol user");
                    symbol_name.name = to;
                }
                cursor.move_next();
            }

            Ok(())
        } else {
            Err(Report::msg(format!(
                "unable to rename '{from}': no such symbol in '{}'",
                self.name().as_str()
            )))
        }
    }
}
