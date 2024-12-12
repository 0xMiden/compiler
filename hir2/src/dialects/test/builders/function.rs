use crate::{
    dialects::{builtin::*, test::*},
    Block, BlockRef, Builder, Immediate, Op, OpBuilder, Region, RegionRef, Report, SourceSpan,
    Type, UnsafeIntrusiveEntityRef, Usable, ValueRef,
};

pub struct FunctionBuilder<'f> {
    pub func: &'f mut Function,
    builder: OpBuilder,
}
impl<'f> FunctionBuilder<'f> {
    pub fn new(func: &'f mut Function) -> Self {
        let current_block = if func.body().is_empty() {
            func.create_entry_block()
        } else {
            func.last_block()
        };
        let context = func.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);

        builder.set_insertion_point_to_end(current_block);

        Self { func, builder }
    }

    pub fn at(func: &'f mut Function, ip: crate::ProgramPoint) -> Self {
        let context = func.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);
        builder.set_insertion_point(ip);

        Self { func, builder }
    }

    pub fn body_region(&self) -> RegionRef {
        unsafe { RegionRef::from_raw(&*self.func.body()) }
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
        self.builder.create_block(self.body_region(), None, &[])
    }

    pub fn detach_block(&mut self, mut block: BlockRef) {
        use crate::EntityWithParent;

        assert_ne!(
            block,
            self.current_block(),
            "cannot remove block the builder is currently inserting in"
        );
        assert_eq!(
            block.borrow().parent().map(|p| RegionRef::as_ptr(&p)),
            Some(&*self.func.body() as *const Region),
            "cannot detach a block that does not belong to this function"
        );
        let mut body = self.func.body_mut();
        unsafe {
            body.body_mut().cursor_mut_from_ptr(block).remove();
        }
        block.borrow_mut().uses_mut().clear();
        Block::on_removed_from_parent(block, body.as_region_ref());
    }

    pub fn append_block_param(&mut self, block: BlockRef, ty: Type, span: SourceSpan) -> ValueRef {
        self.builder.context().append_block_argument(block, ty, span)
    }

    pub fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a> {
        DefaultInstBuilder::new(self.func, &mut self.builder)
    }
}

pub struct DefaultInstBuilder<'f> {
    func: &'f mut Function,
    builder: &'f mut OpBuilder,
}
impl<'f> DefaultInstBuilder<'f> {
    pub(crate) fn new(func: &'f mut Function, builder: &'f mut OpBuilder) -> Self {
        Self { func, builder }
    }
}
impl<'f> InstBuilderBase<'f> for DefaultInstBuilder<'f> {
    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder) {
        (self.func, self.builder)
    }

    fn builder(&self) -> &OpBuilder {
        self.builder
    }

    fn builder_mut(&mut self) -> &mut OpBuilder {
        self.builder
    }
}

pub trait InstBuilderBase<'f>: Sized {
    fn builder(&self) -> &OpBuilder;
    fn builder_mut(&mut self) -> &mut OpBuilder;
    fn builder_parts(&mut self) -> (&mut Function, &mut OpBuilder);
    /// Get a default instruction builder using the dataflow graph and insertion point of the
    /// current builder
    fn ins<'a, 'b: 'a>(&'b mut self) -> DefaultInstBuilder<'a> {
        let (func, builder) = self.builder_parts();
        DefaultInstBuilder::new(func, builder)
    }
}

pub trait InstBuilder<'f>: InstBuilderBase<'f> {
    fn u32(mut self, value: u32, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Constant, _>(span);
        let constant = op_builder(Immediate::U32(value))?;
        Ok(constant.borrow().result().as_value_ref())
    }

    //signed_integer_literal!(1, bool);
    //integer_literal!(8);
    //integer_literal!(16);
    //integer_literal!(32);
    //integer_literal!(64);
    //integer_literal!(128);

    /*
    fn felt(self, i: Felt, span: SourceSpan) -> Value {
        into_first_result!(self.UnaryImm(Opcode::ImmFelt, Type::Felt, Immediate::Felt(i), span))
    }

    fn f64(self, f: f64, span: SourceSpan) -> Value {
        into_first_result!(self.UnaryImm(Opcode::ImmF64, Type::F64, Immediate::F64(f), span))
    }

    fn character(self, c: char, span: SourceSpan) -> Value {
        self.i32((c as u32) as i32, span)
    }
    */

    /// Two's complement addition which traps on overflow
    fn add(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement addition. Behavior is undefined if the result overflows.
    fn add_unchecked(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, e.g. `wrapping_add`
    fn add_wrapping(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which traps on overflow
    fn mul(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement multiplication. Behavior is undefined if the result overflows.
    fn mul_unchecked(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, e.g. `wrapping_mul`
    fn mul_wrapping(
        mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shl(mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::dialects::test::Shl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn ret(
        mut self,
        returning: Option<ValueRef>,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::dialects::test::Ret>, Report> {
        let op_builder = self
            .builder_mut()
            .create::<crate::dialects::test::Ret, (<Option<ValueRef> as IntoIterator>::IntoIter,)>(
                span,
            );
        op_builder(returning)
    }
}

impl<'f, T: InstBuilderBase<'f>> InstBuilder<'f> for T {}
