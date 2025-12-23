use alloc::rc::Rc;

use crate::{Context, Op, patterns::RewritePatternSet};

/// This trait represents an [Op] that has a canonical/normal form.
///
/// Canonicalization patterns are applied via a single canonicalization pass, which iteratively
/// applies canonicalization patterns of all operations until either fixpoint is reached, or some
/// maximum number of iterations is reached. As a result, canonicalization is performed on a best-
/// effort basis, and the IR overall is not guaranteed to be in perfect canonical form.
///
/// Canonicalization is intended to simplify subsequent analysis and optimization, by allowing them
/// to assume that all operations are in canonical form, and thus not needing to handle all the
/// many different variations of the same op that might occur in practice. This reduces the amount
/// of redundant work that must be done, and improves the performance of compilation overall. It
/// is important to stress though, that canonicalizations must not be required for correctness -
/// if an operation is not in canonical form, compilation should still produce correct output,
/// albeit less optimal output.
///
///
/// Operations which have a canonical/normal form must be able to define what it means for two
/// instances of the op to be equivalent. This is because the mathematical properties of
/// canonicalization are defined in terms of equivalence relations over all forms of an op in the
/// same _equivalence class_. An intuitive way of thinking about this in terms of operations, are
/// that two instances of an operation are in the same equivalence class if they are unique from
/// each other, but would produce identical results, i.e. they represent the same computation in
/// different forms.
///
/// For operations which meet the above requirement, and do provide a set of canonicalization
/// patterns, those patterns must uphold the following properties:
///
/// 1. _Idempotence_. After applying canonicalization pattern(s) to the op, applying again to the
///    resulting op will have no effect, i.e. `canonicalize(op) = canonicalize(canonicalize(op))`.
/// 2. _Decisiveness_. Applying the canonicalization patterns to any two instances of the operation
///    in the same equivalence class, produce an identical result. The result must be the canonical
///    form, must be unique for the equivalence class, and must be itself a member of the
///    equivalence class (i.e. the output is equivalent to the inputs). In other words, given a
///    equivalence class `{a, b, c}`, where `c` is the canonical representation for that equivalence
///    class, then: `canon(a) = canon(b) = c`.
/// 3. _Convergence_. Each canonicalization rewrite must either leave the IR unchanged, or rewrite
///    it such that the output is strictly _more_ canonical than the input. A rewrite which makes
///    the input less canonical "temporarily" so that another rewrite will apply, can easily result
///    in unstable or cyclic rewrites, causing canonicalization to never reach fixpoint.
///
/// Additionally, there are some general rules that should be followed:
///
/// * Canonicalization rewrites should be simple, and focused purely on reaching canonical form.
///   They should not be used to do unrelated optimizations/rewrites that do not pertain to the
///   task of canonicalization.
///
/// * Canonicalization rewrites should never rewrite other operations with canonical forms. However,
///   it is fine to add or remove operations in order to reach canonical form. For example, if we
///   are canonicalizing an `if` expression, we might want to simplify the condition expression such
///   that a condition of the form `!x` is rewritten as just `x`, swapping the then/else blocks so
///   that the resulting `if` is equivalent to the original, but without the unnecessary inversion
///   of the condition. In this case, we removed an operation that became dead as the result of
///   canonicalizing the `if`. It would not, however, have been a good idea for us to try and do
///   more complex analysis/rewrite of the `x` expression itself - instead, the canonicalization
///   should assume that `x` is already in its canonical form.
///
/// * It is best to canonicalize towards fewer uses of a value when operands are duplicated, as
///   some rewrite patterns only match when a value has a single use. For example, canonicalizing
///   `x + x` as `x * 2`, since this reduces the number of uses of `x` by one.
pub trait Canonicalizable {
    /// Populate `rewrites` with the set of rewrite patterns that should be applied to canonicalize
    /// this operation.
    ///
    /// NOTE: This should not be used to register _all_ possible rewrites for this operation, only
    /// those used for canonicalization.
    ///
    /// An operation that has no canonicalization patterns may simply return without adding any
    /// patterns to the set. This is the default behavior implemented for all [Op] impls.
    fn get_canonicalization_patterns(rewrites: &mut RewritePatternSet, context: Rc<Context>);
}

impl<T: Op> Canonicalizable for T {
    default fn get_canonicalization_patterns(
        _rewrites: &mut RewritePatternSet,
        _context: Rc<Context>,
    ) {
    }
}
