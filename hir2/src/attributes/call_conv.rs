use core::fmt;

/// Represents the calling convention of a function.
///
/// Calling conventions are part of a program's ABI (Application Binary Interface), and
/// they define things such how arguments are passed to a function, how results are returned,
/// etc. In essence, the contract between caller and callee is described by the calling convention
/// of a function.
///
/// Importantly, it is perfectly normal to mix calling conventions. For example, the public
/// API for a C library will use whatever calling convention is used by C on the target
/// platform (for Miden, that would be `SystemV`). However, internally that library may use
/// the `Fast` calling convention to allow the compiler to optimize more effectively calls
/// from the public API to private functions. In short, choose a calling convention that is
/// well-suited for a given function, to the extent that other constraints don't impose a choice
/// on you.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum CallConv {
    /// This calling convention is what I like to call "chef's choice" - the
    /// compiler chooses it's own convention that optimizes for call performance.
    ///
    /// As a result of this, it is not permitted to use this convention in externally
    /// linked functions, as the convention is unstable, and the compiler can't ensure
    /// that the caller in another translation unit will use the correct convention.
    Fast,
    /// The standard calling convention used for C on most platforms
    #[default]
    SystemV,
    /// A function which is using the WebAssembly Component Model "Canonical ABI".
    Wasm,
    /// A function with this calling convention must be called using
    /// the `syscall` instruction. Attempts to call it with any other
    /// call instruction will cause a validation error. The one exception
    /// to this rule is when calling another function with the `Kernel`
    /// convention that is defined in the same module, which can use the
    /// standard `call` instruction.
    ///
    /// Kernel functions may only be defined in a kernel [Module].
    ///
    /// In all other respects, this calling convention is the same as `SystemV`
    Kernel,
}
impl fmt::Display for CallConv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Fast => f.write_str("fast"),
            Self::SystemV => f.write_str("C"),
            Self::Wasm => f.write_str("wasm"),
            Self::Kernel => f.write_str("kernel"),
        }
    }
}
