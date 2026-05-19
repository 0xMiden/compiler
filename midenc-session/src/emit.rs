use alloc::{boxed::Box, string::ToString, sync::Arc};

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

impl Emit for miden_assembly_syntax::ast::Module {
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
        _session: &Session,
    ) -> anyhow::Result<()> {
        use miden_core::serde::Serializable;
        match mode {
            OutputMode::Text => {
                let bytes = self.to_bytes();
                writer.write_all(bytes.as_slice())
            }
            OutputMode::Binary => {
                let bytes = self.to_bytes();
                writer.write_all(bytes.as_slice())
            }
        }
    }
}
