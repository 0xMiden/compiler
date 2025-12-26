use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use miden_assembly::{Library, ast::Module};
use miden_mast_package::Package;
use midenc_codegen_masm::{
    self as masm, MasmComponent, ToMasmComponent,
    intrinsics::{
        ADVICE_INTRINSICS_MODULE_NAME, CRYPTO_INTRINSICS_MODULE_NAME, I32_INTRINSICS_MODULE_NAME,
        I64_INTRINSICS_MODULE_NAME, I128_INTRINSICS_MODULE_NAME, MEM_INTRINSICS_MODULE_NAME,
    },
};
use midenc_hir::{interner::Symbol, pass::AnalysisManager};
use midenc_session::{Emit, OutputType, Writer};

use super::*;

/// A wrapper that allows emitting a synthetic MASM output representing the full [MasmComponent].
///
/// The format matches the [core::fmt::Display] implementation for [MasmComponent], which is
/// defined in `codegen/masm/src/artifact.rs`.
struct MasmComponentEmit<'a>(&'a MasmComponent);

impl Emit for MasmComponentEmit<'_> {
    fn name(&self) -> Option<midenc_hir::interner::Symbol> {
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
        writer.write_fmt(format_args!("{}", self.0))?;
        Ok(())
    }
}

pub struct CodegenOutput {
    pub component: Arc<MasmComponent>,
    pub link_libraries: Vec<Arc<Library>>,
    pub link_packages: BTreeMap<Symbol, Arc<Package>>,
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.)
    pub account_component_metadata_bytes: Option<Vec<u8>>,
}

/// Perform code generation on the possibly-linked output of previous stages
pub struct CodegenStage;

impl Stage for CodegenStage {
    type Input = LinkOutput;
    type Output = CodegenOutput;

    fn enabled(&self, context: &Context) -> bool {
        context.session().should_codegen()
    }

    fn run(
        &mut self,
        linker_output: Self::Input,
        context: Rc<Context>,
    ) -> CompilerResult<Self::Output> {
        let LinkOutput {
            component,
            masm: masm_modules,
            mast: link_libraries,
            packages: link_packages,
            ..
        } = linker_output;

        log::debug!("lowering hir program to masm");

        let analysis_manager = AnalysisManager::new(component.as_operation_ref(), None);
        let mut masm_component =
            component.borrow().to_masm_component(analysis_manager).map(Box::new)?;

        let session = context.session();

        // Ensure intrinsics modules are linked
        for intrinsics_module in required_intrinsics_modules(session) {
            log::debug!(
                "adding required intrinsic module '{}' to masm program",
                intrinsics_module.path()
            );
            masm_component.modules.push(intrinsics_module);
        }

        // Link in any MASM inputs provided to the compiler
        for module in masm_modules {
            log::debug!("adding external masm module '{}' to masm program", module.path());
            masm_component.modules.push(module);
        }

        if session.should_emit(OutputType::Masm) {
            let artifact = MasmComponentEmit(&masm_component);
            session.emit(OutputMode::Text, &artifact).into_diagnostic()?;
        }

        Ok(CodegenOutput {
            component: Arc::from(masm_component),
            link_libraries,
            link_packages,
            account_component_metadata_bytes: linker_output.account_component_metadata_bytes,
        })
    }
}

fn required_intrinsics_modules(session: &Session) -> impl IntoIterator<Item = Arc<Module>> {
    [
        masm::intrinsics::load(MEM_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(I32_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(I64_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(I128_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(CRYPTO_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(ADVICE_INTRINSICS_MODULE_NAME, &session.source_manager)
            .map(Arc::from)
            .expect("undefined intrinsics module"),
    ]
}
