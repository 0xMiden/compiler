use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use miden_assembly::{ast::Module, Library};
use miden_mast_package::Package;
use midenc_codegen_masm2::{
    self as masm,
    intrinsics::{
        I32_INTRINSICS_MODULE_NAME, I64_INTRINSICS_MODULE_NAME, MEM_INTRINSICS_MODULE_NAME,
    },
    MasmComponent, ToMasmComponent,
};
use midenc_hir2::{interner::Symbol, pass::AnalysisManager};
use midenc_session::OutputType;

use super::*;

pub struct CodegenOutput {
    pub component: Arc<MasmComponent>,
    pub link_libraries: Vec<Arc<Library>>,
    pub link_packages: BTreeMap<Symbol, Arc<Package>>,
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
        if session.should_emit(OutputType::Masm) {
            for module in masm_component.modules.iter() {
                session.emit(OutputMode::Text, module).into_diagnostic()?;
            }
        }

        // Ensure intrinsics modules are linked
        for intrinsics_module in required_intrinsics_modules(session) {
            log::debug!(
                "adding required intrinsic module '{}' to masm program",
                intrinsics_module.name()
            );
            masm_component.modules.push(intrinsics_module);
        }

        // Link in any MASM inputs provided to the compiler
        for module in masm_modules {
            log::debug!("adding external masm module '{}' to masm program", module.name());
            masm_component.modules.push(module);
        }

        Ok(CodegenOutput {
            component: Arc::from(masm_component),
            link_libraries,
            link_packages,
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
    ]
}
