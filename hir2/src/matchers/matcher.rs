use core::ptr::{DynMetadata, Pointee};

use smallvec::SmallVec;

use crate::{
    AttributeValue, Op, OpFoldResult, OpOperand, Operation, OperationRef, UnsafeIntrusiveEntityRef,
    ValueRef,
};

/// [Matcher] is a pattern matching abstraction with support for expressing both matching and
/// capturing semantics.
///
/// This is used to implement low-level pattern matching primitives for the IR for use in:
///
/// * Folding
/// * Canonicalization
/// * Regionalized transformations and analyses
pub trait Matcher<T: ?Sized> {
    /// The value type produced as a result of a successful match
    ///
    /// Use `()` if this matcher does not capture any value, and simply signals whether or not
    /// the pattern was matched.
    type Matched;

    /// Check if `entity` is matched by this matcher, returning `Self::Matched` if successful.
    fn matches(&self, entity: &T) -> Option<Self::Matched>;
}

#[repr(transparent)]
pub struct MatchWith<F>(pub F);
impl<F, T: ?Sized, U> Matcher<T> for MatchWith<F>
where
    F: Fn(&T) -> Option<U>,
{
    type Matched = U;

    #[inline(always)]
    fn matches(&self, entity: &T) -> Option<Self::Matched> {
        (self.0)(entity)
    }
}

/// A match combinator representing the logical AND of two sub-matchers.
///
/// Both patterns must match on the same IR entity, but only the matched value of `B` is returned,
/// i.e. the captured result of `A` is discarded.
///
/// Returns the result of matching `B` if successful, otherwise `None`
pub struct AndMatcher<A, B> {
    a: A,
    b: B,
}

impl<A, B> AndMatcher<A, B> {
    pub const fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<T, A, B> Matcher<T> for AndMatcher<A, B>
where
    A: Matcher<T>,
    B: Matcher<T>,
{
    type Matched = <B as Matcher<T>>::Matched;

    #[inline]
    fn matches(&self, entity: &T) -> Option<Self::Matched> {
        self.a.matches(entity).and_then(|_| self.b.matches(entity))
    }
}

/// A match combinator representing a monadic bind of two patterns.
///
/// In other words, given two patterns `A` and `B`:
///
/// * `A` is matched, and if it fails, the entire match fails.
/// * `B` is then matched against the output of `A`, and if it fails, the entire match fails
/// * Both matches were successful, and the output of `B` is returned as the final result.
pub struct ChainMatcher<A, B> {
    a: A,
    b: B,
}

impl<A, B> ChainMatcher<A, B> {
    pub const fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<T, U, A, B> Matcher<T> for ChainMatcher<A, B>
where
    A: Matcher<T, Matched = U>,
    B: Matcher<U>,
{
    type Matched = <B as Matcher<U>>::Matched;

    #[inline]
    fn matches(&self, entity: &T) -> Option<Self::Matched> {
        self.a.matches(entity).and_then(|matched| self.b.matches(&matched))
    }
}

/// Matches operations which implement some trait `Trait`, capturing the match as a trait object.
///
/// NOTE: `Trait` must be an object-safe trait.
pub struct OpTraitMatcher<Trait: ?Sized> {
    _marker: core::marker::PhantomData<Trait>,
}

impl<Trait> Default for OpTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<Trait> OpTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    /// Create a new [OpTraitMatcher] from the given matcher.
    pub const fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<Trait> Matcher<Operation> for OpTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    type Matched = UnsafeIntrusiveEntityRef<Trait>;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        entity
            .as_trait::<Trait>()
            .map(|op| unsafe { UnsafeIntrusiveEntityRef::from_raw(op) })
    }
}

/// Matches operations which implement some trait `Trait`.
///
/// Returns a type-erased operation ref, not a trait object like [OpTraitMatcher]
pub struct HasTraitMatcher<Trait: ?Sized> {
    _marker: core::marker::PhantomData<Trait>,
}

impl<Trait> Default for HasTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<Trait> HasTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    /// Create a new [HasTraitMatcher] from the given matcher.
    pub const fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<Trait> Matcher<Operation> for HasTraitMatcher<Trait>
where
    Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
{
    type Matched = OperationRef;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        if !entity.implements::<Trait>() {
            return None;
        }
        Some(entity.as_operation_ref())
    }
}

/// Matches any operation with an attribute named `name`, the value of which matches a matcher of
/// type `M`.
pub struct OpAttrMatcher<M> {
    name: &'static str,
    matcher: M,
}

impl<M> OpAttrMatcher<M>
where
    M: Matcher<dyn AttributeValue>,
{
    /// Create a new [OpAttrMatcher] from the given attribute name and matcher.
    pub const fn new(name: &'static str, matcher: M) -> Self {
        Self { name, matcher }
    }
}

impl<M> Matcher<Operation> for OpAttrMatcher<M>
where
    M: Matcher<dyn AttributeValue>,
{
    type Matched = <M as Matcher<dyn AttributeValue>>::Matched;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        entity.get_attribute(self.name).and_then(|value| self.matcher.matches(value))
    }
}

/// Matches any operation with an attribute `name` and concrete value type of `A`.
///
/// Binds the value as its concrete type `A`.
pub type TypedOpAttrMatcher<A> = OpAttrMatcher<TypedAttrMatcher<A>>;

/// Matches and binds any attribute value whose concrete type is `A`.
pub struct TypedAttrMatcher<A>(core::marker::PhantomData<A>);
impl<A: AttributeValue + Clone> Default for TypedAttrMatcher<A> {
    #[inline(always)]
    fn default() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<A: AttributeValue + Clone> Matcher<dyn AttributeValue> for TypedAttrMatcher<A> {
    type Matched = A;

    #[inline]
    fn matches(&self, entity: &dyn AttributeValue) -> Option<Self::Matched> {
        entity.downcast_ref::<A>().cloned()
    }
}

/// A matcher for operations that always succeeds, binding the operation reference in the process.
struct AnyOpMatcher;
impl Matcher<Operation> for AnyOpMatcher {
    type Matched = OperationRef;

    #[inline(always)]
    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        Some(entity.as_operation_ref())
    }
}

/// A matcher for operations whose concrete type is `T`, binding the op with a strongly-typed
/// reference.
struct OneOpMatcher<T>(core::marker::PhantomData<T>);
impl<T: Op> OneOpMatcher<T> {
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<T: Op> Matcher<Operation> for OneOpMatcher<T> {
    type Matched = UnsafeIntrusiveEntityRef<T>;

    #[inline(always)]
    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        entity
            .downcast_ref::<T>()
            .map(|op| unsafe { UnsafeIntrusiveEntityRef::from_raw(op) })
    }
}

/// A matcher for values that always succeeds, binding the value reference in the process.
struct AnyValueMatcher;
impl Matcher<ValueRef> for AnyValueMatcher {
    type Matched = ValueRef;

    #[inline(always)]
    fn matches(&self, entity: &ValueRef) -> Option<Self::Matched> {
        Some(entity.clone())
    }
}

/// A matcher that only succeeds if it matches exactly the provided value.
struct ExactValueMatcher(ValueRef);
impl Matcher<ValueRef> for ExactValueMatcher {
    type Matched = ValueRef;

    #[inline(always)]
    fn matches(&self, entity: &ValueRef) -> Option<Self::Matched> {
        if ValueRef::ptr_eq(&self.0, entity) {
            Some(entity.clone())
        } else {
            None
        }
    }
}

/// A matcher for operations that implement [crate::traits::ConstantLike]
type ConstantOpMatcher = HasTraitMatcher<dyn crate::traits::ConstantLike>;

/// Like [ConstantOpMatcher], this matcher matches constant operations, but rather than binding
/// the operation itself, it binds the constant value produced by the operation.
#[derive(Default)]
struct ConstantOpBinder;
impl Matcher<Operation> for ConstantOpBinder {
    type Matched = Box<dyn AttributeValue>;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        use crate::traits::Foldable;

        if !entity.implements::<dyn crate::traits::ConstantLike>() {
            return None;
        }

        let mut out = SmallVec::default();
        entity.fold(&mut out).expect("expected constant-like op to be foldable");
        let Some(OpFoldResult::Attribute(value)) = out.pop() else {
            return None;
        };

        Some(value)
    }
}

/// An extension of [ConstantOpBinder] which only matches constant values of type `T`
struct TypedConstantOpBinder<T>(core::marker::PhantomData<T>);
impl<T: AttributeValue + Clone> TypedConstantOpBinder<T> {
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<T: AttributeValue + Clone> Matcher<Operation> for TypedConstantOpBinder<T> {
    type Matched = T;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        ConstantOpBinder.matches(entity).and_then(|value| {
            if !value.is::<T>() {
                None
            } else {
                Some(unsafe {
                    let raw = Box::into_raw(value);
                    *Box::from_raw(raw as *mut T)
                })
            }
        })
    }
}

/// Matches operations which implement [crate::traits::UnaryOp] and binds the operand.
#[derive(Default)]
struct UnaryOpBinder;
impl Matcher<Operation> for UnaryOpBinder {
    type Matched = OpOperand;

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        if !entity.implements::<dyn crate::traits::UnaryOp>() {
            return None;
        }

        Some(entity.operands()[0].borrow().as_operand_ref())
    }
}

/// Matches operations which implement [crate::traits::BinaryOp] and binds both operands.
#[derive(Default)]
struct BinaryOpBinder;
impl Matcher<Operation> for BinaryOpBinder {
    type Matched = [OpOperand; 2];

    fn matches(&self, entity: &Operation) -> Option<Self::Matched> {
        if !entity.implements::<dyn crate::traits::BinaryOp>() {
            return None;
        }

        let operands = entity.operands();
        let lhs = operands[0].borrow().as_operand_ref();
        let rhs = operands[1].borrow().as_operand_ref();

        Some([lhs, rhs])
    }
}

/// Converts the output of [UnaryOpBinder] to an OpFoldResult, by checking if the operand definition
/// is a constant-like op, and either binding the constant value, or the SSA value used as the
/// operand.
///
/// This can be used to set up for folding.
struct FoldResultBinder;
impl Matcher<OpOperand> for FoldResultBinder {
    type Matched = OpFoldResult;

    fn matches(&self, operand: &OpOperand) -> Option<Self::Matched> {
        let operand = operand.borrow();
        let maybe_constant = operand
            .value()
            .get_defining_op()
            .and_then(|defining_op| constant().matches(&defining_op.borrow()));
        if let Some(const_operand) = maybe_constant {
            Some(OpFoldResult::Attribute(const_operand))
        } else {
            Some(OpFoldResult::Value(operand.as_value_ref()))
        }
    }
}

/// Converts the output of [BinaryOpBinder] to a pair of OpFoldResults, by checking if the operand
/// definitions are constant, and either binding the constant values, or the SSA values used by each
/// operand.
///
/// This can be used to set up for folding.
struct BinaryFoldResultBinder;
impl Matcher<[OpOperand; 2]> for BinaryFoldResultBinder {
    type Matched = [OpFoldResult; 2];

    fn matches(&self, operands: &[OpOperand; 2]) -> Option<Self::Matched> {
        let binder = FoldResultBinder;

        let lhs = binder.matches(&operands[0]).unwrap();
        let rhs = binder.matches(&operands[1]).unwrap();

        Some([lhs, rhs])
    }
}

/// Matches the operand of a unary op to determine if it is a candidate for folding.
///
/// A successful match binds the constant value of the operand for use by the [Foldable] impl.
struct FoldableOperandBinder;
impl Matcher<OpOperand> for FoldableOperandBinder {
    type Matched = Box<dyn AttributeValue>;

    fn matches(&self, operand: &OpOperand) -> Option<Self::Matched> {
        let operand = operand.borrow();
        let defining_op = operand.value().get_defining_op()?;
        constant().matches(&defining_op.borrow())
    }
}

struct TypedFoldableOperandBinder<T>(core::marker::PhantomData<T>);
impl<T: AttributeValue + Clone> Default for TypedFoldableOperandBinder<T> {
    fn default() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<T: AttributeValue + Clone> Matcher<OpOperand> for TypedFoldableOperandBinder<T> {
    type Matched = Box<T>;

    fn matches(&self, operand: &OpOperand) -> Option<Self::Matched> {
        FoldableOperandBinder
            .matches(operand)
            .and_then(|value| value.downcast::<T>().ok())
    }
}

/// Matches the operands of a binary op to determine if it is a candidate for folding.
///
/// A successful match binds the constant value of the operands for use by the [Foldable] impl.
///
/// NOTE: Both operands must be constant for this to match. Use [BinaryFoldResultBinder] if you
/// wish to let the [Foldable] impl decide what to do in the presence of mixed constant and non-
/// constant operands.
struct FoldableBinaryOpBinder;
impl Matcher<[OpOperand; 2]> for FoldableBinaryOpBinder {
    type Matched = [Box<dyn AttributeValue>; 2];

    fn matches(&self, operands: &[OpOperand; 2]) -> Option<Self::Matched> {
        let binder = FoldableOperandBinder;
        let lhs = binder.matches(&operands[0])?;
        let rhs = binder.matches(&operands[1])?;

        Some([lhs, rhs])
    }
}

// Match Combinators

/// Matches both `a` and `b`, or fails
pub const fn match_both<A, B>(
    a: A,
    b: B,
) -> impl Matcher<Operation, Matched = <B as Matcher<Operation>>::Matched>
where
    A: Matcher<Operation>,
    B: Matcher<Operation>,
{
    AndMatcher::new(a, b)
}

/// Matches `a` and if successful, matches `b` against the output of `a`, or fails.
pub const fn match_chain<T, A, B>(
    a: A,
    b: B,
) -> impl Matcher<Operation, Matched = <B as Matcher<T>>::Matched>
where
    A: Matcher<Operation, Matched = T>,
    B: Matcher<T>,
{
    ChainMatcher::new(a, b)
}

// Operation Matchers

/// Matches any operation, i.e. it always matches
///
/// Returns a type-erased operation reference
pub const fn match_any() -> impl Matcher<Operation, Matched = OperationRef> {
    AnyOpMatcher
}

/// Matches any operation whose concrete type is `T`
///
/// Returns a strongly-typed op reference
pub const fn match_op<T: Op>() -> impl Matcher<Operation, Matched = UnsafeIntrusiveEntityRef<T>> {
    OneOpMatcher::<T>::new()
}

/// Matches any operation that implements [crate::traits::ConstantLike].
///
/// These operations return a single result, and must be pure (no side effects)
pub const fn constant_like() -> impl Matcher<Operation, Matched = OperationRef> {
    ConstantOpMatcher::new()
}

// Constant Value Binders

/// Matches any operation that implements [crate::traits::ConstantLike], and binds the constant
/// value as the result of the match.
pub const fn constant() -> impl Matcher<Operation, Matched = Box<dyn AttributeValue>> {
    ConstantOpBinder
}

/// Like [constant], but only matches if the constant value has the concrete type `T`.
///
/// Typically, constant values will be [crate::Immediate], but any attribute value can be matched.
pub const fn constant_of<T: AttributeValue + Clone>() -> impl Matcher<Operation, Matched = T> {
    TypedConstantOpBinder::new()
}

// Value Binders

/// Matches any unary operation (i.e. implements [crate::traits::UnaryOp]), and binds its operand.
pub const fn unary() -> impl Matcher<Operation, Matched = OpOperand> {
    UnaryOpBinder
}

/// Matches any unary operation (i.e. implements [crate::traits::UnaryOp]), and binds its operand
/// as an [OpFoldResult].
///
/// This is done by examining the defining op of the operand to determine if it is a constant, and
/// if so, it binds the constant value, rather than the SSA value.
///
/// This can be used to setup for folding.
pub const fn unary_fold_result() -> impl Matcher<Operation, Matched = OpFoldResult> {
    match_chain(UnaryOpBinder, FoldResultBinder)
}

/// Matches any unary operation (i.e. implements [crate::traits::UnaryOp]) whose operand is a
/// materialized constant, and thus a prime candidate for folding.
///
/// The constant value is bound by this matcher, so it can be used immediately for folding.
pub const fn unary_foldable() -> impl Matcher<Operation, Matched = Box<dyn AttributeValue>> {
    match_chain(UnaryOpBinder, FoldableOperandBinder)
}

/// Matches any binary operation (i.e. implements [crate::traits::BinaryOp]), and binds its operands.
pub const fn binary() -> impl Matcher<Operation, Matched = [OpOperand; 2]> {
    BinaryOpBinder
}

/// Matches any binary operation (i.e. implements [crate::traits::BinaryOp]), and binds its operands
/// as [OpFoldResult]s.
///
/// This is done by examining the defining op of the operands to determine if they are constant, and
/// if so, binds the constant value, rather than the SSA value.
///
/// This can be used to setup for folding.
pub const fn binary_fold_results() -> impl Matcher<Operation, Matched = [OpFoldResult; 2]> {
    match_chain(BinaryOpBinder, BinaryFoldResultBinder)
}

/// Matches any binary operation (i.e. implements [crate::traits::BinaryOp]) whose operands are
/// both materialized constants, and thus a prime candidate for folding.
///
/// The constant values are bound by this matcher, so they can be used immediately for folding.
pub const fn binary_foldable() -> impl Matcher<Operation, Matched = [Box<dyn AttributeValue>; 2]> {
    match_chain(BinaryOpBinder, FoldableBinaryOpBinder)
}

// Value Matchers

/// Matches any value, i.e. it always matches
pub const fn match_any_value() -> impl Matcher<ValueRef, Matched = ValueRef> {
    AnyValueMatcher
}

/// Matches any instance of `value`, i.e. it requires an exact match
pub const fn match_value(value: ValueRef) -> impl Matcher<ValueRef, Matched = ValueRef> {
    ExactValueMatcher(value)
}

pub const fn foldable_operand() -> impl Matcher<OpOperand, Matched = Box<dyn AttributeValue>> {
    FoldableOperandBinder
}

pub const fn foldable_operand_of<T>() -> impl Matcher<OpOperand, Matched = Box<T>>
where
    T: AttributeValue + Clone,
{
    TypedFoldableOperandBinder(core::marker::PhantomData)
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use super::*;
    use crate::{
        dialects::hir::{InstBuilder, *},
        *,
    };

    #[test]
    fn matcher_match_any_value() {
        let context = Rc::new(Context::default());

        let (lhs, rhs, sum) = setup(context.clone());

        // All three values should `match_any_value`
        for value in [&lhs, &rhs, &sum] {
            assert_eq!(match_any_value().matches(value).as_ref(), Some(value));
        }
    }

    #[test]
    fn matcher_match_value() {
        let context = Rc::new(Context::default());

        let (lhs, rhs, sum) = setup(context.clone());

        // All three values should match themselves via `match_value`
        for value in [&lhs, &rhs, &sum] {
            assert_eq!(match_value(value.clone()).matches(value).as_ref(), Some(value));
        }
    }

    #[test]
    fn matcher_match_any() {
        let context = Rc::new(Context::default());

        let (lhs, _rhs, sum) = setup(context.clone());

        // We should be able to match `lhs` and `sum` ops using `match_any`
        let lhs_op = lhs.borrow().get_defining_op().unwrap();
        let sum_op = sum.borrow().get_defining_op().unwrap();

        for op in [&lhs_op, &sum_op] {
            assert_eq!(match_any().matches(&op.borrow()).as_ref(), Some(op));
        }
    }

    #[test]
    fn matcher_match_op() {
        let context = Rc::new(Context::default());

        let (lhs, rhs, sum) = setup(context.clone());
        let lhs_op = lhs.borrow().get_defining_op().unwrap();
        let sum_op = sum.borrow().get_defining_op().unwrap();
        assert!(rhs.borrow().get_defining_op().is_none());

        // Both `lhs` and `sum` ops should be matched as their respective operation types, and not
        // as a different operation type
        assert!(match_op::<Constant>().matches(&lhs_op.borrow()).is_some());
        assert!(match_op::<Constant>().matches(&sum_op.borrow()).is_none());
        assert!(match_op::<Add>().matches(&lhs_op.borrow()).is_none());
        assert!(match_op::<Add>().matches(&sum_op.borrow()).is_some());
    }

    #[test]
    fn matcher_match_both() {
        let context = Rc::new(Context::default());

        let (lhs, _rhs, _sum) = setup(context.clone());
        let lhs_op = lhs.borrow().get_defining_op().unwrap();

        // Ensure if the first matcher fails, then the whole match fails
        assert!(match_both(match_op::<Add>(), constant_of::<Immediate>())
            .matches(&lhs_op.borrow())
            .is_none());
        // Ensure if the second matcher fails, then the whole match fails
        assert!(match_both(constant_like(), constant_of::<bool>())
            .matches(&lhs_op.borrow())
            .is_none());
        // Ensure that if both matchers would succeed, then the whole match succeeds
        assert!(match_both(constant_like(), constant_of::<Immediate>())
            .matches(&lhs_op.borrow())
            .is_some());
    }

    #[test]
    fn matcher_match_chain() {
        let context = Rc::new(Context::default());

        let (_, rhs, sum) = setup(context.clone());
        let sum_op = sum.borrow().get_defining_op().unwrap();

        let [lhs_fr, rhs_fr] = binary_fold_results()
            .matches(&sum_op.borrow())
            .expect("expected to bind both operands of 'add'");
        assert_eq!(lhs_fr, OpFoldResult::Attribute(Box::new(Immediate::U32(1))));
        assert_eq!(rhs_fr, OpFoldResult::Value(rhs));
    }

    #[test]
    fn matcher_constant_like() {
        let context = Rc::new(Context::default());

        let (lhs, _rhs, sum) = setup(context.clone());
        let lhs_op = lhs.borrow().get_defining_op().unwrap();
        let sum_op = sum.borrow().get_defining_op().unwrap();

        // Only `lhs` should be matched by `constant_like`
        assert!(constant_like().matches(&lhs_op.borrow()).is_some());
        assert!(constant_like().matches(&sum_op.borrow()).is_none());
    }

    #[test]
    fn matcher_constant() {
        let context = Rc::new(Context::default());

        let (lhs, _rhs, sum) = setup(context.clone());
        let lhs_op = lhs.borrow().get_defining_op().unwrap();
        let sum_op = sum.borrow().get_defining_op().unwrap();

        // Only `lhs` should produce a matching constant value
        assert!(constant().matches(&lhs_op.borrow()).is_some());
        assert!(constant().matches(&sum_op.borrow()).is_none());
    }

    #[test]
    fn matcher_constant_of() {
        let context = Rc::new(Context::default());

        let (lhs, _rhs, sum) = setup(context.clone());
        let lhs_op = lhs.borrow().get_defining_op().unwrap();
        let sum_op = sum.borrow().get_defining_op().unwrap();

        // `lhs` should produce a matching constant value of the correct type and value
        assert_eq!(constant_of::<Immediate>().matches(&lhs_op.borrow()), Some(Immediate::U32(1)));
        assert!(constant_of::<Immediate>().matches(&sum_op.borrow()).is_none());
    }

    fn setup(context: Rc<Context>) -> (ValueRef, ValueRef, ValueRef) {
        let mut builder = OpBuilder::new(Rc::clone(&context));

        let mut function = {
            let builder = builder.create::<Function, (_, _)>(SourceSpan::default());
            let id = Ident::new("test".into(), SourceSpan::default());
            let signature = Signature::new([AbiParam::new(Type::U32)], [AbiParam::new(Type::U32)]);
            builder(id, signature).unwrap()
        };

        // Define function body
        let mut func = function.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut func);
        let lhs = builder.ins().u32(1, SourceSpan::default()).unwrap();
        let block = builder.current_block();
        let rhs = block.borrow().arguments()[0].clone().upcast();
        let sum = builder.ins().add(lhs.clone(), rhs.clone(), SourceSpan::default()).unwrap();
        builder.ins().ret(Some(sum.clone()), SourceSpan::default()).unwrap();

        (lhs, rhs, sum)
    }
}
