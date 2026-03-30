use alloc::{string::ToString, vec::Vec};

use miden_mast_package::{
    Dependency, Package, PackageManifest, Section, SectionId, TargetType, Version,
};
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
            let mast = input.component.assemble(
                &input.link_libraries,
                &input.link_packages,
                input.account_component_metadata_bytes.as_deref(),
                session,
            )?;
            log::debug!(
                "successfully assembled mast artifact with digest {}",
                DisplayHex::new(&mast.digest().as_bytes())
            );
            Ok(Artifact::Assembled(mast))
        } else {
            log::debug!(
                "skipping assembly of mast package from masm artifact (should-assemble=false)"
            );
            Ok(Artifact::Lowered(input))
        }
    }
}

fn build_package(
    artifact: midenc_codegen_masm::AssemblyArtifact,
    outputs: &CodegenOutput,
    session: &Session,
) -> Package {
    let name = session.name.clone().into();

    let mut dependencies = Vec::new();
    for (link_lib, lib) in session.options.link_libraries.iter().zip(outputs.link_libraries.iter())
    {
        let dependency = Dependency {
            name: link_lib.name.to_string().into(),
            kind: TargetType::Library,
            // proper version will be implemented in https://github.com/0xMiden/compiler/issues/1069
            version: Version::new(0, 0, 0),
            digest: *lib.digest(),
        };
        dependencies.push(dependency);
    }

    let kind = artifact.kind();
    let mast = artifact.into_mast();
    let manifest = PackageManifest::from_library(&mast)
        .with_dependencies(dependencies)
        .expect("package dependencies should be unique");

    let account_component_metadata_bytes = outputs.account_component_metadata_bytes.clone();
    let debug_info_bytes = outputs.debug_info_bytes.clone();

    let mut sections = Vec::new();

    if let Some(bytes) = account_component_metadata_bytes {
        sections.push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, bytes));
    }

    if let Some((types_bytes, sources_bytes, functions_bytes)) = debug_info_bytes {
        log::debug!(
            "adding debug sections to package (types={} sources={} functions={} bytes)",
            types_bytes.len(),
            sources_bytes.len(),
            functions_bytes.len(),
        );
        sections.push(Section::new(SectionId::DEBUG_TYPES, types_bytes));
        sections.push(Section::new(SectionId::DEBUG_SOURCES, sources_bytes));
        sections.push(Section::new(SectionId::DEBUG_FUNCTIONS, functions_bytes));
    }

    Package {
        name,
        // proper version will be implemented in https://github.com/0xMiden/compiler/issues/1068
        version: Version::new(0, 0, 0),
        description: None,
        kind,
        mast: mast.into(),
        manifest,
        sections,
    }
}
