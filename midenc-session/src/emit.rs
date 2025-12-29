use alloc::{boxed::Box, fmt, format, string::ToString, sync::Arc, vec};

use miden_core::{prettier::PrettyPrint, utils::Serializable};
use miden_mast_package::MastArtifact;
use midenc_hir_symbol::Symbol;

use crate::{OutputMode, OutputType, Session};

pub trait Emit {
    /// The name of this item, if applicable
    fn name(&self) -> Option<Symbol>;
    /// The output type associated with this item and the given `mode`
    fn output_type(&self, mode: OutputMode) -> OutputType;
    /// Write this item to the given [std::io::Write] handle, using `mode` to determine the output
    /// type
    fn write_to<W: Writer>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()>;
}

#[cfg(feature = "std")]
pub trait EmitExt: Emit {
    /// Write this item to standard output, inferring the best [OutputMode] based on whether or not
    /// stdout is a tty or not
    fn write_to_stdout(&self, session: &Session) -> anyhow::Result<()>;
    /// Write this item to the given file path, using `mode` to determine the output type
    fn write_to_file(
        &self,
        path: &std::path::Path,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()>;
}

#[cfg(feature = "std")]
impl<T: ?Sized + Emit> EmitExt for T {
    default fn write_to_stdout(&self, session: &Session) -> anyhow::Result<()> {
        use std::io::IsTerminal;
        let stdout = std::io::stdout().lock();
        let mode = if stdout.is_terminal() {
            OutputMode::Text
        } else {
            OutputMode::Binary
        };
        self.write_to(stdout, mode, session)
    }

    default fn write_to_file(
        &self,
        path: &std::path::Path,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::File::create(path)?;
        self.write_to(file, mode, session)
    }
}

/// A trait that provides a subset of the [std::io::Write] functionality that is usable in no-std
/// contexts.
pub trait Writer {
    fn write_fmt(&mut self, fmt: core::fmt::Arguments<'_>) -> anyhow::Result<()>;
    fn write_all(&mut self, buf: &[u8]) -> anyhow::Result<()>;
}

#[cfg(feature = "std")]
impl<W: ?Sized + std::io::Write> Writer for W {
    fn write_fmt(&mut self, fmt: core::fmt::Arguments<'_>) -> anyhow::Result<()> {
        <W as std::io::Write>::write_fmt(self, fmt).map_err(|err| err.into())
    }

    fn write_all(&mut self, buf: &[u8]) -> anyhow::Result<()> {
        <W as std::io::Write>::write_all(self, buf).map_err(|err| err.into())
    }
}

#[cfg(not(feature = "std"))]
impl Writer for alloc::vec::Vec<u8> {
    fn write_fmt(&mut self, fmt: core::fmt::Arguments<'_>) -> anyhow::Result<()> {
        if let Some(s) = fmt.as_str() {
            self.extend(s.as_bytes());
        } else {
            let formatted = fmt.to_string();
            self.extend(formatted.as_bytes());
        }
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> anyhow::Result<()> {
        self.extend(buf);
        Ok(())
    }
}

#[cfg(not(feature = "std"))]
impl Writer for alloc::string::String {
    fn write_fmt(&mut self, fmt: core::fmt::Arguments<'_>) -> anyhow::Result<()> {
        if let Some(s) = fmt.as_str() {
            self.push_str(s);
        } else {
            let formatted = fmt.to_string();
            self.push_str(&formatted);
        }
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> anyhow::Result<()> {
        let s = core::str::from_utf8(buf)?;
        self.push_str(s);
        Ok(())
    }
}

impl<T: Emit> Emit for &T {
    #[inline]
    fn name(&self) -> Option<Symbol> {
        (**self).name()
    }

    #[inline]
    fn output_type(&self, mode: OutputMode) -> OutputType {
        (**self).output_type(mode)
    }

    #[inline]
    fn write_to<W: Writer>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        (**self).write_to(writer, mode, session)
    }
}

impl<T: Emit> Emit for &mut T {
    #[inline]
    fn name(&self) -> Option<Symbol> {
        (**self).name()
    }

    #[inline]
    fn output_type(&self, mode: OutputMode) -> OutputType {
        (**self).output_type(mode)
    }

    #[inline]
    fn write_to<W: Writer>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        (**self).write_to(writer, mode, session)
    }
}

impl<T: Emit> Emit for Box<T> {
    #[inline]
    fn name(&self) -> Option<Symbol> {
        (**self).name()
    }

    #[inline]
    fn output_type(&self, mode: OutputMode) -> OutputType {
        (**self).output_type(mode)
    }

    #[inline]
    fn write_to<W: Writer>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        (**self).write_to(writer, mode, session)
    }
}

impl<T: Emit> Emit for Arc<T> {
    #[inline]
    fn name(&self) -> Option<Symbol> {
        (**self).name()
    }

    #[inline]
    fn output_type(&self, mode: OutputMode) -> OutputType {
        (**self).output_type(mode)
    }

    #[inline]
    fn write_to<W: Writer>(
        &self,
        writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        (**self).write_to(writer, mode, session)
    }
}

impl Emit for alloc::string::String {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Hir
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        _mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        writer.write_fmt(format_args!("{self}\n"))
    }
}

impl Emit for miden_assembly::ast::Module {
    fn name(&self) -> Option<Symbol> {
        Some(Symbol::intern(self.path().to_string()))
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Masm
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        assert_eq!(mode, OutputMode::Text, "masm syntax trees do not support binary mode");
        writer.write_fmt(format_args!("{self}\n"))
    }
}

#[cfg(feature = "std")]
macro_rules! serialize_into {
    ($serializable:ident, $writer:expr) => {
        // NOTE: We're protecting against unwinds here due to i/o errors that will get turned into
        // panics if writing to the underlying file fails. This is because ByteWriter does not have
        // fallible APIs, thus WriteAdapter has to panic if writes fail. This could be fixed, but
        // that has to happen upstream in winterfell
        std::panic::catch_unwind(move || {
            let mut writer = ByteWriterAdapter($writer);
            $serializable.write_into(&mut writer)
        })
        .map_err(|p| {
            match p.downcast::<anyhow::Error>() {
                // SAFETY: It is guaranteed to be safe to read Box<anyhow::Error>
                Ok(err) => unsafe { core::ptr::read(&*err) },
                // Propagate unknown panics
                Err(err) => std::panic::resume_unwind(err),
            }
        })
    };
}

struct ByteWriterAdapter<'a, W>(&'a mut W);
impl<W: Writer> miden_assembly::utils::ByteWriter for ByteWriterAdapter<'_, W> {
    fn write_u8(&mut self, value: u8) {
        self.0.write_all(&[value]).unwrap()
    }

    fn write_bytes(&mut self, values: &[u8]) {
        self.0.write_all(values).unwrap()
    }
}

impl Emit for miden_assembly::Library {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, mode: OutputMode) -> OutputType {
        let _ = mode;
        OutputType::Mast
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        struct LibraryTextFormatter<'a>(&'a miden_assembly::Library);
        impl miden_core::prettier::PrettyPrint for LibraryTextFormatter<'_> {
            fn render(&self) -> miden_core::prettier::Document {
                use miden_core::{mast::MastNodeExt, prettier::*};

                let mast_forest = self.0.mast_forest();
                let mut library_doc = Document::Empty;
                for module_info in self.0.module_infos() {
                    let mut fragments = vec![];
                    for (_, info) in module_info.procedures() {
                        if let Some(proc_node_id) = mast_forest.find_procedure_root(info.digest) {
                            let proc = mast_forest
                                .get_node_by_id(proc_node_id)
                                .expect("malformed mast forest")
                                .to_pretty_print(mast_forest)
                                .render();
                            fragments.push(indent(
                                4,
                                display(format!("procedure {} ({})", &info.name, &info.digest))
                                    + nl()
                                    + proc
                                    + nl()
                                    + const_text("end"),
                            ));
                        }
                    }
                    let module_doc = indent(
                        4,
                        display(format!("module {}", module_info.path()))
                            + nl()
                            + fragments
                                .into_iter()
                                .reduce(|l, r| l + nl() + nl() + r)
                                .unwrap_or_default()
                            + const_text("end"),
                    );
                    if matches!(library_doc, Document::Empty) {
                        library_doc = module_doc;
                    } else {
                        library_doc += nl() + nl() + module_doc;
                    }
                }
                library_doc
            }
        }
        impl fmt::Display for LibraryTextFormatter<'_> {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.pretty_print(f)
            }
        }

        match mode {
            OutputMode::Text => writer.write_fmt(format_args!("{}", LibraryTextFormatter(self))),
            OutputMode::Binary => {
                let mut writer = ByteWriterAdapter(&mut writer);
                self.write_into(&mut writer);
                Ok(())
            }
        }
    }
}

impl Emit for miden_core::Program {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, mode: OutputMode) -> OutputType {
        let _ = mode;
        OutputType::Mast
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        match mode {
            //OutputMode::Text => writer.write_fmt(format_args!("{}", self)),
            OutputMode::Text => unimplemented!("emitting mast in text form is currently broken"),
            OutputMode::Binary => {
                let mut writer = ByteWriterAdapter(&mut writer);
                self.write_into(&mut writer);
                Ok(())
            }
        }
    }
}

#[cfg(feature = "std")]
impl EmitExt for miden_core::Program {
    fn write_to_file(
        &self,
        path: &std::path::Path,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::File::create(path)?;
        match mode {
            OutputMode::Text => self.write_to(&mut file, mode, session),
            OutputMode::Binary => serialize_into!(self, &mut file),
        }
    }

    fn write_to_stdout(&self, session: &Session) -> anyhow::Result<()> {
        use std::io::IsTerminal;
        let mut stdout = std::io::stdout().lock();
        let mode = if stdout.is_terminal() {
            OutputMode::Text
        } else {
            OutputMode::Binary
        };
        match mode {
            OutputMode::Text => self.write_to(&mut stdout, mode, session),
            OutputMode::Binary => serialize_into!(self, &mut stdout),
        }
    }
}

impl Emit for miden_mast_package::Package {
    fn name(&self) -> Option<Symbol> {
        Some(Symbol::intern(&self.name))
    }

    fn output_type(&self, mode: OutputMode) -> OutputType {
        match mode {
            OutputMode::Text => OutputType::Mast,
            OutputMode::Binary => OutputType::Masp,
        }
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        match mode {
            OutputMode::Text => match self.mast {
                miden_mast_package::MastArtifact::Executable(ref prog) => {
                    prog.write_to(writer, mode, session)
                }
                miden_mast_package::MastArtifact::Library(ref lib) => {
                    lib.write_to(writer, mode, session)
                }
            },
            OutputMode::Binary => {
                let bytes = self.to_bytes();
                writer.write_all(bytes.as_slice())
            }
        }
    }
}

impl Emit for MastArtifact {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, mode: OutputMode) -> OutputType {
        let _ = mode;
        OutputType::Mast
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        session: &Session,
    ) -> anyhow::Result<()> {
        match mode {
            OutputMode::Text => match self {
                MastArtifact::Executable(program) => program.write_to(writer, mode, session),
                MastArtifact::Library(lib) => lib.write_to(writer, mode, session),
            },
            OutputMode::Binary => {
                let mut writer = ByteWriterAdapter(&mut writer);
                self.write_into(&mut writer);
                Ok(())
            }
        }
    }
}
