use alloc::rc::Rc;

use midenc_hir::{
    adt::SmallDenseMap,
    dialects::builtin::{Function, FunctionRef, LocalVariable},
    pass::{Pass, PassExecutionState, PassIdentifier, PostPassStatus},
    BlockRef, BuilderExt, EntityMut, Op, OpBuilder, OperationName, OperationRef, Report, Rewriter,
    SourceSpan, Spanned, Symbol, ValueRef,
};
use midenc_hir_analysis::analyses::SpillAnalysis;
use midenc_hir_transform::{self as transforms, ReloadLike, SpillLike, TransformSpillsInterface};

pub struct TransformSpills;

impl Pass for TransformSpills {
    type Target = Function;

    fn name(&self) -> &'static str {
        "transform-spills"
    }

    fn pass_id(&self) -> Option<PassIdentifier> {
        Some(PassIdentifier::TransformSpills)
    }

    fn argument(&self) -> &'static str {
        "transform-spills"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<PostPassStatus, Report> {
        let function = op.into_entity_ref();
        log::debug!(target: "insert-spills", "computing and inserting spills for {}", function.as_operation());

        if function.is_declaration() {
            log::debug!(target: "insert-spills", "function has no body, no spills needed!");
            state.preserved_analyses_mut().preserve_all();
            return Ok(PostPassStatus::IRUnchanged);
        }
        let mut analysis =
            state.analysis_manager().get_analysis_for::<SpillAnalysis, Function>()?;

        if !analysis.has_spills() {
            log::debug!(target: "insert-spills", "no spills needed!");
            state.preserved_analyses_mut().preserve_all();
            return Ok(PostPassStatus::IRUnchanged);
        }

        // Take ownership of the spills analysis so that we can mutate the analysis state during
        // spill/reload materialization.
        let analysis = Rc::make_mut(&mut analysis);

        // Place spills and reloads, rewrite IR to ensure live ranges we aimed to split are actually
        // split.
        let mut interface = TransformSpillsImpl {
            function: function.as_function_ref(),
            locals: Default::default(),
        };

        let op = function.as_operation_ref();
        drop(function);
        transforms::transform_spills(op, analysis, &mut interface, state.analysis_manager().clone())
    }
}

struct TransformSpillsImpl {
    function: FunctionRef,
    locals: SmallDenseMap<ValueRef, LocalVariable>,
}

impl TransformSpillsInterface for TransformSpillsImpl {
    fn create_unconditional_branch(
        &self,
        builder: &mut OpBuilder,
        destination: BlockRef,
        arguments: &[ValueRef],
        span: SourceSpan,
    ) -> Result<(), Report> {
        use midenc_dialect_cf::ControlFlowOpBuilder;

        builder.br(destination, arguments.iter().copied(), span)?;

        Ok(())
    }

    fn create_spill(
        &self,
        builder: &mut OpBuilder,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<OperationRef, Report> {
        let op_builder = builder.create::<crate::ops::Spill, _>(span);
        op_builder(value).map(|op| op.as_operation_ref())
    }

    fn create_reload(
        &self,
        builder: &mut OpBuilder,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<OperationRef, Report> {
        let op_builder = builder.create::<crate::ops::Reload, _>(span);
        op_builder(value).map(|op| op.as_operation_ref())
    }

    fn convert_spill_to_store(
        &mut self,
        rewriter: &mut dyn Rewriter,
        spill: OperationRef,
    ) -> Result<(), Report> {
        use crate::HirOpBuilder;

        let spilled = spill.borrow().as_trait::<dyn SpillLike>().unwrap().spilled_value();
        let mut function = self.function;
        let local = *self.locals.entry(spilled).or_insert_with(|| {
            let mut function = function.borrow_mut();
            function.alloc_local(spilled.borrow().ty().clone())
        });

        let store = rewriter.store_local(local, spilled, spill.span())?;

        rewriter.replace_op(spill, store.as_operation_ref());

        Ok(())
    }

    fn convert_reload_to_load(
        &mut self,
        rewriter: &mut dyn Rewriter,
        reload: OperationRef,
    ) -> Result<(), Report> {
        use crate::HirOpBuilder;

        let spilled = reload.borrow().as_trait::<dyn ReloadLike>().unwrap().spilled_value();
        let local = self.locals[&spilled];
        let reloaded = rewriter.load_local(local, reload.span())?;

        rewriter.replace_op_with_values(reload, &[Some(reloaded)]);

        Ok(())
    }
}
