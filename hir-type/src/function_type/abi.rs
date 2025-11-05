use core::fmt;

/// Represents the calling convention of a function.
///
/// Calling conventions are part of a program's ABI (Application Binary Interface), and defines the
/// contract between caller and callee, by specifying the architecture-specific details of how
/// arguments are passed, results are returned, what effects may/will occur (e.g. context switches),
/// etc.
///
/// Additionally, calling conventions define the set of allowed types for function arguments and
/// results, and how those types are represented in the target ABI. It may impose additional
/// restrictions on callers, such as only allowing calls under specific conditions.
///
/// It is not required that callers be functions of the same convention as the callee, it is
/// perfectly acceptable to mix conventions in a program. The only requirement is that the
/// convention used at a given call site, matches the convention of the callee, i.e. it must
/// be the case that caller and callee agree on the convention used for that call.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr)
)]
#[repr(u8)]
pub enum CallConv {
    /// This convention passes all arguments and results by value, and thus requires that the types
    /// of its arguments and results be valid immediates in the Miden ABI. The representation of
    /// those types on the operand stack is specified by the Miden ABI.
    ///
    /// Additional properties of this convention:
    ///
    /// * It is always executed in the caller's context
    /// * May only be the target of a `exec` or `dynexec` instruction
    /// * Must not require more than 16 elements of the operand stack for arguments or results
    /// * Callees are expected to preserve the state of the operand stack following the function
    ///   arguments, such that from the caller's perspective, the effect of the call on the operand
    ///   stack is as if the function arguments had been popped, and the function results were
    ///   pushed.
    ///
    /// This convention is optimal when both caller and callee are in the same language, no context
    /// switching is required, and the arguments and results can be passed on the operand stack
    /// without the potential for overflow.
    Fast,
    /// The C calling convention, as specified for System V (x86), adapted to Miden's ABI.
    ///
    /// The purpose of this convention is to support cross-language interop via a foreign function
    /// interface (FFI) based on the C data layout rules. It is specifically designed for interop
    /// occurring _within_ the same context. For cross-language, cross-context interop, we require
    /// the use of the Wasm Component Model, see the `CanonLift` convention for more.
    ///
    /// Notable properties of this convention:
    ///
    /// * It is always executed in the caller's context
    /// * May only be the target of a `exec` or `dynexec` instruction
    /// * Supports any IR type that corresponds to a valid C type (i.e. structs, arrays, etc.)
    /// * Aggregates (structs and arrays) must be returned by reference, where the caller is
    ///   responsible for allocating the memory to which the return value will be written. The
    ///   caller passes the pointer to that memory as the first parameter of the function, which
    ///   must be of matching pointer type, and marked with the `sret` attribute. When the "struct
    ///   return" protocol is in use, the function does not return any values directly.
    /// * Aggregates up to 64 bits may be passed by value, all other structs must be passed by
    ///   reference.
    /// * Integer types up to 128 bits are supported, and are passed by value
    /// * Floating-point types are not supported
    /// * Callees must preserve the state of the operand stack following the function arguments
    /// * If the function arguments require more than 16 elements of the operand stack to represent,
    ///   then arguments will be spilled to the caller's stack frame, such that no more than 15
    ///   elements are required. The caller must then obtain the value of the stack pointer on
    ///   entry, and offset it to access spilled arguments as desired. The arguments are spilled
    ///   in reverse order, i.e. the last argument in the argument list has the greatest offset,
    ///   while the first argument of the argument list to be spilled starts at `sp` (the value
    ///   of the stack pointer on entry).
    ///
    /// NOTE: This convention is non-optimal if not being used for cross-language interop.
    ///
    #[default]
    SystemV,
    /// This convention is used to represent functions translated from WebAssembly.
    ///
    /// It has the following properties:
    ///
    /// * It is always executed in the caller's context
    /// * May only be the target of a `exec` or `dynexec` instruction
    /// * Only supports IR types which correspond to a valid WebAssembly type. Notably this does
    ///   not include aggregates (except via reference types, which are not currently supported).
    /// * Floating-point types are not allowed, except `f32` which is used to represent field
    ///   elements in the WebAssembly type system.
    /// * Callees must preserve the state of the operand stack following the function arguments
    /// * Uses the same argument spilling strategy as the `SystemV` convention
    Wasm,
    /// This convention represents one of the host-defined primitives of the Wasm Component Model.
    ///
    /// In particular, this convention corresponds to functions synthesized via a `(canon lift)`
    /// declaration, which is used to export a core Wasm function with a Canonical ABI signature.
    /// These synthesized functions are responsible for "lowering" arguments out of the Canonical
    /// ABI into a `Wasm`-compatible representation, and "lifting" results back into the Canonical
    /// ABI.
    ///
    /// NOTE: Some important details of this calling convention are described by the Canonical ABI
    /// spec covering the `canon lift` primitive, as well as how types are lifted/lowered from/to
    /// the "flattened" core Wasm type representation. See
    /// [this document](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#lifting-and-lowering-values)
    /// for those details, and other useful information about the Canonical ABI.
    ///
    /// It has the following properties:
    ///
    /// * It is always executed in a new context
    /// * May only be the target of a `call` or `dyncall` instruction
    /// * May only be called from a `CanonLower` function
    /// * Only supports IR types which correspond to a valid Canonical ABI type. Notably this does
    ///   not include pointer types. No support for `resource` types exists at this time due to
    ///   limitations of the Miden VM.
    /// * Callers must ensure that sensitive data is removed from the first 16 elements of the
    ///   operand stack when lowering calls to `CanonLift` functions. This compiler will zero-pad
    ///   the unused portions of the operand stack where applicable.
    /// * If the function arguments require more than 16 elements of the operand stack to represent,
    ///   then arguments will be spilled to the advice provider, as a block of memory (in words)
    ///   sufficiently large to hold all of the spilled arguments. The block will be hashed and
    ///   the digest used as the key in the advice map under which the block will be stored. The
    ///   digest will then be passed by-value to the callee as the first argument. This requires
    ///   a word (4 elements) of operand stack to be unused, so when spilling arguments, the first
    ///   12 elements are preserved, while the rest are spilled. The callee is expected to, as part
    ///   of its prologue, immediately fetch the spilled arguments from the advice map using the
    ///   provided digest on top of the operand stack, and write them into the current stack frame.
    ///   The spilled arguments can then be accessed by computing the offset from the stack pointer
    ///   to the desired argument. Note that, like the spill strategy for `SystemV`, the spilled
    ///   arguments will be written into memory in reverse order (the closer to the front of the
    ///   argument list, the smaller the offset).
    ///
    /// Unlike `CanonLower`, the details of this calling convention are stable, as it is designed
    /// expressly for cross-language, cross-context interop, and is in fact the only supported
    /// way to represent cross-context function calls at this time.
    CanonLift,
    /// This convention represents one of the host-defined primitives of the Wasm Component Model.
    ///
    /// In particular, this convention corresponds to functions synthesized via a `(canon lower)`
    /// declaration, which is used to import a Canonical ABI function into a core Wasm module,
    /// by providing a `Wasm`-compatible adapter for the underlying Canonical ABI function. These
    /// synthesized functions are responsible for "lifting" the core Wasm arguments into the
    /// Canonical ABI representation, and "lowering" the results out of that representation.
    ///
    /// This convention is identical to `Wasm`, with the following additional properties:
    ///
    /// * It is the only convention which may contain calls to a `CanonLift` function
    /// * Functions using this convention are not allowed to have `Public` visibility
    /// * Functions using this convention are considered to be compiler-generated, and thus are
    ///   aggressively inlined/eliminated where possible.
    ///
    /// This should be considered an unstable, compiler-internal calling convention, and the details
    /// of this convention can change at any time. Currently, it is only used by the Wasm frontend
    /// to distinguish functions synthesized from a `(canon lower)`.
    CanonLower,
    /// This convention is like `Fast`, but indicates that the function implements a syscall as
    /// part of a kernel module definition.
    ///
    /// Additional properties include:
    ///
    /// * It is always executed in the _root_ context, and therefore a context switch is
    ///   involved.
    /// * This convention may only be called via the `syscall` instruction, and may not be
    ///   called from another `Kernel` function.
    /// * This convention is only permitted on function _definitions_ when emitting a kernel library
    /// * In addition to the type restrictions described by the `Fast` convention, it additionally
    ///   forbids any arguments/results of pointer type, due to the context switch that occurs.
    Kernel,
}

impl CallConv {
    /// Returns true if this convention corresponds to one of the two Canonical ABI conventions
    pub fn is_wasm_canonical_abi(&self) -> bool {
        matches!(self, Self::CanonLift | Self::CanonLower)
    }
}

impl fmt::Display for CallConv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use miden_formatting::prettier::PrettyPrint;
        self.pretty_print(f)
    }
}

impl miden_formatting::prettier::PrettyPrint for CallConv {
    fn render(&self) -> miden_formatting::prettier::Document {
        use miden_formatting::prettier::const_text;

        match self {
            Self::Fast => const_text("fast"),
            Self::SystemV => const_text("C"),
            Self::Wasm => const_text("wasm"),
            Self::CanonLift => const_text("canon-lift"),
            Self::CanonLower => const_text("canon-lower"),
            Self::Kernel => const_text("kernel"),
        }
    }
}
