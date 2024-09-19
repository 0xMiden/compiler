use alloc::collections::BTreeMap;

use super::{EntityList, Function, Ident, Symbol, SymbolTable, UnsafeIntrusiveEntityRef};

pub struct Module {
    name: Ident,
    functions: EntityList<Function>,
    registry: BTreeMap<Ident, UnsafeIntrusiveEntityRef<Function>>,
}
impl Module {
    pub const fn name(&self) -> Ident {
        self.name
    }

    pub fn functions(&self) -> &EntityList<Function> {
        &self.functions
    }

    pub fn functions_mut(&mut self) -> &mut EntityList<Function> {
        &mut self.functions
    }
}
impl SymbolTable for Module {
    type Entry = UnsafeIntrusiveEntityRef<Function>;
    type Key = Ident;

    fn get(&self, id: &Self::Key) -> Option<Self::Entry> {
        self.registry.get(id).cloned()
    }

    fn insert(&mut self, entry: Self::Entry) -> bool {
        let id = entry.borrow().id();
        if self.registry.contains_key(&id) {
            return false;
        }
        self.registry.insert(id, entry.clone());
        self.functions.push_back(entry);
        true
    }

    fn remove(&mut self, id: &Self::Key) -> Option<Self::Entry> {
        if let Some(ptr) = self.registry.remove(id) {
            let mut cursor = unsafe { self.functions.cursor_mut_from_ptr(ptr) };
            cursor.remove()
        } else {
            None
        }
    }
}
