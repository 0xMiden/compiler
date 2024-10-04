use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

// TODO(pauls): Implement support for:
//
// * Inferring op constraints from callee signature
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct Exec {
    #[symbol(callable)]
    callee: SymbolNameAttr,
    #[operands]
    arguments: AnyType,
}

/*
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct ExecIndirect {
    #[attr]
    signature: Signature,
    /// TODO(pauls): Change this to FunctionType
    #[operand]
    callee: AnyType,
}
 */
impl CallOpInterface for Exec {
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        self.callee().into()
    }

    fn set_callee(&mut self, callable: Callable) {
        let callee = callable.unwrap_symbol_name();
        *self.callee_mut() = callee;
    }

    #[inline(always)]
    fn arguments(&self) -> OpOperandRange<'_> {
        self.operands().group(0)
    }

    #[inline(always)]
    fn arguments_mut(&mut self) -> OpOperandRangeMut<'_> {
        self.operands_mut().group_mut(0)
    }

    fn resolve(&self) -> Option<SymbolRef> {
        let callee = self.callee();
        if callee.has_parent() {
            todo!()
        }
        let module = self.as_operation().nearest_symbol_table()?;
        let module = module.borrow();
        let symbol_table = module.as_trait::<dyn SymbolTable>().unwrap();
        symbol_table.get(callee.name)
    }

    fn resolve_in_symbol_table(&self, symbols: &dyn crate::SymbolTable) -> Option<SymbolRef> {
        let callee = self.callee();
        symbols.get(callee.name)
    }
}
