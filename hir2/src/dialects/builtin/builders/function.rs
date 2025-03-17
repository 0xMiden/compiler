use crate::{dialects::builtin::Function, *};

pub struct FunctionBuilder<'f, B: ?Sized> {
    pub func: &'f mut Function,
    builder: &'f mut B,
}
impl<'f, B: ?Sized + Builder> FunctionBuilder<'f, B> {
    pub fn new(func: &'f mut Function, builder: &'f mut B) -> Self {
        let current_block = if func.body().is_empty() {
            func.create_entry_block()
        } else {
            func.last_block()
        };

        builder.set_insertion_point_to_end(current_block);

        Self { func, builder }
    }

    pub fn at(builder: &'f mut B, func: &'f mut Function, ip: ProgramPoint) -> Self {
        builder.set_insertion_point(ip);

        Self { func, builder }
    }

    pub fn as_parts_mut(&mut self) -> (&mut Function, &mut B) {
        (self.func, self.builder)
    }

    pub fn body_region(&self) -> RegionRef {
        self.func.body().as_region_ref()
    }

    pub fn entry_block(&self) -> BlockRef {
        self.func.entry_block()
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
            Some(RegionRef::as_ptr(&self.func.body().as_region_ref())),
            "cannot detach a block that does not belong to this function"
        );
        let mut body = self.func.body_mut();
        unsafe {
            body.body_mut().cursor_mut_from_ptr(block).remove();
        }
        block.borrow_mut().uses_mut().clear();
    }

    pub fn append_block_param(&mut self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.builder.context().append_block_argument(block, ty, span)
    }

    pub fn builder(&self) -> &B {
        self.builder
    }

    pub fn builder_mut(&mut self) -> &mut B {
        self.builder
    }
}
