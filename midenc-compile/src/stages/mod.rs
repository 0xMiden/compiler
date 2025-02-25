use alloc::rc::Rc;

use midenc_frontend_wasm2 as wasm;
use midenc_hir2::{dialects::builtin, Context};
use midenc_session::{
    diagnostics::{IntoDiagnostic, Report, WrapErr},
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
