use alloc::collections::BTreeMap;

use crate::{
    derive,
    dialects::hir::HirDialect,
    traits::{NoRegionArguments, SingleBlock, SingleRegion},
    Ident, InsertionPoint, Operation, Report, SymbolName, SymbolRef, SymbolTable,
};

derive! {
    pub struct Module : Op {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        name: Ident,
        #[region]
        body: RegionRef,
        registry: BTreeMap<SymbolName, SymbolRef>,
    }

    derives SingleRegion, SingleBlock, NoRegionArguments;
    implements SymbolTable;
}

impl SymbolTable for Module {
    #[inline(always)]
    fn as_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.registry.get(&name).cloned()
    }

    //TODO(pauls): Insert symbol ref in module body
    fn insert_new(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> bool {
        let symbol = entry.borrow();
        let name = symbol.name();
        if self.registry.contains_key(&name) {
            return false;
        }
        drop(symbol);
        self.registry.insert(name, entry);
        true
    }

    //TODO(pauls): Insert symbol ref in module body
    fn insert(&mut self, mut entry: SymbolRef, ip: Option<InsertionPoint>) -> SymbolName {
        let mut symbol = entry.borrow_mut();
        let mut name = symbol.name();
        if self.registry.contains_key(&name) {
            // Unique the symbol name
            let mut counter = 0;
            name = super::symbol_table::generate_symbol_name(name, &mut counter, |name| {
                self.registry.contains_key(name)
            });
            symbol.set_name(name);
        }
        drop(symbol);
        self.registry.insert(name, entry);
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
            while let Some(mut next_use) = cursor.get_mut() {
                next_use.symbol.name = to;
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
