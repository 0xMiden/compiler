mod derive;
mod name;
mod registration;
mod value;

use alloc::rc::Rc;
use core::{
    any::Any,
    clone::CloneToUninit,
    fmt,
    ptr::{DynMetadata, NonNull, Pointee},
};

pub use self::{
    derive::{
        DerivableTypeAttribute, InferAttributeType, InferAttributeValueType,
        MaybeInferAttributeType,
    },
    name::AttributeName,
    registration::AttributeRegistration,
    value::*,
};
use crate::{
    Context, Dialect, Entity, EntityList, EntityListCursor, EntityListCursorMut, EntityListItem,
    Immediate, Type, UnsafeIntrusiveEntityRef,
};

pub type AttributeRef = UnsafeIntrusiveEntityRef<dyn Attribute>;

/// Converts an attribute handle into a type-erased [AttributeRef].
pub trait IntoAttributeRef {
    /// Converts this handle into a type-erased [AttributeRef].
    fn into_attribute_ref(self) -> AttributeRef;
}

impl IntoAttributeRef for AttributeRef {
    #[inline(always)]
    fn into_attribute_ref(self) -> AttributeRef {
        self
    }
}

impl<T> IntoAttributeRef for UnsafeIntrusiveEntityRef<T>
where
    T: Attribute,
{
    #[inline(always)]
    fn into_attribute_ref(self) -> AttributeRef {
        self.as_attribute_ref()
    }
}

impl PartialEq for AttributeRef {
    fn eq(&self, other: &Self) -> bool {
        if Self::ptr_eq(self, other) {
            true
        } else {
            self.borrow().dyn_eq(&other.borrow())
        }
    }
}

pub type AttrList = EntityList<Attr>;
pub type AttrCursor<'a> = EntityListCursor<'a, Attr>;
pub type AttrCursorMut<'a> = EntityListCursorMut<'a, Attr>;

pub trait Attribute:
    crate::any::AsAny
    + CloneToUninit
    + fmt::Debug
    + crate::PartialEqable
    + crate::DynPartialEq
    + crate::DynHash
{
    fn type_name(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn context(&self) -> &Context;
    fn context_rc(&self) -> Rc<Context>;
    fn name(&self) -> &AttributeName;
    fn value(&self) -> &dyn AttributeValue;
    fn value_mut(&mut self) -> &mut dyn AttributeValue;
    fn ty(&self) -> &Type;
    fn set_type(&mut self, ty: Type);
    fn as_attr(&self) -> &Attr;
    fn as_attr_mut(&mut self) -> &mut Attr;
}

impl core::hash::Hash for dyn Attribute {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use crate::DynHash;

        let hashable = self as &dyn DynHash;
        hashable.dyn_hash(state);
    }
}

impl Eq for dyn Attribute {}
impl PartialEq for dyn Attribute {
    fn eq(&self, other: &Self) -> bool {
        self.dyn_eq(other)
    }
}

impl dyn Attribute {
    /// Returns true if this attribute is an instance of type `T`
    pub fn is<T: AttributeRegistration>(&self) -> bool {
        Attribute::as_any(self).is::<T>()
    }

    /// Attempts to downcast a `&dyn Attribute` to `&T`, if the value is an instance of type `T`.
    pub fn downcast_ref<T: AttributeRegistration>(&self) -> Option<&T> {
        Attribute::as_any(self).downcast_ref::<T>()
    }

    /// Attempts to downcast this attribute value reference to `&mut T`, if the value is an instance
    /// of type `T`.
    ///
    /// Returns `None` if this value is not of type `T`.
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        Attribute::as_any_mut(self).downcast_mut::<T>()
    }

    /// A convenience function for downcasting a `bool` attribute value to the concrete boolean
    /// value.
    ///
    /// Returns `None` if this value is not a `bool` or `Immediate` that can be cast to `bool`.
    pub fn as_bool(&self) -> Option<bool> {
        self.value().as_any().downcast_ref::<bool>().copied()
    }

    /// A convenience function for downcasting a `u32` attribute value to the concrete value.
    ///
    /// Returns `None` if this value is not a `u32` or `Immediate` that can be cast to `u32`.
    pub fn as_u32(&self) -> Option<u32> {
        self.as_immediate().and_then(|imm| imm.as_u32())
    }

    /// A convenience function for downcasting an `Immediate` attribute value to the concrete value.
    ///
    /// Returns `None` if this value is not an `Immediate`.
    pub fn as_immediate(&self) -> Option<Immediate> {
        use super::IntegerLikeAttr;

        Some(self.as_attr().as_trait::<dyn IntegerLikeAttr>()?.as_immediate())
    }

    /// Get a deep clone of this attribute value in the same context
    pub fn dyn_clone(&self) -> AttributeRef {
        self.name().dyn_clone(self)
    }
}

/// The [Attr] struct provides the common foundation for all [Attribute] implementations.
///
/// It provides:
///
/// * Support for casting between the concrete attribute type `T`, `dyn Attribute`, the
///   underlying `Attribute`, and any of the attribute traits that the attribute implements. Not
///   only can the casts be performed, but an [Attr] can be queried to see if it implements a
///   specific trait at runtime to conditionally perform some behavior. This makes working with
///   attributes in the IR very flexible and allows for adding or modifying attributes without
///   needing to change most of the compiler, which predominately works on attribute traits rather
///   than concrete types.
/// * Many utility functions related to working with attributes, many of which are also accessible
///   via the [Attribute] trait, so that working with an [Attribute] or an [Attr] are
///   largely indistinguishable.
///
/// All [Attribute] implementations can be cast to the underlying [Attr], but most of the
/// fucntionality is re-exported via default implementations of methods on the [Attribute]
/// trait. The main benefit is avoiding any potential overhead of casting when going through the
/// trait, rather than calling the underlying [Attr] method directly.
///
/// # Safety
///
/// Similar to [crate::Operation], [Attr] is an IR entity that must be allocated via the arena
/// managed by [Context]. See the documentation for [crate::Operation] for more details on what
/// this means.
#[repr(C)]
#[derive(Clone)]
pub struct Attr {
    context: NonNull<Context>,
    name: AttributeName,
    ty: Type,
    /// The offset of the field containing the [Attr] field within the containing [Attribute].
    ///
    /// This is required in order to be able to perform casts from [Attribute]. An [Attribute]
    /// cannot be constructed without providing it to the `uninit` function, and callers of that
    /// function are required to ensure that it is correct.
    offset: usize,
}

impl Eq for Attr {}
impl PartialEq for Attr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ty == other.ty
    }
}

impl core::hash::Hash for Attr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.ty.hash(state);
    }
}

impl Attr {
    // This is public so that it is accessible for use by derive(DialectAttribute)
    #[doc(hidden)]
    pub unsafe fn uninit<T: Attribute>(
        context: &Rc<Context>,
        name: AttributeName,
        ty: Type,
        offset: usize,
    ) -> Self {
        assert!(name.is::<T>());

        Self {
            context: unsafe { NonNull::new_unchecked(Rc::as_ptr(context).cast_mut()) },
            name,
            ty,
            offset,
        }
    }

    /// Get a reference to the containing [Attribute] implementation.
    pub fn into_dyn_attribute(&self) -> &dyn Attribute {
        // This is guaranteed to succeed because all [Attr] instances are members of a [Attribute]
        // implementation.
        self.as_trait::<dyn Attribute>().unwrap()
    }
}

impl fmt::Debug for Attr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attr")
            .field_with("name", |f| write!(f, "{}", &self.name))
            .field("ty", &self.ty)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for UnsafeIntrusiveEntityRef<Attr> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Debug::fmt(&self.borrow(), f)
    }
}

impl fmt::Display for UnsafeIntrusiveEntityRef<Attr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.borrow().name())
    }
}

impl AsRef<dyn Attribute> for Attr {
    fn as_ref(&self) -> &dyn Attribute {
        self.name.upcast(self.container()).unwrap()
    }
}

impl AsMut<dyn Attribute> for Attr {
    fn as_mut(&mut self) -> &mut dyn Attribute {
        self.name.upcast_mut(self.container().cast_mut()).unwrap()
    }
}

impl Entity for Attr {}
impl EntityListItem for Attr {}

/// Metadata
impl UnsafeIntrusiveEntityRef<dyn Attribute> {
    pub fn name(&self) -> AttributeName {
        // SAFETY: This relies on the fact that we generate Attribute implementations such that the
        // first field is always the [Attr], and the containing struct is #[repr(C)], guaranteeing
        // that the data pointer for a Attribute trait object can be safely cast to Attr.
        let ptr = Self::into_raw(*self).cast::<Attr>();
        // SAFETY: The `name` field of Attribute is read-only after it is allocated, and the
        // safety guarantees of UnsafeIntrusiveEntityRef require that the allocation never moves for
        // the lifetime of the ref. So it is always safe to read this field via direct pointer, even
        // if a mutable borrow of the containing attribute exists, because the field is never
        // written to after allocation.
        unsafe {
            let name_ptr = core::ptr::addr_of!((*ptr).name);
            AttributeName::clone(&*name_ptr)
        }
    }

    /// Returns this attribute as a handle to an implemented trait object, if supported.
    ///
    /// The returned handle preserves the original intrusive allocation identity and swaps only the
    /// pointee metadata for the requested trait object.
    pub fn as_trait_ref<Trait>(self) -> Option<UnsafeIntrusiveEntityRef<Trait>>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let ptr = self.name().upcast_raw::<Trait>(Self::into_raw(self).cast())?;
        let (_, metadata) = ptr.to_raw_parts();
        Some(unsafe { self.cast_unsized_unchecked::<Trait>(metadata) })
    }

    /// Attempts to cast this handle to the concrete attribute type `T`.
    ///
    /// This preserves the original intrusive allocation identity rather than routing through the
    /// generic `RawEntityRef` downcast helpers.
    pub fn try_downcast_attr<T>(self) -> Result<UnsafeIntrusiveEntityRef<T>, Self>
    where
        T: AttributeRegistration,
    {
        if self.name().is::<T>() {
            Ok(unsafe { self.cast_unchecked::<T>() })
        } else {
            Err(self)
        }
    }

    /// Casts this handle to the concrete attribute type `T`.
    ///
    /// Panics if the cast is not valid for this attribute.
    #[track_caller]
    pub fn downcast_attr<T>(self) -> UnsafeIntrusiveEntityRef<T>
    where
        T: AttributeRegistration,
    {
        match self.try_downcast_attr::<T>() {
            Ok(attr) => attr,
            Err(_) => panic!("invalid cast"),
        }
    }
}

/// Metadata
impl Attr {
    /// Get the [AttributeName] of this attribute
    pub fn name(&self) -> &AttributeName {
        &self.name
    }

    /// Get the dialect associated with this attribute
    pub fn dialect(&self) -> Rc<dyn Dialect> {
        self.context().get_registered_dialect(self.name.dialect())
    }

    /// Get a borrowed reference to the owning [Context] of this attribute
    #[inline(always)]
    pub fn context(&self) -> &Context {
        // SAFETY: This is safe so long as this attribute is allocated in a Context, since the
        // Context by definition outlives the allocation.
        unsafe { self.context.as_ref() }
    }

    /// Get a owned reference to the owning [Context] of this attribute
    pub fn context_rc(&self) -> Rc<Context> {
        // SAFETY: This is safe so long as this attribute is allocated in a Context, since the
        // Context by definition outlives the allocation.
        //
        // Additionally, constructing the Rc from a raw pointer is safe here, as the pointer was
        // obtained using `Rc::as_ptr`, so the only requirement to call `Rc::from_raw` is to
        // increment the strong count, as `as_ptr` does not preserve the count for the reference
        // held by this attribute. Incrementing the count first is required to manufacture new
        // clones of the `Rc` safely.
        unsafe {
            let ptr = self.context.as_ptr().cast_const();
            Rc::increment_strong_count(ptr);
            Rc::from_raw(ptr)
        }
    }

    /// Get the [Type] associated with this attribute
    #[inline]
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    /// Set the [Type] associated with this attribute
    #[inline]
    pub fn set_type(&mut self, ty: Type) {
        self.ty = ty;
    }
}

/// Traits/Casts
impl Attr {
    #[doc(hidden)]
    #[inline]
    const fn container(&self) -> *const () {
        unsafe {
            let ptr = self as *const Self;
            ptr.byte_sub(self.offset).cast()
        }
    }

    /// Convert this reference into an [`UnsafeIntrusiveEntityRef<Attr>`]
    #[inline(always)]
    pub fn as_attr_ref(&self) -> UnsafeIntrusiveEntityRef<Self> {
        // SAFETY: This is safe under the assumption that we always allocate Attrs using the
        // arena, i.e. it is a child of a RawEntityMetadata structure.
        //
        // Additionally, this relies on the fact that Attribute implementations are #[repr(C)]
        // and ensure that their Attr field is always first in the generated struct
        unsafe { UnsafeIntrusiveEntityRef::from_raw(self.container().cast()) }
    }

    /// Convert this reference into an [AttributeRef]
    #[inline(always)]
    pub fn as_attribute_ref(&self) -> AttributeRef {
        // SAFETY: This is safe under the assumption that we always allocate Attrs using the
        // arena, i.e. it is a child of a RawEntityMetadata structure.
        //
        // Additionally, this relies on the fact that Attribute implementations are #[repr(C)]
        // and ensure that their Attr field is always first in the generated struct
        let ptr = self.name.upcast_raw(self.container()).unwrap();
        unsafe { AttributeRef::from_raw(ptr) }
    }

    /// Returns true if the concrete type of this attribute is `T`
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        self.name.is::<T>()
    }

    /// Returns true if this attribute implements `Trait`
    #[inline]
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.implements::<Trait>()
    }

    /// Attempt to downcast to the concrete [Attribute] type of this operation
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.name.downcast_ref::<T>(self.container())
    }

    /// Attempt to downcast mutably to the concrete [Attribute] type of this operation
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.name.downcast_mut::<T>(self.container().cast_mut())
    }

    /// Attempt to cast this attribute reference to an implementation of `Trait`
    pub fn as_trait<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.upcast(self.container())
    }

    /// Attempt to cast this mutable attribute reference to an implementation of `Trait`
    pub fn as_trait_mut<Trait>(&mut self) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.upcast_mut(self.container().cast_mut())
    }
}

#[cfg(test)]
mod tests {
    use core::hash::Hasher;

    use crate::{
        Immediate, ImmediateAttr, Type, attributes::IntegerLikeAttr,
        dialects::builtin::attributes::U32Attr, testing::Test,
    };

    #[test]
    fn attribute_dyn_hash() {
        let test = Test::default();

        let zero = test.context_rc().create_attribute::<U32Attr, _>(0u32).as_attribute_ref();
        let zero_two = test.context_rc().create_attribute::<U32Attr, _>(0u32).as_attribute_ref();
        let zero_three =
            test.context_rc().create_attribute::<ImmediateAttr, _>(0u32).as_attribute_ref();
        let one = test.context_rc().create_attribute::<U32Attr, _>(1u32).as_attribute_ref();

        let mut hasher = crate::FxHasher::default();
        zero.borrow().dyn_hash(&mut hasher);
        let zero_hash = hasher.finish();

        let mut hasher = crate::FxHasher::default();
        zero_two.borrow().dyn_hash(&mut hasher);
        let zero_two_hash = hasher.finish();

        let mut hasher = crate::FxHasher::default();
        zero_three.borrow().dyn_hash(&mut hasher);
        let zero_three_hash = hasher.finish();

        let mut hasher = crate::FxHasher::default();
        one.borrow().dyn_hash(&mut hasher);
        let one_hash = hasher.finish();

        assert_eq!(zero_hash, zero_two_hash);
        assert_ne!(zero_hash, zero_three_hash);
        assert_ne!(zero_hash, one_hash);
    }

    #[test]
    fn attribute_dyn_eq() {
        let test = Test::default();

        let zero = test.context_rc().create_attribute::<U32Attr, _>(0u32).as_attribute_ref();
        let zero_two = test.context_rc().create_attribute::<U32Attr, _>(0u32).as_attribute_ref();
        let zero_three =
            test.context_rc().create_attribute::<ImmediateAttr, _>(0u32).as_attribute_ref();
        let one = test.context_rc().create_attribute::<U32Attr, _>(1u32).as_attribute_ref();

        let zero = zero.borrow();
        let zero_two = zero_two.borrow();
        let zero_three = zero_three.borrow();
        let one = one.borrow();
        assert_eq!(&zero, &zero_two);
        assert_ne!(&zero, &zero_three);
        assert_ne!(&zero, &one);
    }

    #[test]
    fn immediate_attribute_ref_roundtrips_with_type() {
        let test = Test::default();

        let immediate = test.context_rc().create_attribute::<ImmediateAttr, _>(Immediate::I32(1));
        assert_eq!(immediate.borrow().ty().clone(), Type::I32);

        let erased = immediate.as_attribute_ref();
        assert_eq!(erased.borrow().ty().clone(), Type::I32);

        let roundtrip = erased.try_downcast_attr::<ImmediateAttr>().unwrap();
        let roundtrip = roundtrip.borrow();
        assert_eq!(roundtrip.ty().clone(), Type::I32);
        assert_eq!(roundtrip.as_immediate(), Immediate::I32(1));
    }
}
