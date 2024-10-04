use core::{any::Any, marker::Unsize};

pub trait AsAny: Any + Unsize<dyn Any> {
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self
    }
    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    #[inline(always)]
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl<T: ?Sized + Any + Unsize<dyn Any>> AsAny for T {}

/// # Safety
///
/// This trait is not safe (or possible) to implement manually.
///
/// It is automatically derived for all `T: Unsize<Trait>`
pub unsafe trait Is<Trait: ?Sized>: Unsize<Trait> {}
unsafe impl<Trait: ?Sized, T> Is<Trait> for T where T: ?Sized + Unsize<Trait> {}

pub trait IsObjOf<T: ?Sized> {}
impl<T, Trait> IsObjOf<T> for Trait
where
    T: ?Sized + Is<Trait>,
    Trait: ?Sized,
{
}

#[allow(unused)]
pub trait DowncastRef<To: ?Sized>: IsObjOf<To> {
    fn downcast_ref(&self) -> Option<&To>;
    fn downcast_mut(&mut self) -> Option<&mut To>;
}
impl<From, To> DowncastRef<To> for From
where
    From: ?Sized,
    To: ?Sized + DowncastFromRef<From>,
{
    #[inline(always)]
    fn downcast_ref(&self) -> Option<&To> {
        To::downcast_from_ref(self)
    }

    #[inline(always)]
    fn downcast_mut(&mut self) -> Option<&mut To> {
        To::downcast_from_mut(self)
    }
}

pub trait DowncastFromRef<From: ?Sized>: Is<From> {
    fn downcast_from_ref(from: &From) -> Option<&Self>;
    fn downcast_from_mut(from: &mut From) -> Option<&mut Self>;
}
impl<From, To> DowncastFromRef<From> for To
where
    From: ?Sized + AsAny,
    To: Is<From> + 'static,
{
    #[inline]
    fn downcast_from_ref(from: &From) -> Option<&Self> {
        from.as_any().downcast_ref()
    }

    #[inline]
    fn downcast_from_mut(from: &mut From) -> Option<&mut Self> {
        from.as_any_mut().downcast_mut()
    }
}

#[allow(unused)]
pub trait Downcast<To: ?Sized, Obj: ?Sized>: DowncastRef<To> + Is<Obj> {
    fn downcast(self: Box<Self>) -> Result<Box<To>, Box<Obj>>;
}
impl<From, To, Obj> Downcast<To, Obj> for From
where
    From: ?Sized + DowncastRef<To> + Is<Obj>,
    To: ?Sized + DowncastFrom<Self, Obj>,
    Obj: ?Sized,
{
    #[inline]
    fn downcast(self: Box<Self>) -> Result<Box<To>, Box<Obj>> {
        To::downcast_from(self)
    }
}

pub trait DowncastFrom<From, Obj>: DowncastFromRef<From>
where
    From: ?Sized + Is<Obj>,
    Obj: ?Sized,
{
    fn downcast_from(from: Box<From>) -> Result<Box<Self>, Box<Obj>>;
}
impl<From, To, Obj> DowncastFrom<From, Obj> for To
where
    From: ?Sized + Is<Obj> + AsAny + 'static,
    To: DowncastFromRef<From> + 'static,
    Obj: ?Sized,
{
    fn downcast_from(from: Box<From>) -> Result<Box<Self>, Box<Obj>> {
        if !from.as_any().is::<To>() {
            Ok(from.into_any().downcast().unwrap())
        } else {
            Err(from)
        }
    }
}

pub trait Upcast<To: ?Sized>: TryUpcastRef<To> {
    fn upcast_ref(&self) -> &To;
    fn upcast_mut(&mut self) -> &mut To;
    fn upcast(self: Box<Self>) -> Box<To>;
}
impl<From, To> Upcast<To> for From
where
    From: ?Sized + Is<To>,
    To: ?Sized,
{
    #[inline(always)]
    fn upcast_ref(&self) -> &To {
        self
    }

    #[inline(always)]
    fn upcast_mut(&mut self) -> &mut To {
        self
    }

    #[inline(always)]
    fn upcast(self: Box<Self>) -> Box<To> {
        self
    }
}

pub trait TryUpcastRef<To>: Is<To>
where
    To: ?Sized,
{
    #[allow(unused)]
    fn is_of(&self) -> bool;
    fn try_upcast_ref(&self) -> Option<&To>;
    fn try_upcast_mut(&mut self) -> Option<&mut To>;
}
impl<From, To> TryUpcastRef<To> for From
where
    From: Upcast<To> + ?Sized,
    To: ?Sized,
{
    #[inline(always)]
    fn is_of(&self) -> bool {
        true
    }

    #[inline(always)]
    fn try_upcast_ref(&self) -> Option<&To> {
        Some(self.upcast_ref())
    }

    #[inline(always)]
    fn try_upcast_mut(&mut self) -> Option<&mut To> {
        Some(self.upcast_mut())
    }
}

pub trait TryUpcast<To, Obj>: Is<Obj> + TryUpcastRef<To>
where
    To: ?Sized,
    Obj: ?Sized,
{
    #[inline(always)]
    fn try_upcast(self: Box<Self>) -> Result<Box<To>, Box<Obj>> {
        Err(self)
    }
}
impl<From, To, Obj> TryUpcast<To, Obj> for From
where
    From: Is<Obj> + Upcast<To> + ?Sized,
    To: ?Sized,
    Obj: ?Sized,
{
    #[inline]
    fn try_upcast(self: Box<Self>) -> Result<Box<To>, Box<Obj>> {
        Ok(self.upcast())
    }
}

/// Upcasts a type into a trait object
#[allow(unused)]
pub trait UpcastFrom<From>
where
    From: ?Sized,
{
    fn upcast_from_ref(from: &From) -> &Self;
    fn upcast_from_mut(from: &mut From) -> &mut Self;
    fn upcast_from(from: Box<From>) -> Box<Self>;
}

impl<From, To> UpcastFrom<From> for To
where
    From: Upcast<To> + ?Sized,
    To: ?Sized,
{
    #[inline]
    fn upcast_from_ref(from: &From) -> &Self {
        from.upcast_ref()
    }

    #[inline]
    fn upcast_from_mut(from: &mut From) -> &mut Self {
        from.upcast_mut()
    }

    #[inline]
    fn upcast_from(from: Box<From>) -> Box<Self> {
        from.upcast()
    }
}

#[allow(unused)]
pub trait TryUpcastFromRef<From>: IsObjOf<From>
where
    From: ?Sized,
{
    fn try_upcast_from_ref(from: &From) -> Option<&Self>;
    fn try_upcast_from_mut(from: &mut From) -> Option<&mut Self>;
}

impl<From, To> TryUpcastFromRef<From> for To
where
    From: TryUpcastRef<To> + ?Sized,
    To: ?Sized,
{
    #[inline]
    fn try_upcast_from_ref(from: &From) -> Option<&Self> {
        from.try_upcast_ref()
    }

    #[inline]
    fn try_upcast_from_mut(from: &mut From) -> Option<&mut Self> {
        from.try_upcast_mut()
    }
}

#[allow(unused)]
pub trait TryUpcastFrom<From, Obj>: TryUpcastFromRef<From>
where
    From: Is<Obj> + ?Sized,
    Obj: ?Sized,
{
    fn try_upcast_from(from: Box<From>) -> Result<Box<Self>, Box<Obj>>;
}

impl<From, To, Obj> TryUpcastFrom<From, Obj> for To
where
    From: TryUpcast<Self, Obj> + ?Sized,
    To: ?Sized,
    Obj: ?Sized,
{
    #[inline]
    fn try_upcast_from(from: Box<From>) -> Result<Box<Self>, Box<Obj>> {
        from.try_upcast()
    }
}
