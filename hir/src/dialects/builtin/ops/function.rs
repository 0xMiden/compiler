use alloc::format;

use smallvec::SmallVec;

use crate::{
    AttrPrinter, BlockRef, CallableOpInterface, Context, Ident, Immediate, Op, OpPrinter,
    OpPrintingFlags, Operation, RegionKind, RegionKindInterface, RegionRef, Signature, Symbol,
    SymbolName, SymbolUse, SymbolUseList, Type, UnsafeIntrusiveEntityRef, Usable, ValueRef,
    Visibility, define_attr_type,
    derive::operation,
    dialects::builtin::BuiltinDialect,
    traits::{AnyType, IsolatedFromAbove, ReturnLike, SingleRegion, Terminator},
};

trait UsableSymbol = Usable<Use = SymbolUse>;

pub type FunctionRef = UnsafeIntrusiveEntityRef<Function>;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct LocalVariable {
    function: FunctionRef,
    index: u16,
}

impl LocalVariable {
    fn new(function: FunctionRef, id: usize) -> Self {
        assert!(
            id <= u16::MAX as usize,
            "system limit: unable to allocate more than u16::MAX locals per function"
        );
        Self {
            function,
            index: id as u16,
        }
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.index as usize
    }

    pub fn ty(&self) -> Type {
        self.function.borrow().get_local(self).clone()
    }

    /// Compute the absolute offset from the start of the procedure locals for this local variable
    pub fn absolute_offset(&self) -> usize {
        let index = self.as_usize();
        self.function.borrow().locals()[..index]
            .iter()
            .map(|ty| ty.size_in_felts())
            .sum::<usize>()
    }
}

impl core::fmt::Debug for LocalVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalVariable")
            .field_with("function", |f| write!(f, "{}", self.function.borrow().name().as_str()))
            .field("index", &self.index)
            .finish()
    }
}

define_attr_type!(LocalVariable);

impl AttrPrinter for LocalVariable {
    fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> crate::formatter::Document {
        use crate::formatter::*;

        text(format!("lv{}", self.as_usize()))
    }
}

#[operation(
    dialect = BuiltinDialect,
    traits(SingleRegion, IsolatedFromAbove),
    implements(
        UsableSymbol,
        Symbol,
        CallableOpInterface,
        RegionKindInterface,
        OpPrinter
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
    locals: SmallVec<[Type; 2]>,
    /// The uses of this function as a symbol
    #[default]
    uses: SymbolUseList,
}

impl OpPrinter for Function {
    fn print(&self, flags: &OpPrintingFlags, _context: &Context) -> crate::formatter::Document {
        use crate::formatter::*;

        let signature = self.signature();
        let prelude = display(signature.visibility)
            + const_text(" ")
            + display(self.as_operation().name())
            + text(format!(" @{}", self.name().as_str()));
        let arglist = if self.body().is_empty() {
            // Declaration
            signature.params().iter().enumerate().fold(const_text("("), |doc, (i, param)| {
                let doc = if i > 0 { doc + const_text(", ") } else { doc };
                let mut param_attrs = Document::Empty;
                match param.purpose {
                    crate::ArgumentPurpose::Default => (),
                    crate::ArgumentPurpose::StructReturn => {
                        param_attrs += const_text("sret ");
                    }
                }
                match param.extension {
                    crate::ArgumentExtension::None => (),
                    crate::ArgumentExtension::Zext => {
                        param_attrs += const_text("zext ");
                    }
                    crate::ArgumentExtension::Sext => {
                        param_attrs += const_text("sext ");
                    }
                }
                doc + display(&param.ty)
            }) + const_text(")")
        } else {
            let body = self.body();
            let entry = body.entry();
            entry.arguments().iter().zip(signature.params().iter()).enumerate().fold(
                const_text("("),
                |doc, (i, (entry_arg, param))| {
                    let doc = if i > 0 { doc + const_text(", ") } else { doc };
                    let mut param_attrs = Document::Empty;
                    match param.purpose {
                        crate::ArgumentPurpose::Default => (),
                        crate::ArgumentPurpose::StructReturn => {
                            param_attrs += const_text("sret ");
                        }
                    }
                    match param.extension {
                        crate::ArgumentExtension::None => (),
                        crate::ArgumentExtension::Zext => {
                            param_attrs += const_text("zext ");
                        }
                        crate::ArgumentExtension::Sext => {
                            param_attrs += const_text("sext ");
                        }
                    }
                    doc + display(*entry_arg as ValueRef) + const_text(": ") + display(&param.ty)
                },
            ) + const_text(")")
        };

        let results =
            signature
                .results()
                .iter()
                .enumerate()
                .fold(Document::Empty, |doc, (i, result)| {
                    if i > 0 {
                        doc + const_text(", ") + display(&result.ty)
                    } else {
                        doc + display(&result.ty)
                    }
                });
        let results = if results.is_empty() {
            results
        } else {
            const_text(" -> ") + results
        };

        let signature = prelude + arglist + results;
        if self.body().is_empty() {
            signature + const_text(";")
        } else {
            signature + const_text(" ") + self.body().print(flags) + const_text(";")
        }
    }
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
    pub fn get_local(&self, id: &LocalVariable) -> &Type {
        assert_eq!(
            self.as_operation_ref(),
            id.function.as_operation_ref(),
            "attempted to use local variable reference from different function"
        );
        &self.locals[id.as_usize()]
    }

    pub fn alloc_local(&mut self, ty: Type) -> LocalVariable {
        let id = self.locals.len();
        self.locals.push(ty);
        LocalVariable::new(self.as_function_ref(), id)
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

/// Returns from the enclosing function with the provided operands as its results.
#[operation(
    dialect = BuiltinDialect,
    traits(Terminator, ReturnLike),
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}

/// Returns from the enclosing function with the provided immediate value as its result.
#[operation(
    dialect = BuiltinDialect,
    traits(Terminator, ReturnLike),
    implements(OpPrinter)
)]
pub struct RetImm {
    #[attr(hidden)]
    value: Immediate,
}

impl OpPrinter for RetImm {
    fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> crate::formatter::Document {
        use crate::formatter::*;

        display(self.op.name()) + const_text(" ") + display(self.value()) + const_text(";")
    }
}
