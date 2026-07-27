use alloc::sync::Arc;
use core::{fmt, ops::ControlFlow};

use miden_assembly::{Path, ProjectSourceInputs, ast::InvocationTarget};
use miden_core::Word;
use midenc_hir::{constants::ConstantData, dialects::builtin, interner::Symbol};
use midenc_session::{
    Emit, OutputMode, OutputType, Session, Writer,
    diagnostics::{IntoDiagnostic, Report, SourceSpan, Span, WrapErr},
};

use crate::{Event, lower::NativePtr, masm};

pub struct MasmComponent {
    pub id: Option<builtin::ComponentId>,
    /// The path of the root module for this component
    ///
    /// All components must have a canonical root module, even if empty
    pub root: Arc<Path>,
    /// The symbol name of the component initializer function
    ///
    /// This function is responsible for initializing global variables and writing data segments
    /// into memory at program startup, and at cross-context call boundaries (in callee prologue).
    pub init: Option<masm::InvocationTarget>,
    /// The symbol name of the program entrypoint, if this component is executable.
    ///
    /// If unset, it indicates that the component is a library, even if it could be made executable.
    pub entrypoint: Option<masm::InvocationTarget>,
    /// The rodata segments of this component keyed by the offset of the segment
    pub rodata: Vec<Rodata>,
    /// The address of the start of the global heap
    pub heap_base: u32,
    /// The address of the `__stack_pointer` global, if such a global has been defined
    pub stack_pointer: Option<u32>,
    /// The set of modules in this component
    pub modules: Vec<Arc<masm::Module>>,
}

impl Emit for MasmComponent {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Masm
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        if mode != OutputMode::Text {
            anyhow::bail!("masm emission does not support binary mode");
        }
        writer.write_fmt(core::format_args!("{self}"))?;
        Ok(())
    }
}

/// Represents a read-only data segment, combined with its content digest
#[derive(Clone, PartialEq, Eq)]
pub struct Rodata {
    /// The component to which this read-only data segment belongs
    pub component: builtin::ComponentId,
    /// The content digest computed for `data`
    pub digest: Word,
    /// The address at which the data for this segment begins
    pub start: NativePtr,
    /// The raw binary data for this segment
    pub data: Arc<ConstantData>,
}
impl fmt::Debug for Rodata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rodata")
            .field("digest", &format_args!("{}", &self.digest))
            .field("start", &self.start)
            .field_with("data", |f| {
                f.debug_struct("ConstantData")
                    .field("len", &self.data.len())
                    .finish_non_exhaustive()
            })
            .finish()
    }
}
impl Rodata {
    pub fn size_in_bytes(&self) -> usize {
        self.data.len()
    }

    pub fn size_in_felts(&self) -> usize {
        self.data.len().div_ceil(4)
    }

    pub fn size_in_words(&self) -> usize {
        self.size_in_felts().div_ceil(4)
    }

    /// Attempt to convert this rodata object to its equivalent representation in felts
    ///
    /// See [Self::bytes_to_elements] for more details.
    pub fn to_elements(&self) -> Vec<miden_processor::Felt> {
        Self::bytes_to_elements(self.data.as_slice())
    }

    /// Attempt to convert the given bytes to their equivalent representation in felts
    ///
    /// The resulting felts will be in padded out to the nearest number of words, i.e. if the data
    /// only takes up 3 felts worth of bytes, then the resulting `Vec` will contain 4 felts, so that
    /// the total size is a valid number of words.
    pub fn bytes_to_elements(bytes: &[u8]) -> Vec<miden_processor::Felt> {
        use miden_processor::Felt;

        let mut felts = Vec::with_capacity(bytes.len() / 4);
        let mut iter = bytes.iter().copied().array_chunks::<4>();
        felts.extend(
            iter.by_ref().map(|chunk| Felt::new_unchecked(u32::from_le_bytes(chunk) as u64)),
        );
        let remainder = iter.into_remainder();
        if remainder.len() > 0 {
            let mut chunk = [0u8; 4];
            for (i, byte) in remainder.enumerate() {
                chunk[i] = byte;
            }
            felts.push(Felt::new_unchecked(u32::from_le_bytes(chunk) as u64));
        }

        let size_in_felts = bytes.len().div_ceil(4);
        let size_in_words = size_in_felts.div_ceil(4);
        let padding = (size_in_words * 4).abs_diff(felts.len());
        felts.resize(felts.len() + padding, Felt::ZERO);
        debug_assert_eq!(felts.len() % 4, 0, "expected to be a valid number of words");
        felts
    }
}

inventory::submit! {
    midenc_session::CompileFlag::new("test_harness")
        .long("test-harness")
        .action(midenc_session::FlagAction::SetTrue)
        .help("If present, causes the code generator to emit extra code for the VM test harness")
        .help_heading("Testing")
}

impl fmt::Display for MasmComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for module in self.modules.iter() {
            writeln!(f, "{module}")?;
        }
        Ok(())
    }
}

impl MasmComponent {
    pub fn source_inputs(
        &self,
        target: &midenc_session::miden_project::Target,
        session: &Session,
    ) -> Result<ProjectSourceInputs, Report> {
        let is_executable_target = target.is_executable();
        let emit_test_harness = session.get_flag("test_harness");
        let mut support = Vec::with_capacity(self.modules.len());
        let mut root = None;
        for module in self.modules.iter() {
            if module.path() == self.root.as_ref() {
                root = Some(Box::new(Arc::unwrap_or_clone(module.clone())));
                continue;
            }

            support.push(Box::new(Arc::unwrap_or_clone(module.clone())));
        }

        if is_executable_target && let Some(entrypoint) = self.entrypoint.as_ref() {
            // Our generated main module takes precedence here, so move the root module into support
            support.extend(root);
            let root =
                self.generate_main(entrypoint, emit_test_harness, session.source_manager.clone())?;
            return Ok(ProjectSourceInputs { root, support });
        }

        let mut root = root.expect("components must always have a root module");

        // A library-like target's root module must sit exactly at the target's namespace, or
        // the assembler rejects the whole target (`load_target_sources`). Two of the shapes that
        // reach here can never satisfy that on their own, and both for the same reason: the root
        // code generation gave them is not a name any source declares, so no namespace derived
        // from the source can equal it. See [`Self::has_no_authored_identity`].
        //
        // Re-rooting is correct rather than merely expedient, for that same reason. The target's
        // namespace is the name the author *did* choose, and is where they expect this library's
        // procedures to be addressable from; the root being replaced is one the compiler picked
        // on their behalf and never told them about. A component whose id its author wrote is
        // left exactly where it is: that id is part of the code's own identity, and moving it
        // would silently rename the procedures every dependent addresses.
        //
        // Nothing is done here for an executable, which is handled above: its root is discarded
        // in favor of the generated `$exec` module, so its namespace already agrees.
        //
        // The equality check is what keeps a target that *already* agrees from being rewritten to
        // itself. That is the ordinary case for a component-less world with one top-level module,
        // whose root is that module's name and whose synthesized namespace is read from the very
        // same declaration; it is also the case for a manifest that declares
        // `namespace = "root_ns:root@1.0.0"`, which is how projects worked around the wrapper
        // before it was fixed.
        //
        // Only the modules handed to the assembler move. `MasmComponent`'s own `root`, `init` and
        // `entrypoint` still name the old root afterwards, which is why a library's `--emit=masm`
        // document (written from the component, not from these inputs) shows it while the
        // assembled package uses the target's namespace. Neither field has a consumer on this
        // path — `init` is invoked symbolically from within the component, and `entrypoint` is
        // only read by `generate_main` on the executable branch above — so they are left as they
        // are rather than rewritten to no effect.
        let namespace = target.namespace.inner();
        if self.has_no_authored_identity() && self.root.as_ref() != namespace.as_ref() {
            let mut rebase = Rebase {
                from: &self.root,
                to: namespace,
            };
            rebase.apply(&mut root);
            for module in support.iter_mut() {
                rebase.apply(module);
            }
        }

        Ok(ProjectSourceInputs { root, support })
    }

    /// Returns true if this component declares no identity its author chose, and so belongs
    /// wherever its target says rather than where code generation put it.
    ///
    /// Two shapes qualify:
    ///
    /// - **The synthetic wrapper** the Wasm frontend builds around every *core* Wasm module
    ///   (`frontend/wasm`'s `build_ir_component`). Its identity is the same for every such build
    ///   and carries no information about this one, and `ComponentId::to_library_path` renders it
    ///   as the single quoted component `"root_ns:root@1.0.0"` — a spelling no target is named.
    /// - **A world declaring no component**, which has no id at all. Its modules "belong to one
    ///   logical component, which has no identity beyond the namespace those modules sit in"
    ///   (`world_body_to_masm_component`), so lowering has to invent a root: the placeholder
    ///   constant `::init` for zero or several top-level modules, and that module's own name for
    ///   exactly one.
    ///
    /// A component whose id its author wrote is the complement, and is left where it is: that id
    /// is part of the code's identity, and moving it would rename the procedures every dependent
    /// addresses.
    ///
    /// # Why one top-level module is not carved out
    ///
    /// `::{module}` *is* a name the file says, so it is the one root here that could arguably be
    /// preserved. It is not, for three reasons.
    ///
    /// First, it costs the ordinary case nothing: preparation synthesizes that target's namespace
    /// by reading the very same declaration (`hir_declared_namespace` in `midenc-compile`), so
    /// the two agree and the equality guard in [`Self::source_inputs`] makes the rewrite a no-op.
    /// Second, `MasmComponent` has no way to tell the two roots apart — nothing records how many
    /// top-level modules the world had — so carving it out would mean either comparing `root`
    /// against the literal `"::init"`, which is exactly the one-value-in-two-places duplication
    /// preparation refused, or threading a flag down from lowering for a case that is a no-op.
    /// Third, a world is not a component: a module's name says where its procedures sit *within*
    /// a namespace, not what that namespace is, so a target that names a different one is not
    /// contradicting the file the way a component id would be.
    fn has_no_authored_identity(&self) -> bool {
        match self.id.as_ref() {
            Some(id) => id.is_synthetic_wrapper(),
            None => true,
        }
    }

    /// Generate an executable module which when run expects the raw data segment data to be
    /// provided on the advice stack in the same order as initialization, and the operands of
    /// the entrypoint function on the operand stack.
    fn generate_main(
        &self,
        entrypoint: &InvocationTarget,
        emit_test_harness: bool,
        source_manager: Arc<dyn midenc_session::SourceManager>,
    ) -> Result<Box<masm::Module>, Report> {
        use masm::{Instruction as Inst, Op};

        let mut exe = Box::new(masm::Module::new_executable());
        let span = SourceSpan::default();
        let mut invoked = Vec::new();
        let body = {
            let mut block = masm::Block::new(span, Vec::with_capacity(64));
            // Invoke component initializer, if present
            if let Some(init) = self.init.as_ref() {
                invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, init.clone()));
                block.push(Op::Inst(Span::new(span, Inst::Exec(init.clone()))));
            }

            // Initialize test harness, if requested
            if emit_test_harness {
                self.emit_test_harness(&mut block);
            }

            // Invoke the program entrypoint
            block.push(Op::Inst(Span::new(
                span,
                Inst::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            )));
            invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, entrypoint.clone()));
            block.push(Op::Inst(Span::new(span, Inst::Exec(entrypoint.clone()))));
            block.push(Op::Inst(Span::new(
                span,
                Inst::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()),
            )));

            // Truncate the stack to 16 elements on exit
            let truncate_stack = {
                let name = masm::ProcedureName::new("truncate_stack").unwrap();
                let module = masm::LibraryPath::new("::miden::core::sys").unwrap();
                let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
                InvocationTarget::Path(Span::new(span, qualified.into_inner()))
            };
            invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, truncate_stack.clone()));
            block.push(Op::Inst(Span::new(span, Inst::Exec(truncate_stack))));
            block
        };
        let mut start = masm::Procedure::new(
            span,
            masm::Visibility::Public,
            masm::ProcedureName::main(),
            0,
            body,
        );
        start.extend_invoked(invoked);
        exe.define_procedure(start, source_manager)
            .into_diagnostic()
            .wrap_err("failed to define executable `main` procedure")?;
        Ok(exe)
    }

    fn emit_test_harness(&self, block: &mut masm::Block) {
        use masm::{Instruction as Inst, IntValue, Op, PushValue};
        use miden_core::Felt;

        let span = SourceSpan::default();

        let pipe_words_to_memory = {
            let name = masm::ProcedureName::new("pipe_words_to_memory").unwrap();
            let module = masm::LibraryPath::new("::miden::core::mem").unwrap();
            let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
            InvocationTarget::Path(Span::new(span, qualified.into_inner()))
        };

        // Step 1: Get the number of initializers to run
        // => [inits] on operand stack
        block.push(Op::Inst(Span::new(span, Inst::AdvPush)));

        // Step 2: Evaluate the initial state of the loop condition `inits > 0`
        // => [inits, inits]
        block.push(Op::Inst(Span::new(span, Inst::Dup0)));
        // => [inits > 0, inits]
        block.push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
        block.push(Op::Inst(Span::new(span, Inst::Gt)));

        // Step 3: Loop until `inits == 0`
        let mut loop_body = Vec::with_capacity(16);

        // State of operand stack on entry to `loop_body`: [inits]
        // State of advice stack on entry to `loop_body`: [dest_ptr, num_words, ...]
        //
        // Step 3a: Compute next value of `inits`, i.e. `inits'`
        // => [inits - 1]
        loop_body.push(Op::Inst(Span::new(span, Inst::SubImm(Felt::ONE.into()))));

        // Step 3b: Copy initializer data to memory
        // => [num_words, dest_ptr, inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::AdvPush)));
        loop_body.push(Op::Inst(Span::new(span, Inst::AdvPush)));
        // => [C, B, A, dest_ptr, inits'] on operand stack
        loop_body.push(Op::Inst(Span::new(
            span,
            Inst::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
        )));
        loop_body.push(Op::Inst(Span::new(span, Inst::Exec(pipe_words_to_memory))));
        loop_body.push(Op::Inst(Span::new(
            span,
            Inst::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()),
        )));
        // Drop C, B, A
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        // => [inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::Drop)));

        // Step 3c: Evaluate loop condition `inits' > 0`
        // => [inits', inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::Dup0)));
        // => [inits' > 0, inits']
        loop_body
            .push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
        loop_body.push(Op::Inst(Span::new(span, Inst::Gt)));

        // Step 4: Enter (or skip) loop
        block.push(Op::While {
            span,
            body: masm::Block::new(span, loop_body),
        });

        // Step 5: Drop `inits` after loop is evaluated
        block.push(Op::Inst(Span::new(span, Inst::Drop)));
    }
}

/// Moves a component's modules from one root path to another, in place.
///
/// A component's modules are *nested under* its root — code generation defines them relative to
/// it (`MasmComponentBuilder::define_module`) — and the calls between them are emitted as
/// absolute paths carrying that same root. So moving the root is not a matter of renaming one
/// module: every module path and every intra-component invocation target has to move with it, or
/// the root ends up declaring submodules that do not exist and the procedures end up calling
/// modules that are no longer there.
///
/// Paths that are not under `from` — the intrinsics and the core library, notably — are left
/// alone, which is what confines this to the component's own modules.
struct Rebase<'a> {
    from: &'a Path,
    to: &'a Path,
}

impl Rebase<'_> {
    /// Move `module`, and everything it refers to within the component, under [`Self::to`].
    fn apply(&mut self, module: &mut masm::Module) {
        use masm::visit::VisitMut;

        if let Some(path) = self.rebase(module.path()) {
            module.set_path(&path);
        }
        // The rewrite below never breaks out of the walk, so there is no outcome to inspect.
        let _ = self.visit_mut_module(module);
    }

    /// The path `path` becomes under [`Self::to`], or `None` if it is not under [`Self::from`].
    fn rebase(&self, path: &Path) -> Option<masm::LibraryPath> {
        path.strip_prefix(self.from).map(|rest| self.to.join(rest))
    }

    /// Move `target` under [`Self::to`] if it names something in the component, reporting whether
    /// it did.
    fn rebase_target(&self, target: &mut InvocationTarget) -> bool {
        let InvocationTarget::Path(path) = target else {
            return false;
        };
        let Some(rebased) = self.rebase(path.inner()) else {
            return false;
        };
        *path = Span::new(path.span(), Arc::from(rebased.into_boxed_path()));
        true
    }

    /// Replace `procedure` with an equivalent one whose recorded callees are `invoked`.
    ///
    /// A procedure carries a set of the callees code generation emitted for it, and the linker
    /// resolves every entry in that set to build the call graph — so an entry naming a module
    /// that has moved fails the link with "undefined item", even though the body it was derived
    /// from now says otherwise. The set can be added to (`extend_invoked`) but not pruned from
    /// outside the syntax crate, hence rebuilding rather than editing in place.
    ///
    /// WARNING: rebuilding means enumerating everything a `Procedure` carries, so **a field added
    /// to `miden_assembly_syntax::ast::Procedure` upstream is silently dropped here** — for the
    /// procedures whose callees moved, on the live path, with no compile error and nothing
    /// mechanical to catch it (`Procedure`'s hand-written `PartialEq` already omits `span` and
    /// `invoked`, so even a round-trip equality check would not reliably notice). The field list
    /// below was audited against **miden-assembly-syntax 0.25.8** and is complete for that
    /// version; re-audit it when that dependency is bumped. The real fix is a `clear_invoked` on
    /// `Procedure` upstream, which would make this whole function unnecessary.
    fn replace_invoked(procedure: &mut masm::Procedure, invoked: Vec<masm::Invoke>) {
        use masm::Spanned;

        let span = procedure.span();
        let body = core::mem::replace(procedure.body_mut(), masm::Block::new(span, Vec::new()));
        let mut rebuilt = masm::Procedure::new(
            span,
            procedure.visibility(),
            procedure.name().clone(),
            procedure.num_locals(),
            body,
        )
        .with_docs(procedure.docs().map(|docs| docs.map(alloc::string::String::from)))
        .with_attributes(procedure.attributes().iter().cloned());
        rebuilt.set_syscall(procedure.is_syscall());
        if let Some(signature) = procedure.signature() {
            rebuilt.set_signature(signature.clone());
        }
        rebuilt.extend_invoked(invoked);
        *procedure = rebuilt;
    }
}

impl masm::visit::VisitMut for Rebase<'_> {
    /// Every call-like instruction reaches this, as `exec`, `call`, `syscall` and `procref` all
    /// funnel through it.
    fn visit_mut_invoke_target(&mut self, target: &mut InvocationTarget) -> ControlFlow<()> {
        self.rebase_target(target);
        ControlFlow::Continue(())
    }

    /// The body is rewritten by the default walk; a procedure's *recorded* callees are not
    /// reachable from it, so they are rebased here as well. See [`Rebase::replace_invoked`].
    fn visit_mut_procedure(&mut self, procedure: &mut masm::Procedure) -> ControlFlow<()> {
        masm::visit::visit_mut_procedure(self, procedure)?;

        let mut moved = false;
        let invoked = procedure
            .invoked()
            .cloned()
            .map(|mut invoke| {
                moved |= self.rebase_target(&mut invoke.target);
                invoke
            })
            .collect::<Vec<_>>();
        if moved {
            Self::replace_invoked(procedure, invoked);
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn validate_bytes_to_elements(bytes: &[u8]) {
        let result = Rodata::bytes_to_elements(bytes);

        // Each felt represents 4 bytes
        let expected_felts = bytes.len().div_ceil(4);
        // Felts should be padded to a multiple of 4 (1 word = 4 felts)
        let expected_total_felts = expected_felts.div_ceil(4) * 4;

        assert_eq!(
            result.len(),
            expected_total_felts,
            "For {} bytes, expected {} felts (padded from {} felts), but got {}",
            bytes.len(),
            expected_total_felts,
            expected_felts,
            result.len()
        );

        // Verify padding is zeros
        for (i, felt) in result.iter().enumerate().skip(expected_felts) {
            assert_eq!(*felt, miden_processor::Felt::ZERO, "Padding at index {i} should be zero");
        }
    }

    #[test]
    fn test_bytes_to_elements_edge_cases() {
        validate_bytes_to_elements(&[]);
        validate_bytes_to_elements(&[1]);
        validate_bytes_to_elements(&[0u8; 4]);
        validate_bytes_to_elements(&[0u8; 15]);
        validate_bytes_to_elements(&[0u8; 16]);
        validate_bytes_to_elements(&[0u8; 17]);
        validate_bytes_to_elements(&[0u8; 31]);
        validate_bytes_to_elements(&[0u8; 32]);
        validate_bytes_to_elements(&[0u8; 33]);
        validate_bytes_to_elements(&[0u8; 64]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]
        #[test]
        fn proptest_bytes_to_elements(bytes in prop::collection::vec(any::<u8>(), 0..=1000)) {
            validate_bytes_to_elements(&bytes);
        }

        #[test]
        fn proptest_bytes_to_elements_word_boundaries(size_factor in 0u32..=100) {
            // Test specifically around word boundaries
            // Test sizes around multiples of 16 (since 1 word = 4 felts = 16 bytes)
            let base_size = size_factor * 16;
            for offset in -2i32..=2 {
                let size = (base_size as i32 + offset).max(0) as usize;
                let bytes = vec![0u8; size];
                validate_bytes_to_elements(&bytes);
            }
        }
    }

    // -------------------------------------------------------------------------------------------
    // Where a component's Miden Assembly is rooted.
    //
    // `load_target_sources` rejects a root module whose path is not exactly the target's
    // namespace, so this is what decides whether a target assembles at all.
    // -------------------------------------------------------------------------------------------

    mod rooting {
        use alloc::rc::Rc;

        use midenc_hir::{Context, version::Version};
        use midenc_session::miden_project::{Target, Uri};

        use super::*;

        /// The identity `frontend/wasm` gives every component it wraps around a core Wasm module.
        fn wrapper_id() -> builtin::ComponentId {
            builtin::ComponentId {
                namespace: Symbol::intern("root_ns"),
                name: Symbol::intern("root"),
                version: Version::new(1, 0, 0),
            }
        }

        /// The identity a real Wasm *component* carries, which its author chose.
        fn authored_id() -> builtin::ComponentId {
            builtin::ComponentId {
                namespace: Symbol::intern("miden:example"),
                name: Symbol::intern("example"),
                version: Version::new(1, 0, 0),
            }
        }

        /// A component of `id`, rooted at the path that id renders to.
        fn component(id: builtin::ComponentId) -> MasmComponent {
            let root_path: Arc<Path> = Arc::from(
                id.to_library_path()
                    .to_absolute()
                    .expect("absolute")
                    .into_owned()
                    .into_boxed_path(),
            );
            rooted_component(Some(id), root_path)
        }

        /// What a world declaring **no** component lowers to: no id at all, and a root
        /// `world_body_to_masm_component` chose rather than one any source declares — either the
        /// world's single top-level module (`::{module}`) or, for zero or several of them, the
        /// placeholder constant `::init`.
        fn component_less(root: &str) -> MasmComponent {
            let root_path: Arc<Path> = Arc::from(
                masm::LibraryPath::new(root)
                    .unwrap()
                    .to_absolute()
                    .expect("absolute")
                    .into_owned()
                    .into_boxed_path(),
            );
            rooted_component(None, root_path)
        }

        /// A component rooted at `root_path` holding a root module and one submodule, in the
        /// shape code generation produces: the submodule is nested under the component's path,
        /// the root declares it, and the submodule's exported procedure calls one of its own by
        /// absolute path as well as an intrinsic that lives outside the component.
        fn rooted_component(
            id: Option<builtin::ComponentId>,
            root_path: Arc<Path>,
        ) -> MasmComponent {
            let child_path = root_path.join(masm::Path::new("child"));

            let mut root = masm::Module::new(masm::ModuleKind::Library, &root_path);
            root.declare_submodule(masm::Ident::new("child").unwrap(), masm::Visibility::Public)
                .expect("should declare submodule");

            let mut child = masm::Module::new(masm::ModuleKind::Library, &child_path);
            child
                .define_procedure(
                    procedure("callee", []),
                    Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
                )
                .expect("should define callee");
            child
                .define_procedure(
                    procedure("caller", [child_path.join(masm::Path::new("callee")), intrinsic()]),
                    Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
                )
                .expect("should define caller");

            MasmComponent {
                id,
                root: root_path,
                init: None,
                entrypoint: Some(exec_target(&child_path.join(masm::Path::new("caller")))),
                rodata: Vec::new(),
                heap_base: 0,
                stack_pointer: None,
                modules: vec![Arc::new(root), Arc::new(child)],
            }
        }

        /// A procedure named `name` that `exec`s each of `callees`, recording them the way code
        /// generation does — in the body *and* in the procedure's set of invoked callees.
        ///
        /// It carries a signature and a marker attribute because re-rooting rebuilds procedures
        /// whose callees moved, and everything code generation attached has to survive that: the
        /// signature is what the assembler type-checks exported procedures against, and the
        /// markers are the ones `MasmFunctionBuilder::build` copies onto a procedure
        /// (`lower/component.rs:903`) from the attributes the frontend sets on lifted exports
        /// (`frontend/wasm`'s `lift_exports.rs:442`), which classify an account component's
        /// procedures.
        fn procedure<I>(name: &str, callees: I) -> masm::Procedure
        where
            I: IntoIterator<Item = masm::LibraryPath>,
        {
            let span = SourceSpan::default();
            let mut ops = Vec::new();
            let mut invoked = Vec::new();
            for callee in callees {
                let target = exec_target(&callee);
                invoked.push(masm::Invoke::new(masm::InvokeKind::Exec, target.clone()));
                ops.push(masm::Op::Inst(Span::new(span, masm::Instruction::Exec(target))));
            }
            ops.push(masm::Op::Inst(Span::new(span, masm::Instruction::Nop)));
            let mut procedure = masm::Procedure::new(
                span,
                masm::Visibility::Public,
                masm::ProcedureName::new(name).unwrap(),
                3,
                masm::Block::new(span, ops),
            )
            .with_signature(masm::FunctionType::new(
                midenc_hir::CallConv::Fast,
                vec![masm::TypeExpr::from(midenc_hir::Type::U32)],
                vec![],
            ))
            .with_attributes([masm::Attribute::Marker(
                masm::Ident::new("account_procedure").unwrap(),
            )]);
            procedure.extend_invoked(invoked);
            procedure
        }

        /// Everything code generation attached to `name` beyond its body, rendered for comparison.
        fn decorations(module: &masm::Module, name: &str) -> alloc::string::String {
            use alloc::string::ToString;

            let procedure = module
                .items()
                .iter()
                .find_map(|item| match item {
                    masm::Item::Procedure(procedure) if procedure.name().as_str() == name => {
                        Some(procedure)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no procedure named '{name}' in '{}'", module.path()));
            format!(
                "{:?} locals={} syscall={} signature={:?} attributes={:?}",
                procedure.visibility(),
                procedure.num_locals(),
                procedure.is_syscall(),
                procedure.signature().map(|signature| format!("{signature:?}")),
                procedure.attributes().iter().map(|attr| attr.to_string()).collect::<Vec<_>>(),
            )
        }

        /// A call target outside any component, which must survive re-rooting untouched.
        fn intrinsic() -> masm::LibraryPath {
            masm::LibraryPath::new("::intrinsics::mem::heap_init").unwrap()
        }

        fn exec_target(path: &masm::LibraryPath) -> InvocationTarget {
            InvocationTarget::Path(Span::new(
                SourceSpan::default(),
                Arc::from(path.clone().into_boxed_path()),
            ))
        }

        fn library_target(namespace: &str) -> Target {
            Target::library(
                Arc::<Path>::from(
                    masm::LibraryPath::new(namespace)
                        .unwrap()
                        .to_absolute()
                        .unwrap()
                        .into_owned()
                        .into_boxed_path(),
                ),
                Uri::new("lib.wasm"),
            )
        }

        /// A default compiler context, which is where `source_inputs` gets its session.
        fn context() -> Rc<Context> {
            Rc::new(Context::default())
        }

        /// Every path in `module`, i.e. its own and each call target in each of its procedures,
        /// including the targets recorded on the procedure rather than written in its body.
        fn paths(module: &masm::Module) -> Vec<alloc::string::String> {
            use alloc::string::ToString;

            let mut paths = vec![module.path().to_string()];
            for item in module.items() {
                let masm::Item::Procedure(procedure) = item else {
                    continue;
                };
                for op in procedure.iter() {
                    if let masm::Op::Inst(inst) = op
                        && let masm::Instruction::Exec(InvocationTarget::Path(path)) = &**inst
                    {
                        paths.push(path.to_string());
                    }
                }
                for invoke in procedure.invoked() {
                    paths.push(invoke.target.to_string());
                }
            }
            paths
        }

        /// The *allocation* behind each call target in `module`, cloned so that it can be
        /// compared later by identity rather than by value.
        ///
        /// This is what tells a component that was never rewritten from one rewritten to the very
        /// same paths, and nothing comparing values can: rebuilding a procedure is lossless by
        /// design — that is the entire point of [`Rebase::replace_invoked`] — so every path, every
        /// decoration and every recorded callee comes back *equal* either way.
        ///
        /// Identity survives the copy that [`MasmComponent::source_inputs`] makes, because an
        /// `InvocationTarget::Path` holds an `Arc<Path>` and cloning a module shares it. A rewrite
        /// cannot preserve it: `Rebase::rebase_target` builds its replacement with `Arc::from`,
        /// which allocates unconditionally, whether or not the path it produces differs.
        fn target_allocations(module: &masm::Module) -> Vec<Arc<Path>> {
            let mut targets = Vec::new();
            for item in module.items() {
                let masm::Item::Procedure(procedure) = item else {
                    continue;
                };
                for op in procedure.iter() {
                    if let masm::Op::Inst(inst) = op
                        && let masm::Instruction::Exec(InvocationTarget::Path(path)) = &**inst
                    {
                        targets.push(path.inner().clone());
                    }
                }
            }
            targets
        }

        /// Whether every call target in `module` is still the allocation it was in `before`.
        ///
        /// See [`target_allocations`]. A `false` here means the rewrite ran, regardless of what it
        /// produced.
        fn targets_are_untouched(before: &[Arc<Path>], module: &masm::Module) -> bool {
            let after = target_allocations(module);
            before.len() == after.len()
                && before
                    .iter()
                    .zip(after.iter())
                    .all(|(before, after)| Arc::ptr_eq(before, after))
        }

        /// A synthetic wrapper compiled for a library target is rooted at the target's namespace,
        /// and its whole module tree moves with it.
        ///
        /// The wrapper's id renders as the single quoted component `::"root_ns:root@1.0.0"`, so
        /// it can never equal a target namespace; re-rooting is the only way such a target
        /// satisfies the assembler. Everything that named the old root has to move too — module
        /// paths, call targets, and the callee set each procedure carries — or the root declares
        /// submodules that are not there and the linker fails to resolve the calls.
        #[test]
        fn a_synthetic_wrappers_library_is_rooted_at_the_target_namespace() {
            let context = context();
            let target = library_target("::example");
            let component = component(wrapper_id());
            let decorated = decorations(&component.modules[1], "caller");

            let sources = component.source_inputs(&target, context.session()).unwrap();

            assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
            assert_eq!(sources.support.len(), 1, "the component's one submodule");
            assert_eq!(
                paths(&sources.support[0]),
                vec![
                    "::example::child",
                    "::example::child::callee",
                    // The intrinsic is outside the component, so it stays where it is; the two
                    // call targets appear twice because each is both written in the body and
                    // recorded on the procedure, and the linker resolves both.
                    "::intrinsics::mem::heap_init",
                    "::example::child::callee",
                    "::intrinsics::mem::heap_init",
                ],
                "nothing may be left addressing the wrapper's id"
            );
            assert_eq!(
                decorations(&sources.support[0], "caller"),
                decorated,
                "a procedure whose callees moved is rebuilt, and must come back whole"
            );
        }

        /// A library target already named after the wrapper comes back untouched.
        ///
        /// A manifest may declare `namespace = "root_ns:root@1.0.0"`, which is how projects worked
        /// around this defect before it was fixed, and which parses to exactly the path
        /// `ComponentId::to_library_path` produces. Such a target needs no re-rooting, and the
        /// equality guard in [`MasmComponent::source_inputs`] is what keeps it from being rewritten
        /// to itself — which is what lets those existing projects be said to be unaffected by this
        /// change.
        ///
        /// The last assertion is what pins the *guard* rather than merely the outcome. Deleting
        /// the guard degenerates the rewrite into an identity mapping, which every value-based
        /// assertion above survives — rebuilding a procedure is lossless by design, so equal paths
        /// and equal decorations come back either way. [`target_allocations`] compares identity
        /// instead, which a rewrite cannot preserve however little it changes.
        #[test]
        fn a_library_target_named_after_the_wrapper_is_left_alone() {
            let context = context();
            let component = component(wrapper_id());
            let expected = paths(&component.modules[1]);
            let decorated = decorations(&component.modules[1], "caller");
            let allocations = target_allocations(&component.modules[1]);
            let target = library_target("root_ns:root@1.0.0");
            assert_eq!(
                target.namespace.inner().as_ref(),
                component.root.as_ref(),
                "the manifest namespace and the wrapper's id must really be the same path, or \
                 this test is about some other case"
            );

            let sources = component.source_inputs(&target, context.session()).unwrap();

            assert_eq!(sources.root.path(), component.root.as_ref());
            assert_eq!(paths(&sources.support[0]), expected);
            assert_eq!(decorations(&sources.support[0], "caller"), decorated);
            assert!(
                targets_are_untouched(&allocations, &sources.support[0]),
                "the rewrite must not have run at all, not merely have produced the same paths"
            );
        }

        /// A component whose id its author chose is left exactly where it is.
        ///
        /// Re-rooting is justified only by the wrapper being invisible to whoever wrote the code.
        /// An authored component id is part of the code's own identity, and moving it would
        /// silently rename the procedures every dependent addresses.
        #[test]
        fn an_authored_components_library_keeps_its_own_path() {
            let context = context();
            let target = library_target("::example");
            let component = component(authored_id());
            let expected = paths(&component.modules[1]);

            let sources = component.source_inputs(&target, context.session()).unwrap();

            assert_eq!(
                sources.root.path(),
                component.root.as_ref(),
                "an authored component's root is its own library path, whatever the target is \
                 called"
            );
            assert_eq!(paths(&sources.support[0]), expected);
        }

        /// A component-less world's library is rooted at the target's namespace too.
        ///
        /// The second half of the same rule the wrapper is the first half of: a world declaring
        /// no component has no identity of its own, so lowering has to invent a root. With
        /// several top-level modules — or none — that root is the constant `::init`, which no
        /// source declares and which therefore no synthesized namespace can equal, so such a
        /// target could never satisfy `load_target_sources` at all. The whole module tree moves
        /// with the root here for the same reason it does for the wrapper.
        #[test]
        fn a_component_less_worlds_library_is_rooted_at_the_target_namespace() {
            let context = context();
            let target = library_target("::example");
            let component = component_less("::init");
            let decorated = decorations(&component.modules[1], "caller");

            let sources = component.source_inputs(&target, context.session()).unwrap();

            assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
            assert_eq!(sources.support.len(), 1, "the component's one submodule");
            assert_eq!(
                paths(&sources.support[0]),
                vec![
                    "::example::child",
                    "::example::child::callee",
                    "::intrinsics::mem::heap_init",
                    "::example::child::callee",
                    "::intrinsics::mem::heap_init",
                ],
                "nothing may be left addressing the placeholder root"
            );
            assert_eq!(
                decorations(&sources.support[0], "caller"),
                decorated,
                "a procedure whose callees moved is rebuilt, and must come back whole"
            );
        }

        /// A component-less world already sitting at its target's namespace comes back untouched.
        ///
        /// This is the *single*-module shape, and it is why subsuming it into the same rule costs
        /// nothing: lowering roots it at `::{module}` and preparation's `.hir` scan reads that
        /// same module's name, so the two agree and the equality guard makes the rewrite a no-op.
        /// Treating one module and several by one rule is what keeps codegen from having two
        /// answers preparation would have to mirror separately.
        ///
        /// "Costs nothing" is a claim about the guard, so the last assertion is about the guard:
        /// see [`target_allocations`] for why comparing values cannot distinguish a rewrite that
        /// never ran from one that reproduced its input exactly.
        #[test]
        fn a_component_less_world_already_at_the_target_namespace_is_left_alone() {
            let context = context();
            let component = component_less("::lib");
            let expected = paths(&component.modules[1]);
            let decorated = decorations(&component.modules[1], "caller");
            let allocations = target_allocations(&component.modules[1]);
            let target = library_target("::lib");

            let sources = component.source_inputs(&target, context.session()).unwrap();

            assert_eq!(sources.root.path(), component.root.as_ref());
            assert_eq!(paths(&sources.support[0]), expected);
            assert_eq!(decorations(&sources.support[0], "caller"), decorated);
            assert!(
                targets_are_untouched(&allocations, &sources.support[0]),
                "the rewrite must not have run at all, not merely have produced the same paths"
            );
        }

        /// An executable target is untouched: its root is the generated `$exec` module, and the
        /// component's own modules keep the paths that module calls them by.
        ///
        /// Both shapes that re-rooting applies to are checked, because the early return for an
        /// executable is what keeps either from reaching it: the synthetic wrapper, and a
        /// component-less world, which is the shape a bare-module `.hir` or a disassembled
        /// `.masm` program takes.
        #[test]
        fn an_executable_target_still_gets_the_generated_main_module() {
            for component in [component(wrapper_id()), component_less("::init")] {
                let context = context();
                let expected = paths(&component.modules[1]);
                let target = Target::executable("main", Uri::new("main.wasm"));

                let sources = component.source_inputs(&target, context.session()).unwrap();

                assert_eq!(sources.root.path(), target.namespace.inner().as_ref());
                assert!(sources.root.kind().is_executable());
                let child = sources
                    .support
                    .iter()
                    .find(|module| module.path().last() == Some("child"))
                    .expect("the component's submodule is carried over as a support module");
                assert_eq!(
                    paths(child),
                    expected,
                    "the generated `$exec` module calls the component by its own path, so \
                     re-rooting here would break the very case that already works"
                );
            }
        }
    }
}
