use alloc::{boxed::Box, sync::Arc, vec::Vec};

use miden_assembly::ast::Module;
use midenc_codegen_masm::{
    self as masm, MasmComponent, ToMasmComponent,
    intrinsics::{
        ADVICE_INTRINSICS_MODULE_NAME, I32_INTRINSICS_MODULE_NAME, I64_INTRINSICS_MODULE_NAME,
        MEM_INTRINSICS_MODULE_NAME,
    },
};
use midenc_hir::pass::AnalysisManager;
use midenc_session::OutputType;

use super::*;

pub struct CodegenOutput {
    pub component: Arc<MasmComponent>,
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.)
    pub account_component_metadata_bytes: Option<Vec<u8>>,
}

/// Perform code generation on the possibly-linked output of previous stages
pub struct CodegenStage;

impl Stage for CodegenStage {
    type Input = MidenComponent;
    type Output = CodegenOutput;

    fn enabled(&self, context: &Context) -> bool {
        context.session().should_codegen()
    }

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let MidenComponent {
            world,
            component,
            account_component_metadata_bytes,
        } = input;

        log::debug!("lowering miden component to masm");

        let anchor = component.map(|c| c.as_operation_ref()).unwrap_or(world.as_operation_ref());
        let analysis_manager = AnalysisManager::new(anchor, None);
        let mut masm_component = match component {
            Some(component) => {
                component.borrow().to_masm_component(analysis_manager).map(Box::new)?
            }
            None => world.borrow().to_masm_component(analysis_manager).map(Box::new)?,
        };

        let session = context.session();

        // Ensure intrinsics modules are linked
        for intrinsics_module in required_intrinsics_modules(session) {
            log::debug!(
                "adding required intrinsic module '{}' to masm program",
                intrinsics_module.path()
            );
            masm_component.modules.push(intrinsics_module);
        }

        if session.should_emit(OutputType::Masm) {
            session.emit(OutputMode::Text, masm_component.as_ref()).into_diagnostic()?;
        }

        if session.options.link_only {
            log::debug!("stopping compiler early (link-only=true)");
            return Err(CompilerStopped("link-only=true").into());
        }

        Ok(CodegenOutput {
            component: Arc::from(masm_component),
            account_component_metadata_bytes,
        })
    }
}

fn required_intrinsics_modules(session: &Session) -> impl IntoIterator<Item = Arc<Module>> {
    [
        masm::intrinsics::load(MEM_INTRINSICS_MODULE_NAME, session.source_manager.clone())
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(I32_INTRINSICS_MODULE_NAME, session.source_manager.clone())
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(I64_INTRINSICS_MODULE_NAME, session.source_manager.clone())
            .map(Arc::from)
            .expect("undefined intrinsics module"),
        masm::intrinsics::load(ADVICE_INTRINSICS_MODULE_NAME, session.source_manager.clone())
            .map(Arc::from)
            .expect("undefined intrinsics module"),
    ]
}
