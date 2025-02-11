use miden_assembly::Library as CompiledLibrary;
use miden_mast_package::MastArtifact;
use midenc_hir::Symbol;
use midenc_session::{diagnostics::Report, Emit, OutputMode, OutputType, Session};

use crate::{Library, Module, Program};

/// The artifact produced by lowering an [hir::Program] to Miden Assembly
///
/// This type is used in compilation pipelines to abstract over the type of output requested.
pub enum MasmArtifact {
    /// An executable program, with a defined entrypoint
    Executable(Box<Program>),
    /// A library, linkable into a program as needed
    Library(Box<Library>),
}

impl MasmArtifact {
    pub fn assemble(&self, session: &Session) -> Result<MastArtifact, Report> {
        match self {
            Self::Executable(program) => program.assemble(session).map(MastArtifact::Executable),
            Self::Library(library) => library.assemble(session).map(MastArtifact::Library),
        }
    }

    /// Get an iterator over the modules in this library
    pub fn modules(&self) -> impl Iterator<Item = &Module> + '_ {
        match self {
            Self::Executable(ref program) => program.library().modules(),
            Self::Library(ref lib) => lib.modules(),
        }
    }

    pub fn insert(&mut self, module: Box<Module>) {
        match self {
            Self::Executable(ref mut program) => program.insert(module),
            Self::Library(ref mut lib) => lib.insert(module),
        }
    }

    pub fn link_library(&mut self, lib: CompiledLibrary) {
        match self {
            Self::Executable(ref mut program) => program.link_library(lib),
            Self::Library(ref mut library) => library.link_library(lib),
        }
    }

    pub fn unwrap_executable(self) -> Box<Program> {
        match self {
            Self::Executable(program) => program,
            Self::Library(_) => panic!("tried to unwrap a mast library as an executable"),
        }
    }
}

impl Emit for MasmArtifact {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Masm
    }

    fn write_to<W: std::io::Write>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> std::io::Result<()> {
        match self {
            Self::Executable(ref prog) => prog.write_to(writer, mode, session),
            Self::Library(ref lib) => lib.write_to(writer, mode, session),
        }
    }
}
