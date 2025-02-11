use miden_assembly::ast::QualifiedProcedureName;
use miden_mast_package::{Dependency, MastArtifact, Package};
use midenc_codegen_masm::MasmArtifact;

use super::*;

/// The artifact produced by the full compiler pipeline.
///
/// The type of artifact depends on what outputs were requested, and what options were specified.
pub enum Artifact {
    /// The user requested MASM outputs, but
    Lowered(masm::ModuleTree),
    Linked(masm::MasmArtifact),
    Assembled(Package),
}
impl Artifact {
    pub fn unwrap_mast(self) -> Package {
        match self {
            Self::Assembled(mast) => mast,
            Self::Linked(_) => {
                panic!("expected 'mast' artifact, but got linked 'masm' artifact instead")
            }
            Self::Lowered(_) => {
                panic!("expected 'mast' artifact, but got unlinked 'masm' artifact instead")
            }
        }
    }
}

/// Perform assembly of the generated Miden Assembly, producing MAST
pub struct AssembleStage;
impl Stage for AssembleStage {
    type Input = Either<masm::MasmArtifact, masm::ModuleTree>;
    type Output = Artifact;

    fn run(
        &mut self,
        input: Self::Input,
        _analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        use midenc_hir::formatter::DisplayHex;

        match input {
            Left(masm_artifact) if session.should_assemble() => {
                let mast = masm_artifact.assemble(session)?;
                log::debug!(
                    "successfully assembled mast artifact with digest {}",
                    DisplayHex::new(&mast.digest().as_bytes())
                );
                session.emit(OutputMode::Text, &mast).into_diagnostic()?;
                session.emit(OutputMode::Binary, &mast).into_diagnostic()?;
                Ok(Artifact::Assembled(build_package(mast, &masm_artifact, session)))
            }
            Left(masm_artifact) => {
                log::debug!(
                    "skipping assembly of mast package from masm artifact (should-assemble=false)"
                );
                Ok(Artifact::Linked(masm_artifact))
            }
            Right(_masm_modules) if session.should_assemble() => todo!(), /* Ok(Artifact::Assembled(todo!())), */
            Right(masm_modules) => {
                log::debug!(
                    "skipping assembly of mast package from unlinked modules \
                     (should-assemble=false)"
                );
                Ok(Artifact::Lowered(masm_modules))
            }
        }
    }
}

fn build_package(mast: MastArtifact, masm: &MasmArtifact, session: &Session) -> Package {
    let name = session.name.clone();

    let mut dependencies = Vec::new();
    for link_lib in session.options.link_libraries.iter() {
        log::debug!(
            "registering link library '{}' ({}, from {:#?}) with linker",
            link_lib.name,
            link_lib.kind,
            link_lib.path.as_ref()
        );
        // TODO(denysz): We already loaded these libraries in the LinkStage. Pass them as inputs?
        let lib = link_lib
            .load(session)
            .unwrap_or_else(|_| panic!("failed to load link library {}", link_lib.name));
        let dependency = Dependency {
            name: link_lib.name.to_string().into(),
            digest: *lib.digest(),
        };
        dependencies.push(dependency);
    }

    let mut manifest = miden_mast_package::PackageManifest {
        exports: Default::default(),
        dependencies,
    };

    // Gather all of the procedure metadata for exports of this package
    if let MastArtifact::Library(ref lib) = mast {
        let MasmArtifact::Library(ref _masm_lib) = masm else {
            unreachable!("expected MasmArtifact to be a library");
        };
        for module_info in lib.module_infos() {
            for (_, proc_info) in module_info.procedures() {
                let name =
                    QualifiedProcedureName::new(module_info.path().clone(), proc_info.name.clone());
                let digest = proc_info.digest;
                manifest.exports.insert(miden_mast_package::PackageExport { name, digest });
            }
        }
    }

    miden_mast_package::Package {
        name,
        mast,
        manifest,
    }
}
