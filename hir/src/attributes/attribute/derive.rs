use alloc::rc::Rc;

use super::*;

/// This trait is implemented by all [Attribute] impls for which a [Type] can be derived from the
/// associated attribute value.
pub trait DerivableTypeAttribute: AttributeRegistration {
    /// Get a new instance of this attribute, deriving the attribute type from `value`.
    fn create<V>(context: &Rc<Context>, value: V) -> UnsafeIntrusiveEntityRef<Self>
    where
        <Self as AttributeRegistration>::Value: From<V>;
}

impl<A> DerivableTypeAttribute for A
where
    A: AttributeRegistration + MaybeInferAttributeType,
{
    fn create<V>(context: &Rc<Context>, value: V) -> UnsafeIntrusiveEntityRef<Self>
    where
        <A as AttributeRegistration>::Value: From<V>,
    {
        let value = <<A as AttributeRegistration>::Value>::from(value);
        let ty = <A as MaybeInferAttributeType>::maybe_infer_type_from_value(&value);
        A::create::<<A as AttributeRegistration>::Value>(
            context,
            value,
            ty.unwrap_or(Type::Unknown),
        )
    }
}

/// This trait is implemented by any [AttributeValue] impl for which a [Type] can be derived.
pub trait InferAttributeValueType: AttributeValue {
    /// Infer a [Type] for any value of this type
    ///
    /// Implementations should return `Type::Unknown` if a type cannot be inferred for all values
    /// of this type.
    fn infer_type() -> Type;
    /// Infer a [Type] for this value
    fn infer_type_from_value(&self) -> Type {
        Self::infer_type()
    }
}

/// This trait is implemented by any [Attribute] impl for which a [Type] can be derived from its
/// associated value.
///
/// A blanket implementation is derived for all attributes whose value type implements
/// [InferAttributeValueType].
pub trait InferAttributeType: AttributeRegistration {
    /// Infer a [Type] for any value of this attribute
    ///
    /// Implementations should return `Type::Unknown` if a type cannot be inferred for all values
    /// of this attribute.
    fn infer_type() -> Type;
    /// Infer the type for `value`
    fn infer_type_from_value(value: &<Self as AttributeRegistration>::Value) -> Type;
}

impl<T: AttributeRegistration> InferAttributeType for T
where
    <T as AttributeRegistration>::Value: InferAttributeValueType,
{
    #[inline]
    fn infer_type() -> Type {
        <<Self as AttributeRegistration>::Value as InferAttributeValueType>::infer_type()
    }

    #[inline]
    fn infer_type_from_value(value: &<Self as AttributeRegistration>::Value) -> Type {
        <<Self as AttributeRegistration>::Value as InferAttributeValueType>::infer_type_from_value(
            value,
        )
    }
}

/// This trait is implemented by all [Attribute] impls, and enables deriving a [Type] for the
/// attribute from its value, or returning `None` if one cannot be inferred.
///
/// A blanket implementation exists for all types which simply returns `None`, however a
/// specialization exists for attributes which implement [InferAttributeType], which will use a
/// valid instance of the attribute value to derive a [Type]
pub trait MaybeInferAttributeType: AttributeRegistration {
    /// Infer a type for any value of this attribute, or return `None` if one cannot be inferred
    fn maybe_infer_type() -> Option<Type>;
    /// Infer a type for `value`, or return `None` if one cannot be inferred
    fn maybe_infer_type_from_value(value: &<Self as AttributeRegistration>::Value) -> Option<Type>;
}

impl<T, V> MaybeInferAttributeType for T
where
    V: AttributeValue,
    T: AttributeRegistration<Value = V>,
{
    default fn maybe_infer_type() -> Option<Type> {
        None
    }

    default fn maybe_infer_type_from_value(
        _value: &<Self as AttributeRegistration>::Value,
    ) -> Option<Type> {
        None
    }
}

impl<T, V> MaybeInferAttributeType for T
where
    V: AttributeValue,
    T: AttributeRegistration<Value = V> + InferAttributeType,
{
    fn maybe_infer_type() -> Option<Type> {
        Some(<T as InferAttributeType>::infer_type())
    }

    fn maybe_infer_type_from_value(value: &<Self as AttributeRegistration>::Value) -> Option<Type> {
        Some(<T as InferAttributeType>::infer_type_from_value(value))
    }
}

macro_rules! value_infers_as {
    ($value_ty:ty, $ty:expr) => {
        impl InferAttributeValueType for $value_ty {
            #[inline(always)]
            fn infer_type() -> Type {
                $ty
            }
        }
    };
}

value_infers_as!(bool, Type::I1);
value_infers_as!(i8, Type::I8);
value_infers_as!(u8, Type::U8);
value_infers_as!(i16, Type::I16);
value_infers_as!(u16, Type::U16);
value_infers_as!(i32, Type::I32);
value_infers_as!(u32, Type::U32);
value_infers_as!(i64, Type::I64);
value_infers_as!(u64, Type::U64);
value_infers_as!(i128, Type::I128);
value_infers_as!(u128, Type::U128);
value_infers_as!(f64, Type::F64);
