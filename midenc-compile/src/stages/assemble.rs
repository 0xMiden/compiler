use alloc::{string::ToString, vec::Vec};

use miden_assembly::ast::QualifiedProcedureName;
use miden_mast_package::{Dependency, MastArtifact, Package, PackageExport};
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
        use midenc_hir::formatter::DisplayHex;

        let session = context.session();
        if session.should_assemble() {
            log::debug!("assembling mast artifact");
            let mast =
                input.component.assemble(&input.link_libraries, &input.link_packages, session)?;
            log::debug!(
                "successfully assembled mast artifact with digest {}",
                DisplayHex::new(&mast.digest().as_bytes())
            );
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

    // Gather all of the procedure metadata for exports of this package
    let mut exports: Vec<PackageExport> = Vec::new();
    if let MastArtifact::Library(ref lib) = mast {
        assert!(outputs.component.entrypoint.is_none(), "expect masm component to be a library");
        for module_info in lib.module_infos() {
            for (_, proc_info) in module_info.procedures() {
                let name =
                    QualifiedProcedureName::new(module_info.path().clone(), proc_info.name.clone());
                let digest = proc_info.digest;
                let signature = proc_info.signature.as_deref().cloned();
                exports.push(miden_mast_package::PackageExport {
                    name,
                    digest,
                    signature,
                    attributes: Default::default(),
                });
            }
        }
    }

    let manifest =
        miden_mast_package::PackageManifest::new(exports).with_dependencies(dependencies);

    let account_component_metadata_bytes = outputs.account_component_metadata_bytes.clone();
    let debug_info_bytes = outputs.debug_info_bytes.clone();

    let mut sections = Vec::new();

    // Add account component metadata section if present
    if let Some(bytes) = account_component_metadata_bytes {
        sections.push(miden_mast_package::Section::new(
            miden_mast_package::SectionId::ACCOUNT_COMPONENT_METADATA,
            bytes,
        ));
    }

    // Add debug info section if present
    if let Some(bytes) = debug_info_bytes {
        log::debug!("adding .debug_info section to package ({} bytes)", bytes.len());
        sections.push(miden_mast_package::Section::new(
            miden_mast_package::SectionId::DEBUG_INFO,
            bytes,
        ));
    }

    miden_mast_package::Package {
        name,
        version: None,
        description: None,
        mast,
        manifest,
        sections,
    }
}
