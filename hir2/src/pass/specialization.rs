use crate::{
    traits::BranchOpInterface, Context, EntityMut, EntityRef, Op, Operation, OperationName,
    OperationRef, Symbol, SymbolTable,
};

pub trait PassTarget {
    fn target_name(context: &Context) -> Option<OperationName>;
    fn into_target(op: &OperationRef) -> EntityRef<'_, Self>;
    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, Self>;
}

impl<T: 'static> PassTarget for T {
    default fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    #[inline]
    #[track_caller]
    default fn into_target(op: &OperationRef) -> EntityRef<'_, T> {
        EntityRef::map(op.borrow(), |t| {
            t.downcast_ref::<T>().unwrap_or_else(|| expected_type::<T>(op))
        })
    }

    #[inline]
    #[track_caller]
    default fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, T> {
        EntityMut::map(op.borrow_mut(), |t| {
            t.downcast_mut::<T>().unwrap_or_else(|| expected_type::<T>(op))
        })
    }
}
impl PassTarget for Operation {
    #[inline(always)]
    fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    #[inline]
    #[track_caller]
    fn into_target(op: &OperationRef) -> EntityRef<'_, Operation> {
        op.borrow()
    }

    #[inline]
    #[track_caller]
    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, Operation> {
        op.borrow_mut()
    }
}
impl PassTarget for dyn Op {
    #[inline(always)]
    fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    fn into_target(op: &OperationRef) -> EntityRef<'_, dyn Op> {
        EntityRef::map(op.borrow(), |op| op.as_trait::<dyn Op>().unwrap())
    }

    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, dyn Op> {
        EntityMut::map(op.borrow_mut(), |op| op.as_trait_mut::<dyn Op>().unwrap())
    }
}
impl PassTarget for dyn BranchOpInterface {
    #[inline(always)]
    fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    #[track_caller]
    fn into_target(op: &OperationRef) -> EntityRef<'_, dyn BranchOpInterface> {
        EntityRef::map(op.borrow(), |t| {
            t.as_trait::<dyn BranchOpInterface>()
                .unwrap_or_else(|| expected_implementation::<dyn BranchOpInterface>(op))
        })
    }

    #[track_caller]
    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, dyn BranchOpInterface> {
        EntityMut::map(op.borrow_mut(), |t| {
            t.as_trait_mut::<dyn BranchOpInterface>()
                .unwrap_or_else(|| expected_implementation::<dyn BranchOpInterface>(op))
        })
    }
}
impl PassTarget for dyn Symbol {
    #[inline(always)]
    fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    #[track_caller]
    fn into_target(op: &OperationRef) -> EntityRef<'_, dyn Symbol> {
        EntityRef::map(op.borrow(), |t| {
            t.as_trait::<dyn Symbol>()
                .unwrap_or_else(|| expected_implementation::<dyn Symbol>(op))
        })
    }

    #[track_caller]
    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, dyn Symbol> {
        EntityMut::map(op.borrow_mut(), |t| {
            t.as_trait_mut::<dyn Symbol>()
                .unwrap_or_else(|| expected_implementation::<dyn Symbol>(op))
        })
    }
}
impl PassTarget for dyn SymbolTable + 'static {
    #[inline(always)]
    fn target_name(_context: &Context) -> Option<OperationName> {
        None
    }

    #[track_caller]
    fn into_target(op: &OperationRef) -> EntityRef<'_, dyn SymbolTable + 'static> {
        EntityRef::map(op.borrow(), |t| {
            t.as_trait::<dyn SymbolTable>()
                .unwrap_or_else(|| expected_implementation::<dyn SymbolTable>(op))
        })
    }

    #[track_caller]
    fn into_target_mut(op: &mut OperationRef) -> EntityMut<'_, dyn SymbolTable + 'static> {
        EntityMut::map(op.borrow_mut(), |t| {
            t.as_trait_mut::<dyn SymbolTable>()
                .unwrap_or_else(|| expected_implementation::<dyn SymbolTable>(op))
        })
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn expected_type<T: 'static>(op: &OperationRef) -> ! {
    panic!(
        "expected operation '{}' to be a `{}`",
        op.borrow().name(),
        core::any::type_name::<T>(),
    )
}

#[cold]
#[inline(never)]
#[track_caller]
fn expected_implementation<Trait: ?Sized + 'static>(op: &OperationRef) -> ! {
    panic!(
        "expected '{}' to implement `{}`, but no vtable was found",
        op.borrow().name(),
        core::any::type_name::<Trait>()
    )
}
