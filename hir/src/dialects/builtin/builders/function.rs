use crate::{
    dialects::builtin::{FunctionRef, LocalVariable},
    *,
};

pub struct FunctionBuilder<'f, B: ?Sized> {
    pub func: FunctionRef,
    builder: &'f mut B,
}
impl<'f, B: ?Sized + Builder> FunctionBuilder<'f, B> {
    pub fn new(mut function: FunctionRef, builder: &'f mut B) -> Self {
        let mut func = function.borrow_mut();
        let current_block = if func.body().is_empty() {
            func.create_entry_block()
        } else {
            func.last_block()
        };

        builder.set_insertion_point_to_end(current_block);

        Self {
            func: function,
            builder,
        }
    }

    pub fn at(builder: &'f mut B, func: FunctionRef, ip: ProgramPoint) -> Self {
        builder.set_insertion_point(ip);

        Self { func, builder }
    }

    pub fn as_parts_mut(&mut self) -> (FunctionRef, &mut B) {
        (self.func, self.builder)
    }

    pub fn body_region(&self) -> RegionRef {
        self.func.borrow().body().as_region_ref()
    }

    pub fn entry_block(&self) -> BlockRef {
        self.func.borrow().entry_block()
    }

    #[inline]
    pub fn current_block(&self) -> BlockRef {
        self.builder.insertion_block().expect("builder has no insertion point set")
    }

    #[inline]
    pub fn switch_to_block(&mut self, block: BlockRef) {
        self.builder.set_insertion_point_to_end(block);
    }

    pub fn create_block(&mut self) -> BlockRef {
        let ip = *self.builder.insertion_point();
        let block = self.builder.create_block(self.body_region(), None, &[]);
        self.builder.restore_insertion_point(ip);
        block
    }

    pub fn create_block_in_region(&mut self, region: RegionRef) -> BlockRef {
        let ip = *self.builder.insertion_point();
        let block = self.builder.create_block(region, None, &[]);
        self.builder.restore_insertion_point(ip);
        block
    }

    pub fn detach_block(&mut self, mut block: BlockRef) {
        assert_ne!(
            block,
            self.current_block(),
            "cannot remove block the builder is currently inserting in"
        );
        assert_eq!(
            block.parent().map(|p| RegionRef::as_ptr(&p)),
            Some(RegionRef::as_ptr(&self.func.borrow().body().as_region_ref())),
            "cannot detach a block that does not belong to this function"
        );
        unsafe {
            let mut func = self.func.borrow_mut();
            let mut body = func.body_mut();
            body.body_mut().cursor_mut_from_ptr(block).remove();
        }
        block.borrow_mut().uses_mut().clear();
    }

    pub fn append_block_param(&mut self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.builder.context().append_block_argument(block, ty, span)
    }

    pub fn alloc_local(&mut self, ty: Type) -> LocalVariable {
        self.func.borrow_mut().alloc_local(ty)
    }

    pub fn builder(&self) -> &B {
        self.builder
    }

    pub fn builder_mut(&mut self) -> &mut B {
        self.builder
    }
}
