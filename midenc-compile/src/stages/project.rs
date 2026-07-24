use alloc::{boxed::Box, rc::Rc};
use core::cell::RefCell;

use miden_assembly::{ProjectSourceProvider, ProjectTargetSelector};
use miden_assembly_syntax::DisplayHex;
use midenc_hir::{Context, FxHashMap, Report};
use midenc_session::InputFile;

use super::Artifact;
use crate::{
    CompilerResult, Stage,
    stages::assemble::{RustSourceProvider, prepare_assembler},
};

/// Perform assembly of a Miden Assembly project
pub struct ProjectAssemblyStage;

impl Stage for ProjectAssemblyStage {
    type Input = InputFile;
    type Output = Artifact;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let session = context.session_rc();
        let package = session.project.package();
        let mut registry = session.package_registry()?;

        let file_name = input.file_name();
        let manifest_path = match file_name.file_name() {
            Some(name) if name.eq_ignore_ascii_case("Cargo.toml") => {
                let manifest_dir = file_name.as_path().parent().unwrap();
                manifest_dir.join("miden-project.toml")
            }
            Some(name) if name.eq_ignore_ascii_case("miden-project.toml") => {
                file_name.as_path().to_path_buf()
            }
            _ => {
                return Err(Report::msg(
                    "unsupported toml input: expected either `miden-project.toml` or `Cargo.toml`",
                ));
            }
        };

        let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
            .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());

        prepare_assembler(&mut assembler, &package, &session)?;

        let selector = if session.options.target_type.unwrap_or_default().is_executable() {
            ProjectTargetSelector::Executable(session.name.as_str())
        } else {
            ProjectTargetSelector::Library
        };
        let providers = [Box::new(RustSourceProvider {
            session: session.clone(),
            compiled: RefCell::new(FxHashMap::default()),
        }) as Box<dyn ProjectSourceProvider>];
        let mut project_assembler = assembler.for_project_at_path_with_providers(
            &manifest_path,
            registry.as_mut(),
            providers,
        )?;
        let package = project_assembler.assemble(selector, "dev")?;

        log::debug!(
            "successfully assembled package with digest {}",
            DisplayHex::new(&package.digest().as_bytes())
        );

        Ok(Artifact::Assembled(package))
    }
}
