use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
};

use super::{Function, FunctionIdent, Symbol, SymbolTable};
use crate::UnsafeRef;

pub struct Module {
    name: midenc_hir_symbol::Symbol,
    functions: BTreeMap<midenc_hir_symbol::Symbol, UnsafeRef<Function>>,
}
impl SymbolTable for Module {
    type Key = midenc_hir_symbol::Symbol;

    fn get<T>(&self, id: &Self::Key) -> Option<UnsafeRef<T>>
    where
        T: Symbol<Id = Self::Key>,
    {
        if TypeId::of::<T>() == TypeId::of::<Function>() {
            self.functions.get(id).copied().map(|unsafe_ref| {
                let ptr = unsafe_ref.into_raw();
                UnsafeRef::new(ptr.cast())
            })
        } else {
            None
        }
    }

    fn insert<T>(&self, entry: UnsafeRef<T>) -> bool
    where
        T: Symbol<Id = Self::Key>,
    {
        todo!()
    }

    fn remove<T>(&self, id: &Self::Key) -> Option<UnsafeRef<T>>
    where
        T: Symbol<Id = Self::Key>,
    {
        todo!()
    }
}
