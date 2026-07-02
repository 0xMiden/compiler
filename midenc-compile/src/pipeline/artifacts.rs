//! The concrete values that flow between parse, codegen and assembly.
//!
//! Three types, one per boundary: [`MidenComponent`] is what a parse produces,
//! [`CodegenOutput`] is what lowering produces from it, and [`CompiledArtifact`] is what a
//! whole compilation returns — either the lowered Miden Assembly, when assembly was not asked
//! for, or the assembled package.
//!
//! These are the *concrete payloads* the compiler's phases hand each other. They are not
//! [`Artifact`](super::Artifact), the pipeline's type-erased checkpoint envelope: that one is
//! the box a frontend publishes at a checkpoint, and it can hold any of these — or anything a
//! frontend invents. The distinction is why [`CompiledArtifact`] carries the longer name; it
//! is the *finished* artifact of one whole compilation, not the artifact of a checkpoint.

use alloc::{sync::Arc, vec::Vec};

use miden_mast_package::Package;
use midenc_codegen_masm::MasmComponent;
use midenc_hir::dialects::builtin;

/// A parsed Miden component, together with everything assembly will need from the parse.
pub struct MidenComponent {
    pub world: builtin::WorldRef,
    pub component: Option<builtin::ComponentRef>,
    pub account_component_metadata_bytes: Option<Vec<u8>>,
    pub component_wit_bytes: Option<Vec<u8>>,
    #[cfg(feature = "std")]
    pub source_provenance: miden_assembly::ProjectSourceProvenanceInputs,
}

impl Clone for MidenComponent {
    fn clone(&self) -> Self {
        Self {
            world: self.world,
            component: self.component,
            account_component_metadata_bytes: self.account_component_metadata_bytes.clone(),
            component_wit_bytes: self.component_wit_bytes.clone(),
            #[cfg(feature = "std")]
            source_provenance: miden_assembly::ProjectSourceProvenanceInputs {
                root: miden_assembly::SourceFileProvenance {
                    path: self.source_provenance.root.path.clone(),
                    content: self.source_provenance.root.content.clone(),
                },
                support: self
                    .source_provenance
                    .support
                    .iter()
                    .map(|sfp| miden_assembly::SourceFileProvenance {
                        path: sfp.path.clone(),
                        content: sfp.content.clone(),
                    })
                    .collect(),
            },
        }
    }
}

/// The Miden Assembly a component was lowered to, ready to be assembled.
pub struct CodegenOutput {
    pub component: Arc<MasmComponent>,
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.)
    pub account_component_metadata_bytes: Option<Vec<u8>>,
    /// The component's public WIT source emitted by the `#[component]` macro.
    pub component_wit_bytes: Option<Vec<u8>>,
    #[cfg(feature = "std")]
    pub source_provenance: miden_assembly::ProjectSourceProvenanceInputs,
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

impl Clone for CodegenOutput {
    fn clone(&self) -> Self {
        Self {
            component: self.component.clone(),
            account_component_metadata_bytes: self.account_component_metadata_bytes.clone(),
            component_wit_bytes: self.component_wit_bytes.clone(),
            source_provenance: self.source_provenance(),
        }
    }
}

/// The finished artifact of one whole compilation.
///
/// Which of the two shapes comes back depends on what outputs were requested, and on what
/// options were specified: a run that was not asked to assemble stops with the lowered Miden
/// Assembly in hand.
pub enum CompiledArtifact {
    Lowered(CodegenOutput),
    Assembled(Arc<Package>),
}

impl CompiledArtifact {
    pub fn unwrap_mast(self) -> Arc<Package> {
        match self {
            Self::Assembled(mast) => mast,
            Self::Lowered(_) => {
                panic!("expected 'mast' artifact, but assembler stage was not run")
            }
        }
    }
}
