use super::*;

/// The advice stack
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AdviceStackResource;
impl Resource for AdviceStackResource {
    fn name(&self) -> &'static str {
        "advice-stack"
    }
}

/// The advice map
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AdviceMapResource;
impl Resource for AdviceMapResource {
    fn name(&self) -> &'static str {
        "advice-map"
    }
}

/// The merkle store
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MerkleStoreResource;
impl Resource for MerkleStoreResource {
    fn name(&self) -> &'static str {
        "advice-merkles-tore"
    }
}

/// Advice effects are side effects modeled much like memory effects, as they are observable
/// globally, including to other contexts.
///
/// Thus optimizations must be very conservative with reordering of operations with advice
/// effects, so as to avoid accidentally violating assumptions of advice-related code.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AdviceEffect {
    /// Indicates that the operation reads from an advice resource.
    Read,
    /// Indicates that the operation writes to an advice resource.
    ///
    /// Mutations of the advice provider state are visible globally, so the optimizer
    /// is never allowed to reorder operations with advice effects on the same advice
    /// resource, with respect to write effects on that resource.
    Write,
    /// Indicates that the operation allocates some resource.
    ///
    /// This corresponds to inserting new keys in the advice map, or pushing a value
    /// on the advice stack.
    ///
    /// An 'allocate' effect implies only allocation of the resource, and not any visible mutation or
    /// dereference. In the case of a debugger, this might correspond to allocating a new call frame
    /// or start tracking the state of a local variable.
    Allocate,
    /// Indicates that the operation frees some resource.
    ///
    /// This is currently only relevant for the advice stack, to model popping values from the
    /// advice stack.
    Free,
}

impl AsRef<str> for AdviceEffect {
    fn as_ref(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Allocate => "allocate",
            Self::Free => "free",
        }
    }
}

impl core::fmt::Display for AdviceEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl core::str::FromStr for AdviceEffect {
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

impl PartialEq<AdviceEffect> for &AdviceEffect {
    #[inline]
    fn eq(&self, other: &AdviceEffect) -> bool {
        (**self).eq(other)
    }
}

impl Effect for AdviceEffect {}

pub trait AdviceEffectOpInterface = EffectOpInterface<AdviceEffect>;

/// Marker trait for ops with recursive advice effects.
///
/// See [HasRecursiveEffects] for more details on the semantics of recursive effects.
pub trait HasRecursiveAdviceEffects {}
