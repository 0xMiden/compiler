use cranelift_entity::entity_impl;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};

use self::formatter::PrettyPrint;
use crate::{
    diagnostics::{miette, Diagnostic, Spanned},
    *,
};

/// This error is raised when two function declarations conflict with the same symbol name
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("item named '{}' has already been declared, or cannot be merged", .0)]
#[diagnostic()]
pub struct SymbolConflictError(pub FunctionIdent);

/// A handle that refers to an [ExternalFunction]
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncRef(u32);
entity_impl!(FuncRef, "fn");

/// Represents the calling convention of a function.
///
/// Calling conventions are part of a program's ABI (Application Binary Interface), and
/// they define things such how arguments are passed to a function, how results are returned,
/// etc. In essence, the contract between caller and callee is described by the calling convention
/// of a function.
///
/// Importantly, it is perfectly normal to mix calling conventions. For example, the public
/// API for a C library will use whatever calling convention is used by C on the target
/// platform (for Miden, that would be `SystemV`). However, internally that library may use
/// the `Fast` calling convention to allow the compiler to optimize more effectively calls
/// from the public API to private functions. In short, choose a calling convention that is
/// well-suited for a given function, to the extent that other constraints don't impose a choice
/// on you.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum CallConv {
    /// This calling convention is what I like to call "chef's choice" - the
    /// compiler chooses it's own convention that optimizes for call performance.
    ///
    /// As a result of this, it is not permitted to use this convention in externally
    /// linked functions, as the convention is unstable, and the compiler can't ensure
    /// that the caller in another translation unit will use the correct convention.
    Fast,
    /// The standard calling convention used for C on most platforms
    #[default]
    SystemV,
    /// A function which is using the WebAssembly Component Model "Canonical ABI".
    Wasm,
    /// A function with this calling convention must be called using
    /// the `syscall` instruction. Attempts to call it with any other
    /// call instruction will cause a validation error. The one exception
    /// to this rule is when calling another function with the `Kernel`
    /// convention that is defined in the same module, which can use the
    /// standard `call` instruction.
    ///
    /// Kernel functions may only be defined in a kernel [Module].
    ///
    /// In all other respects, this calling convention is the same as `SystemV`
    Kernel,
    /// This calling convention is used for the synthetic functions generated for the Wasm CM
    /// export (`canon lift`), see
    /// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#lifting-and-lowering-values
    ///
    /// A function with this calling convention is expected to be called using the `call`
    /// instruction. Since execution moves to a different contexe he callee does not have access to
    /// caller's memory, so arguments and results are passed through the stack or through the
    /// advice provider.
    /// see https://0xpolygonmiden.github.io/miden-vm/user_docs/assembly/execution_contexts.html
    CanonLift,
    /// This calling convention is used for the synthetic functions generated for the Wasm CM
    /// import (`canon lower`), see
    /// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#lifting-and-lowering-values
    ///
    /// A function with this calling convention is expected to be called using the `exec` instruction.
    /// see https://0xpolygonmiden.github.io/miden-vm/user_docs/assembly/execution_contexts.html
    CanonLower,
}
impl fmt::Display for CallConv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Fast => f.write_str("fast"),
            Self::SystemV => f.write_str("C"),
            Self::Wasm => f.write_str("wasm"),
            Self::Kernel => f.write_str("kernel"),
            Self::CanonLift => f.write_str("lift"),
            Self::CanonLower => f.write_str("lower"),
        }
    }
}

/// Represents whether an argument or return value has a special purpose in
/// the calling convention of a function.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum ArgumentPurpose {
    /// No special purpose, the argument is passed/returned by value
    #[default]
    Default,
    /// Used for platforms where the calling convention expects return values of
    /// a certain size to be written to a pointer passed in by the caller.
    StructReturn,
}
impl fmt::Display for ArgumentPurpose {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::StructReturn => f.write_str("sret"),
        }
    }
}

/// Represents how to extend a small integer value to native machine integer width.
///
/// For Miden, native integrals are unsigned 64-bit field elements, but it is typically
/// going to be the case that we are targeting the subset of Miden Assembly where integrals
/// are unsigned 32-bit integers with a standard twos-complement binary representation.
///
/// It is for the latter scenario that argument extension is really relevant.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum ArgumentExtension {
    /// Do not perform any extension, high bits have undefined contents
    #[default]
    None,
    /// Zero-extend the value
    Zext,
    /// Sign-extend the value
    Sext,
}
impl fmt::Display for ArgumentExtension {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Zext => f.write_str("zext"),
            Self::Sext => f.write_str("sext"),
        }
    }
}

/// Describes a function parameter or result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AbiParam {
    /// The type associated with this value
    pub ty: Type,
    /// The special purpose, if any, of this parameter or result
    pub purpose: ArgumentPurpose,
    /// The desired approach to extending the size of this value to
    /// a larger bit width, if applicable.
    pub extension: ArgumentExtension,
}
impl AbiParam {
    pub fn new(ty: Type) -> Self {
        Self {
            ty,
            purpose: ArgumentPurpose::default(),
            extension: ArgumentExtension::default(),
        }
    }

    pub fn sret(ty: Type) -> Self {
        assert!(ty.is_pointer(), "sret parameters must be pointers");
        Self {
            ty,
            purpose: ArgumentPurpose::StructReturn,
            extension: ArgumentExtension::default(),
        }
    }
}
impl formatter::PrettyPrint for AbiParam {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let mut doc = const_text("(") + const_text("param") + const_text(" ");
        if !matches!(self.purpose, ArgumentPurpose::Default) {
            doc += const_text("(") + display(self.purpose) + const_text(")") + const_text(" ");
        }
        if !matches!(self.extension, ArgumentExtension::None) {
            doc += const_text("(") + display(self.extension) + const_text(")") + const_text(" ");
        }
        doc + text(format!("{}", &self.ty)) + const_text(")")
    }
}

/// A [Signature] represents the type, ABI, and linkage of a function.
///
/// A function signature provides us with all of the necessary detail to correctly
/// validate and emit code for a function, whether from the perspective of a caller,
/// or the callee.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Signature {
    /// The arguments expected by this function
    pub params: Vec<AbiParam>,
    /// The results returned by this function
    pub results: Vec<AbiParam>,
    /// The calling convention that applies to this function
    pub cc: CallConv,
    /// The linkage that should be used for this function
    pub linkage: Linkage,
}
impl Signature {
    /// Create a new signature with the given parameter and result types,
    /// for a public function using the `SystemV` calling convention
    pub fn new<P: IntoIterator<Item = AbiParam>, R: IntoIterator<Item = AbiParam>>(
        params: P,
        results: R,
    ) -> Self {
        Self {
            params: params.into_iter().collect(),
            results: results.into_iter().collect(),
            cc: CallConv::SystemV,
            linkage: Linkage::External,
        }
    }

    /// Returns true if this function is externally visible
    pub fn is_public(&self) -> bool {
        matches!(self.linkage, Linkage::External)
    }

    /// Returns true if this function is only visible within it's containing module
    pub fn is_private(&self) -> bool {
        matches!(self.linkage, Linkage::Internal)
    }

    /// Returns true if this function is a kernel function
    pub fn is_kernel(&self) -> bool {
        matches!(self.cc, CallConv::Kernel)
    }

    /// Returns the number of arguments expected by this function
    pub fn arity(&self) -> usize {
        self.params().len()
    }

    /// Returns a slice containing the parameters for this function
    pub fn params(&self) -> &[AbiParam] {
        self.params.as_slice()
    }

    /// Returns the parameter at `index`, if present
    #[inline]
    pub fn param(&self, index: usize) -> Option<&AbiParam> {
        self.params.get(index)
    }

    /// Returns a slice containing the results of this function
    pub fn results(&self) -> &[AbiParam] {
        match self.results.as_slice() {
            [AbiParam { ty: Type::Unit, .. }] => &[],
            [AbiParam {
                ty: Type::Never, ..
            }] => &[],
            results => results,
        }
    }
}
impl Eq for Signature {}
impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.linkage == other.linkage
            && self.cc == other.cc
            && self.params.len() == other.params.len()
            && self.results.len() == other.results.len()
    }
}
impl formatter::PrettyPrint for Signature {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let cc = if matches!(self.cc, CallConv::SystemV) {
            None
        } else {
            Some(
                const_text("(")
                    + const_text("cc")
                    + const_text(" ")
                    + display(self.cc)
                    + const_text(")"),
            )
        };

        let params = self.params.iter().fold(cc.unwrap_or(Document::Empty), |acc, param| {
            if acc.is_empty() {
                param.render()
            } else {
                acc + const_text(" ") + param.render()
            }
        });

        if self.results.is_empty() {
            params
        } else {
            let open = const_text("(") + const_text("result");
            let results = self
                .results
                .iter()
                .fold(open, |acc, e| acc + const_text(" ") + text(format!("{}", &e.ty)))
                + const_text(")");
            if matches!(params, Document::Empty) {
                results
            } else {
                params + const_text(" ") + results
            }
        }
    }
}

/// An [ExternalFunction] represents a function whose name and signature are known,
/// but which may or may not be compiled as part of the current translation unit.
///
/// When building a [Function], we use [ExternalFunction] to represent references to
/// other functions in the program which are called from its body. One "imports" a
/// function to make it callable.
///
/// At link time, we make sure all external function references are either defined in
/// the current program, or are well-known functions that are provided as part of a kernel
/// or standard library in the Miden VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFunction {
    pub id: FunctionIdent,
    pub signature: Signature,
}
impl Ord for ExternalFunction {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}
impl PartialOrd for ExternalFunction {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl formatter::PrettyPrint for ExternalFunction {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let header = const_text("(")
            + const_text("func")
            + const_text(" ")
            + const_text("(")
            + const_text("import")
            + const_text(" ")
            + display(self.id.module)
            + const_text(" ")
            + display(self.id.function)
            + const_text(")");
        let signature = (const_text(" ") + self.signature.render() + const_text(")"))
            | indent(6, nl() + self.signature.render() + const_text(")"));

        header + signature
    }
}

intrusive_adapter!(pub FunctionListAdapter = Box<Function>: Function { link: LinkedListLink });

/// A type alias for `LinkedList<FunctionListAdapter>`
pub type FunctionList = LinkedList<FunctionListAdapter>;

/// [Function] corresponds to a function definition, in single-static assignment (SSA) form.
///
/// * Functions may have zero or more parameters, and produce zero or more results.
/// * Functions are namespaced in [Module]s. You may define a function separately from a module,
///   to aid in parallelizing compilation, but functions must be attached to a module prior to code
///   generation. Furthermore, in order to reference other functions, you must do so using their
///   fully-qualified names.
/// * Functions consist of one or more basic blocks, where the entry block is predefined based
///   on the function signature.
/// * Basic blocks consist of a sequence of [Instruction] without any control flow (excluding
///   calls), terminating with a control flow instruction. Our SSA representation uses block
///   arguments rather than phi nodes to represent join points in the control flow graph.
/// * Instructions consume zero or more arguments, and produce zero or more results. Results
///   produced by an instruction constitute definitions of those values. A value may only ever have
///   a single definition, e.g. you can't reassign a value after it is introduced by an instruction.
///
/// References to functions and global variables from a [Function] are not fully validated until
/// link-time/code generation.
#[derive(Spanned, AnalysisKey)]
pub struct Function {
    link: LinkedListLink,
    #[span]
    #[analysis_key]
    pub id: FunctionIdent,
    pub signature: Signature,
    pub dfg: DataFlowGraph,
}
impl Function {
    /// Create a new [Function] with the given name, signature, and source location.
    ///
    /// The resulting function will be given default internal linkage, i.e. it will only
    /// be visible within it's containing [Module].
    pub fn new(id: FunctionIdent, signature: Signature) -> Self {
        let mut dfg = DataFlowGraph::default();
        let entry = dfg.entry_block();
        for param in signature.params() {
            dfg.append_block_param(entry, param.ty.clone(), id.span());
        }
        dfg.imports.insert(
            id,
            ExternalFunction {
                id,
                signature: signature.clone(),
            },
        );
        Self {
            link: Default::default(),
            id,
            signature,
            dfg,
        }
    }

    /// This function is like [Function::new], except it does not initialize the
    /// function entry block using the provided [Signature]. Instead, it is expected
    /// that the caller does this manually.
    ///
    /// This is primarily intended for use by the IR parser.
    pub(crate) fn new_uninit(id: FunctionIdent, signature: Signature) -> Self {
        let mut dfg = DataFlowGraph::new_uninit();
        dfg.imports.insert(
            id,
            ExternalFunction {
                id,
                signature: signature.clone(),
            },
        );
        Self {
            link: Default::default(),
            id,
            signature,
            dfg,
        }
    }

    /// Returns true if this function has yet to be attached to a [Module]
    pub fn is_detached(&self) -> bool {
        !self.link.is_linked()
    }

    /// Returns true if this function is a kernel function
    pub fn is_kernel(&self) -> bool {
        self.signature.is_kernel()
    }

    /// Returns true if this function has external linkage
    pub fn is_public(&self) -> bool {
        self.signature.is_public()
    }

    /// Return the [Signature] for this function
    #[inline]
    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Return the [Signature] for this function
    #[inline]
    pub fn signature_mut(&mut self) -> &mut Signature {
        &mut self.signature
    }

    /// Return the number of parameters this function expects
    pub fn arity(&self) -> usize {
        self.signature.arity()
    }

    /// Return the [Linkage] type for this function
    pub fn linkage(&self) -> Linkage {
        self.signature.linkage
    }

    /// Set the linkage type for this function
    pub fn set_linkage(&mut self, linkage: Linkage) {
        self.signature.linkage = linkage;
    }

    /// Return the [CallConv] type for this function
    pub fn calling_convention(&self) -> CallConv {
        self.signature.cc
    }

    /// Set the linkage type for this function
    pub fn set_calling_convention(&mut self, cc: CallConv) {
        self.signature.cc = cc;
    }

    /// Return true if this function has attribute `name`
    pub fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        Q: Ord + ?Sized,
        Symbol: std::borrow::Borrow<Q>,
    {
        self.dfg.has_attribute(name)
    }

    /// Iterate over all of the external functions imported by this function
    pub fn imports<'a, 'b: 'a>(&'b self) -> impl Iterator<Item = &'a ExternalFunction> + 'a {
        self.dfg.imports().filter(|ext| ext.id != self.id)
    }

    pub fn builder(&mut self) -> FunctionBuilder {
        FunctionBuilder::new(self)
    }

    pub fn cfg_printer(&self) -> impl fmt::Display + '_ {
        CfgPrinter { function: self }
    }
}
impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Function")
            .field("id", &self.id)
            .field("signature", &self.signature)
            .finish()
    }
}
impl formatter::PrettyPrint for Function {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let name = if self.is_public() {
            const_text("(")
                + const_text("export")
                + const_text(" ")
                + self.id.function.render()
                + const_text(")")
        } else {
            self.id.function.render()
        };
        let header = const_text("(") + const_text("func") + const_text(" ") + name;

        let signature =
            (const_text(" ") + self.signature.render()) | indent(6, nl() + self.signature.render());

        let body = self.dfg.blocks().fold(nl(), |acc, (_, block_data)| {
            let open = const_text("(")
                + const_text("block")
                + const_text(" ")
                + display(block_data.id.as_u32());
            let params = block_data
                .params(&self.dfg.value_lists)
                .iter()
                .map(|value| {
                    let ty = self.dfg.value_type(*value);
                    const_text("(")
                        + const_text("param")
                        + const_text(" ")
                        + display(*value)
                        + const_text(" ")
                        + text(format!("{ty}"))
                        + const_text(")")
                })
                .collect::<Vec<_>>();

            let params_singleline = params
                .iter()
                .cloned()
                .reduce(|acc, e| acc + const_text(" ") + e)
                .map(|params| const_text(" ") + params)
                .unwrap_or(Document::Empty);
            let params_multiline = params
                .into_iter()
                .reduce(|acc, e| acc + nl() + e)
                .map(|doc| indent(8, nl() + doc))
                .unwrap_or(Document::Empty);
            let header = open + (params_singleline | params_multiline);

            let body = indent(
                4,
                block_data
                    .insts()
                    .map(|inst| {
                        let inst_printer = crate::instruction::InstPrettyPrinter {
                            current_function: self.id,
                            id: inst,
                            dfg: &self.dfg,
                        };
                        inst_printer.render()
                    })
                    .reduce(|acc, doc| acc + nl() + doc)
                    .map(|body| nl() + body)
                    .unwrap_or_default(),
            );

            if matches!(acc, Document::Newline) {
                indent(4, acc + header + body + const_text(")"))
            } else {
                acc + nl() + indent(4, nl() + header + body + const_text(")"))
            }
        });

        header + signature + body + nl() + const_text(")")
    }
}
impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl Eq for Function {}
impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        let is_eq = self.id == other.id && self.signature == other.signature;
        if !is_eq {
            return false;
        }

        // We expect the entry block to be the same
        if self.dfg.entry != other.dfg.entry {
            return false;
        }

        // We expect the blocks to be laid out in the same order, and to have the same parameter
        // lists
        for (block_id, block) in self.dfg.blocks() {
            if let Some(other_block) = other.dfg.blocks.get(block_id) {
                if block.params.as_slice(&self.dfg.value_lists)
                    != other_block.params.as_slice(&other.dfg.value_lists)
                {
                    return false;
                }
                // We expect the instructions in each block to be the same
                if !block
                    .insts
                    .iter()
                    .map(|i| InstructionWithValueListPool {
                        inst: i,
                        value_lists: &self.dfg.value_lists,
                    })
                    .eq(other_block.insts.iter().map(|i| InstructionWithValueListPool {
                        inst: i,
                        value_lists: &other.dfg.value_lists,
                    }))
                {
                    return false;
                }
            } else {
                return false;
            }
        }

        // We expect both functions to have the same imports
        self.dfg.imports == other.dfg.imports
    }
}
impl midenc_session::Emit for Function {
    fn name(&self) -> Option<crate::Symbol> {
        Some(self.id.function.as_symbol())
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
            "binary mode is not supported for HIR functions"
        );
        writer.write_fmt(format_args!("{}\n", self))
    }
}

struct CfgPrinter<'a> {
    function: &'a Function,
}
impl fmt::Display for CfgPrinter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::collections::{BTreeSet, VecDeque};

        f.write_str("flowchart TB\n")?;

        let mut block_q = VecDeque::from([self.function.dfg.entry_block()]);
        let mut visited = BTreeSet::<Block>::default();
        while let Some(block_id) = block_q.pop_front() {
            if !visited.insert(block_id) {
                continue;
            }
            if let Some(last_inst) = self.function.dfg.last_inst(block_id) {
                match self.function.dfg.analyze_branch(last_inst) {
                    BranchInfo::NotABranch => {
                        // Must be a return or unreachable, print opcode for readability
                        let opcode = self.function.dfg.inst(last_inst).opcode();
                        writeln!(f, "    {block_id} --> {opcode}")?;
                    }
                    BranchInfo::SingleDest(info) => {
                        assert!(
                            self.function.dfg.is_block_linked(info.destination),
                            "reference to detached block in attached block {}",
                            info.destination
                        );
                        writeln!(f, "    {block_id} --> {}", info.destination)?;
                        block_q.push_back(info.destination);
                    }
                    BranchInfo::MultiDest(ref infos) => {
                        for info in infos {
                            assert!(
                                self.function.dfg.is_block_linked(info.destination),
                                "reference to detached block in attached block {}",
                                info.destination
                            );
                            writeln!(f, "    {block_id} --> {}", info.destination)?;
                            block_q.push_back(info.destination);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
