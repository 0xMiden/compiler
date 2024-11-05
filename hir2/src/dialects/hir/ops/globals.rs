use crate::{
    derive::operation,
    dialects::hir::HirDialect,
    traits::{
        ConstantLike, InferTypeOpInterface, IsolatedFromAbove, NoRegionArguments, PointerOf,
        SingleBlock, SingleRegion, UInt8,
    },
    AsSymbolRef, Context, Ident, Operation, Report, Spanned, Symbol, SymbolName, SymbolRef,
    SymbolUseList, Type, Usable, Value, Visibility,
};

/// A [GlobalVariable] represents a concrete definition for a symbolic value,
/// i.e. it corresponds to the actual allocated memory referenced by a [GlobalValueData::Symbol]
/// value.
#[operation(
    dialect = HirDialect,
    traits(
        SingleRegion,
        SingleBlock,
        NoRegionArguments,
        IsolatedFromAbove,
    ),
    implements(Symbol)
)]
pub struct GlobalVariable {
    #[attr]
    name: Ident,
    #[attr]
    visibility: Visibility,
    #[attr]
    ty: Type,
    #[region]
    initializer: RegionRef,
    #[default]
    uses: SymbolUseList,
}

impl Usable for GlobalVariable {
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

impl Symbol for GlobalVariable {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        GlobalVariable::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        let id = self.name_mut();
        id.name = name;
    }

    fn visibility(&self) -> Visibility {
        *GlobalVariable::visibility(self)
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.visibility_mut() = visibility;
    }

    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    #[inline]
    fn is_declaration(&self) -> bool {
        self.initializer().is_empty()
    }
}

impl AsSymbolRef for GlobalVariable {
    fn as_symbol_ref(&self) -> SymbolRef {
        unsafe { SymbolRef::from_raw(self as &dyn Symbol) }
    }
}

/// A symbolic reference to a global variable symbol
///
/// The type of a symbolic global value is always a pointer: the address of the referenced global
/// variable.
#[operation(
    dialect = HirDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface)
)]
pub struct GlobalSymbol {
    /// The name of the global variable that is referenced
    #[symbol]
    symbol: GlobalVariable,
    /// A constant offset, in bytes, from the address of the symbol
    #[attr]
    #[default]
    offset: i32,
    #[result]
    addr: PointerOf<UInt8>,
}

impl InferTypeOpInterface for GlobalSymbol {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.addr_mut().set_type(Type::Ptr(Box::new(Type::U8)));
        Ok(())
    }
}
