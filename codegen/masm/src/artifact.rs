use alloc::sync::Arc;
use core::fmt;

use miden_assembly::{Path, ProjectSourceInputs, ast::InvocationTarget};
use miden_core::Word;
use midenc_hir::{SymbolPath, constants::ConstantData, interner::Symbol};
use midenc_session::{
    Emit, OutputMode, OutputType, Session, Writer,
    diagnostics::{IntoDiagnostic, Report, SourceSpan, WrapErr},
};

use crate::{lower::NativePtr, masm};

pub struct MasmComponent {
    /// The namespace path of the component, see
    /// [`Component::namespace_path`](midenc_hir::dialects::builtin::Component::namespace_path)
    pub id: Option<SymbolPath>,
    /// True if [`Self::id`] belongs to a component the compiler invented to wrap a bare core
    /// module, rather than one an author wrote — see
    /// [`Component::SYNTHETIC_WRAPPER_ATTR`](midenc_hir::dialects::builtin::Component::SYNTHETIC_WRAPPER_ATTR),
    /// which is where this comes from.
    pub synthetic_wrapper: bool,
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
    /// Argument/result lane counts of the canonically resolved same-context entrypoint.
    pub entrypoint_arity: Option<(u32, u32)>,
    /// Target namespace and executable classification fixed before selection, if target-aware.
    pub prepared_target: Option<(Arc<Path>, bool)>,
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
    pub component: SymbolPath,
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
            .field("digest", &format_args!("{}", self.digest))
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
        if let Some((namespace, executable)) = &self.prepared_target {
            if namespace.as_ref() != target.namespace.inner().as_ref()
                || *executable != target.is_executable()
            {
                return Err(Report::msg(
                    "assembly target differs from the namespace selected before verification",
                ));
            }
        } else if !target.is_executable() && self.root.as_ref() != target.namespace.inner().as_ref()
        {
            return Err(Report::msg(
                "source-identity component requires lowering again for another assembly namespace",
            ));
        }
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
            let root = self
                .generate_main(entrypoint, emit_test_harness, session.source_manager.clone())
                .map_err(|report| {
                    crate::lower::report_planning_failure(report, &session.diagnostics)
                })?;
            return Ok(ProjectSourceInputs { root, support });
        }

        let root = root.expect("components must always have a root module");
        Ok(ProjectSourceInputs { root, support })
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
        use crate::{
            plan::{CallingConvention, ProcedureAbi},
            select::generated::{GeneratedHarness, GeneratedRoot},
        };
        let mut exe = Box::new(masm::Module::new_executable());
        let span = SourceSpan::UNKNOWN;
        let harness = if emit_test_harness {
            let name = masm::ProcedureName::new("__test_harness").unwrap();
            let procedure = crate::lower::finalize_generated(
                &GeneratedHarness,
                &[GeneratedHarness],
                name.clone(),
                span,
            )?;
            exe.define_procedure(procedure, source_manager.clone()).into_diagnostic()?;
            Some(InvocationTarget::Symbol(name.as_ident()))
        } else {
            None
        };
        let (arguments, results) = self
            .entrypoint_arity
            .ok_or_else(|| Report::msg("executable entrypoint lacks a checked source ABI"))?;
        let source = GeneratedRoot {
            initializer: self.init.clone(),
            harness,
            entry: entrypoint.clone(),
            entry_abi: ProcedureAbi::new(CallingConvention::SameContext, arguments, results)?,
        };
        let start = crate::lower::finalize_generated(
            &source,
            &source.definitions(),
            masm::ProcedureName::main(),
            span,
        )?;
        exe.define_procedure(start, source_manager)
            .into_diagnostic()
            .wrap_err("failed to define executable main")?;
        Ok(exe)
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

    mod rooting {
        use alloc::rc::Rc;

        use midenc_hir::{
            Context, Spanned, diagnostics::Uri, dialects::builtin, formatter::PrettyPrint,
            pass::AnalysisManager,
        };
        use midenc_session::miden_project::Target;

        use super::*;
        use crate::{EmissionNamespace, ToMasmComponent};

        fn target(path: &str) -> Target {
            Target::library(
                masm::PathBuf::new(path).unwrap().to_absolute().unwrap().into_owned(),
                Uri::new("namespace.hir"),
            )
        }

        fn lower(text: &str, target: &Target) -> (MasmComponent, Rc<Context>) {
            let context = Rc::new(Context::default());
            let op = midenc_hir::parse::parse_any(
                midenc_hir::parse::ParserConfig {
                    context: context.clone(),
                    verify: true,
                },
                Uri::new("namespace.hir"),
                text,
            )
            .unwrap();
            let manager = AnalysisManager::new(op, None);
            let component = if let Ok(world) = op.try_downcast_op::<builtin::World>() {
                world
                    .borrow()
                    .to_masm_component(manager, EmissionNamespace::Target(target))
                    .unwrap()
            } else {
                op.try_downcast_op::<builtin::Component>()
                    .unwrap()
                    .borrow()
                    .to_masm_component(manager, EmissionNamespace::Target(target))
                    .unwrap()
            };
            (component, context)
        }

        #[test]
        fn component_less_roots_choose_the_target_before_lowering() {
            for text in [
                "builtin.world { ^entry: };",
                "builtin.world { builtin.module public @lib { builtin.function public \
                 extern(\"C\") @entry() { builtin.ret; }; }; };",
                "builtin.world { builtin.module public @a { builtin.function public extern(\"C\") \
                 @a() { builtin.ret; }; }; builtin.module public @b { builtin.function public \
                 extern(\"C\") @b() { builtin.ret; }; }; };",
            ] {
                let target = target("::chosen");
                let (component, context) = lower(text, &target);
                assert_eq!(component.root.as_str(), "::chosen");
                let before = component.to_string();
                let sources = component.source_inputs(&target, context.session()).unwrap();
                assert_eq!(sources.root.path(), component.root.as_ref());
                assert_eq!(before, component.to_string());
            }
        }

        #[test]
        fn equivalent_spelling_is_selected_early_and_packaging_is_inert() {
            let target = target("::\"lib\"");
            let (component, context) = lower(
                "builtin.component private @lib { builtin.function public extern(\"C\") @entry() \
                 { builtin.ret; }; };",
                &target,
            );
            assert_eq!(component.root.as_str(), target.namespace.inner().as_str());
            let sources = component.source_inputs(&target, context.session()).unwrap();
            let original = component.modules[0].procedures().next().unwrap();
            let packaged = sources.root.procedures().next().unwrap();
            assert_eq!(original.span(), packaged.span());
            assert_eq!(original.visibility(), packaged.visibility());
            assert_eq!(original.num_locals(), packaged.num_locals());
            assert_eq!(original.to_pretty_string(), packaged.to_pretty_string());
            assert_eq!(
                original.invoked().collect::<Vec<_>>(),
                packaged.invoked().collect::<Vec<_>>()
            );
        }

        #[test]
        fn authored_namespace_and_quoted_separators_retain_their_identity() {
            let target = target("::left::right");
            let (component, context) = lower(
                "builtin.component private @\"left::right\" { builtin.function public \
                 extern(\"C\") @entry() { builtin.ret; }; };",
                &target,
            );
            assert_ne!(component.root.as_ref(), target.namespace.inner().as_ref());
            let sources = component.source_inputs(&target, context.session()).unwrap();
            assert_eq!(sources.root.path(), component.root.as_ref());
        }

        #[test]
        fn a_later_incompatible_target_requires_lowering_again() {
            let target = target("::chosen");
            let (component, context) = lower("builtin.world { ^entry: };", &target);
            let later = self::target("::different");
            assert!(component.source_inputs(&later, context.session()).is_err());
        }

        #[test]
        fn generated_root_planning_failure_renders_unknown_source_fallback() {
            let target = Target::executable("invalid-root", Uri::new("generated.hir"));
            let (mut component, context) = lower("builtin.world { ^entry: };", &target);
            // Corrupt the checked source ABI at the public packaging boundary.
            component.entrypoint = Some(crate::lower::raw::target("::entry"));
            component.entrypoint_arity = Some((17, 0));
            let report = component
                .source_inputs(&target, context.session())
                .err()
                .expect("a root cannot expose more than sixteen entry lanes");
            let rendered =
                alloc::format!("{}", midenc_session::diagnostics::PrintDiagnostic::new(report));
            assert!(rendered.contains("InvalidSelectedLowering"), "{rendered}");
            assert!(rendered.contains("source location unavailable"), "{rendered}");
        }
    }
}
