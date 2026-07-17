mod hir;
mod masm;
mod rust;
mod wasm;

#[cfg(feature = "std")]
use alloc::{borrow::Cow, format, rc::Rc};
use alloc::{borrow::ToOwned, boxed::Box, vec::Vec};

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
    hir::ParseHirStage, masm::ParseMasmStage, rust::ParseRustStage, wasm::ParseWasmStage,
};
use crate::{CompilerResult, Stage};

#[derive(Clone)]
pub struct MidenComponent {
    pub world: builtin::WorldRef,
    pub component: Option<builtin::ComponentRef>,
    /// Out-of-band payloads destined for the compiled package's sections.
    pub sections: midenc_frontend_wasm_metadata::PackageSections,
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
