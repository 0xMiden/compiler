mod builders;
mod ops;

use alloc::rc::Rc;
use core::cell::{Cell, RefCell};

pub use self::{
    builders::{DefaultInstBuilder, FunctionBuilder},
    ops::*,
};
use crate::{interner, Dialect, DialectName, DialectRegistration, OperationName};

#[derive(Default)]
pub struct HirDialect {
    registered_ops: RefCell<Vec<OperationName>>,
    registered_op_cache: Cell<Option<Rc<[OperationName]>>>,
}

impl core::fmt::Debug for HirDialect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HirDialect")
            .field_with("registered_ops", |f| {
                f.debug_set().entries(self.registered_ops.borrow().iter()).finish()
            })
            .finish_non_exhaustive()
    }
}

impl HirDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops.borrow().len()
    }
}

impl Dialect for HirDialect {
    #[inline]
    fn name(&self) -> DialectName {
        DialectName::from_symbol(interner::symbols::Hir)
    }

    fn registered_ops(&self) -> Rc<[OperationName]> {
        let registered = unsafe { (*self.registered_op_cache.as_ptr()).clone() };
        if registered.as_ref().is_some_and(|ops| self.num_registered() == ops.len()) {
            registered.unwrap()
        } else {
            let registered = self.registered_ops.borrow();
            let ops = Rc::from(registered.clone().into_boxed_slice());
            self.registered_op_cache.set(Some(Rc::clone(&ops)));
            ops
        }
    }

    fn get_or_register_op(
        &self,
        opcode: midenc_hir_symbol::Symbol,
        register: fn(DialectName, midenc_hir_symbol::Symbol) -> crate::OperationName,
    ) -> crate::OperationName {
        let mut registered = self.registered_ops.borrow_mut();
        match registered.binary_search_by_key(&opcode, |op| op.name()) {
            Ok(index) => registered[index].clone(),
            Err(index) => {
                let name = register(self.name(), opcode);
                registered.insert(index, name.clone());
                name
            }
        }
    }
}

impl DialectRegistration for HirDialect {
    const NAMESPACE: &'static str = "hir";

    #[inline]
    fn init() -> Self {
        Self::default()
    }
}
