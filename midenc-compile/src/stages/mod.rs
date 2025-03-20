use alloc::rc::Rc;

use midenc_frontend_wasm2 as wasm;
use midenc_hir::{dialects::builtin, Context};
#[cfg(feature = "std")]
use midenc_session::diagnostics::WrapErr;
use midenc_session::{
    diagnostics::{IntoDiagnostic, Report},
    OutputMode, Session,
};

use super::Stage;
use crate::{CompilerResult, CompilerStopped};

mod assemble;
mod codegen;
mod link;
mod parse;
mod rewrite;

pub use self::{
    assemble::{Artifact, AssembleStage},
    codegen::{CodegenOutput, CodegenStage},
    link::{LinkOutput, LinkStage},
    parse::{ParseOutput, ParseStage},
    rewrite::ApplyRewritesStage,
};
