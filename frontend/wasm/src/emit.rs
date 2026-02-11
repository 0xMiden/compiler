//! Utilities for emitting frontend-specific artifacts.

use alloc::string::String;

use midenc_session::{Emit, OutputMode, OutputType, Session, Writer};

/// A wrapper that emits WebAssembly text format (WAT).
#[derive(Debug, Copy, Clone)]
pub struct WatEmit<'a>(pub &'a str);

impl Emit for WatEmit<'_> {
    fn name(&self) -> Option<midenc_hir::interner::Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Wat
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        if mode != OutputMode::Text {
            anyhow::bail!("wat emission does not support binary mode");
        }
        writer.write_fmt(core::format_args!("{}", self.0))?;
        Ok(())
    }
}

/// Convert a WebAssembly binary to WAT text, filtering out highly variable custom sections.
#[cfg(feature = "std")]
pub fn wasm_to_wat(wasm_bytes: &[u8]) -> anyhow::Result<String> {
    use core::fmt;

    // Disable printing of the various custom sections, e.g. "producers", either because they
    // contain strings which are highly variable (but not important), or because they are debug info
    // related.
    struct NoCustomSectionsPrinter<T: wasmprinter::Print>(T);
    impl<T: wasmprinter::Print> wasmprinter::Print for NoCustomSectionsPrinter<T> {
        fn write_str(&mut self, s: &str) -> std::io::Result<()> {
            self.0.write_str(s)
        }

        fn newline(&mut self) -> std::io::Result<()> {
            self.0.newline()
        }

        fn start_line(&mut self, binary_offset: Option<usize>) {
            self.0.start_line(binary_offset);
        }

        fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> std::io::Result<()> {
            self.0.write_fmt(args)
        }

        fn print_custom_section(
            &mut self,
            name: &str,
            binary_offset: usize,
            data: &[u8],
        ) -> std::io::Result<bool> {
            match name {
                "producers" | "target_features" => Ok(true),
                debug if debug.starts_with(".debug") => Ok(true),
                _ => self.0.print_custom_section(name, binary_offset, data),
            }
        }

        fn start_literal(&mut self) -> std::io::Result<()> {
            self.0.start_literal()
        }

        fn start_name(&mut self) -> std::io::Result<()> {
            self.0.start_name()
        }

        fn start_keyword(&mut self) -> std::io::Result<()> {
            self.0.start_keyword()
        }

        fn start_type(&mut self) -> std::io::Result<()> {
            self.0.start_type()
        }

        fn start_comment(&mut self) -> std::io::Result<()> {
            self.0.start_comment()
        }

        fn reset_color(&mut self) -> std::io::Result<()> {
            self.0.reset_color()
        }

        fn supports_async_color(&self) -> bool {
            self.0.supports_async_color()
        }
    }

    // WAT text should be at least ~3x larger than the binary Wasm representation
    let mut wat = String::with_capacity(wasm_bytes.len() * 3);
    let config = wasmprinter::Config::new();
    let mut wasm_printer = NoCustomSectionsPrinter(wasmprinter::PrintFmtWrite(&mut wat));
    config.print(wasm_bytes, &mut wasm_printer)?;
    Ok(wat)
}
