use core::fmt;

use midenc_session::diagnostics::Severity;

use crate::{derive, Context, Operation, Report, Spanned};

/// OpInterface to compute the return type(s) of an operation.
pub trait InferTypeOpInterface {
    /// Run type inference for this op's results, using the current state, and apply any changes.
    ///
    /// Returns an error if unable to infer types, or if some type constraint is violated.
    fn infer_types(&mut self) -> Result<(), Report>;
}

derive! {
    /// Op expects all operands to be of the same type
    pub trait SameTypeOperands {}

    verify {
        fn operands_are_the_same_type(op: &Operation, context: &Context) -> Result<(), Report> {
            let mut operands = op.operands().iter();
            if let Some(first_operand) = operands.next() {
                let (expected_ty, set_by) = {
                    let operand = first_operand.borrow();
                    let value = operand.value();
                    (value.ty().clone(), value.span())
                };

                for operand in operands {
                    let operand = operand.borrow();
                    let value = operand.value();
                    let value_ty = value.ty();
                    if value_ty != &expected_ty {
                        return Err(context
                            .session
                            .diagnostics
                            .diagnostic(Severity::Error)
                            .with_message("invalid operation")
                            .with_primary_label(
                                op.span(),
                                "this operation expects all operands to be of the same type"
                            )
                            .with_secondary_label(
                                set_by,
                                "inferred the expected type from this value"
                            )
                            .with_secondary_label(
                                value.span(),
                                "which differs from this value"
                            )
                            .with_help(format!("expected '{expected_ty}', got '{value_ty}'"))
                            .into_report()
                        );
                    }
                }
            }

            Ok(())
        }
    }
}

derive! {
    /// Op expects all operands and results to be of the same type
    ///
    /// TODO(pauls): Implement verification for this. Ideally we could require `SameTypeOperands`
    /// as a super trait, check the operands using its implementation, and then check the results
    /// separately
    pub trait SameOperandsAndResultType {}
}

/// An operation trait that indicates it expects a variable number of operands, matching the given
/// type constraint, i.e. zero or more of the base type.
pub trait Variadic<T: TypeConstraint> {}

impl<T, C: TypeConstraint> crate::Verify<dyn Variadic<C>> for T
where
    T: crate::Op + Variadic<C>,
{
    fn verify(&self, context: &Context) -> Result<(), Report> {
        self.as_operation().verify(context)
    }
}
impl<C: TypeConstraint> crate::Verify<dyn Variadic<C>> for Operation {
    fn should_verify(&self, _context: &Context) -> bool {
        self.implements::<dyn Variadic<C>>()
    }

    fn verify(&self, context: &Context) -> Result<(), Report> {
        for operand in self.operands().iter() {
            let operand = operand.borrow();
            let value = operand.value();
            let ty = value.ty();
            if <C as TypeConstraint>::matches(ty) {
                continue;
            } else {
                let description = <C as TypeConstraint>::description();
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand")
                    .with_primary_label(
                        value.span(),
                        format!("expected operand type to be {description}, but got {ty}"),
                    )
                    .into_report());
            }
        }

        Ok(())
    }
}

pub trait TypeConstraint: 'static {
    fn description() -> impl fmt::Display;
    fn matches(ty: &crate::Type) -> bool;
    fn check(ty: &crate::Type) -> Result<(), Report> {
        if Self::matches(ty) {
            Ok(())
        } else {
            let expected = Self::description();
            Err(Report::msg(format!("expected {expected}, got '{ty}'")))
        }
    }
}

/// A type that can be constructed as a [crate::Type]
pub trait BuildableTypeConstraint: TypeConstraint {
    fn build() -> crate::Type;
}

macro_rules! type_constraint {
    ($Constraint:ident, $description:literal, $matcher:literal) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $Constraint;
        impl TypeConstraint for $Constraint {
            #[inline(always)]
            fn description() -> impl core::fmt::Display {
                $description
            }

            #[inline(always)]
            fn matches(_ty: &$crate::Type) -> bool {
                $matcher
            }
        }
    };

    ($Constraint:ident, $description:literal, $matcher:path) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $Constraint;
        impl TypeConstraint for $Constraint {
            #[inline(always)]
            fn description() -> impl core::fmt::Display {
                $description
            }

            #[inline(always)]
            fn matches(ty: &$crate::Type) -> bool {
                $matcher(ty)
            }
        }
    };
}

type_constraint!(AnyType, "any type", true);
// TODO(pauls): Extend Type with new Function variant, we'll use that to represent function handles
//type_constraint!(AnyFunction, "a function type", crate::Type::is_function);
type_constraint!(AnyList, "any list type", crate::Type::is_list);
type_constraint!(AnyArray, "any array type", crate::Type::is_array);
type_constraint!(AnyStruct, "any struct type", crate::Type::is_struct);
type_constraint!(AnyPointer, "a pointer type", crate::Type::is_pointer);
type_constraint!(AnyInteger, "an integral type", crate::Type::is_integer);
type_constraint!(AnySignedInteger, "a signed integral type", crate::Type::is_signed_integer);
type_constraint!(
    AnyUnsignedInteger,
    "an unsigned integral type",
    crate::Type::is_unsigned_integer
);
type_constraint!(IntFelt, "a field element", crate::Type::is_felt);

/// A signless 8-bit integer
pub type Int8 = SizedInt<8>;
/// A signed 8-bit integer
pub type SInt8 = And<AnySignedInteger, SizedInt<8>>;
/// An unsigned 8-bit integer
pub type UInt8 = And<AnyUnsignedInteger, SizedInt<8>>;

/// A signless 16-bit integer
pub type Int16 = SizedInt<16>;
/// A signed 16-bit integer
pub type SInt16 = And<AnySignedInteger, SizedInt<16>>;
/// An unsigned 16-bit integer
pub type UInt16 = And<AnyUnsignedInteger, SizedInt<16>>;

/// A signless 32-bit integer
pub type Int32 = SizedInt<32>;
/// A signed 16-bit integer
pub type SInt32 = And<AnySignedInteger, SizedInt<32>>;
/// An unsigned 16-bit integer
pub type UInt32 = And<AnyUnsignedInteger, SizedInt<32>>;

/// A signless 64-bit integer
pub type Int64 = SizedInt<64>;
/// A signed 64-bit integer
pub type SInt64 = And<AnySignedInteger, SizedInt<64>>;
/// An unsigned 64-bit integer
pub type UInt64 = And<AnyUnsignedInteger, SizedInt<64>>;

impl BuildableTypeConstraint for IntFelt {
    fn build() -> crate::Type {
        crate::Type::Felt
    }
}
impl BuildableTypeConstraint for UInt8 {
    fn build() -> crate::Type {
        crate::Type::U8
    }
}
impl BuildableTypeConstraint for SInt8 {
    fn build() -> crate::Type {
        crate::Type::I8
    }
}
impl BuildableTypeConstraint for UInt16 {
    fn build() -> crate::Type {
        crate::Type::U16
    }
}
impl BuildableTypeConstraint for SInt16 {
    fn build() -> crate::Type {
        crate::Type::I16
    }
}
impl BuildableTypeConstraint for UInt32 {
    fn build() -> crate::Type {
        crate::Type::U32
    }
}
impl BuildableTypeConstraint for SInt32 {
    fn build() -> crate::Type {
        crate::Type::I32
    }
}
impl BuildableTypeConstraint for UInt64 {
    fn build() -> crate::Type {
        crate::Type::U64
    }
}
impl BuildableTypeConstraint for SInt64 {
    fn build() -> crate::Type {
        crate::Type::I64
    }
}

/// Represents a fixed-width integer of `N` bits
pub struct SizedInt<const N: usize>(core::marker::PhantomData<[(); N]>);
impl<const N: usize> Copy for SizedInt<N> {}
impl<const N: usize> Clone for SizedInt<N> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<const N: usize> fmt::Debug for SizedInt<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<const N: usize> fmt::Display for SizedInt<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{N}-bit integral type")
    }
}
impl<const N: usize> TypeConstraint for SizedInt<N> {
    fn description() -> impl fmt::Display {
        Self(core::marker::PhantomData)
    }

    fn matches(ty: &crate::Type) -> bool {
        ty.is_integer()
    }
}
impl BuildableTypeConstraint for SizedInt<8> {
    fn build() -> crate::Type {
        crate::Type::I8
    }
}
impl BuildableTypeConstraint for SizedInt<16> {
    fn build() -> crate::Type {
        crate::Type::I16
    }
}
impl BuildableTypeConstraint for SizedInt<32> {
    fn build() -> crate::Type {
        crate::Type::I32
    }
}
impl BuildableTypeConstraint for SizedInt<64> {
    fn build() -> crate::Type {
        crate::Type::I64
    }
}

/// A type constraint for pointer values
pub struct PointerOf<T>(core::marker::PhantomData<T>);
impl<T> Copy for PointerOf<T> {}
impl<T> Clone for PointerOf<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> fmt::Debug for PointerOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<T: TypeConstraint> fmt::Display for PointerOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pointee = <T as TypeConstraint>::description();
        write!(f, "a pointer to {pointee}")
    }
}
impl<T: TypeConstraint> TypeConstraint for PointerOf<T> {
    #[inline(always)]
    fn description() -> impl fmt::Display {
        Self(core::marker::PhantomData)
    }

    fn matches(ty: &crate::Type) -> bool {
        ty.pointee().is_some_and(|pointee| <T as TypeConstraint>::matches(pointee))
    }
}
impl<T: BuildableTypeConstraint> BuildableTypeConstraint for PointerOf<T> {
    fn build() -> crate::Type {
        let pointee = Box::new(<T as BuildableTypeConstraint>::build());
        crate::Type::Ptr(pointee)
    }
}

/// A type constraint for array values
pub struct AnyArrayOf<T>(core::marker::PhantomData<T>);
impl<T> Copy for AnyArrayOf<T> {}
impl<T> Clone for AnyArrayOf<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> fmt::Debug for AnyArrayOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<T: TypeConstraint> fmt::Display for AnyArrayOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let element = <T as TypeConstraint>::description();
        write!(f, "an array of {element}")
    }
}
impl<T: TypeConstraint> TypeConstraint for AnyArrayOf<T> {
    #[inline(always)]
    fn description() -> impl fmt::Display {
        Self(core::marker::PhantomData)
    }

    fn matches(ty: &crate::Type) -> bool {
        match ty {
            crate::Type::Array(ref elem, _) => <T as TypeConstraint>::matches(elem),
            _ => false,
        }
    }
}

/// A type constraint for array values
pub struct ArrayOf<const N: usize, T>(core::marker::PhantomData<[T; N]>);
impl<const N: usize, T> Copy for ArrayOf<N, T> {}
impl<const N: usize, T> Clone for ArrayOf<N, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<const N: usize, T> fmt::Debug for ArrayOf<N, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<const N: usize, T: TypeConstraint> fmt::Display for ArrayOf<N, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let element = <T as TypeConstraint>::description();
        write!(f, "an array of {N} {element}")
    }
}
impl<const N: usize, T: TypeConstraint> TypeConstraint for ArrayOf<N, T> {
    #[inline(always)]
    fn description() -> impl fmt::Display {
        Self(core::marker::PhantomData)
    }

    fn matches(ty: &crate::Type) -> bool {
        match ty {
            crate::Type::Array(ref elem, arity) if *arity == N => {
                <T as TypeConstraint>::matches(elem)
            }
            _ => false,
        }
    }
}
impl<const N: usize, T: BuildableTypeConstraint> BuildableTypeConstraint for ArrayOf<N, T> {
    fn build() -> crate::Type {
        let element = Box::new(<T as BuildableTypeConstraint>::build());
        crate::Type::Array(element, N)
    }
}

/// Represents a conjunction of two constraints as a concrete value
pub struct And<T, U> {
    _left: core::marker::PhantomData<T>,
    _right: core::marker::PhantomData<U>,
}
impl<T, U> Copy for And<T, U> {}
impl<T, U> Clone for And<T, U> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, U> fmt::Debug for And<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<T: TypeConstraint, U: TypeConstraint> TypeConstraint for And<T, U> {
    fn description() -> impl fmt::Display {
        struct Both<L, R> {
            left: L,
            right: R,
        }
        impl<L: fmt::Display, R: fmt::Display> fmt::Display for Both<L, R> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "both {} and {}", &self.left, &self.right)
            }
        }
        let left = <T as TypeConstraint>::description();
        let right = <U as TypeConstraint>::description();
        Both { left, right }
    }

    #[inline]
    fn matches(ty: &crate::Type) -> bool {
        <T as TypeConstraint>::matches(ty) && <U as TypeConstraint>::matches(ty)
    }
}

/// Represents a disjunction of two constraints as a concrete value
pub struct Or<T, U> {
    _left: core::marker::PhantomData<T>,
    _right: core::marker::PhantomData<U>,
}
impl<T, U> Copy for Or<T, U> {}
impl<T, U> Clone for Or<T, U> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, U> fmt::Debug for Or<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::any::type_name::<Self>())
    }
}
impl<T: TypeConstraint, U: TypeConstraint> TypeConstraint for Or<T, U> {
    fn description() -> impl fmt::Display {
        struct Either<L, R> {
            left: L,
            right: R,
        }
        impl<L: fmt::Display, R: fmt::Display> fmt::Display for Either<L, R> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "either {} or {}", &self.left, &self.right)
            }
        }
        let left = <T as TypeConstraint>::description();
        let right = <U as TypeConstraint>::description();
        Either { left, right }
    }

    #[inline]
    fn matches(ty: &crate::Type) -> bool {
        <T as TypeConstraint>::matches(ty) || <U as TypeConstraint>::matches(ty)
    }
}
