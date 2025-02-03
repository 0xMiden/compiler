use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    implements(CallOpInterface, InferTypeOpInterface)
)]
pub struct Exec {
    #[symbol(callable)]
    callee: SymbolPath,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for Exec {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;

        let span = self.span();
        let owner = self.as_operation().as_operation_ref();
        if let Some(symbol) = self.resolve() {
            let symbol = symbol.borrow();
            if let Some(callable) =
                symbol.as_symbol_operation().as_trait::<dyn CallableOpInterface>()
            {
                let signature = callable.signature();
                for (i, result) in signature.results().iter().enumerate() {
                    let value = context.make_result(span, result.ty.clone(), owner, i as u8);
                    self.op.results.push(value);
                }

                Ok(())
            } else {
                Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operation")
                    .with_primary_label(
                        span,
                        "invalid callee: does not implement CallableOpInterface",
                    )
                    .with_secondary_label(
                        symbol.as_symbol_operation().span,
                        "symbol refers to this definition",
                    )
                    .into_report())
            }
        } else {
            Err(context
                .session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid operation")
                .with_primary_label(span, "invalid callee: symbol is undefined")
                .into_report())
        }
    }
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
        let callee = callable.unwrap_symbol_path();
        self.callee_mut().path = callee;
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
        let symbol_table = self.as_operation().nearest_symbol_table()?;
        let symbol_table = symbol_table.borrow();
        let symbol_table = symbol_table.as_symbol_table().unwrap();
        symbol_table.resolve(&callee.path)
    }

    fn resolve_in_symbol_table(&self, symbols: &dyn SymbolTable) -> Option<SymbolRef> {
        let callee = self.callee();
        symbols.resolve(&callee.path)
    }
}

// TODO(pauls): Validate that the arguments/results of the callee of this operation do not contain
// any types which are invalid for cross-context calls
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct Call {
    #[symbol(callable)]
    callee: SymbolPath,
    #[operands]
    arguments: AnyType,
}

impl InferTypeOpInterface for Call {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;

        let span = self.span();
        let owner = self.as_operation().as_operation_ref();
        if let Some(symbol) = self.resolve() {
            let symbol = symbol.borrow();
            if let Some(callable) =
                symbol.as_symbol_operation().as_trait::<dyn CallableOpInterface>()
            {
                let signature = callable.signature();
                for (i, result) in signature.results().iter().enumerate() {
                    let value = context.make_result(span, result.ty.clone(), owner, i as u8);
                    self.op.results.push(value);
                }

                Ok(())
            } else {
                Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operation")
                    .with_primary_label(
                        span,
                        "invalid callee: does not implement CallableOpInterface",
                    )
                    .with_secondary_label(
                        symbol.as_symbol_operation().span,
                        "symbol refers to this definition",
                    )
                    .into_report())
            }
        } else {
            Err(context
                .session
                .diagnostics
                .diagnostic(Severity::Error)
                .with_message("invalid operation")
                .with_primary_label(span, "invalid callee: symbol is undefined")
                .into_report())
        }
    }
}

/*
#[operation(
    dialect = HirDialect,
    implements(CallOpInterface)
)]
pub struct CallIndirect {
    #[attr]
    signature: Signature,
    /// TODO(pauls): Change this to FunctionType
    #[operand]
    callee: AnyType,
}
 */
impl CallOpInterface for Call {
    #[inline(always)]
    fn callable_for_callee(&self) -> Callable {
        self.callee().into()
    }

    fn set_callee(&mut self, callable: Callable) {
        let callee = callable.unwrap_symbol_path();
        self.callee_mut().path = callee;
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
        let symbol_table = self.as_operation().nearest_symbol_table()?;
        let symbol_table = symbol_table.borrow();
        let symbol_table = symbol_table.as_symbol_table().unwrap();
        symbol_table.resolve(&callee.path)
    }

    fn resolve_in_symbol_table(&self, symbols: &dyn SymbolTable) -> Option<SymbolRef> {
        let callee = self.callee();
        symbols.resolve(&callee.path)
    }
}
