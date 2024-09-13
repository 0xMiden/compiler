mod multitrait;

pub(crate) use self::multitrait::MultiTraitVtable;

/// Marker trait for commutative ops, e.g. `X op Y == Y op X`
pub trait Commutative {}

/// Marker trait for constant-like ops
pub trait ConstantLike {}

/// Marker trait for ops with side effects
pub trait HasSideEffects {}

/// Marker trait for ops which read memory
pub trait MemoryRead {}

/// Marker trait for ops which write memory
pub trait MemoryWrite {}

/// Marker trait for return-like ops
pub trait ReturnLike {}

/// All operands of the given op are the same type
pub trait SameTypeOperands {}

/// Marker trait for ops whose regions contain only a single block
pub trait SingleBlock {}

/// Marker trait for ops which can terminate a block
pub trait Terminator {}
