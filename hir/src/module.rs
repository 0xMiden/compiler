use alloc::collections::BTreeMap;

use intrusive_collections::{
    intrusive_adapter,
    linked_list::{Cursor, CursorMut},
    LinkedList, LinkedListLink, RBTreeLink,
};

use self::formatter::PrettyPrint;
use crate::{
    diagnostics::{miette, Diagnostic, DiagnosticsHandler, Report, Severity, Spanned},
    *,
};

/// This error is raised when two modules conflict with the same symbol name
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("module {} has already been declared", .name)]
#[diagnostic()]
pub struct ModuleConflictError {
    #[label("duplicate declaration occurs here")]
    pub span: SourceSpan,
    pub name: Symbol,
}
impl ModuleConflictError {
    pub fn new(name: Ident) -> Self {
        Self {
            span: name.span,
            name: name.as_symbol(),
        }
    }
}

pub type ModuleTree = intrusive_collections::RBTree<ModuleTreeAdapter>;
pub type ModuleList = intrusive_collections::LinkedList<ModuleListAdapter>;

intrusive_adapter!(pub ModuleListAdapter = Box<Module>: Module { list_link: LinkedListLink });
intrusive_adapter!(pub ModuleTreeAdapter = Box<Module>: Module { link: RBTreeLink });
impl<'a> intrusive_collections::KeyAdapter<'a> for ModuleTreeAdapter {
    type Key = Ident;

    #[inline]
    fn get_key(&self, module: &'a Module) -> Ident {
        module.name
    }
}

/// Represents a SSA IR module
///
/// These correspond to MASM modules.
///
/// This module is largely a container for functions, but it also provides
/// as the owner for pooled resources available to functions:
///
/// * Mapping from Signature to FuncRef
/// * Mapping from FunctionName to FuncRef
#[derive(Spanned, AnalysisKey)]
pub struct Module {
    /// The link used to attach this module to a [Program]
    link: RBTreeLink,
    /// The link used to store this module in a list of modules
    list_link: LinkedListLink,
    /// The name of this module
    #[span]
    #[analysis_key]
    pub name: Ident,
    /// Documentation attached to this module, to be passed through to
    /// Miden Assembly during code generation.
    pub docs: Option<String>,
    /// The size of the linear memory region (in pages) which is reserved by the module creator.
    ///
    /// For example, with rustc-compiled Wasm modules, it reserves 16 pages of memory for the
    /// shadow stack, and if there is any static data, a minimum of 1 page for the static data.
    /// As a result, we must ensure that we do not allocate any globals or other items in this
    /// reserved region.
    reserved_memory_pages: u32,
    /// The page size (in bytes) used by this module.
    ///
    /// Set to 64k by default.
    page_size: u32,
    /// The set of data segments allocated in this module
    pub(crate) segments: DataSegmentTable,
    /// The set of global variables declared in this module
    pub(crate) globals: GlobalVariableTable,
    /// The set of functions which belong to this module, in the order
    /// in which they were defined.
    pub(crate) functions: LinkedList<FunctionListAdapter>,
    /// This flag indicates whether this module is a kernel module
    ///
    /// Kernel modules have additional constraints imposed on them that regular
    /// modules do not, in exchange for some useful functionality:
    ///
    /// * Functions with external linkage are required to use the `Kernel` calling convention.
    /// * A kernel module executes in the root context of the Miden VM, allowing one to expose
    ///   functionality that is protected from tampering by other non-kernel functions in the
    ///   program.
    /// * Due to the above, you may not reference globals outside the kernel module, from within
    ///   kernel functions, as they are not available in the root context.
    is_kernel: bool,
}
impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl formatter::PrettyPrint for Module {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let mut header =
            const_text("(") + const_text("module") + const_text(" ") + display(self.name);
        if self.is_kernel {
            header += const_text(" ") + const_text("(") + const_text("kernel") + const_text(")");
        }

        let segments = self
            .segments
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Data Segments") + nl() + doc)
            .unwrap_or(Document::Empty);

        let constants = self
            .globals
            .constants()
            .map(|(constant, constant_data)| {
                const_text("(")
                    + const_text("const")
                    + const_text(" ")
                    + const_text("(")
                    + const_text("id")
                    + const_text(" ")
                    + display(constant.as_u32())
                    + const_text(")")
                    + const_text(" ")
                    + text(format!("{:#x}", constant_data.as_ref()))
                    + const_text(")")
            })
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Constants") + nl() + doc)
            .unwrap_or(Document::Empty);

        let globals = self
            .globals
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Global Variables") + nl() + doc)
            .unwrap_or(Document::Empty);

        let mut external_functions = BTreeMap::<FunctionIdent, Signature>::default();
        let functions = self
            .functions
            .iter()
            .map(|fun| {
                for import in fun.dfg.imports() {
                    // Don't print declarations for functions in this module
                    if import.id.module == self.name {
                        continue;
                    }
                    external_functions.entry(import.id).or_insert_with(|| import.signature.clone());
                }
                fun.render()
            })
            .reduce(|acc, doc| acc + nl() + nl() + doc)
            .map(|doc| const_text(";; Functions") + nl() + doc)
            .unwrap_or(Document::Empty);

        let imports = external_functions
            .into_iter()
            .map(|(id, signature)| ExternalFunction { id, signature }.render())
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Imports") + nl() + doc)
            .unwrap_or(Document::Empty);

        let body = vec![segments, constants, globals, functions, imports]
            .into_iter()
            .filter(|section| !section.is_empty())
            .fold(nl(), |a, b| {
                if matches!(a, Document::Newline) {
                    indent(4, a + b)
                } else {
                    a + nl() + indent(4, nl() + b)
                }
            });

        if body.is_empty() {
            header + const_text(")") + nl()
        } else {
            header + body + nl() + const_text(")") + nl()
        }
    }
}
impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Module")
            .field("name", &self.name)
            .field("reserved_memory_pages", &self.reserved_memory_pages)
            .field("page_size", &self.page_size)
            .field("is_kernel", &self.is_kernel)
            .field("docs", &self.docs)
            .field("segments", &self.segments)
            .field("globals", &self.globals)
            .field("functions", &self.functions)
            .finish()
    }
}
impl midenc_session::Emit for Module {
    fn name(&self) -> Option<crate::Symbol> {
        Some(self.name.as_symbol())
    }

    fn output_type(&self, _mode: midenc_session::OutputMode) -> midenc_session::OutputType {
        midenc_session::OutputType::Hir
    }

    fn write_to<W: std::io::Write>(
        &self,
        mut writer: W,
        mode: midenc_session::OutputMode,
        _session: &midenc_session::Session,
    ) -> std::io::Result<()> {
        assert_eq!(
            mode,
            midenc_session::OutputMode::Text,
            "binary mode is not supported for HIR modules"
        );
        writer.write_fmt(format_args!("{}", self))
    }
}
impl Eq for Module {}
impl PartialEq for Module {
    fn eq(&self, other: &Self) -> bool {
        let is_eq = self.name == other.name
            && self.is_kernel == other.is_kernel
            && self.reserved_memory_pages == other.reserved_memory_pages
            && self.page_size == other.page_size
            && self.docs == other.docs
            && self.segments.iter().eq(other.segments.iter())
            && self.globals.len() == other.globals.len()
            && self.functions.iter().count() == other.functions.iter().count();
        if !is_eq {
            return false;
        }

        for global in self.globals.iter() {
            let id = global.id();
            if !other.globals.contains_key(id) {
                return false;
            }
            let other_global = other.globals.get(id);
            if global != other_global {
                return false;
            }
        }

        for function in self.functions.iter() {
            if !other.contains(function.id.function) {
                return false;
            }
            if let Some(other_function) = other.function(function.id.function) {
                if function != other_function {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// This macro asserts that a function is valid for insertion into a given module.
macro_rules! assert_valid_function {
    ($module:ident, $function:ident) => {
        assert_eq!($module.name, $function.id.module, "mismatched module identifiers");
        assert!(
            $function.is_detached(),
            "cannot attach a function to a module that is already attached to a module"
        );
        // Validate the kernel rules
        if $function.is_kernel() {
            assert!($module.is_kernel, "cannot add kernel functions to a non-kernel module");
        } else if $module.is_kernel && $function.is_public() {
            panic!(
                "functions with external linkage in kernel modules must use the kernel calling \
                 convention"
            );
        }
    };
}

impl Module {
    /// Create a new, empty [Module]
    pub fn new<S: Into<Ident>>(name: S) -> Self {
        Self::make(name.into(), /* is_kernel= */ false)
    }

    /// Create a new, empty [Module] with the given source location
    pub fn new_with_span<S: AsRef<str>>(name: S, span: SourceSpan) -> Self {
        let name = Ident::new(Symbol::intern(name.as_ref()), span);
        Self::make(name, /* is_kernel= */ false)
    }

    /// Create a new, empty kernel [Module]
    pub fn new_kernel<S: Into<Ident>>(name: S) -> Self {
        Self::make(name.into(), /* is_kernel= */ true)
    }

    /// Create a new, empty kernel [Module] with the given source location
    pub fn new_kernel_with_span<S: AsRef<str>>(name: S, span: SourceSpan) -> Self {
        let name = Ident::new(Symbol::intern(name.as_ref()), span);
        Self::make(name, /* is_kernel= */ true)
    }

    fn make(name: Ident, is_kernel: bool) -> Self {
        Self {
            link: Default::default(),
            list_link: Default::default(),
            name,
            docs: None,
            reserved_memory_pages: 0,
            page_size: 64 * 1024,
            segments: Default::default(),
            globals: GlobalVariableTable::new(ConflictResolutionStrategy::None),
            functions: Default::default(),
            is_kernel,
        }
    }

    /// Get the page size to use by default for this module.
    #[inline]
    pub const fn page_size(&self) -> u32 {
        self.page_size
    }

    /// Get the size (in pages) of the linear memory address space (starting from offset 0), which
    /// is reserved for use by the caller.
    #[inline]
    pub const fn reserved_memory_pages(&self) -> u32 {
        self.reserved_memory_pages
    }

    /// Get the size (in bytes) of the linear memory address space (starting from offset 0), which
    /// is reserved for use by the caller.
    #[inline]
    pub const fn reserved_memory_bytes(&self) -> u32 {
        self.reserved_memory_pages * self.page_size
    }

    /// Set the size of the reserved linear memory region.
    ///
    /// NOTE: Declared data segments can be placed in the reserved area, but global variables will
    /// never be allocated in the reserved area.
    pub fn set_reserved_memory_size(&mut self, size: u32) {
        self.reserved_memory_pages = size;
    }

    /// Returns true if this module is a kernel module
    #[inline]
    pub const fn is_kernel(&self) -> bool {
        self.is_kernel
    }

    /// Returns true if this module has yet to be attached to a [Program]
    pub fn is_detached(&self) -> bool {
        !self.link.is_linked()
    }

    /// Return the table of data segments for this module
    pub fn segments(&self) -> &DataSegmentTable {
        &self.segments
    }

    /// Declare a new [DataSegment] in this module, with the given offset, size, and data.
    ///
    /// Returns `Err` if the segment declaration is invalid, or conflicts with an existing segment
    ///
    /// Data segments are ordered by the address at which they are allocated, at link-time, all
    /// segments from all modules are linked together, and they must either be disjoint, or exactly
    /// identical in order to overlap - it is not permitted to have partially overlapping segments
    /// with different views of the memory represented by that segment.
    pub fn declare_data_segment(
        &mut self,
        offset: Offset,
        size: u32,
        init: ConstantData,
        readonly: bool,
    ) -> Result<(), DataSegmentError> {
        self.segments.declare(offset, size, init, readonly)
    }

    /// Return the table of global variables for this module
    pub fn globals(&self) -> &GlobalVariableTable {
        &self.globals
    }

    /// Declare a new [GlobalVariable] in this module, with the given name, type, linkage, and
    /// optional initializer.
    ///
    /// Returns `Err` if a symbol with the same name but conflicting declaration already exists,
    /// or if the specification of the global variable is invalid in any way.
    ///
    /// NOTE: The [GlobalVariable] returned here is scoped to this module only, it cannot be used to
    /// index into the global variable table of a [Program], which is constructed at link-time.
    pub fn declare_global_variable(
        &mut self,
        name: Ident,
        ty: Type,
        linkage: Linkage,
        init: Option<ConstantData>,
    ) -> Result<GlobalVariable, GlobalVariableError> {
        self.globals.declare(name, ty, linkage, init)
    }

    /// Set the initializer for a [GlobalVariable] to `init`.
    ///
    /// Returns `Err` if the initializer conflicts with the current definition of the global in any
    /// way.
    pub fn set_global_initializer(
        &mut self,
        gv: GlobalVariable,
        init: ConstantData,
    ) -> Result<(), GlobalVariableError> {
        self.globals.set_initializer(gv, init)
    }

    /// Get the data associated with the given [GlobalVariable]
    #[inline]
    pub fn global(&self, id: GlobalVariable) -> &GlobalVariableData {
        self.globals.get(id)
    }

    /// Look up a global by `name`.
    pub fn find_global(&self, name: Ident) -> Option<&GlobalVariableData> {
        self.globals.find(name).map(|gv| self.globals.get(gv))
    }

    /// Find the first function in this module marked with the `entrypoint` attribute
    pub fn entrypoint(&self) -> Option<FunctionIdent> {
        self.functions.iter().find_map(|f| {
            if f.has_attribute(&symbols::Entrypoint) {
                Some(f.id)
            } else {
                None
            }
        })
    }

    /// Return an iterator over the functions in this module
    ///
    /// The iterator is double-ended, so can be used to traverse the module body in either direction
    pub fn functions<'a, 'b: 'a>(
        &'b self,
    ) -> intrusive_collections::linked_list::Iter<'a, FunctionListAdapter> {
        self.functions.iter()
    }

    /// Get a [Function] in this module by name, if available
    pub fn function<'a, 'b: 'a>(&'b self, id: Ident) -> Option<&'a Function> {
        self.cursor_at(id).get()
    }

    /// Compute the set of imports for this module, automatically aliasing modules when there
    /// are namespace conflicts
    pub fn imports(&self) -> ModuleImportInfo {
        let mut imports = ModuleImportInfo::default();
        let locals = self.functions.iter().map(|f| f.id).collect::<FxHashSet<FunctionIdent>>();

        for function in self.functions.iter() {
            for import in function.imports() {
                if !locals.contains(&import.id) {
                    imports.add(import.id);
                }
            }
        }
        imports
    }

    /// Returns true if this module contains the function `name`
    pub fn contains(&self, name: Ident) -> bool {
        self.function(name).is_some()
    }

    /// Unlinks the given function from this module
    pub fn unlink(&mut self, id: Ident) -> Box<Function> {
        let mut cursor = self.cursor_mut_at(id);
        cursor
            .remove()
            .unwrap_or_else(|| panic!("cursor pointing to a null when removing function id: {id}"))
    }

    /// Append `function` to the end of this module's body, returning the [FuncId]
    /// assigned to it within this module.
    ///
    /// NOTE: This function will panic if either of the following rules are violated:
    ///
    /// * If this module is a kernel module, public functions must use the kernel calling
    ///   convention, however private functions can use any convention.
    /// * If this module is not a kernel module, functions may not use the kernel calling convention
    pub fn push(&mut self, function: Box<Function>) -> Result<(), SymbolConflictError> {
        assert_valid_function!(self, function);
        if let Some(prev) = self.function(function.id.function) {
            return Err(SymbolConflictError(prev.id));
        }
        self.functions.push_back(function);
        Ok(())
    }

    /// Insert `function` in the module body before the function with id `before`
    ///
    /// If `before` is no longer attached to this module, `function` is added to
    /// the end of the module body.
    pub fn insert_before(
        &mut self,
        function: Box<Function>,
        before: Ident,
    ) -> Result<(), SymbolConflictError> {
        assert_valid_function!(self, function);
        if let Some(prev) = self.function(function.id.function) {
            return Err(SymbolConflictError(prev.id));
        }

        let mut cursor = self.cursor_mut_at(before);
        cursor.insert_before(function);

        Ok(())
    }

    /// Insert `function` in the module body after the function with id `after`
    ///
    /// If `after` is no longer attached to this module, `function` is added to
    /// the end of the module body.
    pub fn insert_after(
        &mut self,
        function: Box<Function>,
        after: Ident,
    ) -> Result<(), SymbolConflictError> {
        assert_valid_function!(self, function);
        if let Some(prev) = self.function(function.id.function) {
            return Err(SymbolConflictError(prev.id));
        }

        let mut cursor = self.cursor_mut_at(after);
        if cursor.is_null() {
            cursor.insert_before(function);
        } else {
            cursor.insert_after(function);
        }

        Ok(())
    }

    /// Remove the first function in the module, and return it, if present
    pub fn pop_front(&mut self) -> Option<Box<Function>> {
        self.functions.pop_front()
    }

    /// Returns a mutable cursor to the module body, starting at the first function.
    ///
    /// If the module body is empty, the returned cursor will point to the null object.
    ///
    /// NOTE: If one uses this cursor to insert a function that is invalid
    #[inline]
    pub fn cursor_mut<'a, 'b: 'a>(&'b mut self) -> ModuleCursor<'a> {
        ModuleCursor {
            cursor: self.functions.front_mut(),
            name: self.name,
            is_kernel: self.is_kernel,
        }
    }

    /// Returns a cursor to the module body, located at the function indicated by `id`.
    ///
    /// If no function with `id` is in the list, the returned cursor will point to the null object.
    pub fn cursor_at<'a, 'b: 'a>(&'b self, id: Ident) -> Cursor<'a, FunctionListAdapter> {
        let mut cursor = self.functions.front();
        while let Some(function) = cursor.get() {
            if function.id.function == id {
                break;
            }
            cursor.move_next();
        }
        cursor
    }

    /// Returns a mutable cursor to the module body, located at the function indicated by `id`.
    ///
    /// If no function with `id` is in the list, the returned cursor will point to the null object.
    pub fn cursor_mut_at<'a, 'b: 'a>(&'b mut self, id: Ident) -> ModuleCursor<'a> {
        let mut cursor = self.functions.front_mut();
        while let Some(function) = cursor.get() {
            if function.id.function == id {
                break;
            }
            cursor.move_next();
        }
        ModuleCursor {
            cursor,
            name: self.name,
            is_kernel: self.is_kernel,
        }
    }
}

pub struct ModuleCursor<'a> {
    cursor: CursorMut<'a, FunctionListAdapter>,
    name: Ident,
    is_kernel: bool,
}
impl ModuleCursor<'_> {
    /// Returns true if this cursor is pointing to the null object
    #[inline(always)]
    pub fn is_null(&self) -> bool {
        self.cursor.is_null()
    }

    /// Return a reference to the function pointed to by this cursor
    ///
    /// If the cursor is pointing to the null object, `None` is returned
    #[inline(always)]
    pub fn get(&self) -> Option<&Function> {
        self.cursor.get()
    }

    /// Insert a new function into the module after the cursor.
    ///
    /// If the cursor is pointing to the null object, the insert happens at the front of the list.
    ///
    /// NOTE: This function will panic if the function violates the validation rules for
    /// the module, i.e. must not be attached, follows kernel module rules when applicable.
    pub fn insert_after(&mut self, function: Box<Function>) {
        assert_valid_function!(self, function);
        self.cursor.insert_after(function);
    }

    /// Insert a new function into the module before the cursor.
    ///
    /// If the cursor is pointing to the null object, the insert happens at the end of the list.
    ///
    /// NOTE: This function will panic if the function violates the validation rules for
    /// the module, i.e. must not be attached, follows kernel module rules when applicable.
    pub fn insert_before(&mut self, function: Box<Function>) {
        assert_valid_function!(self, function);
        self.cursor.insert_before(function);
    }

    /// Moves this cursor to the next function in the module.
    ///
    /// If the cursor is pointing to the null object, then this moves the cursor to the front
    /// of the list. If at the end of the list, it moves to the null object.
    #[inline(always)]
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }

    /// Moves this cursor to the previous function in the module.
    ///
    /// If the cursor is pointing to the null object, then this moves the cursor to the end
    /// of the list. If at the front of the list, it moves to the null object.
    #[inline(always)]
    pub fn move_prev(&mut self) {
        self.cursor.move_prev();
    }

    /// Return a cursor pointing to the next function in the module.
    ///
    /// If this cursor is on the null object, then the returned cursor will be on the
    /// front of the list. If at the last element, then the returned cursor will on the
    /// null object.
    #[inline(always)]
    pub fn peek_next(&self) -> Cursor<'_, FunctionListAdapter> {
        self.cursor.peek_next()
    }

    /// Return a cursor pointing to the previous function in the module.
    ///
    /// If this cursor is on the null object, then the returned cursor will be on the
    /// end of the list. If at the first element, then the returned cursor will on the
    /// null object.
    #[inline(always)]
    pub fn peek_prev(&self) -> Cursor<'_, FunctionListAdapter> {
        self.cursor.peek_prev()
    }

    /// Removes the current function from the module.
    ///
    /// The cursor will be moved to the next function in the module, or the null object
    /// if we're at the end of the module.
    #[inline(always)]
    pub fn remove(&mut self) -> Option<Box<Function>> {
        self.cursor.remove()
    }
}

pub struct ModuleBuilder {
    module: Box<Module>,
}
impl From<Box<Module>> for ModuleBuilder {
    fn from(module: Box<Module>) -> Self {
        Self { module }
    }
}
impl ModuleBuilder {
    pub fn new<S: Into<Ident>>(name: S) -> Self {
        Self {
            module: Box::new(Module::new(name)),
        }
    }

    pub fn new_kernel<S: Into<Ident>>(name: S) -> Self {
        Self {
            module: Box::new(Module::new_kernel(name)),
        }
    }

    pub fn with_span(&mut self, span: SourceSpan) -> &mut Self {
        self.module.name = Ident::new(self.module.name.as_symbol(), span);
        self
    }

    pub fn with_docs<S: Into<String>>(&mut self, docs: S) -> &mut Self {
        self.module.docs = Some(docs.into());
        self
    }

    pub fn with_page_size(&mut self, page_size: u32) -> &mut Self {
        self.module.page_size = page_size;
        self
    }

    pub fn with_reserved_memory_pages(&mut self, num_pages: u32) -> &mut Self {
        self.module.reserved_memory_pages = num_pages;
        self
    }

    pub fn name(&self) -> Ident {
        self.module.name
    }

    pub fn declare_global_variable<S: AsRef<str>>(
        &mut self,
        name: S,
        ty: Type,
        linkage: Linkage,
        init: Option<ConstantData>,
        span: SourceSpan,
    ) -> Result<GlobalVariable, GlobalVariableError> {
        let name = Ident::new(Symbol::intern(name.as_ref()), span);
        self.module.declare_global_variable(name, ty, linkage, init)
    }

    pub fn set_global_initializer(
        &mut self,
        gv: GlobalVariable,
        init: ConstantData,
    ) -> Result<(), GlobalVariableError> {
        self.module.set_global_initializer(gv, init)
    }

    pub fn declare_data_segment<I: Into<ConstantData>>(
        &mut self,
        offset: Offset,
        size: u32,
        init: I,
        readonly: bool,
    ) -> Result<(), DataSegmentError> {
        self.module.declare_data_segment(offset, size, init.into(), readonly)
    }

    /// Start building a new function in this module
    pub fn function<'a, 'b: 'a, S: Into<Ident>>(
        &'b mut self,
        name: S,
        signature: Signature,
    ) -> Result<ModuleFunctionBuilder<'a>, SymbolConflictError> {
        let name = name.into();
        if let Some(prev) = self.module.function(name) {
            return Err(SymbolConflictError(prev.id));
        }

        let id = FunctionIdent {
            module: self.module.name,
            function: name,
        };
        let function = Box::new(Function::new(id, signature));
        let entry = function.dfg.entry;

        Ok(ModuleFunctionBuilder {
            builder: self,
            function,
            position: entry,
        })
    }

    pub fn build(self) -> Box<Module> {
        self.module
    }
}

pub struct ModuleFunctionBuilder<'m> {
    builder: &'m mut ModuleBuilder,
    function: Box<Function>,
    position: Block,
}
impl ModuleFunctionBuilder<'_> {
    pub fn with_span(&mut self, span: SourceSpan) -> &mut Self {
        self.function.id.function = Ident::new(self.function.id.function.as_symbol(), span);
        self
    }

    /// Get the fully-qualified name of the underlying function
    pub fn id(&self) -> FunctionIdent {
        self.function.id
    }

    /// Get the signature of the underlying function
    pub fn signature(&self) -> &Signature {
        &self.function.signature
    }

    pub fn module<'a, 'b: 'a>(&'b mut self) -> &'a mut ModuleBuilder {
        self.builder
    }

    #[inline(always)]
    pub fn data_flow_graph(&self) -> &DataFlowGraph {
        &self.function.dfg
    }

    #[inline(always)]
    pub fn data_flow_graph_mut(&mut self) -> &mut DataFlowGraph {
        &mut self.function.dfg
    }

    #[inline]
    pub fn entry_block(&self) -> Block {
        self.function.dfg.entry
    }

    #[inline]
    pub fn current_block(&self) -> Block {
        self.position
    }

    #[inline]
    pub fn switch_to_block(&mut self, block: Block) {
        self.position = block;
    }

    pub fn create_block(&mut self) -> Block {
        self.data_flow_graph_mut().create_block()
    }

    pub fn block_params(&self, block: Block) -> &[Value] {
        self.data_flow_graph().block_params(block)
    }

    pub fn append_block_param(&mut self, block: Block, ty: Type, span: SourceSpan) -> Value {
        self.data_flow_graph_mut().append_block_param(block, ty, span)
    }

    pub fn inst_results(&self, inst: Inst) -> &[Value] {
        self.data_flow_graph().inst_results(inst)
    }

    pub fn first_result(&self, inst: Inst) -> Value {
        self.data_flow_graph().first_result(inst)
    }

    pub fn set_attribute(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>) {
        self.data_flow_graph_mut().set_attribute(name, value);
    }

    pub fn import_function<M, F>(
        &mut self,
        module: M,
        function: F,
        signature: Signature,
    ) -> Result<FunctionIdent, SymbolConflictError>
    where
        M: Into<Ident>,
        F: Into<Ident>,
    {
        self.function.dfg.import_function(module.into(), function.into(), signature)
    }

    pub fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a> {
        DefaultInstBuilder::new(&mut self.function.dfg, self.position)
    }

    pub fn build(self, diagnostics: &DiagnosticsHandler) -> Result<FunctionIdent, Report> {
        let sig = self.function.signature();
        match sig.linkage {
            Linkage::External | Linkage::Internal => (),
            linkage => {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function definition")
                    .with_primary_label(
                        self.function.span(),
                        format!("invalid linkage: '{linkage}'"),
                    )
                    .with_help("Only 'external' and 'internal' linkage are valid for functions")
                    .into_report());
            }
        }

        let is_kernel_module = self.builder.module.is_kernel;
        let is_public = sig.is_public();

        match sig.cc {
            CallConv::Kernel if is_kernel_module => {
                if !is_public {
                    return Err(diagnostics
                        .diagnostic(Severity::Error)
                        .with_message("invalid function definition")
                        .with_primary_label(
                            self.function.span(),
                            format!("expected 'external' linkage, but got '{}'", &sig.linkage),
                        )
                        .with_help(
                            "Functions declared with the 'kernel' calling convention must have \
                             'external' linkage",
                        )
                        .into_report());
                }
            }
            CallConv::Kernel => {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function definition")
                    .with_primary_label(
                        self.function.span(),
                        "unsupported use of 'kernel' calling convention",
                    )
                    .with_help(
                        "The 'kernel' calling convention is only allowed in kernel modules, on \
                         functions with external linkage",
                    )
                    .into_report());
            }
            cc if is_kernel_module && is_public => {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid function definition")
                    .with_primary_label(
                        self.function.span(),
                        format!("unsupported use of '{cc}' calling convention"),
                    )
                    .with_help(
                        "Functions with external linkage, must use the 'kernel' calling \
                         convention when defined in a kernel module",
                    )
                    .into_report());
            }
            _ => (),
        }

        let id = self.function.id;
        self.builder.module.functions.push_back(self.function);

        Ok(id)
    }
}
