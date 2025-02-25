use alloc::{string::ToString, vec::Vec};

use miden_assembly::ast::QualifiedProcedureName;
use miden_mast_package::{Dependency, MastArtifact, Package};
use midenc_session::Session;

use super::*;

/// The artifact produced by the full compiler pipeline.
///
/// The type of artifact depends on what outputs were requested, and what options were specified.
pub enum Artifact {
    Lowered(CodegenOutput),
    Assembled(Package),
}
impl Artifact {
    pub fn unwrap_mast(self) -> Package {
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
        use midenc_hir2::formatter::DisplayHex;

        let session = context.session();
        if session.should_assemble() {
            let mast =
                input.component.assemble(&input.link_libraries, &input.link_packages, session)?;
            log::debug!(
                "successfully assembled mast artifact with digest {}",
                DisplayHex::new(&mast.digest().as_bytes())
            );
            session.emit(OutputMode::Text, &mast).into_diagnostic()?;
            session.emit(OutputMode::Binary, &mast).into_diagnostic()?;
            Ok(Artifact::Assembled(build_package(mast, &input, session)))
        } else {
            log::debug!(
                "skipping assembly of mast package from masm artifact (should-assemble=false)"
            );
            Ok(Artifact::Lowered(input))
        }
    }
}

fn build_package(mast: MastArtifact, outputs: &CodegenOutput, session: &Session) -> Package {
    let name = session.name.clone();

    let mut dependencies = Vec::new();
    for (link_lib, lib) in session.options.link_libraries.iter().zip(outputs.link_libraries.iter())
    {
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
        assert!(outputs.component.entrypoint.is_none(), "expect masm component to be a library");
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
