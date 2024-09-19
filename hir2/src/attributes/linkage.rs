use core::fmt;

/// The policy to apply to a global variable (or function) when linking
/// together a program during code generation.
///
/// Miden doesn't (currently) have a notion of a symbol table for things like global variables.
/// At runtime, there are not actually symbols at all in any familiar sense, instead functions,
/// being the only entities with a formal identity in MASM, are either inlined at all their call
/// sites, or are referenced by the hash of their MAST root, to be unhashed at runtime if the call
/// is executed.
///
/// Because of this, and because we cannot perform linking ourselves (we must emit separate modules,
/// and leave it up to the VM to link them into the MAST), there are limits to what we can do in
/// terms of linking function symbols. We essentially just validate that given a set of modules in
/// a [Program], that there are no invalid references across modules to symbols which either don't
/// exist, or which exist, but have internal linkage.
///
/// However, with global variables, we have a bit more freedom, as it is a concept that we are
/// completely inventing from whole cloth without explicit support from the VM or Miden Assembly.
/// In short, when we compile a [Program] to MASM, we first gather together all of the global
/// variables into a program-wide table, merging and garbage collecting as appropriate, and updating
/// all references to them in each module. This global variable table is then assumed to be laid out
/// in memory starting at the base of the linear memory address space in the same order, with
/// appropriate padding to ensure accesses are aligned. Then, when emitting MASM instructions which
/// reference global values, we use the layout information to derive the address where that global
/// value is allocated.
///
/// This has some downsides however, the biggest of which is that we can't prevent someone from
/// loading modules generated from a [Program] with either their own hand-written modules, or
/// even with modules from another [Program]. In such cases, assumptions about the allocation of
/// linear memory from different sets of modules will almost certainly lead to undefined behavior.
/// In the future, we hope to have a better solution to this problem, preferably one involving
/// native support from the Miden VM itself. For now though, we're working with what we've got.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_repr::Deserialize_repr, serde_repr::Serialize_repr)
)]
#[repr(u8)]
pub enum Linkage {
    /// This symbol is only visible in the containing module.
    ///
    /// Internal symbols may be renamed to avoid collisions
    ///
    /// Unreferenced internal symbols can be discarded at link time.
    Internal,
    /// This symbol will be linked using the "one definition rule", i.e. symbols with
    /// the same name, type, and linkage will be merged into a single definition.
    ///
    /// Unlike `internal` linkage, unreferenced `odr` symbols cannot be discarded.
    ///
    /// NOTE: `odr` symbols cannot satisfy external symbol references
    Odr,
    /// This symbol is visible externally, and can be used to resolve external symbol references.
    #[default]
    External,
}
impl fmt::Display for Linkage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Internal => f.write_str("internal"),
            Self::Odr => f.write_str("odr"),
            Self::External => f.write_str("external"),
        }
    }
}
