use super::{Context, Report};

/// The `OpVerifier` trait is expected to be implemented by all [Op] impls as a prequisite.
///
/// The actual implementation is typically generated as part of deriving [Op].
pub trait OpVerifier {
    fn verify(&self, context: &Context) -> Result<(), Report>;
}

/// The `Verify` trait represents verification logic associated with implementations of some trait.
///
/// This is specifically used for automatically deriving verification checks for [Op] impls that
/// implement traits that imply constraints on the representation or behavior of that op.
///
/// For example, if some [Op] derives an op trait like `SingleBlock`, this information is recorded
/// in the underlying [Operation] metadata, so that we can recover a trait object reference for the
/// trait when needed. However, just deriving the trait is not sufficient to guarantee that the op
/// actually adheres to the implicit constraints and behavior of that trait. For example,
/// `SingleBlock` implies that the implementing op contains only regions that consist of a single
/// [Block]. This cannot be checked statically. The first step to addressing this though, is to
/// reify the implicit validation rules as explicit checks - hence this trait.
///
/// So we've established that some op traits, such as `SingleBlock` mentioned above, have implicit
/// validation rules, and we can implement [Verify] to make the implicit validation rules of such
/// traits explicit - but how do we ensure that when an op derives an op trait, that the [Verify]
/// impl is also derived, _and_ that it is called when the op is verified?
///
/// The answer lies in the use of some tricky type-level code to accomplish the following goals:
///
/// * Do not emit useless checks for op traits that have no verification rules
/// * Do not require storing data in each instance of an [Op] just to verify a trait
/// * Do not require emitting a bunch of redundant type checks for information we know statically
/// * Be able to automatically derive all of the verification machinery along with the op traits
///
/// The way this works is as follows:
///
/// * We `impl<T> Verify<dyn Trait> for T where T: Op` for every trait `Trait` with validation rules.
/// * A blanket impl of [HasVerifier] exists for all `T: Verify<Trait>`. This is a marker trait used
///   in conjunction with specialization. See the trait docs for more details on its purpose.
/// * The [Verifier] trait provides a default vacuous impl for all `Trait` and `T` pairs. However,
///   we also provided a specialized [Verifier] impl for all `T: Verify<Trait>` using the
///   `HasVerifier` marker. The specialized impl applies the underlying `Verify` impl.
/// * When deriving the op traits for an `Op` impl, we generate a hidden type that encodes all of
///   the op traits implemented by the op. We then generate an `OpVerifier` impl for the op, which
///   uses the hidden type we generated to reify the `Verifier` impl for each trait. The
///   `OpVerifier` implementation uses const eval to strip out all of the vacuous verifier impls,
///   leaving behind just the "real" verification rules specific to the traits implemented by that
///   op.
/// * The `OpVerifier` impl is object-safe, and is in fact a required super-trait of `Op` to ensure
///   that verification is part of defining an `Op`, but also to ensure that `verify` is a method
///   of `Op`, and that we can cast an `Operation` to `&dyn OpVerifier` and call `verify` on that.
///
/// As a result of all this, we end up with highly-specialized verifiers for each op, with no
/// dynamic dispatch, and automatically maintained as part of the `Op` definition. When a new
/// op trait is derived, the verifier for the op is automatically updated to verify the new trait.
pub trait Verify<Trait: ?Sized> {
    /// In cases where verification may be disabled via runtime configuration, or based on
    /// dynamic properties of the type, this method can be overridden and used to signal to
    /// the verification driver that verification should be skipped on this item.
    #[inline(always)]
    #[allow(unused_variables)]
    fn should_verify(&self, context: &Context) -> bool {
        true
    }
    /// Apply this verifier, but only if [Verify::should_verify] returns true.
    #[inline]
    fn maybe_verify(&self, context: &Context) -> Result<(), Report> {
        if self.should_verify(context) {
            self.verify(context)
        } else {
            Ok(())
        }
    }
    /// Apply this verifier to the current item.
    fn verify(&self, context: &Context) -> Result<(), Report>;
}

/// A marker trait used for verifier specialization.
///
/// # Safety
///
/// In order for the `#[rustc_unsafe_specialization_marker]` attribute to be used safely and
/// correctly, the following rules must hold:
///
/// * No associated items
/// * No impls with lifetime constraints, as specialization will ignore them
///
/// For our use case, which is specializing verification for a given type and trait combination,
/// by optimizing out verification-related code for type combinations which have no verifier, these
/// are easy rules to uphold.
///
/// However, we must ensure that we continue to uphold these rules moving forward.
#[rustc_unsafe_specialization_marker]
unsafe trait HasVerifier<Trait: ?Sized>: Verify<Trait> {}

// While at first glance, it appears we would be using this to specialize on the fact that a type
// _has_ a verifier, which is strictly-speaking true, the actual goal we're aiming to acheive is
// to be able to identify the _absence_ of a verifier, so that we can eliminate the boilerplate for
// verifying that trait. See `Verifier` for more information.
unsafe impl<T, Trait: ?Sized> HasVerifier<Trait> for T where T: Verify<Trait> {}

/// The `Verifier` trait is used to derive a verifier for a given trait and concrete type.
///
/// It does this by providing a default implementation for all combinations of `Trait` and `T`,
/// which always succeeds, and then specializing that implementation for `T: HasVerifier<Trait>`.
///
/// This has the effect of making all traits "verifiable", but only actually doing any verification
/// for types which implement `Verify<Trait>`.
///
/// We go a step further and actually set things up so that `rustc` can eliminate all of the dead
/// code when verification is vacuous. This is done by using const eval in the hidden type generated
/// for an [Op] impls [OpVerifier] implementation, which will wrap verification in a const-evaluated
/// check of the `VACUOUS` associated const. It can also be used directly, but the general idea
/// behind all of this is that we don't need to directly touch any of this, it's all generated.
///
/// NOTE: Because this trait provides a default blanket impl for all `T`, you should avoid bringing
/// it into scope unless absolutely needed. It is virtually always preferred to explicitly invoke
/// this trait using turbofish syntax, so as to avoid conflict with the [Verify] trait, and to
/// avoid polluting the namespace for all types in scope.
pub trait Verifier<Trait: ?Sized> {
    /// An implementation of `Verifier` sets this flag to true when its implementation is vacuous,
    /// i.e. it always succeeds and is not dependent on runtime context.
    ///
    /// The default implementation of this trait sets this to `true`, since without a verifier for
    /// the type, verification always succeeds. However, we can specialize on the presence of
    /// a verifier and set this to `false`, which will result in all of the verification logic
    /// being applied.
    ///
    /// ## Example Usage
    ///
    /// Shown below is an example of how one can use const eval to eliminate dead code branches
    /// in verifier selection, so that the resulting implementation is specialized and able to
    /// have more optimizations applied as a result.
    ///
    /// ```rust,ignore
    /// #[inline(always)]
    /// fn noop(&T, &Context) -> Result<(), Report> { Ok(()) }
    /// let verify_fn = const {
    ///     if <T as Verifier<Trait>>::VACUOUS {
    ///        noop
    ///     } else {
    ///        <T as Verifier<Trait>>::maybe_verify
    ///     }
    /// };
    /// verify_fn(op, context)
    /// ```
    const VACUOUS: bool;

    /// Checks if this verifier is applicable for the current item
    fn should_verify(&self, context: &Context) -> bool;
    /// Applies the verifier for this item, if [Verifier::should_verify] returns `true`
    fn maybe_verify(&self, context: &Context) -> Result<(), Report>;
    /// Applies the verifier for this item
    fn verify(&self, context: &Context) -> Result<(), Report>;
}

/// The default blanket impl for all types and traits
impl<T, Trait: ?Sized> Verifier<Trait> for T {
    default const VACUOUS: bool = true;

    #[inline(always)]
    default fn should_verify(&self, _context: &Context) -> bool {
        false
    }

    #[inline(always)]
    default fn maybe_verify(&self, _context: &Context) -> Result<(), Report> {
        Ok(())
    }

    #[inline(always)]
    default fn verify(&self, _context: &Context) -> Result<(), Report> {
        Ok(())
    }
}

/// THe specialized impl for types which implement `Verify<Trait>`
impl<T, Trait: ?Sized> Verifier<Trait> for T
where
    T: HasVerifier<Trait>,
{
    const VACUOUS: bool = false;

    #[inline]
    fn should_verify(&self, context: &Context) -> bool {
        <T as Verify<Trait>>::should_verify(self, context)
    }

    #[inline(always)]
    fn maybe_verify(&self, context: &Context) -> Result<(), Report> {
        <T as Verify<Trait>>::maybe_verify(self, context)
    }

    #[inline]
    fn verify(&self, context: &Context) -> Result<(), Report> {
        <T as Verify<Trait>>::verify(self, context)
    }
}

#[cfg(test)]
mod tests {
    use core::hint::black_box;

    use super::*;
    use crate::{traits::SingleBlock, Operation};

    struct Vacuous;

    /// In this test, we're validating that a type that trivially verifies specializes as vacuous,
    /// and that a type we know has a "real" verifier, specializes as _not_ vacuous
    #[test]
    fn verifier_specialization_concrete() {
        assert!(black_box(<Vacuous as Verifier<dyn SingleBlock>>::VACUOUS));
        assert!(black_box(!<Operation as Verifier<dyn SingleBlock>>::VACUOUS));
    }
}
