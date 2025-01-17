use std::{collections::BTreeSet, fmt, sync::Arc};

use cranelift_entity::EntityRef;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListAtomicLink};
use miden_assembly::{
    ast::{self, ProcedureName},
    LibraryNamespace, LibraryPath,
};
use midenc_hir::{
    diagnostics::{SourceSpan, Span, Spanned},
    formatter::PrettyPrint,
    AttributeSet, FunctionIdent, Ident, Signature, Type,
};
use smallvec::SmallVec;

use super::*;

intrusive_adapter!(pub FunctionListAdapter = Box<Function>: Function { link: LinkedListAtomicLink });
intrusive_adapter!(pub FrozenFunctionListAdapter = Arc<Function>: Function { link: LinkedListAtomicLink });

/// This represents a function in Miden Assembly
#[derive(Spanned, Clone)]
pub struct Function {
    link: LinkedListAtomicLink,
    #[span]
    pub span: SourceSpan,
    /// The attributes associated with this function
    pub attrs: AttributeSet,
    /// The name of this function
    pub name: FunctionIdent,
    /// The type signature of this function
    pub signature: Signature,
    /// The [Region] which forms the body of this function
    pub body: Region,
    /// The set of procedures invoked from the body of this function
    invoked: BTreeSet<ast::Invoke>,
    /// Locals allocated for this function
    locals: SmallVec<[Local; 1]>,
    /// The next available local index
    next_local_id: usize,
}
impl Function {
    pub fn new(name: FunctionIdent, signature: Signature) -> Self {
        Self {
            link: Default::default(),
            span: SourceSpan::UNKNOWN,
            attrs: Default::default(),
            name,
            signature,
            body: Default::default(),
            invoked: Default::default(),
            locals: Default::default(),
            next_local_id: 0,
        }
    }

    /// Returns true if this function is decorated with the `entrypoint` attribute.
    pub fn is_entrypoint(&self) -> bool {
        use midenc_hir::symbols;

        self.attrs.has(&symbols::Entrypoint)
    }

    /// Return the number of arguments expected on the operand stack
    #[inline]
    pub fn arity(&self) -> usize {
        self.signature.arity()
    }

    /// Return the number of results produced by this function
    #[inline]
    pub fn num_results(&self) -> usize {
        self.signature.results.len()
    }

    /// Allocate a new local in this function, using the provided data
    ///
    /// The index of the local is returned as it's identifier
    pub fn alloc_local(&mut self, ty: Type) -> LocalId {
        let num_words = ty.size_in_words();
        let next_id = self.next_local_id;
        assert!(
            (next_id + num_words) < (u8::MAX as usize),
            "unable to allocate a local of type {}: unable to allocate enough local memory",
            &ty
        );
        let id = LocalId::new(next_id);
        self.next_local_id += num_words;
        let local = Local { id, ty };
        self.locals.push(local);
        id
    }

    /// Allocate `n` locals for use by this function.
    ///
    /// Each local can be independently accessed, but they are all of type `Felt`
    pub fn alloc_n_locals(&mut self, n: u16) {
        assert!(
            (self.next_local_id + n as usize) < u16::MAX as usize,
            "unable to allocate {n} locals"
        );

        let num_locals = self.locals.len();
        self.locals.resize_with(num_locals + n as usize, || {
            let id = LocalId::new(self.next_local_id);
            self.next_local_id += 1;
            Local { id, ty: Type::Felt }
        });
    }

    /// Get the local with the given identifier
    pub fn local(&self, id: LocalId) -> &Local {
        self.locals.iter().find(|l| l.id == id).expect("invalid local id")
    }

    /// Return the locals allocated in this function as a slice
    #[inline]
    pub fn locals(&self) -> &[Local] {
        self.locals.as_slice()
    }

    /// Get a reference to the entry block for this function
    pub fn body(&self) -> &Block {
        self.body.block(self.body.body)
    }

    /// Get a mutable reference to the entry block for this function
    pub fn body_mut(&mut self) -> &mut Block {
        self.body.block_mut(self.body.body)
    }

    /// Allocate a new code block in this function
    #[inline(always)]
    pub fn create_block(&mut self) -> BlockId {
        self.body.create_block()
    }

    /// Get a reference to a [Block] by [BlockId]
    #[inline(always)]
    pub fn block(&self, id: BlockId) -> &Block {
        self.body.block(id)
    }

    /// Get a mutable reference to a [Block] by [BlockId]
    #[inline(always)]
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        self.body.block_mut(id)
    }

    pub fn invoked(&self) -> impl Iterator<Item = &ast::Invoke> + '_ {
        self.invoked.iter()
    }

    pub fn register_invoked(&mut self, kind: ast::InvokeKind, target: ast::InvocationTarget) {
        self.invoked.insert(ast::Invoke { kind, target });
    }

    #[inline(never)]
    pub fn register_absolute_invocation_target(
        &mut self,
        kind: ast::InvokeKind,
        target: FunctionIdent,
    ) {
        let module_name_span = target.module.span;
        let module_id = ast::Ident::new_unchecked(Span::new(
            module_name_span,
            Arc::from(target.module.as_str().to_string().into_boxed_str()),
        ));
        let name_span = target.function.span;
        let id = ast::Ident::new_unchecked(Span::new(
            name_span,
            Arc::from(target.function.as_str().to_string().into_boxed_str()),
        ));
        let path = LibraryPath::new(target.module.as_str()).unwrap_or_else(|_| {
            LibraryPath::new_from_components(LibraryNamespace::Anon, [module_id])
        });
        let name = ast::ProcedureName::new_unchecked(id);
        self.register_invoked(kind, ast::InvocationTarget::AbsoluteProcedurePath { name, path });
    }

    /// Return an implementation of [std::fmt::Display] for this function
    pub fn display<'a, 'b: 'a>(&'b self, imports: &'b ModuleImportInfo) -> DisplayMasmFunction<'a> {
        DisplayMasmFunction {
            function: self,
            imports,
        }
    }

    pub fn from_ast(module: Ident, proc: &ast::Procedure) -> Box<Self> {
        use midenc_hir::{Linkage, Symbol};

        let proc_span = proc.name().span();
        let proc_name = Symbol::intern(AsRef::<str>::as_ref(proc.name()));
        let id = FunctionIdent {
            module,
            function: Ident::new(proc_name, proc_span),
        };

        let mut signature = Signature::new(vec![], vec![]);
        let visibility = proc.visibility();
        if !visibility.is_exported() {
            signature.linkage = Linkage::Internal;
        } else if visibility.is_syscall() {
            signature.cc = midenc_hir::CallConv::Kernel;
        }

        let mut function = Box::new(Self::new(id, signature));
        if proc.is_entrypoint() {
            function.attrs.set(midenc_hir::attributes::ENTRYPOINT);
        }

        function.alloc_n_locals(proc.num_locals());

        function.invoked.extend(proc.invoked().cloned());
        function.body = Region::from_block(module, proc.body());

        function
    }

    pub fn to_ast(
        &self,
        imports: &midenc_hir::ModuleImportInfo,
        locals: &BTreeSet<FunctionIdent>,
        tracing_enabled: bool,
    ) -> ast::Procedure {
        let visibility = if self.signature.is_kernel() {
            ast::Visibility::Syscall
        } else if self.signature.is_public() {
            ast::Visibility::Public
        } else {
            ast::Visibility::Private
        };

        let id = ast::Ident::new_unchecked(Span::new(
            self.name.function.span,
            Arc::from(self.name.function.as_str().to_string().into_boxed_str()),
        ));
        let name = ast::ProcedureName::new_unchecked(id);

        let mut body = self.body.to_block(imports, locals);

        // Emit trace events on entry/exit from the procedure body, if not already present
        if tracing_enabled {
            emit_trace_frame_events(self.span, &mut body);
        }

        let num_locals = u16::try_from(self.locals.len()).expect("too many locals");
        let mut proc = ast::Procedure::new(self.span, visibility, name, num_locals, body);
        proc.extend_invoked(self.invoked().cloned());
        proc
    }
}

fn emit_trace_frame_events(span: SourceSpan, body: &mut ast::Block) {
    use midenc_hir::{TRACE_FRAME_END, TRACE_FRAME_START};

    let ops = body.iter().as_slice();
    let has_frame_start = match ops.get(1) {
        Some(ast::Op::Inst(inst)) => match inst.inner() {
            ast::Instruction::Trace(imm) => {
                matches!(imm, ast::Immediate::Value(val) if val.into_inner() == TRACE_FRAME_START)
            }
            _ => false,
        },
        _ => false,
    };

    // If we have the frame start event, we do not need to emit any further events
    if has_frame_start {
        return;
    }

    // Because [ast::Block] does not have a mutator that lets us insert an op at the start, we need
    // to push the events at the end, then use access to the mutable slice via `iter_mut` to move
    // elements around.
    body.push(ast::Op::Inst(Span::new(span, ast::Instruction::Nop)));
    body.push(ast::Op::Inst(Span::new(span, ast::Instruction::Trace(TRACE_FRAME_END.into()))));
    body.push(ast::Op::Inst(Span::new(span, ast::Instruction::Nop)));
    body.push(ast::Op::Inst(Span::new(
        span,
        ast::Instruction::Trace(TRACE_FRAME_START.into()),
    )));
    let ops = body.iter_mut().into_slice();
    ops.rotate_right(2);
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Function")
            .field("name", &self.name)
            .field("signature", &self.signature)
            .field("attrs", &self.attrs)
            .field("locals", &self.locals)
            .field("body", &self.body)
            .finish()
    }
}

#[doc(hidden)]
pub struct DisplayMasmFunction<'a> {
    function: &'a Function,
    imports: &'a ModuleImportInfo,
}
impl midenc_hir::formatter::PrettyPrint for DisplayMasmFunction<'_> {
    fn render(&self) -> midenc_hir::formatter::Document {
        use midenc_hir::formatter::*;

        if self.function.name.module.as_str() == LibraryNamespace::EXEC_PATH
            && self.function.name.function.as_str() == ProcedureName::MAIN_PROC_NAME
        {
            let body = self.function.body.display(Some(self.function.name), self.imports);
            return indent(4, const_text("begin") + nl() + body.render())
                + nl()
                + const_text("end")
                + nl();
        }

        let visibility = if self.function.signature.is_kernel() {
            ast::Visibility::Syscall
        } else if self.function.signature.is_public() {
            ast::Visibility::Public
        } else {
            ast::Visibility::Private
        };
        let name = if ast::Ident::validate(self.function.name.function).is_ok() {
            text(self.function.name.function.as_str())
        } else {
            text(format!("\"{}\"", self.function.name.function.as_str()))
        };
        let mut doc = display(visibility) + const_text(".") + name;
        if !self.function.locals.is_empty() {
            doc += const_text(".") + display(self.function.locals.len());
        }

        let body = self.function.body.display(Some(self.function.name), self.imports);
        doc + indent(4, nl() + body.render()) + nl() + const_text("end") + nl() + nl()
    }
}
impl fmt::Display for DisplayMasmFunction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

pub type FunctionList = LinkedList<FunctionListAdapter>;
pub type FunctionListIter<'a> = intrusive_collections::linked_list::Iter<'a, FunctionListAdapter>;

pub type FrozenFunctionList = LinkedList<FrozenFunctionListAdapter>;
pub type FrozenFunctionListIter<'a> =
    intrusive_collections::linked_list::Iter<'a, FrozenFunctionListAdapter>;

pub(super) enum Functions {
    Open(FunctionList),
    Frozen(FrozenFunctionList),
}
impl Clone for Functions {
    fn clone(&self) -> Self {
        match self {
            Self::Open(list) => {
                let mut new_list = FunctionList::new(Default::default());
                for f in list.iter() {
                    new_list.push_back(Box::new(f.clone()));
                }
                Self::Open(new_list)
            }
            Self::Frozen(list) => {
                let mut new_list = FrozenFunctionList::new(Default::default());
                for f in list.iter() {
                    new_list.push_back(Arc::from(Box::new(f.clone())));
                }
                Self::Frozen(new_list)
            }
        }
    }
}
impl Default for Functions {
    fn default() -> Self {
        Self::Open(Default::default())
    }
}
impl Functions {
    pub fn iter(&self) -> impl Iterator<Item = &Function> + '_ {
        match self {
            Self::Open(ref list) => FunctionsIter::Open(list.iter()),
            Self::Frozen(ref list) => FunctionsIter::Frozen(list.iter()),
        }
    }

    pub fn push_back(&mut self, function: Box<Function>) {
        match self {
            Self::Open(ref mut list) => {
                list.push_back(function);
            }
            Self::Frozen(_) => panic!("cannot insert function into frozen module"),
        }
    }

    pub fn freeze(&mut self) {
        if let Self::Open(ref mut functions) = self {
            let mut frozen = FrozenFunctionList::default();

            while let Some(function) = functions.pop_front() {
                frozen.push_back(Arc::from(function));
            }

            *self = Self::Frozen(frozen);
        }
    }
}

enum FunctionsIter<'a> {
    Open(FunctionListIter<'a>),
    Frozen(FrozenFunctionListIter<'a>),
}
impl<'a> Iterator for FunctionsIter<'a> {
    type Item = &'a Function;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Open(ref mut iter) => iter.next(),
            Self::Frozen(ref mut iter) => iter.next(),
        }
    }
}
