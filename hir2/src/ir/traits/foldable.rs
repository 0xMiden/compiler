use smallvec::SmallVec;

use crate::{AttributeValue, ValueRef};

/// Represents the outcome of an attempt to fold an operation.
#[must_use]
pub enum FoldResult<T = ()> {
    /// The operation was folded and erased, and the given fold results were returned
    Ok(T),
    /// The operation was modified in-place, but not erased.
    InPlace,
    /// The operation could not be folded
    Failed,
}
impl<T> FoldResult<T> {
    /// Returns true if folding was successful
    #[inline]
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_) | Self::InPlace)
    }

    /// Returns true if folding was unsuccessful
    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Convert this result to an `Option` representing a successful outcome, where `None` indicates
    /// an in-place fold, and `Some(T)` indicates that the operation was folded away.
    ///
    /// Panics with the given message if the fold attempt failed.
    #[inline]
    #[track_caller]
    pub fn expect(self, message: &'static str) -> Option<T> {
        match self {
            Self::Ok(out) => Some(out),
            Self::InPlace => None,
            Self::Failed => unwrap_failed_fold_result(message),
        }
    }
}

#[cold]
#[track_caller]
#[inline(never)]
fn unwrap_failed_fold_result(message: &'static str) -> ! {
    panic!("tried to unwrap failed fold result as successful: {message}")
}

/// Represents a single result value of a folded operation.
#[derive(Debug)]
pub enum OpFoldResult {
    /// The value is constant
    Attribute(Box<dyn AttributeValue>),
    /// The value is a non-constant SSA value
    Value(ValueRef),
}
impl OpFoldResult {
    #[inline]
    pub fn is_constant(&self) -> bool {
        matches!(self, Self::Attribute(_))
    }
}
impl Eq for OpFoldResult {}
impl PartialEq for OpFoldResult {
    fn eq(&self, other: &Self) -> bool {
        use core::hash::{Hash, Hasher};

        match (self, other) {
            (Self::Attribute(lhs), Self::Attribute(rhs)) => {
                if lhs.as_any().type_id() != rhs.as_any().type_id() {
                    return false;
                }
                let lhs_hash = {
                    let mut hasher = rustc_hash::FxHasher::default();
                    lhs.hash(&mut hasher);
                    hasher.finish()
                };
                let rhs_hash = {
                    let mut hasher = rustc_hash::FxHasher::default();
                    rhs.hash(&mut hasher);
                    hasher.finish()
                };
                lhs_hash == rhs_hash
            }
            (Self::Value(lhs), Self::Value(rhs)) => ValueRef::ptr_eq(lhs, rhs),
            _ => false,
        }
    }
}
impl core::fmt::Display for OpFoldResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Attribute(ref attr) => attr.pretty_print(f),
            Self::Value(ref value) => write!(f, "{}", value.borrow().id()),
        }
    }
}

/// An operation that can be constant-folded must implement the folding logic via this trait.
///
/// NOTE: Any `ConstantLike` operation must implement this trait as a no-op, i.e. returning the
/// value of the constant directly, as this is used by the pattern matching infrastructure to
/// extract the value of constant operations without knowing anything about the specific op.
pub trait Foldable {
    /// Attempt to fold this operation using its current operand values.
    ///
    /// If folding was successful and the operation should be erased, `results` will contain the
    /// folded results. See [FoldResult] for more details on what the various outcomes of folding
    /// are.
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult;

    /// Attempt to fold this operation with the specified operand values.
    ///
    /// The elements in `operands` will correspond 1:1 with the operands of the operation, but will
    /// be `None` if the value is non-constant.
    ///
    /// If folding was successful and the operation should be erased, `results` will contain the
    /// folded results. See [FoldResult] for more details on what the various outcomes of folding
    /// are.
    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult;
}
