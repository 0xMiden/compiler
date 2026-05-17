use miden_assembly::ProjectSourceInputs;

use super::*;

/// Perform analysis of HIR before lowering to Miden Assembly
pub struct AnalysisStage;

impl Stage for AnalysisStage {
    type Input = Option<ProjectSourceInputs>;
    type Output = Option<ProjectSourceInputs>;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let session = context.session();
        if session.options.lint {
            #[cfg(feature = "std")]
            {
                use midenc_hir::Op;
                // We need to disassemble the sources from the input project
                let target = session.name();
                let target = if session.options.name.is_none() {
                    None
                } else {
                    Some(target)
                };
                let config = midenc_frontend_masm::DisassemblerConfig {
                    infer_missing_signatures: true,
                };
                let world = midenc_frontend_masm::disassemble_project_target(
                    &session.project,
                    target,
                    input.as_ref().map(|ProjectSourceInputs { root, support }| {
                        ProjectSourceInputs {
                            root: root.clone(),
                            support: support.clone(),
                        }
                    }),
                    &config,
                    context.clone(),
                )?;
                crate::emit_hir_if_requested(world.world.borrow().as_operation(), context.clone())?;
                let analysis_manager =
                    midenc_hir::pass::AnalysisManager::new(world.world.as_operation_ref(), None);
                let analysis = analysis_manager
                    .get_analysis::<midenc_dialect_hir::analyses::AdviceTaintAnalysis>()?;
                let source_manager = context.source_manager();
                if !analysis.findings().is_empty() {
                    for diagnostic in analysis.diagnostics(&source_manager) {
                        session.diagnostics.emit(diagnostic);
                    }
                    if session.diagnostics.has_errors() || session.analyze_only() {
                        return Err(CompilerStopped(
                            "either errors were raised, or analyze-only is set",
                        )
                        .into());
                    }
                }
            }
            #[cfg(not(feature = "std"))]
            log::warn!("skipping lint stage, as compiler was built without std feature");
        }

        Ok(input)
    }
}
