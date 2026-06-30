use super::*;

/// Marker trait for ops with recursive memory effects.
///
/// See [HasRecursiveEffects] for more details on the semantics of recursive effects.
pub trait HasRecursiveMemoryEffects {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MemoryEffect {
    /// The following effect indicates that the operation reads from some resource.
    ///
    /// A 'read' effect implies only dereferencing of the resource, and not any visible mutation.
    Read,
    /// The following effect indicates that the operation writes to some resource.
    ///
    /// A 'write' effect implies only mutating a resource, and not any visible dereference or read.
    Write,
    /// The following effect indicates that the operation allocates from some resource.
    ///
    /// An 'allocate' effect implies only allocation of the resource, and not any visible mutation or
    /// dereference.
    Allocate,
    /// The following effect indicates that the operation frees some resource that has been
    /// allocated.
    ///
    /// An 'allocate' effect implies only de-allocation of the resource, and not any visible
    /// allocation, mutation or dereference.
    Free,
}

impl AsRef<str> for MemoryEffect {
    fn as_ref(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Allocate => "allocate",
            Self::Free => "free",
        }
    }
}

impl core::fmt::Display for MemoryEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl core::str::FromStr for MemoryEffect {
    type Err = alloc::string::String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use alloc::string::ToString;
        match s {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "allocate" => Ok(Self::Allocate),
            "free" => Ok(Self::Free),
            s => Err(s.to_string()),
        }
    }
}

impl PartialEq<MemoryEffect> for &MemoryEffect {
    #[inline]
    fn eq(&self, other: &MemoryEffect) -> bool {
        (**self).eq(other)
    }
}

impl Effect for MemoryEffect {}

pub trait MemoryEffectOpInterface = EffectOpInterface<MemoryEffect>;
