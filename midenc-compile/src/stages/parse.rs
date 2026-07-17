mod hir;
mod masm;
mod rust;
mod wasm;

#[cfg(feature = "std")]
use alloc::{borrow::Cow, format, rc::Rc};
use alloc::{borrow::ToOwned, string::String, vec::Vec};

use miden_mast_package::TargetType;
use midenc_hir::{
    Context,
    diagnostics::{Report, Uri},
    dialects::builtin,
};
#[cfg(feature = "std")]
use midenc_session::{FileName, Path};
use midenc_session::{
    InputFile, InputType,
    diagnostics::{IntoDiagnostic, WrapErr},
};

pub use self::{
    hir::ParseHirStage,
    masm::{MasmSources, ParseMasmStage},
    rust::ParseRustStage,
    wasm::ParseWasmStage,
};
use crate::{CompilerResult, Stage};

pub struct MidenComponent {
    pub world: builtin::WorldRef,
    pub component: Option<builtin::ComponentRef>,
    pub account_component_metadata_bytes: Option<Vec<u8>>,
    #[cfg(feature = "std")]
    pub source_provenance: miden_assembly::ProjectSourceProvenanceInputs,
}

impl Clone for MidenComponent {
    fn clone(&self) -> Self {
        Self {
            world: self.world,
            component: self.component,
            account_component_metadata_bytes: self.account_component_metadata_bytes.clone(),
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

/// Parses any input that can be converted to a [MidenComponent]
pub struct ParseComponentStage;

impl Stage for ParseComponentStage {
    type Input = InputFile;
    type Output = MidenComponent;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_session::FileType;

        let file_type = input.file_type();
        match file_type {
            #[cfg(feature = "std")]
            FileType::Hir => {
                use miden_assembly::SourceFileProvenance;
                let provenance = match &input.file {
                    InputType::Real(path) => SourceFileProvenance::from_path(path.clone())?,
                    InputType::Stdin { name, input } => SourceFileProvenance {
                        path: name.as_path().to_path_buf().into_boxed_path(),
                        content: String::from_utf8_lossy(input).into_owned().into_boxed_str(),
                    },
                };
                let mut stage = ParseHirStage.map(super::extract_miden_component_or_bail);
                let mut component = stage.run(input, context)?;
                component.source_provenance.root = provenance;
                Ok(component)
            }
            #[cfg(not(feature = "std"))]
            FileType::Hir => {
                let mut stage = ParseHirStage.map(super::extract_miden_component_or_bail);
                stage.run(input, context)
            }
            FileType::Wasm | FileType::Wat => {
                let mut stage = ParseWasmStage;
                stage.run(input, context)
            }
            FileType::Rust => {
                let mut stage = ParseRustStage.next(ParseWasmStage);
                stage.run(input, context)
            }
            file_type => Err(Report::msg(format!(
                "unsupported file type '{file_type}' for parsing miden components"
            ))),
        }
    }
}
