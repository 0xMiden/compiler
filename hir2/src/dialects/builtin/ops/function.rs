use crate::{
    derive::operation,
    dialects::builtin::BuiltinDialect,
    traits::{IsolatedFromAbove, SingleRegion},
    BlockRef, CallableOpInterface, Ident, Op, Operation, RegionKind, RegionKindInterface,
    RegionRef, Signature, Symbol, SymbolName, SymbolUse, SymbolUseList, Type,
    UnsafeIntrusiveEntityRef, Usable, Visibility,
};

trait UsableSymbol = Usable<Use = SymbolUse>;

pub type FunctionRef = UnsafeIntrusiveEntityRef<Function>;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(u16);
impl LocalId {
    fn new(id: usize) -> Self {
        assert!(
            id <= u16::MAX as usize,
            "system limit: unable to allocate more than u16::MAX locals per function"
        );
        Self(id as u16)
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

#[operation(
    dialect = BuiltinDialect,
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
    /// The set of local variables allocated within this function
    #[default]
    locals: Vec<Type>,
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
        assert!(self.body().is_empty(), "entry block already exists");
        let signature = self.signature();
        let block = self
            .as_operation()
            .context()
            .create_block_with_params(signature.params().iter().map(|p| p.ty.clone()));
        let mut body = self.body_mut();
        body.push_back(block);
        block
    }
}

/// Accessors
impl Function {
    #[inline]
    pub fn entry_block(&self) -> BlockRef {
        self.body()
            .body()
            .front()
            .as_pointer()
            .expect("cannot get entry block for declaration")
    }

    pub fn last_block(&self) -> BlockRef {
        self.body()
            .body()
            .back()
            .as_pointer()
            .expect("cannot access blocks of a function declaration")
    }

    pub fn num_locals(&self) -> usize {
        self.locals.len()
    }

    #[inline]
    pub fn locals(&self) -> &[Type] {
        &self.locals
    }

    #[inline]
    pub fn get_local(&self, id: LocalId) -> &Type {
        &self.locals[id.as_usize()]
    }

    pub fn alloc_local(&mut self, ty: Type) -> LocalId {
        let id = self.locals.len();
        self.locals.push(ty);
        LocalId::new(id)
    }

    #[inline(always)]
    pub fn as_function_ref(&self) -> FunctionRef {
        unsafe { FunctionRef::from_raw(self) }
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
        self.name_mut().name = name;
    }

    fn visibility(&self) -> Visibility {
        self.signature().visibility
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        self.signature_mut().visibility = visibility;
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
