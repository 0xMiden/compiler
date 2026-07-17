use alloc::{boxed::Box, sync::Arc, vec::Vec};

use midenc_codegen_masm::{LegalizeForMasm, MasmComponent, ToMasmComponent};
use midenc_hir::pass::{AnalysisManager, IRPrintingConfig, Nesting, OpPassManager, PassManager};
use midenc_session::OutputType;

use super::*;

pub struct CodegenOutput {
    pub component: Arc<MasmComponent>,
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.)
    pub account_component_metadata_bytes: Option<Vec<u8>>,
    #[cfg(feature = "std")]
    pub source_provenance: miden_assembly::ProjectSourceProvenanceInputs,
}

impl Clone for CodegenOutput {
    fn clone(&self) -> Self {
        Self {
            component: self.component.clone(),
            account_component_metadata_bytes: self.account_component_metadata_bytes.clone(),
            source_provenance: self.source_provenance(),
        }
    }
}

impl CodegenOutput {
    #[cfg(feature = "std")]
    pub fn source_provenance(&self) -> miden_assembly::ProjectSourceProvenanceInputs {
        use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};

        let ProjectSourceProvenanceInputs { root, support } = &self.source_provenance;
        ProjectSourceProvenanceInputs {
            root: SourceFileProvenance {
                path: root.path.clone(),
                content: root.content.clone(),
            },
            support: support
                .iter()
                .map(|sfp| SourceFileProvenance {
                    path: sfp.path.clone(),
                    content: sfp.content.clone(),
                })
                .collect(),
        }
    }
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
            #[cfg(feature = "std")]
            source_provenance,
        } = input;

        log::debug!("lowering miden component to masm");

        let anchor = component.map(|c| c.as_operation_ref()).unwrap_or(world.as_operation_ref());
        legalize_for_masm(anchor, context.clone())?;

        let analysis_manager = AnalysisManager::new(anchor, None);
        let masm_component = match component {
            Some(component) => {
                component.borrow().to_masm_component(analysis_manager).map(Box::new)?
            }
            None => world.borrow().to_masm_component(analysis_manager).map(Box::new)?,
        };

        let session = context.session();

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
            #[cfg(feature = "std")]
            source_provenance,
        })
    }
}

fn legalize_for_masm(anchor: midenc_hir::OperationRef, context: Rc<Context>) -> CompilerResult<()> {
    let ir_print_config = IRPrintingConfig::try_from(context.session().options.as_ref())?;
    let mut pm = PassManager::new(context, OpPassManager::ANY, Nesting::Implicit)
        .enable_ir_printing(ir_print_config);
    pm.add_pass(Box::new(LegalizeForMasm));
    pm.run(anchor)?;

    Ok(())
}
