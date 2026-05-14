use alloc::sync::Arc;

use miden_assembly::{ProjectSourceInputs, ProjectTargetSelector, utils::DisplayHex};
use miden_mast_package::Package;

use super::*;

/// The artifact produced by the full compiler pipeline.
///
/// The type of artifact depends on what outputs were requested, and what options were specified.
pub enum Artifact {
    Lowered(CodegenOutput),
    Assembled(Arc<Package>),
}
impl Artifact {
    pub fn unwrap_mast(self) -> Arc<Package> {
        match self {
            Self::Assembled(mast) => mast,
            Self::Lowered(_) => {
                panic!("expected 'mast' artifact, but assembler stage was not run")
            }
        }
    }
}

/// Perform assembly of the generated Miden Assembly, producing MAST
pub struct AssembleStage;

impl Stage for AssembleStage {
    type Input = CodegenOutput;
    type Output = Artifact;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_hir::formatter::DisplayHex;

        let session = context.session();
        if !session.should_assemble() {
            log::debug!(
                "skipping assembly of mast package from masm artifact (should-assemble=false)"
            );
            return Ok(Artifact::Lowered(input));
        }

        log::debug!("assembling package");

        let CodegenOutput {
            component,
            account_component_metadata_bytes,
        } = input;

        let package = component.assemble(account_component_metadata_bytes.as_deref(), session)?;

        log::debug!(
            "successfully assembled package with digest {}",
            DisplayHex::new(&package.digest().as_bytes())
        );
        Ok(Artifact::Assembled(package))
    }
}

/// Perform assembly of a Miden Assembly project
pub struct AssembleProjectStage;

impl Stage for AssembleProjectStage {
    type Input = Option<ProjectSourceInputs>;
    type Output = Artifact;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let session = context.session();
        let package = session.project.package();
        let mut registry = session.package_registry()?;
        let mut project_assembler = miden_assembly::Assembler::new(session.source_manager.clone())
            .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors())
            .for_project(package, registry.as_mut())?;

        let selector = if session.options.target_type.is_executable() {
            ProjectTargetSelector::Executable(session.name.as_str())
        } else {
            ProjectTargetSelector::Library
        };

        let package = match input {
            Some(sources) => project_assembler.assemble_with_sources(selector, "dev", sources)?,
            None => project_assembler.assemble(selector, "dev")?,
        };

        log::debug!(
            "successfully assembled package with digest {}",
            DisplayHex::new(&package.digest().as_bytes())
        );

        Ok(Artifact::Assembled(package))
    }
}
