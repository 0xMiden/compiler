use midenc_hir2::{
    dialects::builtin::FunctionBuilder, ArrayAttr, Builder, BuilderExt, OpBuilder, Region, Report,
    SourceSpan, Type, UnsafeIntrusiveEntityRef, ValueRef,
};

use crate::*;

pub trait StructuredControlFlowOpBuilder<'f, B: ?Sized + Builder> {
    fn r#if(
        &mut self,
        cond: ValueRef,
        results: &[Type],
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<If>, Report> {
        let op_builder = self.builder_mut().create::<If, (_,)>(span);
        let if_op = op_builder(cond)?;
        {
            let mut owner = if_op.as_operation_ref();
            let context = self.builder().context();
            for result_ty in results {
                let result = context.make_result(span, result_ty.clone(), owner, 0);
                owner.borrow_mut().results_mut().push(result);
            }
        }
        Ok(if_op)
    }

    fn r#while<T>(
        &mut self,
        loop_init_variables: T,
        results: &[Type],
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<While>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<While, (T,)>(span);
        let mut while_op = op_builder(loop_init_variables)?;
        {
            let mut owner = while_op.as_operation_ref();
            let context = self.builder().context();
            for result_ty in results {
                let result = context.make_result(span, result_ty.clone(), owner, 0);
                owner.borrow_mut().results_mut().push(result);
            }
        }
        {
            let mut while_op = while_op.borrow_mut();
            let before_block = self
                .builder()
                .context()
                .create_block_with_params(while_op.inits().iter().map(|v| v.borrow().ty()));
            while_op.before_mut().body_mut().push_back(before_block);
            let after_block =
                self.builder().context().create_block_with_params(results.iter().cloned());
            while_op.after_mut().body_mut().push_back(after_block);
        }
        Ok(while_op)
    }

    fn index_switch<T>(
        &mut self,
        selector: ValueRef,
        cases: T,
        results: &[Type],
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<IndexSwitch>, Report>
    where
        T: IntoIterator<Item = u32>,
    {
        let cases = ArrayAttr::from_iter(cases);
        let num_cases = cases.len();
        let op_builder = self.builder_mut().create::<IndexSwitch, (_, _)>(span);
        let switch_op = op_builder(selector, cases)?;
        let mut owner = switch_op.as_operation_ref();

        // Create results
        {
            let context = self.builder().context();
            for result_ty in results {
                let result = context.make_result(span, result_ty.clone(), owner, 0);
                owner.borrow_mut().results_mut().push(result);
            }
        }

        // Create regions for all cases
        {
            for _ in 0..num_cases {
                let region = self.builder().context().alloc_tracked(Region::default());
                owner.borrow_mut().regions_mut().push_back(region);
            }
        }

        Ok(switch_op)
    }

    fn condition<T>(
        &mut self,
        cond: ValueRef,
        forwarded: T,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Condition>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<Condition, (_, T)>(span);
        op_builder(cond, forwarded)
    }

    fn r#yield<T>(
        &mut self,
        yielded: T,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Yield>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<Yield, (T,)>(span);
        op_builder(yielded)
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> StructuredControlFlowOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> StructuredControlFlowOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> StructuredControlFlowOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
