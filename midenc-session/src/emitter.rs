#[cfg(feature = "std")]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Deref;

#[cfg(not(feature = "std"))]
use midenc_hir_symbol::sync::RwLock;

#[cfg(feature = "std")]
use crate::diagnostics::IntoDiagnostic;
use crate::{ColorChoice, diagnostics::Report};

/// The [Emitter] trait is used for controlling how diagnostics are displayed.
///
/// An [Emitter] must produce a [Buffer] for use by the rendering
/// internals, and its own print implementation.
///
/// When a diagnostic is being emitted, a new [Buffer] is allocated,
/// the diagnostic is rendered into it, and then the buffer is passed
/// to `print` for display by the [Emitter] implementation.
pub trait Emitter: Send + Sync {
    /// Construct a new [Buffer] for use by the renderer
    fn buffer(&self) -> Buffer;
    /// Display the contents of the given [Buffer]
    fn print(&self, buffer: Buffer) -> Result<(), Report>;
}

/// [DefaultEmitter] is used for rendering to stderr, and as is implied
/// by the name, is the default emitter implementation.
pub struct DefaultEmitter(DefaultEmitterImpl);
impl DefaultEmitter {
    /// Construct a new [DefaultEmitter] with the given [ColorChoice] behavior.
    pub fn new(color: ColorChoice) -> Self {
        Self(DefaultEmitterImpl::new(color))
    }
}
impl Emitter for DefaultEmitter {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        self.0.buffer()
    }

    #[inline(always)]
    fn print(&self, buffer: Buffer) -> Result<(), Report> {
        self.0.print(buffer)
    }
}
impl Deref for DefaultEmitter {
    type Target = DefaultEmitterImpl;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "std")]
#[doc(hidden)]
pub struct DefaultEmitterImpl {
    writer: termcolor::BufferWriter,
}

#[cfg(not(feature = "std"))]
#[doc(hidden)]
pub struct DefaultEmitterImpl {
    writer: RwLock<Vec<u8>>,
    ansi: bool,
}

#[cfg(feature = "std")]
impl DefaultEmitterImpl {
    fn new(color: ColorChoice) -> Self {
        Self {
            writer: termcolor::BufferWriter::stderr(color.into()),
        }
    }
}

#[cfg(feature = "std")]
impl Emitter for DefaultEmitterImpl {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        Buffer(self.writer.buffer())
    }

    #[inline(always)]
    fn print(&self, buffer: Buffer) -> Result<(), Report> {
        self.writer.print(&buffer.0).into_diagnostic()
    }
}

#[cfg(not(feature = "std"))]
impl DefaultEmitterImpl {
    fn new(color: ColorChoice) -> Self {
        Self {
            ansi: color.should_ansi(),
            writer: Default::default(),
        }
    }
}

#[cfg(not(feature = "std"))]
impl Emitter for DefaultEmitterImpl {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        if self.ansi {
            Buffer::ansi()
        } else {
            Buffer::no_color()
        }
    }

    #[inline(always)]
    fn print(&self, buffer: Buffer) -> Result<(), Report> {
        let mut writer = self.writer.write();
        writer.push(b'\n');
        writer.extend(buffer.into_inner());
        Ok(())
    }
}

/// [CaptureEmitter] is used to capture diagnostics which are emitted, for later examination.
///
/// This is intended for use in testing, where it is desirable to emit diagnostics
/// and write assertions about what was displayed to the user.
#[derive(Default)]
#[cfg(feature = "std")]
pub struct CaptureEmitter {
    buffer: parking_lot::Mutex<Vec<u8>>,
}
#[cfg(feature = "std")]
impl CaptureEmitter {
    /// Create a new [CaptureEmitter]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn captured(&self) -> String {
        let buf = self.buffer.lock();
        String::from_utf8_lossy(buf.as_slice()).into_owned()
    }
}
#[cfg(feature = "std")]
impl Emitter for CaptureEmitter {
    #[inline]
    fn buffer(&self) -> Buffer {
        Buffer::no_color()
    }

    #[inline]
    fn print(&self, buffer: Buffer) -> Result<(), Report> {
        let mut bytes = buffer.into_inner();
        let mut buf = self.buffer.lock();
        buf.append(&mut bytes);
        Ok(())
    }
}

/// [NullEmitter] is used to silence diagnostics entirely, without changing
/// anything in the diagnostic infrastructure.
///
/// When used, the rendered buffer is thrown away.
#[derive(Clone, Copy, Default)]
pub struct NullEmitter {
    ansi: bool,
}
impl NullEmitter {
    #[cfg(feature = "std")]
    pub fn new(color: ColorChoice) -> Self {
        use std::io::IsTerminal;

        let ansi = match color {
            ColorChoice::Never => false,
            ColorChoice::Always | ColorChoice::AlwaysAnsi => true,
            ColorChoice::Auto => std::io::stdout().is_terminal(),
        };
        Self { ansi }
    }

    #[cfg(not(feature = "std"))]
    pub fn new(color: ColorChoice) -> Self {
        let ansi = match color {
            ColorChoice::Never => false,
            ColorChoice::Always | ColorChoice::AlwaysAnsi => true,
            ColorChoice::Auto => false,
        };
        Self { ansi }
    }
}
impl Emitter for NullEmitter {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        if self.ansi {
            Buffer::ansi()
        } else {
            Buffer::no_color()
        }
    }

    #[inline(always)]
    fn print(&self, _buffer: Buffer) -> Result<(), Report> {
        Ok(())
    }
}

#[doc(hidden)]
#[cfg(not(feature = "std"))]
#[derive(Clone, Debug)]
pub struct Buffer(Vec<u8>);

#[doc(hidden)]
#[cfg(feature = "std")]
#[derive(Clone, Debug)]
pub struct Buffer(termcolor::Buffer);

impl Buffer {
    /// Create a new buffer with the given color settings.
    #[cfg(not(feature = "std"))]
    pub fn new(_choice: ColorChoice) -> Buffer {
        Self::no_color()
    }

    #[cfg(feature = "std")]
    pub fn new(choice: ColorChoice) -> Buffer {
        match choice {
            ColorChoice::Never => Self::no_color(),
            ColorChoice::Auto => {
                if choice.should_attempt_color() {
                    Self::ansi()
                } else {
                    Self::no_color()
                }
            }
            ColorChoice::Always | ColorChoice::AlwaysAnsi => Self::ansi(),
        }
    }

    /// Create a buffer that drops all color information.
    #[cfg(not(feature = "std"))]
    pub fn no_color() -> Buffer {
        Self(vec![])
    }

    /// Create a buffer that drops all color information.
    #[cfg(feature = "std")]
    pub fn no_color() -> Buffer {
        Self(termcolor::Buffer::no_color())
    }

    /// Create a buffer that uses ANSI escape sequences.
    #[cfg(not(feature = "std"))]
    pub fn ansi() -> Buffer {
        Buffer(vec![])
    }

    /// Create a buffer that uses ANSI escape sequences.
    #[cfg(feature = "std")]
    pub fn ansi() -> Buffer {
        Self(termcolor::Buffer::ansi())
    }

    /// Returns true if and only if this buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the length of this buffer in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Clears this buffer.
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// Consume this buffer and return the underlying raw data.
    ///
    /// On Windows, this unrecoverably drops all color information associated
    /// with the buffer.
    #[inline(always)]
    #[cfg(not(feature = "std"))]
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }

    #[cfg(feature = "std")]
    pub fn into_inner(self) -> Vec<u8> {
        self.0.into_inner()
    }

    /// Return the underlying data of the buffer.
    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Return the underlying data of the buffer as a mutable slice.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.0.as_mut_slice()
    }
}

#[cfg(not(feature = "std"))]
impl core::fmt::Write for Buffer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        use miden_assembly::utils::ByteWriter;
        self.0.write_bytes(s.as_bytes());
        Ok(())
    }
}

#[cfg(feature = "std")]
impl std::io::Write for Buffer {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
