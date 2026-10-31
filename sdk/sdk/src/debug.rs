use core::{
    fmt::{self, Write},
    panic::PanicInfo,
};

#[doc(inline)]
pub use miden_base_macros::MidenDebug;

/// A borrowed string formatted without allocation, quotes, or escaping.
///
/// Its [`core::fmt::Debug`] implementation writes the string directly, bypassing standard
/// string debug formatting and its string-slicing panic paths.
pub struct RawString<'a>(pub &'a str);

impl fmt::Debug for RawString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Prints a message through Miden's trace-based `PrintLn` functionality.
///
/// # Formatting
///
/// The formatting variant requires the `alloc` crate and a configured global allocator.
///
/// This supports most common use cases of Rust's `format!`. However, named arguments without
/// argument list are not supported, for example `println!("result: {res}");`
#[macro_export]
macro_rules! println {
    ($message:literal) => {{
        $crate::debug::println($message);
    }};
    ($message:expr) => {{
        $crate::debug::println($message);
    }};
    ($format:literal, $($arg:tt)+) => {{
        let message = ::alloc::format!($format, $($arg)+);
        $crate::debug::println(&message);
    }};
}

/// Prints a string through Miden's trace-based `PrintLn` functionality.
#[inline(always)]
pub fn println(s: &str) {
    let bytes = s.as_bytes();
    miden_stdlib_sys::intrinsics::debug::println(bytes.as_ptr(), bytes.len());
}

const PANIC_BUFFER_CAPACITY: usize = 256;

/// Prints a panic message without allocation, then returns to the caller.
///
/// Uses a fixed-size local buffer. If formatting fails or exceeds its capacity, prints
/// `panic message formatting failed` instead of any partial output. Only the message is
/// printed, not the source location.
///
/// Use this only when targeting Miden VM. It's purpose is avoiding call graph cycles that can be
/// introduced by printing and formatting based on `alloc`.
pub fn print_panic(info: &PanicInfo) {
    let mut buffer = PanicBuffer::new();
    if write!(&mut buffer, "{}", info.message()).is_ok() {
        miden_stdlib_sys::intrinsics::debug::println(buffer.bytes.as_ptr(), buffer.len);
    } else {
        println("panic message formatting failed");
    }
}

/// Prints a panic message using [`print_panic`], then traps.
///
/// Available on wasm32 targets. Delegate to this function from a `#[panic_handler]` function
/// to register it. Calling or importing this helper does not register a handler automatically.
/// Use [`print_panic`] instead to perform custom work before trapping.
#[cfg(target_arch = "wasm32")]
pub fn printing_panic_handler(info: &PanicInfo) -> ! {
    print_panic(info);
    core::arch::wasm32::unreachable()
}

/// Fixed-capacity buffer for formatting panic messages without allocation.
struct PanicBuffer {
    bytes: [u8; PANIC_BUFFER_CAPACITY],
    len: usize,
}

impl PanicBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; PANIC_BUFFER_CAPACITY],
            len: 0,
        }
    }
}

impl Write for PanicBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        // Avoid copy_from_slice's length-mismatch panic path in the panic handler.
        for (destination, source) in destination.iter_mut().zip(s.as_bytes()) {
            *destination = *source;
        }
        self.len = end;
        Ok(())
    }
}
